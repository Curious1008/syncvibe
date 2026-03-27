# Add Multiplayer to Your AI Coding Agent

SyncVibe adds real-time multiplayer collaboration to Claude Code, Codex, and Gemini CLI. Your AI agent joins a shared chat room with your teammates and their agents.

## Quick Setup

### 1. Install SyncVibe

```bash
brew tap Curious1008/syncvibe && brew install syncvibe
```

Or without Homebrew:
```bash
curl -fsSL https://syncvibe.online/install.sh | sh
```

### 2. Register the MCP Server

**Claude Code:**
```bash
claude mcp add syncvibe -- syncvibe mcp-server
```

**Codex:**
Add to `.codex/config.toml`:
```toml
[mcp_servers.syncvibe]
command = "syncvibe"
args = ["mcp-server"]
```

**Gemini CLI:**
Add to `.gemini/settings.json`:
```json
{
  "mcpServers": {
    "syncvibe": {
      "command": "syncvibe",
      "args": ["mcp-server"]
    }
  }
}
```

### 3. Create or Join a Room

```bash
# Create a new room
syncvibe

# Or join someone else's room
syncvibe connect XXXX-YYYY
```

That's it. Your AI agent now has `read_chat` and `send_chat` tools. It can see team messages, respond to @mentions, and coordinate work in real-time.

## What Your Agent Can Do

- **read_chat** -- See what the team is discussing (supports incremental reads)
- **send_chat** -- Send messages to the team chat
- **Respond to @mentions** -- When a teammate says "@claude do X", your agent sees it as a task

## How It Works

```
You (Terminal A)              Your Friend (Terminal B)
┌──────────────┐              ┌──────────────┐
│ Claude Code  │              │ Codex CLI    │
│  + SyncVibe  │◄── relay ──►│  + SyncVibe  │
│  MCP server  │              │  MCP server  │
└──────────────┘              └──────────────┘
       │                             │
       └──── shared chat room ───────┘
         humans + agents together
```

Zero LLM API costs. The relay only handles message sync -- no AI calls.

## Links

- Website: https://syncvibe.online
- GitHub: https://github.com/Curious1008/syncvibe
- Discord: https://discord.gg/Nb3wkCBZ55
