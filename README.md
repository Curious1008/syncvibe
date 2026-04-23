# SyncVibe

**Teach someone how to use Claude Code over the shoulder, from the other side of the world — without touching their keyboard.**

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-macOS%20%7C%20Linux-lightgrey.svg)]()
[![Website](https://img.shields.io/badge/Web-syncvibe.online-teal.svg)](https://syncvibe.online)
[![Discord](https://img.shields.io/badge/Discord-Join%20Community-5865F2.svg?logo=discord&logoColor=white)](https://discord.gg/Nb3wkCBZ55)

SyncVibe is a remote pair-teaching tool for terminal-based AI coding agents — Claude Code, Codex CLI, Gemini CLI, or anything MCP-compatible. The teacher stays on their own machine. The learner stays on theirs. Both join a room. The teacher drives the **learner's own agent** through chat, and that agent runs locally on the learner's machine, with the learner's auth, against the learner's repo.

> [Watch the demo](https://github.com/Curious1008/syncvibe/releases/download/v0.4.3/SyncVibe-Demo.mp4) — two developers collaborating with their AI agents (Claude + Codex) in real time.

---

## Why SyncVibe exists

Teaching someone to use Claude Code / Codex / Gemini CLI today means one of:

1. **Zoom screen share.** You see the teacher's screen. You type from memory. You make mistakes. Nothing sticks.
2. **Zoom remote control.** Teacher drives your machine. Security prompt every time. You watch passively — the prompts never ran on your machine, so you never internalized them.
3. **tmate / shared tmux.** No chat layer, no per-user scoping, no agent awareness.
4. **Type-it-yourself coaching.** "Now type: `claude 'refactor auth.ts to…'`" — slow, error-prone, context dies on copy-paste.

SyncVibe collapses this into one room: chat, terminal view, and agent trigger, all co-located. The teacher types `@alice-claude refactor the auth token check` in chat. Alice's Claude runs locally, on Alice's keyboard, against Alice's repo. Alice watches the **prompt → agent behavior → code change** loop in her own environment. That's what makes it stick.

---

## What SyncVibe is *not*

Hard lines — if a feature request crosses one of these, it belongs in a different product:

- **Not IRC / a chat tool.** Chat is the substrate, not the product. The product is the agent-trigger loop riding on top.
- **Not Cursor multiplayer / Live Share.** Each participant's workspace stays local. No shared filesystem, no shared git state, no file stomping.
- **Not remote desktop / TeamViewer.** The teacher never touches the learner's keyboard. Every action is mediated by the learner's agent, which the learner can pause or Ctrl-C at any moment.
- **Not a Slack / Discord replacement.** Keep using those for general team chat. Open SyncVibe for teaching sessions.

---

## Install

```bash
curl -fsSL https://syncvibe.online/install.sh | sh
```

Or via **Homebrew** (macOS):

```bash
brew tap Curious1008/syncvibe
brew install syncvibe
```

Supports **macOS** (Apple Silicon + Intel) and **Linux** (x86_64, aarch64).

---

## Quick Start

**1. Create a room**

```bash
syncvibe
```

Interactive onboarding — pick your name, choose your agent (Claude, Codex, or Gemini), create a room.

**2. Invite your learner**

Type `/invite` — a short code like `HKPT-3NWV` is copied to your clipboard. Send it to them.

**3. Learner joins**

```bash
syncvibe connect HKPT-3NWV
```

Chat syncs in real time. If the room has a linked repo, it auto-clones on connect. The learner's agent auto-configures via MCP — no manual setup.

**4. Teach**

- `/watch alice` — watch Alice's agent pane live.
- `@alice-claude refactor auth.rs to use JWT` — trigger Alice's agent with a concrete prompt. Alice sees her Claude run locally.
- When names collide, the TUI auto-appends a short suffix: `@claude(Alice#7af)` vs `@claude(Alice#b2c)`.

---

## How It Works

```
┌─────────────────────────────────────────────────────────────────┐
│  Split Terminal                                                 │
│  ┌─────────────────────┐    ┌────────────────────────────────┐  │
│  │  SyncVibe Chat (30%)│    │  AI Agent — Claude/Codex/Gemini│  │
│  │                     │    │                                │  │
│  │  Harry: @alice-     │    │  Reading team chat via MCP...  │  │
│  │    claude refactor  │───►│  ⚡ Task: refactor auth.rs      │  │
│  │    auth.rs          │    │  Editing src/auth.rs...        │  │
│  │  Alice's-Claude: ✓  │◄───│  Done — reporting to chat      │  │
│  └─────────────────────┘    └────────────────────────────────┘  │
│                    Ctrl+G to switch                             │
└─────────────────────────────────────────────────────────────────┘
```

**Data flow:**
- **Human ↔ Human:** TUI → WebSocket relay → other TUI
- **Teacher → Learner's agent:** `@alice-claude` message → Alice's local Claude reads via MCP `read_chat` and executes on Alice's machine
- **Agent → Everyone:** agent calls MCP `send_chat` → broadcasts to all teammates

All state lives locally in `.syncvibe/`. The relay only handles real-time sync — no messages are stored server-side.

---

## Features

### Chat, tuned for teaching

- Real-time presence, @mention with tab completion, bell notifications
- Message grouping, scroll-back, drag-to-paste images
- Two-row status bar: brand/version/online on top, agents/users/me on the bottom. Collapses gracefully on narrow terminals.
- tmux pane titles carry the **room name**, so the brand is stamped once not twice.

### Agent triggering

- **Pick Claude Code, Codex, or Gemini** from a menu — SyncVibe auto-configures `.mcp.json`, `.codex/config.toml`, and `.gemini/settings.json`.
- **MCP tools** — `read_chat` with incremental reads, session scoping, and digest offloading; `send_chat` for agent-to-human messages.
- **`@agent`** — mention your own AI to assign tasks. Agent auto-reads chat for full context.
- **Cross-machine triggering** — `@alice-claude` from a teammate triggers Alice's local Claude via tmux `send-keys` (30s debounce to prevent loops).
- **Disambiguation** — ambiguous mentions require an owner: `@claude(Alice)`. Username collisions get an auto-suffix: `@claude(Alice#7af)` vs `@claude(Alice#b2c)`. Tab-completion shows the right form.
- **Broadcast** — agent responses sync to all teammates in real time.

### Screen sharing

- `/share` — toggle sharing your agent pane
- `/watch <name>` — view a teammate's agent screen live
- Delta-encoded frames for efficient bandwidth

### Git integration

- Auto-detects your git remote on room creation, or prompts for one (optional).
- Learner joins → repo auto-clones. One step.
- `CLAUDE.md` / `AGENTS.md` instruct agents to commit & push after tasks.
- `/remote` — set or show git remote
- `/collab` — open GitHub collaborator settings

### Invite codes

- Short codes (`HKPT-3NWV`) auto-copied to clipboard
- Paste to join — one step, no URLs, no config files
- Clipboard auto-detection on launch

### Split terminal

- Auto-creates side-by-side layout: Chat (30%) | Agent (70%)
- `Ctrl+G` to switch panes
- `/chats` to switch between room sessions
- **Ctrl+C twice** to exit (first press shows a banner; two presses within 2s quits)

### Zero AI costs

- SyncVibe is a coordination layer, not an AI provider. No LLM API calls, no token costs.
- All AI costs stay with whatever agent each person already pays for.

---

## Commands

### CLI

| Command | Description |
|---------|-------------|
| `syncvibe` | Launch — create/join rooms, open TUI |
| `syncvibe invite` | Show invite code for current room |
| `syncvibe connect <code>` | Join a room with an invite code |
| `syncvibe profile` | Edit display name and color |
| `syncvibe chat "<msg>"` | Send a message without opening the TUI |
| `syncvibe auth` | Link CLI to your web account at syncvibe.online |
| `syncvibe status` | Show current room info |
| `syncvibe switch` | Switch between rooms |
| `syncvibe leave` | Leave current room |
| `syncvibe completions` | Generate shell completions (bash/zsh/fish) |

### TUI slash commands

| Command | Alias | Description |
|---------|-------|-------------|
| `/help` | `/?` | Show all commands |
| `/invite` | `/i` | Copy invite code |
| `/new` | `/n` | Create a new room |
| `/join` | `/j` | Join with invite code |
| `/chats` | | Switch between rooms |
| `/share` | | Toggle agent screen sharing |
| `/watch <name>` | | Watch a teammate's screen |
| `/name <n>` | | Change display name |
| `/color <#hex>` | | Change color |
| `/remote` | | Set or show git remote |
| `/collab` | | Manage repo collaborators |
| `/mute` | `/m` | Toggle @mention bell |
| `/clear` | | Clear chat view |
| `/rc` | | Reconnect to relay |
| `/leave` | | Leave room |
| `/quit` | `/q` | Exit |

### Keyboard shortcuts

| Key | Action |
|-----|--------|
| `Ctrl+G` | Switch Chat ↔ Agent pane |
| `Ctrl+C` | First press: "press again to exit" banner · Second press within 2s: quit |
| `Tab` | Autocomplete @mentions and commands |
| `↑` / `↓` | Select messages (quote, open images) |
| `Enter` | Send message / quote selected / open image |
| `PageUp` / `PageDown` | Scroll chat history |
| `Mouse scroll` | Scroll chat panel |
| `Esc` | Deselect message, cancel prompt, return to bottom |

---

## MCP Integration

[Model Context Protocol (MCP)](https://modelcontextprotocol.io/) lets AI agents interact with external tools. SyncVibe uses MCP to give agents access to the team chat.

Room setup auto-generates:

| File | Purpose |
|------|---------|
| `.mcp.json` | MCP server config for Claude Code |
| `.codex/config.toml` | MCP server config for Codex CLI |
| `.gemini/settings.json` | MCP server config for Gemini CLI |
| `CLAUDE.md` | Instructs Claude to use chat tools |
| `AGENTS.md` | Instructs Codex/Gemini/other agents to use chat tools |

| MCP Tool | Description |
|----------|-------------|
| `read_chat` | Incremental read with session scoping, time filtering, @agent task extraction, and digest offloading |
| `send_chat` | Send a message to the shared chat as the AI agent |

---

## Local-First Architecture

All room state lives in `.syncvibe/` inside your project:

```
.syncvibe/
├── room.json          # Room identity (room_id, secret, relay_url, git_remote)
├── chat-log.jsonl     # Append-only chat, one JSON per line
├── chat-digest.md     # Auto-generated digest for large conversations
└── images/            # Shared images (UUID-named)
```

`.syncvibe/` is gitignored. The WebSocket relay provides real-time sync only — no messages stored server-side.

For full architectural detail, see [ARCHITECTURE.md](ARCHITECTURE.md).

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
- tmux 3.0+ (auto-installed on first run if missing)

### Tests

```bash
cargo test                    # Unit + integration tests (115 CLI, 13 audit, 6 relay)
cargo test -- --ignored       # Include relay deployment tests
```

### Shell completions

```bash
syncvibe completions bash > ~/.local/share/bash-completion/completions/syncvibe
syncvibe completions zsh  > ~/.zfunc/_syncvibe
syncvibe completions fish > ~/.config/fish/completions/syncvibe.fish
```

---

## Project Structure

```
syncvibe/
├── crates/
│   ├── syncvibe-cli/              # TUI binary
│   │   └── src/
│   │       ├── cli.rs             # Command definitions (Clap)
│   │       ├── app.rs             # Event loop, dispatches to commands::
│   │       ├── render.rs          # Top-level TUI rendering
│   │       ├── theme.rs           # Single source of truth for colors
│   │       ├── tui.rs             # Terminal UI bootstrap
│   │       ├── onboarding.rs      # Interactive setup wizard
│   │       ├── auth.rs            # Web account linking
│   │       ├── config.rs          # Configuration management
│   │       ├── invite.rs          # Invite code logic
│   │       ├── agents.rs          # Agent config + @mention parsing
│   │       ├── sync.rs            # Chat sync engine
│   │       ├── picker.rs          # Room picker
│   │       ├── init.rs            # Room init (MCP, CLAUDE.md, agent configs)
│   │       ├── tmux.rs            # tmux session management + pane titles
│   │       ├── commands/          # Slash-command registry (one file per /cmd)
│   │       ├── flows/             # Onboarding + room-join flows
│   │       ├── events/            # Key, mouse, and WebSocket message handlers
│   │       ├── git/               # Git integration
│   │       ├── mcp/               # MCP server: read_chat, send_chat
│   │       ├── network/           # WebSocket client
│   │       └── components/        # TUI widgets (chat, status bar, autocomplete, …)
│   │
│   └── syncvibe-core/             # Shared library
│       └── src/
│           ├── protocol.rs        # WebSocket message types
│           ├── storage.rs         # Atomic file I/O
│           └── models/            # Chat, room, user types
│
└── install.sh                     # One-line installer
```

---

## Security

- Install script verifies binary integrity via **SHA256 checksums**
- GitHub Actions workflows are **pinned to commit SHAs**
- `cargo audit` runs in CI to catch known vulnerabilities
- **TLS enforced** — all relay connections use WSS; plaintext rejected
- **No message storage** — the relay forwards chat, screen shares, and MCP traffic in real time; nothing is logged or persisted
- Room secrets authenticate your connection over TLS — stored server-side only for reconnection support
- Invite codes expire automatically after 7 days
- Local files use `0600` permissions (owner-only read/write)

For full details, see [Data & Privacy](https://syncvibe.online/docs/data-privacy).

---

## Roadmap

What's coming — every item passes the teaching-loop test (does this make teaching faster, terminal-first, workspace-local, inside the chat + agent-trigger model?):

- **Session recording + replay** — play back a teaching session: chat, agent prompts, and agent output as a single artifact.
- **tmate substrate integration** — adopt tmate's battle-tested multi-user tmux pairing under the hood; keep SyncVibe's chat + agent-trigger overlay.
- **IRC gateway** — power users connect via weechat / irssi / HexChat. SyncVibe speaks IRCv3 natively.
- **End-to-end encryption** — message content encrypted client-side so the relay can't read it.
- **Windows support** — native binary + PowerShell / Windows Terminal integration.
- **Managed relay for teams** — dedicated relay instances with custom domains and SLA.

Have an idea? [Open an issue](https://github.com/Curious1008/syncvibe/issues) or [join the Discord](https://discord.gg/Nb3wkCBZ55).

---

## Contributing

Contributions welcome!

1. Fork the repo
2. Create a feature branch (`git checkout -b feature/amazing`)
3. Commit your changes
4. Open a Pull Request

---

## License

MIT License. See [LICENSE](LICENSE) for details.
