# SyncVibe Architecture

Add multiplayer to your AI coding agent. Zero AI costs.

## Design Philosophy

SyncVibe is a **coordination layer, not an AI layer**. Zero LLM API calls, zero token costs. It gives AI agents (Claude Code, Codex, Gemini CLI) two MCP tools (`read_chat`, `send_chat`) so they can participate in a shared chat room alongside humans.

**Key principles:**
1. **MCP-first**: The MCP server is the primary interface. Everything else (TUI, tmux, invite codes) exists to support it.
2. **Agents as participants**: AI agents are equal members of the chat room, not tools you invoke. They read messages, respond to @mentions, and coordinate with each other.
3. **Local-first**: All state lives in `.syncvibe/` inside the project. The relay handles real-time sync only, stores nothing.
4. **Zero config for agents**: `syncvibe init` auto-generates MCP configs for whichever agent the user picks (Claude, Codex, or Gemini).

## System Overview

```
┌────────────────────────────────────────────────────────────────┐
│  User A's Machine                                              │
│                                                                │
│  ┌──────────────┐     ┌──────────────────────────────────────┐ │
│  │ SyncVibe TUI │     │ Claude Code                          │ │
│  │  (chat +     │     │                                      │ │
│  │   presence)  │     │  MCP: syncvibe mcp-server            │ │
│  │              │     │    ├── read_chat → .syncvibe/        │ │
│  │              │     │    └── send_chat → .syncvibe/ + relay│ │
│  └──────┬───────┘     └──────────────────────────────────────┘ │
│         │                                                      │
│         └──── .syncvibe/chat-log.jsonl (shared local state) ───┘
│                           │
│                    WebSocket (WSS)
│                           │
│              ┌────────────┴────────────┐
│              │   Relay (CF Workers)    │
│              │   relay.syncvibe.online │
│              │                         │
│              │   - Message forwarding  │
│              │   - Presence tracking   │
│              │   - Invite codes (KV)   │
│              │   - Auth token exchange │
│              └────────────┬────────────┘
│                           │
│                    WebSocket (WSS)
│                           │
┌────────────────────────────────────────────────────────────────┐
│  User B's Machine                                              │
│                                                                │
│  ┌──────────────┐     ┌──────────────────────────────────────┐ │
│  │ SyncVibe TUI │     │ Codex CLI                            │ │
│  │              │     │                                      │ │
│  │              │     │  MCP: syncvibe mcp-server            │ │
│  │              │     │    ├── read_chat                     │ │
│  │              │     │    └── send_chat                     │ │
│  └──────────────┘     └──────────────────────────────────────┘ │
└────────────────────────────────────────────────────────────────┘
```

## Three Repositories

| Repo | Stack | Deployed to | Purpose |
|------|-------|-------------|---------|
| **syncvibe** (CLI) | Rust, rmcp, ratatui, tokio | Homebrew, install script | MCP server + TUI + tmux integration |
| **syncvibe-relay** | TypeScript, Cloudflare Workers + Durable Objects | relay.syncvibe.online | WebSocket message forwarding, invite codes, auth exchange |
| **syncvibe-web** | React 19, Vite, Tailwind, Supabase | syncvibe.online (Vercel) | Landing page, auth, dashboard |

## Crate Structure (CLI)

```
syncvibe/
├── Cargo.toml                     # Workspace root (v0.4.5)
├── crates/
│   ├── syncvibe-cli/              # Main binary
│   │   └── src/
│   │       ├── main.rs            # Entry point, clap dispatch
│   │       ├── mcp/server.rs      # MCP server: read_chat, send_chat (440+ lines)
│   │       ├── init.rs            # Room init (.mcp.json, .codex/, .gemini/, CLAUDE.md)
│   │       ├── session.rs         # Interactive onboarding + project launch
│   │       ├── tmux.rs            # tmux session management, split layout
│   │       ├── app.rs             # TUI event loop + slash commands
│   │       ├── auth.rs            # Web account linking via relay token exchange
│   │       ├── sync.rs            # Room metadata sync to Supabase
│   │       ├── invite.rs          # Invite code create/resolve via relay API
│   │       ├── agents.rs          # Agent definitions (Claude, Codex, Gemini)
│   │       ├── network/ws_client.rs  # WebSocket client (tokio-tungstenite)
│   │       ├── components/        # TUI rendering (ratatui)
│   │       └── git/ops.rs         # Git remote detect, clone, remote set
│   │
│   └── syncvibe-core/             # Shared library
│       └── src/
│           ├── models/            # ChatMessage, Room, User types
│           ├── protocol.rs        # WsMessage enum (WebSocket types)
│           └── storage.rs         # .syncvibe/ file I/O (atomic writes, file locking)
```

## MCP Server (Primary Interface)

The MCP server is how AI agents interact with SyncVibe. It runs as `syncvibe mcp-server` and provides two tools:

### read_chat

Smart incremental reads with session scoping and context window protection.

```
Parameters:
  all: bool     -- return all sessions or just current (default: current)
  since: string -- ISO 8601 timestamp filter
  format: string -- "compact" (default, token-efficient) or "json" (structured)

Behavior:
  - Tracks byte offset across calls (incremental reads within one MCP session)
  - Groups consecutive messages from the same user (saves ~20-30% tokens)
  - Highlights @agent tasks with lightning bolt icon
  - Small conversations (< 30 msgs): returns inline
  - Large conversations (30+ msgs): writes to .syncvibe/chat-digest.md, returns summary
  - Escapes boundary markers in user content to prevent prompt injection
```

### send_chat

Sends a message to the team chat as the AI agent.

```
Parameters:
  content: string -- message text (truncated to 500 chars)

Behavior:
  - Creates ChatMessage with agent identity (user_id: "agent-{id}")
  - Appends to .syncvibe/chat-log.jsonl
  - Broadcasts via WebSocket relay to all connected peers
```

### MCP Registration

`syncvibe init` auto-generates per-agent config files:

| Agent | Config File | Format |
|-------|-------------|--------|
| Claude Code | `.mcp.json` | `{"mcpServers":{"syncvibe":{"command":"syncvibe","args":["mcp-server"]}}}` |
| Codex CLI | `.codex/config.toml` | `[mcp_servers.syncvibe]` block |
| Gemini CLI | `.gemini/settings.json` | Same JSON format as Claude |

Manual registration (Claude Code): `claude mcp add syncvibe -- syncvibe mcp-server`

### Agent Behavior Layer

Three reinforcement layers ensure agents always read chat before working:

1. **CLAUDE.md / AGENTS.md** -- `syncvibe init` injects "Before starting ANY task, call read_chat"
2. **MCP server instructions** -- tool description includes "IMPORTANT: call read_chat before starting ANY task"
3. **Claude Code skill** -- `.claude/skills/syncvibe/SKILL.md` provides collaboration instructions

```
Agent Task Flow:
  1. User gives agent a task
  2. Agent calls read_chat (instructed by CLAUDE.md)
  3. Agent sees team discussions, @mentions, pending tasks
  4. Agent works with full context
  5. Agent calls send_chat to report results
```

## Data Flow

### Human to Human (TUI chat)
```
User A types → TUI → .syncvibe/chat-log.jsonl → WebSocket relay → User B's TUI
```

### Human to Agent (@mention)
```
User types "@claude do X" → chat-log.jsonl → Agent's MCP read_chat → Agent sees task
                                            → Shows as "TASKS FOR YOU (1): ..."
```

### Agent to Human (send_chat)
```
Agent calls send_chat → chat-log.jsonl → TUI detects agent- prefix → Broadcasts via relay
                                       → All teammates see agent's response
```

### Agent to Agent (cross-machine)
```
Agent A sends_chat → relay → User B's TUI → chat-log.jsonl → Agent B reads via MCP
```

### Remote @mentions (cross-machine agent tasks)
```
User B types "@claude do X" → relay → User A's TUI → tmux send-keys to agent pane
                                                    → Agent reads chat, sees task
                                                    → 30-second debounce prevents spam
```

## Interactive Features

### TUI Chat
- Real-time chat with presence indicators (online/offline dots)
- @mention with tab completion and bell notifications
- Image sharing, message grouping, scroll-back with PageUp/PageDown
- Mouse scroll scoped to chat panel
- Unread indicator: "N new messages" banner when scrolled up
- Message selection for quoting
- Chat truncation: >5000 msgs keeps last 2000 in memory

### tmux Integration
- Auto-creates split layout: SyncVibe Dashboard (30%) | AI Agent (70%)
- Session naming: `sv-{project}-{path_hash}` (collision-free)
- Ctrl+G to switch panes
- Auto-installs tmux if missing (Homebrew/apt/dnf/pacman)
- Session cleanup prevents orphaned agent processes

### Screen Sharing
- `/share` toggles sharing your agent pane
- `/watch <name>` views a teammate's screen in real time
- Delta-encoded frames (only changed lines sent)
- Max 500 lines per frame, 1-second capture interval

### Git Remote Sync
- Room creator: auto-detects git remote or prompts for URL
- Teammate joins: repo auto-clones on `syncvibe connect`
- Agents instructed to commit+push after tasks, pull before new work
- Zero token storage: uses existing git credentials

### Invite Codes
- Short codes (`HKPT-3NWV`) auto-copied to clipboard
- 7-day TTL, stored in relay KV
- Rate limited: 10 requests/minute per IP

## Server-Side Infrastructure

### Relay (Cloudflare Workers + Durable Objects)

WebSocket message forwarding at `relay.syncvibe.online`. No message persistence.

| Data | Storage | Duration |
|------|---------|----------|
| Room secret (for auth) | Durable Objects | Persistent |
| Invite codes | KV store | 7-day TTL |
| Auth tokens | KV store | 5-min TTL, one-time use |
| Connected users | In-memory | Session only |
| Chat messages | Not stored | Forwarded in real time |

Security:
- Rate limiting: 20 msgs/sec per client, 10 API requests/min per IP
- Max message size: 256 KB
- Identity stamping: relay stamps authenticated identity on messages
- Agent identity: only connections with matching `agentId` can send as `agent-{id}`
- Idle cleanup: rooms with no users for 1 hour auto-delete
- Max 100 WebSocket connections per room

### Database (Supabase)

Room metadata sync when users link CLI to web account (`syncvibe auth`).

| Table | Purpose |
|-------|---------|
| profiles | User info, CLI tokens (32-byte hex), auto-created on signup |
| user_projects | Room associations per user (RLS enforced) |
| feedback | User feedback from web app |
| collaborator_requests | GitHub collaborator access requests |

RPC functions: `sync_room`, `bulk_sync_rooms`, `leave_room`, `regenerate_cli_token`, `approve_collaborator`, `request_collaborator`, `store_github_token`

No chat content, messages, or files are ever stored in the database.

### Auth Token Exchange (CLI to Web via Relay)

```
CLI                          Relay                        Web
 │                            │                            │
 ├─ generate auth_code (UUID) │                            │
 ├─ open browser ─────────────┼──────────────────────────► │ /authorize?code=ABC
 │                            │                            │
 │  poll GET /auth/ABC ──────►│ 404 (not yet)              │ user signs in
 │                            │◄── POST /auth/ABC ─────────┤ { token, urls }
 │  poll GET /auth/ABC ──────►│ 200 { token, urls }        │
 │  save to ~/.syncvibe/      │ (delete from KV)           │
```

UUID auth code, 5-min TTL, one-time use, one-write-only.

## Local State

```
.syncvibe/                        # Per-project, gitignored
├── room.json                     # Room identity (room_id, secret, relay_url, git_remote)
├── chat-log.jsonl                # Append-only chat, one JSON per line
├── chat-digest.md                # Auto-generated by MCP for large conversations
└── images/                       # Shared images (UUID-named)

~/.syncvibe/                      # Global config
├── config.toml                   # User profile, account credentials, preferences
└── projects.json                 # Registry of all initialized projects
```

## Security

- **TLS enforced**: all relay connections use WSS, plaintext rejected
- **No message persistence**: relay forwards in memory only, nothing logged
- **Transport encryption (not E2E)**: relay sees plaintext during forwarding. E2E encryption is on the roadmap
- **Room secrets**: 256-bit random (64 hex chars), constant-time comparison
- **File permissions**: `.syncvibe/` dir is 0700, all files are 0600
- **Atomic writes**: write to temp file + rename prevents partial reads
- **Advisory file locking**: exclusive lock on chat-log.jsonl for concurrent write safety
- **ANSI stripping**: remote content stripped of terminal escape sequences
- **Identity stamping**: relay stamps authenticated identity, prevents spoofing
- **Prompt injection prevention**: MCP `read_chat` escapes boundary markers (`[USER:`, `[END MESSAGE]`) in user content using zero-width spaces
- **Invite code expiry**: 7-day TTL, auto-deleted from KV
- **Message retention**: configurable `retention_days` (default 90), pruned atomically on startup

## Distribution

| Channel | Command | Audience |
|---------|---------|----------|
| Homebrew | `brew tap Curious1008/syncvibe && brew install syncvibe` | macOS users |
| Install script | `curl -fsSL https://syncvibe.online/install.sh \| sh` | macOS + Linux |
| MCP registration | `claude mcp add syncvibe -- syncvibe mcp-server` | Claude Code users |
| Claude Code skill | `.claude/skills/syncvibe/SKILL.md` | Claude Code sessions |
| Skill install script | `curl -fsSL https://syncvibe.online/skill-install.sh \| sh` | One-command setup |

Supported platforms: macOS (Apple Silicon + Intel), Linux (x86_64, aarch64).

## Key Design Decisions

1. **MCP-first distribution**: the MCP server is the primary interface. TUI and tmux are the experience layer, but the value is in `read_chat` and `send_chat`.
2. **Local-first state**: all state in `.syncvibe/`, gitignored. Relay is ephemeral.
3. **JSONL for chat**: append-only, no merge conflicts, advisory file locking.
4. **Atomic file writes**: write to `.tmp`, then rename. Prevents partial reads.
5. **Context window protection**: large conversations offloaded to digest file, not returned inline.
6. **Session auto-segmentation**: 30min silence starts a new session ID. MCP defaults to current session.
7. **Per-agent config generation**: `syncvibe init` writes the right config file format for each agent.
8. **Zero AI costs**: pure coordination. Each user pays for their own agent, SyncVibe adds no token costs.
9. **Prompt injection defense**: boundary markers in user content are escaped with zero-width spaces before being returned via MCP.
10. **Git remote sync**: URLs only, zero token storage. Auth uses existing git credentials.
