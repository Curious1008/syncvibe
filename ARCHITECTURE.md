# SyncVibe Architecture

Terminal-native collaboration tool for multi-person vibe coding.

## Design Philosophy

SyncVibe is a **coordination layer, not an AI layer**. Zero LLM API calls, zero token costs. It leverages existing AI agent capabilities (Claude Code's native file reading and hooks) rather than duplicating them.

**Key principle**: Only build MCP tools for capabilities that agents can't do natively. Everything else uses direct file access guided by CLAUDE.md instructions.

## Crate Structure

```
syncvibe/
├── Cargo.toml                     # Workspace root
├── crates/
│   ├── syncvibe-cli/              # Main binary
│   │   └── src/
│   │       ├── main.rs            # Entry point, clap CLI dispatch, simple commands
│   │       ├── init.rs            # Room init (.syncvibe/, .mcp.json, .claude, CLAUDE.md)
│   │       ├── session.rs         # Interactive onboarding + project launch
│   │       ├── tmux.rs            # tmux session management, layout, keybindings
│   │       ├── app.rs             # TUI app state + async event loop
│   │       ├── picker.rs          # Project switcher (ratatui)
│   │       ├── cli.rs             # Clap command definitions
│   │       ├── config.rs          # ~/.syncvibe/ config + project registry
│   │       ├── onboarding.rs      # Interactive prompts + input validation
│   │       ├── auth.rs            # Web account linking via relay-mediated token exchange
│   │       ├── invite.rs          # Invite code create/resolve via relay API
│   │       ├── agents.rs          # AI agent configuration (Claude, Codex)
│   │       ├── sync.rs            # Room metadata sync to Supabase
│   │       ├── tui.rs             # Terminal setup/teardown (crossterm)
│   │       ├── components/        # TUI rendering (ratatui)
│   │       │   ├── status_bar.rs  # Top bar: project name + presence
│   │       │   ├── chat.rs        # Chat message display
│   │       │   ├── input.rs       # Input bar with cursor
│   │       │   └── util.rs        # Color parsing helpers
│   │       ├── network/
│   │       │   └── ws_client.rs   # WebSocket client (tokio-tungstenite)
│   │       ├── mcp/
│   │       │   └── server.rs      # MCP server: read_chat, send_chat
│   │       └── git/
│   │           └── ops.rs         # Git operations: remote detect, clone, remote set
│   │
│   └── syncvibe-core/             # Shared library
│       └── src/
│           ├── lib.rs
│           ├── error.rs           # SyncVibeError enum
│           ├── models/            # Data types (chat, room, user)
│           ├── protocol.rs        # WsMessage enum (WebSocket types)
│           └── storage.rs         # .syncvibe/ file I/O (atomic writes, file locking)
│
└── worker/                        # Cloudflare Worker (TypeScript)
    └── src/
        ├── index.ts               # HTTP routing → Durable Object
        ├── room.ts                # Durable Object: WS relay + presence
        └── types.ts
```

## Features

### Interactive Onboarding
- `syncvibe` with no args: profile setup, project list, create/join room
- `syncvibe://` invite codes: base64-encoded room_id + room_secret
- Auto-detects git user.name for profile defaults

### TUI Chat
- Real-time chat with presence indicators
- Slash commands: `/help` (`/?`), `/invite` (`/i`), `/new` (`/n`), `/join` (`/j`), `/name`, `/color`, `/mute` (`/m`), `/remote`, `/collab`, `/clear`, `/rc`, `/quit` (`/q`)
- **@mentions**: `@name` highlights + bell notification; `@agent` / `@claude` prompts agent to read chat for new tasks (only the room's configured agent responds — mentioning a different agent is ignored)
- Image sharing (drag path into input)
- Horizontal scrolling for long input lines
- **Mouse scroll** + PageUp/PageDown for chat navigation (mouse events scoped to chat panel)
- **Unread indicator**: "↓ N new messages" banner when scrolled up and new messages arrive; auto-resets on return to bottom
- **Message selection**: Up/Down arrows select messages for quoting or opening images; Esc deselects
- Message grouping by user, bell only on @mention (with debounce)
- Chat truncation for performance (>5000 msgs → keep last 2000 in memory, msg_id_set pruned in sync)

### Status Bar & Presence
- **Version display**: shows current version (e.g. `v0.3.9`) in status bar
- **Fixed positions**: current user (rightmost, bold with color) + agent indicator (`◆ Agent` in teal, when in tmux)
- **Carousel rotation**: other online users rotate every 3 seconds when they don't all fit
- **Dynamic width**: calculates available space, shows as many users as fit, `+N` indicator for hidden ones
- **Online/offline indicator**: green `●` when connected, gray `○ offline` when disconnected

### tmux Integration
- Auto-creates split layout: SyncVibe Dashboard (30%, left) | AI Agent (70%, right)
- Dashboard is the long-lived process; agent pane is split after session creation
- Session cleanup: `kill-session` on detach (outside tmux) or `destroy-unattached` (inside tmux) prevents orphaned agent processes
- Ctrl+G to switch panes, styled pane borders
- Project switching between tmux sessions

### Screen Sharing
- `/share` — toggle sharing your agent pane with teammates
- `/watch <name>` — view a teammate's agent screen in real time
- Delta-encoded frames: compares current frame to previous, sends only changed lines
- Max 500 lines per frame, 1-second capture interval
- Protocol: `ScreenShareStart` → `ScreenFrame` (delta) → `ScreenShareStop`

### Git Remote Sync

Allows teammates to auto-clone the project repo when joining a room.

**Data flow:**
1. Room creator: `detect_or_prompt_git_remote()` detects or prompts for git remote URL → stored in `room.json` as `git_remote`
2. `/invite`: refreshes `git_remote` from actual git remote before creating invite code → relay stores it in KV alongside room credentials
3. Joiner: `resolve_short_invite()` receives `git_remote` → `prepare_project_dir_with_remote()` attempts `git clone` → falls back to empty dir on failure
4. Agents: CLAUDE.md/AGENTS.md instruct commit+push after tasks, pull before new work

**Security model:** Zero token storage. `git_remote` is just a URL (public info). Authentication is handled entirely by the user's existing git credential manager — same as running `git push` manually.

**TUI commands:**
- `/remote` — show current remote or set a new one (`/remote https://github.com/...`)
- `/collab` — open GitHub collaborator settings page, or `github.com/new` if no remote exists

**Room creation menu** (`detect_or_prompt_git_remote`):
1. Auto-detect existing git remote → done
2. If no remote: show 3-option menu — Paste URL / Create new repo (opens browser) / Skip

### MCP Server
- 2 tools: `read_chat` and `send_chat`, injected via `.mcp.json` at init time
- **`read_chat`**: smart incremental reads with session scoping, time filtering, byte-offset tracking, and digest file offloading
- **`send_chat`**: sends a message to the team chat as the AI agent
- **Context-aware response sizing**: small conversations (< 30 msgs) return inline; large conversations write full content to `.syncvibe/chat-digest.md` and return a brief summary — prevents flooding the agent's context window
- **Message grouping**: consecutive messages from the same user are collapsed under one header, saving ~20-30% tokens

### Project Registry
- `~/.syncvibe/projects.json` tracks all initialized projects
- Canonical paths to avoid duplicates (macOS `/tmp` vs `/private/tmp`)

## AI Agent Integration Strategy

### Agent Auto-Read Flow

The core UX: **users discuss in SyncVibe chat, then assign tasks to their agent — the agent automatically understands the team's context.**

```
1. syncvibe init
   → .mcp.json       (registers SyncVibe MCP server)
   → CLAUDE.md        (instructs agent: "call read_chat before ANY task")
   → .claude/settings (hooks for file change notifications)

2. User gives agent a task
   → Agent calls read_chat (MCP) — instructed by CLAUDE.md
   → Small chat: messages inline
   → Large chat: full content → .syncvibe/chat-digest.md, agent reads the file
   → Agent acknowledges: "I've read the team chat — you're working on X..."
   → Agent proceeds with full context
```

Three reinforcement layers ensure the agent always reads chat:
1. **CLAUDE.md** — "Before starting ANY task, call read_chat"
2. **MCP server instructions** — "IMPORTANT: call read_chat before starting ANY task"
3. **Agent behavior** — agent acknowledges the discussion, making it visible to the user

### What MCP adds
| MCP Tool | Why it exists |
|---|---|
| `read_chat` | **Smart filtering**: session scoping, incremental byte-offset reads, time-based filtering, digest file offloading for large conversations — not possible with raw file read |
| `send_chat` | **Structured messaging**: creates properly formatted ChatMessage with user metadata, appends to JSONL, and broadcasts via WebSocket relay |

### What Claude Code provides natively
| Capability | How SyncVibe leverages it |
|---|---|
| CLAUDE.md | `syncvibe init` injects collaboration instructions — agent auto-reads chat |
| .mcp.json | `syncvibe init` registers MCP server — agent discovers `read_chat` tool |
| File Read/Write/Edit | Agents read `.syncvibe/chat-digest.md`, write `.syncvibe/chat-log.jsonl` |
| Hooks | `.claude/settings.json` — PostToolUse hook touches `.syncvibe/.updated` on file changes |

## Data Flow

### Human ↔ Human (TUI)
```
User input → TUI → Storage (.syncvibe/) → WebSocket relay → Other TUI
                                         → File watcher → Re-render
```

### Human ↔ AI Agent
```
Human (TUI) → Storage (.syncvibe/) → File watcher on agent side
                                    → Agent reads files / MCP read_chat
AI Agent → Writes .syncvibe/ files → Hook touches .updated → TUI file watcher → Re-render
```

### AI Agent ↔ AI Agent
```
Agent A → Appends chat-log.jsonl → Agent B reads via MCP read_chat (incremental)
```

### Screen Sharing
```
Sharer's agent pane → tmux capture-pane → Delta encode
  → ScreenFrame (WSS) → Relay (forward, no storage) → Viewer's TUI overlay
```

## .syncvibe/ Directory

```
.syncvibe/
├── room.json            # Room config (room_id, room_secret, relay_url, git_remote)
├── chat-log.jsonl       # Append-only, one JSON per line
├── chat-digest.md       # Auto-generated by MCP read_chat for large conversations
└── images/              # Shared images (UUID-named)
```

`.syncvibe/` is gitignored. The WebSocket relay provides real-time sync; local files are the source of truth.

## Server-Side Infrastructure

### Relay (Cloudflare Workers + Durable Objects)

The relay at `relay.syncvibe.online` handles real-time WebSocket forwarding only.

**What it stores:**

| Data | Storage | Duration |
|------|---------|----------|
| Room secret (for auth) | Durable Objects | Persistent |
| Invite codes | KV store | 7-day TTL, auto-deleted |
| Connected users | In-memory only | Session only |
| Chat messages | Not stored | Forwarded in real time |
| Screen share frames | Not stored | Forwarded in real time |

**What it does NOT do:** log messages, index content, read source code, run analytics.

All relay traffic uses WSS (TLS). The relay sees message content in memory during forwarding — this is transport encryption, not E2E. E2E encryption is on the roadmap.

### Database (Supabase)

When a user links their CLI to a web account (`syncvibe auth`), room metadata is synced via RPC:

| Data | Purpose |
|------|---------|
| Room ID | Identify the room |
| Project name | Display name in dashboard |
| Room secret | Allow re-joining from other devices |

**No chat content, messages, or files are ever stored in the database.**

Three RPC functions:
- `sync_room` — sync a single room on create/join
- `bulk_sync_rooms` — sync all local rooms after authentication
- `leave_room` — remove room association, returns remaining member count

### Invite Code Flow

```
Creator: POST /invite {room_id, room_secret, room_name, git_remote?}
  → Relay generates short code (XXXX-XXXX), stores in KV with 7-day TTL
  → Returns code to creator

Joiner: GET /invite/{code}
  → Relay looks up KV, returns {room_id, room_secret, relay_url, git_remote?}
  → If git_remote present: auto-clone repo to project dir
  → Joiner connects to room with those credentials
```

Rate limited: 10 requests/minute per IP.

### Auth Token Exchange (CLI ↔ Web via Relay)

Allows users to link their CLI to their web account at syncvibe.online. The token exchange is mediated by the relay to avoid Chrome's Private Network Access restrictions (HTTPS page → HTTP localhost is blocked).

```
CLI                          Relay                        Web
 │                            │                            │
 ├─ generate auth_code (UUID) │                            │
 ├─ open browser ─────────────┼──────────────────────────► │ /authorize?code=ABC
 │                            │                            │
 │  poll GET /auth/ABC ──────►│ 404 (not yet)              │ user signs in
 │  poll GET /auth/ABC ──────►│ 404                        │ user clicks Authorize
 │                            │◄── POST /auth/ABC ─────────┤ { token, urls }
 │  poll GET /auth/ABC ──────►│ 200 { token, urls }        │ shows "Authenticated!"
 │  ◄─────────────────────────│ (delete from KV)           │
 │  save token                │                            │
```

**Security:**
- Auth code is UUID v4 (128-bit random) — guessing is infeasible
- 5-minute TTL — expired codes auto-deleted from KV
- One-time use — token deleted from KV after first retrieval
- One-write-only — relay rejects POST if auth code already has a token (prevents overwrite / session fixation)
- Explicit user click on "Authorize" prevents login CSRF
- Token validated as hex on relay, CLI, and web
- Rate limited: 10 requests/minute per IP

## Security & Privacy

- **TLS enforced** — all relay connections use WSS; plaintext `ws://` rejected
- **No message persistence** — relay forwards chat, screen shares, and MCP traffic in memory only; nothing logged or written to disk
- **Relay visibility** — the relay sees plaintext message content during forwarding (transport encryption, not E2E). E2E is on the roadmap
- **Room secrets** — 256-bit random (64 hex chars), sent over TLS for auth, stored server-side for reconnection support
- **File permissions** — `.syncvibe/` dir is 0700, all files (room.json, chat-log.jsonl, config.toml) are 0600
- **Atomic writes** — write to temp file + rename prevents partial reads
- **Advisory file locking** — exclusive lock on chat-log.jsonl for concurrent write safety
- **Message retention** — `retention_days` in `~/.syncvibe/config.toml` (default: 90). Old messages pruned atomically on startup
- **ANSI stripping** — remote peer content is stripped of CSI, OSC, DCS, APC, PM, SOS escape sequences and control chars to prevent terminal injection
- **Identity stamping** — relay stamps user identity server-side on messages to prevent spoofing
- **Invite code expiry** — KV-stored codes auto-delete after 7 days
- **Auth token exchange** — relay-mediated, UUID auth codes with 5-min TTL, one-time use, one-write-only (see Auth Token Exchange section)

For full details, see [Data & Privacy](https://syncvibe.online/docs/data-privacy).

## Key Design Decisions

1. **Local-first**: All state in `.syncvibe/`, gitignored. WebSocket relay is ephemeral for real-time sync.
2. **JSONL for chat**: Append-only, no merge conflicts, advisory file locking for concurrent writes.
3. **Atomic file writes**: Write to `.tmp`, then rename. Prevents partial reads.
4. **2 MCP tools**: `read_chat` (smart filtering, digest offloading) and `send_chat` (structured messaging with relay broadcast).
5. **Session auto-segmentation**: 30min silence → new session ID. MCP `read_chat` defaults to current session.
6. **Offline-first**: TUI works fully with local files. WebSocket is optional.
7. **Hooks integration**: `PostToolUse` hook signals TUI when agents modify `.syncvibe/` files.
8. **Security**: All local files use 0600 permissions, TLS enforced on relay, identity stamped server-side. See Security & Privacy section above.
9. **Context window protection**: MCP `read_chat` offloads large conversations to `.syncvibe/chat-digest.md` instead of returning them inline. Tool response stays bounded (~3 lines for large chats), agent reads the file at its own discretion.
10. **Message retention**: Configurable `retention_days` (default 90). Old messages pruned atomically on startup — temp file + rename to prevent data loss.
11. **Git remote sync**: Zero token storage. The `git_remote` URL is passed via invite codes; cloning/pushing uses the user's existing git credentials. No PATs, no OAuth for repo access.
