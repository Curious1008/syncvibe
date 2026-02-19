# SyncVibe

**Terminal-native coordination for vibe coding teams. Zero AI costs.**

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-macOS%20%7C%20Linux-lightgrey.svg)]()
[![Website](https://img.shields.io/badge/Web-syncvibe.online-teal.svg)](https://syncvibe.online)
[![Discord](https://img.shields.io/badge/Discord-Join%20Community-5865F2.svg?logo=discord&logoColor=white)](https://discord.gg/Nb3wkCBZ55)

SyncVibe is a real-time terminal chat and coordination layer for teams doing vibe coding with AI agents. It connects teammates and their AI agents (Claude Code, Codex, or any MCP-compatible agent) in a shared chat room — without making a single LLM API call.

```
┌─────────────────────────────────────────────────────────────┐
│                      tmux session                           │
│  ┌────────────────────┐     ┌────────────────────────────┐  │
│  │  SyncVibe Chat     │     │  AI Agent (Claude/Codex)   │  │
│  │  (30%)             │     │  (70%)                     │  │
│  │                    │     │                            │  │
│  │  @alice refactor   │     │  I've read the team chat   │  │
│  │  the auth module   │────►│  — you agreed on splitting │  │
│  │                    │     │  auth into middleware...    │  │
│  │  @bob sounds good  │     │                            │  │
│  └────────────────────┘     └────────────────────────────┘  │
│                     Ctrl+G to switch                        │
└─────────────────────────────────────────────────────────────┘
```

---

## Install

```bash
curl -fsSL https://syncvibe.online/install.sh | sh
```

Supports **macOS** (universal) and **Linux** (x86_64, aarch64). Requires `tmux`.

---

## Quick Start

**1. Create a room**

```bash
syncvibe
```

Follow the interactive onboarding — set your name, pick an AI agent, and create a room.

**2. Share the invite**

```bash
syncvibe invite
```

Generates a short invite code (e.g. `HKPT-3NWV`) — send it to your team.

**3. Collaborate**

```bash
syncvibe
# Paste the invite code when prompted
```

Teammates join the room, chat syncs in real time, and the AI agent auto-configures via MCP.

---

## Features

### Real-time Chat
- Terminal TUI with live presence indicators
- @mention with tab completion and bell notifications
- Image sharing (drag file paths into chat)
- Message grouping by user, chat history with scroll-back

### AI Agent Integration
- **Zero config** — pick Claude Code or Codex from a menu; SyncVibe wires up `.mcp.json`, `CLAUDE.md`, and hooks automatically
- **MCP `read_chat` tool** — agents read the team discussion with smart incremental filtering, session scoping, and digest offloading for large conversations
- **@agent** — send tasks to your AI agent directly from chat, no pane switching needed
- Each teammate picks their own agent; all agents share the same chat context

### Screen Sharing
- `/share` — toggle sharing your agent pane with teammates
- `/watch <name>` — view a teammate's shared agent screen in real time
- Delta-encoded frames over WebSocket for efficient bandwidth

### Invite Codes
- Short codes (`HKPT-3NWV`) auto-copied to clipboard
- Paste to join — one step, no URLs or config files
- Clipboard auto-detection: if an invite code is on your clipboard, SyncVibe asks to join on launch

### tmux Integration
- Auto-creates split layout: SyncVibe Chat (30%) | AI Agent (70%)
- `Ctrl+G` to switch between panes
- Styled pane borders and status bar
- Project switching between tmux sessions via `/chats`

### Zero AI Costs
- Pure coordination layer — no LLM API calls, no token costs
- All AI costs stay with whatever agent each person already uses
- Local-first: all state lives in `.syncvibe/`, WebSocket relay is for real-time sync only

---

## Commands

### CLI

| Command | Description |
|---------|-------------|
| `syncvibe` | Launch interactive mode — create/join rooms, open TUI |
| `syncvibe invite` | Generate an invite code for the current room |
| `syncvibe connect <code>` | Join a room with an invite code |
| `syncvibe profile` | Edit your display name and color |
| `syncvibe auth` | Authenticate CLI with your SyncVibe web account |
| `syncvibe status` | Show current room status |
| `syncvibe switch` | Switch between SyncVibe rooms |
| `syncvibe leave` | Leave the current room |

### TUI Slash Commands

| Command | Alias | Description |
|---------|-------|-------------|
| `/help` | `/?` | Show all commands |
| `/invite` | `/i` | Show room invite code |
| `/new` | `/n` | Create a new room |
| `/join` | `/j` | Join with invite code |
| `/chats` | | Switch between rooms |
| `/share` | | Toggle agent screen sharing |
| `/watch` | | View a teammate's shared screen |
| `/name <n>` | | Change display name |
| `/color <#hex>` | | Change your color |
| `/mute` | `/m` | Toggle @mention bell |
| `/clear` | | Clear chat view |
| `/rc` | | Reconnect to relay |
| `/leave` | | Leave current room |
| `/quit` | `/q` | Exit SyncVibe |

### Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `Ctrl+G` | Switch between Chat and Agent pane |
| `Tab` | Autocomplete @mentions and slash commands |
| `↑` / `↓` | Navigate autocomplete suggestions |
| `Enter` | Send message or confirm autocomplete |
| `PageUp` / `PageDown` | Scroll chat history |

---

## How It Works

### MCP Integration

[Model Context Protocol (MCP)](https://modelcontextprotocol.io/) is an open standard that lets AI agents interact with external tools. SyncVibe uses MCP to give agents read access to the team chat.

When you create or join a room, SyncVibe automatically:

1. **`.mcp.json`** — registers the SyncVibe MCP server with your AI agent
2. **`CLAUDE.md`** — instructs the agent to call `read_chat` before any task
3. **`.claude/settings.json`** — configures hooks to notify the TUI when the agent modifies files

| MCP Tool | Description |
|----------|-------------|
| `read_chat` | Smart incremental read with session scoping, time filtering, and digest offloading for large conversations |

The agent sends messages by appending to `.syncvibe/chat-log.jsonl` directly — no MCP tool needed.

### Data Flow

```
Human ↔ Human:   TUI → .syncvibe/ → WebSocket relay → Other TUI
Human → Agent:   TUI → .syncvibe/ → Agent reads via MCP read_chat
Agent → Human:   Agent appends to .syncvibe/ → Hook → TUI re-renders
Agent ↔ Agent:   Agent A writes → Agent B reads (incremental)
```

### Local-First Architecture

All state lives in `.syncvibe/` inside your project:

```
.syncvibe/
├── room.json            # Room config (room_id, room_secret, relay_url)
├── chat-log.jsonl       # Append-only chat log, one JSON per line
├── chat-digest.md       # Auto-generated summary for large conversations
└── images/              # Shared images (UUID-named)
```

`.syncvibe/` is gitignored. The WebSocket relay provides real-time sync; local files are the source of truth.

---

## Project Structure

```
syncvibe/
├── Cargo.toml                     # Workspace root
├── crates/
│   ├── syncvibe-cli/              # Main binary
│   │   └── src/
│   │       ├── main.rs            # Entry point + clap dispatch
│   │       ├── app.rs             # TUI event loop + slash commands
│   │       ├── session.rs         # Interactive onboarding
│   │       ├── init.rs            # Room init (.syncvibe/, .mcp.json, CLAUDE.md)
│   │       ├── tmux.rs            # tmux session management + layout
│   │       ├── cli.rs             # Clap command definitions
│   │       ├── config.rs          # ~/.syncvibe/ config + project registry
│   │       ├── onboarding.rs      # Interactive prompts + validation
│   │       ├── invite.rs          # Invite code generation + resolution
│   │       ├── auth.rs            # CLI ↔ web authentication
│   │       ├── agents.rs          # AI agent selection
│   │       ├── picker.rs          # Room switcher (ratatui)
│   │       ├── sync.rs            # File sync utilities
│   │       ├── tui.rs             # Terminal setup/teardown
│   │       ├── components/        # TUI rendering (ratatui)
│   │       │   ├── status_bar.rs  # Presence bar + toast notifications
│   │       │   ├── chat.rs        # Chat message display
│   │       │   ├── input.rs       # Input bar with cursor + autocomplete
│   │       │   └── util.rs        # Color parsing helpers
│   │       ├── network/
│   │       │   └── ws_client.rs   # WebSocket client (tokio-tungstenite)
│   │       ├── mcp/
│   │       │   └── server.rs      # MCP server: read_chat
│   │       └── git/
│   │           └── ops.rs         # Git repo name detection
│   │
│   └── syncvibe-core/             # Shared library
│       └── src/
│           ├── lib.rs
│           ├── error.rs           # Error types
│           ├── models/            # Data types (chat, room, user)
│           ├── protocol.rs        # WebSocket message types
│           └── storage.rs         # File I/O (atomic writes, locking)
│
└── install.sh                     # One-line installer script
```

---

## Development

```bash
git clone https://github.com/Curious1008/syncvibe.git
cd syncvibe
cargo build --release
./target/release/syncvibe
```

### Requirements

- Rust 1.75+
- tmux 3.0+

---

## Contributing

Contributions welcome!

1. Fork the repo
2. Create a feature branch (`git checkout -b feature/amazing`)
3. Commit your changes
4. Push and open a Pull Request

---

## License

MIT License. See [LICENSE](LICENSE) for details.
