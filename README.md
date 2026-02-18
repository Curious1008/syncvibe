# SyncVibe

**Terminal-native coordination for vibe coding teams. Zero AI costs.**

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-macOS%20%7C%20Linux-lightgrey.svg)]()
[![Website](https://img.shields.io/badge/Web-syncvibe.online-teal.svg)](https://syncvibe.online)

SyncVibe is a real-time terminal chat and coordination layer for teams doing vibe coding with AI agents. It connects teammates and their AI agents (Claude Code, Codex, or any MCP-compatible agent) in a shared chat room — without making a single LLM API call. All AI costs stay with whatever agent each person already uses.

---

## Demo

<!-- Replace PLACEHOLDER with your actual asciinema recording ID -->
[![asciicast](https://asciinema.org/a/PLACEHOLDER.svg)](https://asciinema.org/a/PLACEHOLDER)

---

## Features

- **Real-time terminal chat with presence** — see who's online, get @mention notifications
- **Multi-agent MCP integration** — Claude Code, Codex, or any MCP-compatible agent
- **Invite codes** — share a `syncvibe://` link and teammates join instantly
- **tmux auto-split layout** — Chat (30%) | Agent (70%), switch panes with Ctrl+G
- **Zero AI costs** — pure coordination layer, no LLM API calls
- **Per-user agent selection** — each teammate picks their own agent

---

## Quick Start

```bash
# Install
curl -fsSL https://syncvibe.online/install.sh | sh
```

**1. Create a room**

```bash
syncvibe
# Follow the interactive onboarding: set your name, create a new room
```

**2. Share the invite**

```bash
syncvibe invite
# Generates a syncvibe:// invite code — send it to your team
```

**3. Collaborate**

```bash
syncvibe connect <invite-code>
# Teammate joins the room, chat syncs in real time, agents auto-configure
```

---

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    tmux session                          │
│  ┌──────────────────┐     ┌──────────────────────────┐  │
│  │  SyncVibe Chat   │     │  AI Agent (Claude/Codex) │  │
│  │  (TUI - 30%)     │◄───►│  (70%)                   │  │
│  │                   │     │                          │  │
│  │  Real-time chat   │     │  Reads chat via MCP      │  │
│  │  @mentions        │     │  Responds via send_chat  │  │
│  │  Presence         │     │  Auto-configured         │  │
│  └────────┬─────────┘     └────────────┬─────────────┘  │
│           │         Ctrl+G             │                 │
└───────────┼────────────────────────────┼─────────────────┘
            │                            │
            ▼                            ▼
     ┌──────────────┐          ┌──────────────────┐
     │  .syncvibe/  │          │  .mcp.json       │
     │  room.json   │          │  MCP server      │
     │  chat-log    │◄────────►│  read_chat       │
     │  images/     │          │  send_chat       │
     └──────┬───────┘          └──────────────────┘
            │
            ▼
     ┌──────────────────┐
     │  WebSocket Relay  │
     │  (Cloudflare DO)  │
     │  Real-time sync   │
     └──────────────────┘
```

**Data flow**: Users chat in the TUI. Messages are stored locally in `.syncvibe/` and synced via WebSocket relay. AI agents read the chat through MCP (or direct file access) and respond by appending to the chat log. The TUI picks up agent messages through a file watcher and renders them in real time.

---

## MCP Integration

[Model Context Protocol (MCP)](https://modelcontextprotocol.io/) is an open standard that lets AI agents interact with external tools. SyncVibe uses MCP to give agents access to the team chat.

### Tools

| Tool | Description |
|------|-------------|
| `read_chat` | Smart incremental read with session scoping, time filtering, and digest offloading for large conversations |
| `send_chat` | Append a message to the chat log as the agent |

### How It Works

When you initialize a SyncVibe room (`syncvibe` or `syncvibe connect`), it automatically:

1. Creates `.mcp.json` in your project root — registers the SyncVibe MCP server
2. Injects instructions into `CLAUDE.md` — tells the agent to call `read_chat` before any task
3. Configures hooks in `.claude/settings.json` — notifies the TUI when the agent modifies files

### Example Flow

```
1. You type in SyncVibe chat: "We need to refactor the auth module"
2. Teammates discuss the approach in real time
3. You switch to the agent pane (Ctrl+G) and say: "Refactor auth as discussed"
4. Agent calls read_chat (MCP) → sees the full team discussion
5. Agent responds: "I've read the chat — you agreed on splitting auth into middleware..."
6. Agent proceeds with full context of the team's decisions
```

### Multi-Agent Support

Each teammate picks their own AI agent. All agents share the same chat log:

- Alice uses Claude Code — her agent reads chat via MCP `read_chat`
- Bob uses Codex — his agent reads `.syncvibe/chat-log.jsonl` directly
- Both agents can see each other's messages and the full team discussion

---

## Crate Structure

```
syncvibe/
├── Cargo.toml                     # Workspace root
├── crates/
│   ├── syncvibe-cli/              # Main binary
│   │   └── src/
│   │       ├── main.rs            # Entry point, clap CLI dispatch
│   │       ├── init.rs            # Room init (.syncvibe/, .mcp.json, CLAUDE.md)
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
│   │       │   └── server.rs      # MCP server: read_chat + send_chat
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
```

---

## Commands

| Command | Description |
|---------|-------------|
| `syncvibe` | Launch interactive mode (create/join room) |
| `syncvibe invite` | Generate an invite code for the current room |
| `syncvibe connect <code>` | Join a room with an invite code |
| `syncvibe profile` | Edit your display name and color |
| `syncvibe auth` | Authenticate with the SyncVibe relay |
| `syncvibe mcp-server` | Start the MCP server (used by agents, not typically run manually) |

---

## TUI Shortcuts

| Key | Action |
|-----|--------|
| `Ctrl+G` | Switch between Chat and Agent pane |
| `/help` | Show all slash commands |
| `@agent` | Send a task to any connected agent |
| `@claude` / `@codex` | Target a specific agent |

### Slash Commands

| Command | Alias | Description |
|---------|-------|-------------|
| `/help` | `/?` | Show help |
| `/invite` | `/i` | Generate invite code |
| `/projects` | `/p` | Switch projects |
| `/name` | | Change display name |
| `/color` | | Change display color |
| `/mute` | `/m` | Toggle notifications |
| `/clear` | | Clear chat display |
| `/rc` | | Reconnect to relay |
| `/quit` | `/q` | Exit SyncVibe |

---

## Contributing

Contributions welcome! Please:

1. Fork the repo
2. Create a feature branch (`git checkout -b feature/amazing`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing`)
5. Open a Pull Request

### Development

```bash
# Clone
git clone https://github.com/Curious1008/syncvibe.git
cd syncvibe

# Build
cargo build --release

# Run
./target/release/syncvibe
```

### Project Structure

- `crates/syncvibe-cli/` — Main binary (TUI, tmux, MCP, onboarding)
- `crates/syncvibe-core/` — Shared library (models, storage, protocol)

---

## License

MIT License. See [LICENSE](LICENSE) for details.
