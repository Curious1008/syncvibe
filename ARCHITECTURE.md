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
│   │       ├── tui.rs             # Terminal setup/teardown (crossterm)
│   │       ├── components/        # TUI rendering (ratatui)
│   │       │   ├── status_bar.rs  # Top bar: project name + presence
│   │       │   ├── chat.rs        # Chat message display
│   │       │   ├── input.rs       # Input bar with cursor
│   │       │   └── util.rs        # Color parsing helpers
│   │       ├── network/
│   │       │   └── ws_client.rs   # WebSocket client (tokio-tungstenite)
│   │       ├── mcp/
│   │       │   └── server.rs      # MCP server: read_chat tool
│   │       └── git/
│   │           └── ops.rs         # Git repo name detection
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
- Slash commands: `/help` (`/?`), `/invite` (`/i`), `/projects` (`/p`), `/name`, `/color`, `/mute` (`/m`), `/clear`, `/rc`, `/quit` (`/q`)
- **@mentions**: `@name` highlights + bell notification; `@agent` / `@claude` sends task directly to Claude Code pane via tmux send-keys
- Image sharing (drag path into input)
- Message grouping by user, bell only on @mention (with debounce)
- Chat truncation for performance (>5000 msgs → keep last 2000 in memory)

### Status Bar & Presence
- **Fixed positions**: current user (rightmost, bold with color) + agent indicator (`◆ Agent` in teal, when in tmux)
- **Carousel rotation**: other online users rotate every 3 seconds when they don't all fit
- **Dynamic width**: calculates available space, shows as many users as fit, `+N` indicator for hidden ones
- **Online/offline indicator**: green `●` when connected, gray `○ offline` when disconnected

### tmux Integration
- Auto-creates split layout: SyncVibe Chat (30%) | Claude Code (70%)
- Ctrl+G to switch panes, styled pane borders
- Project switching between tmux sessions

### MCP Server
- 1 tool: `read_chat` (smart incremental reads with digest file offloading)
- Injected via `.mcp.json` at init time
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

### What agents do natively (no MCP needed)
- **Send chat**: Append JSONL line to `.syncvibe/chat-log.jsonl` — native file writing

### What MCP adds (smart capabilities agents can't do natively)
| MCP Tool | Why it exists |
|---|---|
| `read_chat` | **Smart filtering**: session scoping, incremental byte-offset reads, time-based filtering, digest file offloading for large conversations — not possible with raw file read |

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

## .syncvibe/ Directory

```
.syncvibe/
├── room.json            # Room config (room_id, room_secret, relay_url)
├── chat-log.jsonl       # Append-only, one JSON per line
├── chat-digest.md       # Auto-generated by MCP read_chat for large conversations
└── images/              # Shared images (UUID-named)
```

`.syncvibe/` is gitignored. The WebSocket relay provides real-time sync; local files are the source of truth.

## Key Design Decisions

1. **Local-first**: All state in `.syncvibe/`, gitignored. WebSocket relay is ephemeral for real-time sync.
2. **JSONL for chat**: Append-only, no merge conflicts, advisory file locking for concurrent writes.
3. **Atomic file writes**: Write to `.tmp`, then rename. Prevents partial reads.
4. **1 MCP tool, not many**: Only `read_chat` — adds value beyond native file access. Chat sending handled via direct file append.
5. **Session auto-segmentation**: 30min silence → new session ID. MCP `read_chat` defaults to current session.
6. **Offline-first**: TUI works fully with local files. WebSocket is optional.
7. **Hooks integration**: `PostToolUse` hook signals TUI when agents modify `.syncvibe/` files.
8. **Security**: room.json and config.toml use 0600 permissions. Room secret is 32 random bytes (64 hex chars).
9. **Context window protection**: MCP `read_chat` offloads large conversations to `.syncvibe/chat-digest.md` instead of returning them inline. Tool response stays bounded (~3 lines for large chats), agent reads the file at its own discretion.
