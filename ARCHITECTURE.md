# SyncVibe Architecture

Terminal-native collaboration tool for multi-person vibe coding.

## Design Philosophy

SyncVibe is a **coordination layer, not an AI layer**. Zero LLM API calls, zero token costs. It leverages existing AI agent capabilities (Claude Code's native file reading, task system, and hooks) rather than duplicating them.

**Key principle**: Only build MCP tools for capabilities that agents can't do natively. Everything else uses direct file access guided by CLAUDE.md instructions.

## Crate Structure

```
syncvibe/
├── Cargo.toml                     # Workspace root
├── crates/
│   ├── syncvibe-cli/              # Main binary
│   │   └── src/
│   │       ├── main.rs            # Entry point, clap CLI dispatch
│   │       ├── app.rs             # TUI app state + event loop
│   │       ├── cli.rs             # Clap command definitions
│   │       ├── config.rs          # ~/.syncvibe/config.toml management
│   │       ├── tui.rs             # Terminal setup/teardown (crossterm)
│   │       ├── components/        # TUI panels (ratatui)
│   │       │   ├── status_bar.rs  # Top bar: project name + presence
│   │       │   ├── chat.rs        # Chat message display
│   │       │   ├── input.rs       # Input bar with cursor
│   │       │   └── util.rs        # Shared helpers
│   │       ├── network/
│   │       │   ├── ws_client.rs   # WebSocket client (tokio-tungstenite)
│   │       │   └── sync.rs        # (future: git sync)
│   │       ├── mcp/
│   │       │   └── server.rs      # MCP server: 3 tools, 1 resource
│   │       └── git/
│   │           └── ops.rs         # Git operations (branch, commits, conflicts)
│   │
│   └── syncvibe-core/             # Shared library
│       └── src/
│           ├── lib.rs
│           ├── error.rs           # SyncVibeError enum
│           ├── models/            # Data types (chat, plan, room, user)
│           ├── protocol.rs        # WsMessage enum (WebSocket types)
│           └── storage.rs         # .syncvibe/ file I/O (atomic writes)
│
└── worker/                        # Cloudflare Worker (TypeScript)
    └── src/
        ├── index.ts               # HTTP routing → Durable Object
        ├── room.ts                # Durable Object: WS relay + presence
        └── types.ts
```

## AI Agent Integration Strategy

### What agents do natively (no MCP needed)
- **Read chat**: `Read .syncvibe/chat-log.jsonl` — native file reading
- **Send chat**: Append JSONL line to `.syncvibe/chat-log.jsonl` — native file writing
- **Read plan**: `Read .syncvibe/plan.md` — native file reading

All guided by CLAUDE.md instructions injected at `syncvibe init`.

### What MCP adds (smart filtering only agents can't do natively)
| MCP Tool | Why it exists |
|---|---|
| `read_plan` | Returns plan + metadata (who edited, when, version) |
| `update_plan` | Writes plan + updates metadata atomically |
| `read_chat` | **Smart filtering**: session scoping, incremental reads, time-based — not possible with raw file read |

### What Claude Code provides natively
| Capability | How SyncVibe leverages it |
|---|---|
| CLAUDE.md | `syncvibe init` injects collaboration instructions |
| .mcp.json | `syncvibe init` generates MCP server config |
| File Read/Write/Edit | Agents read/write `.syncvibe/` files directly |
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
Agent A → Updates plan.md → Agent B reads via MCP read_plan
```

## .syncvibe/ Directory

```
.syncvibe/
├── room.json            # Room config (relay URL, secret)
├── plan.md              # Shared plan (raw markdown)
├── plan-meta.json       # Who edited, when, version
└── chat-log.jsonl       # Append-only, one JSON per line
```

All files committed to git = primary persistence + sync.

## Key Design Decisions

1. **Git as source of truth**: All state in `.syncvibe/`, committed to repo. WebSocket relay is ephemeral.
2. **JSONL for chat**: Append-only, git-friendly (line-based diffs), no merge conflicts.
3. **Atomic file writes**: Write to `.tmp`, then rename. Prevents partial reads.
4. **3 MCP tools, not 7**: Only tools that add value beyond native file access. Chat sending handled via direct file append.
5. **Session auto-segmentation**: 30min silence → new session ID. MCP `read_chat` defaults to current session.
6. **Offline-first**: TUI works fully with local files. WebSocket is optional.
7. **Hooks integration**: `PostToolUse` hook signals TUI when agents modify `.syncvibe/` files.
