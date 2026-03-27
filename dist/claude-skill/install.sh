#!/bin/bash
# Install SyncVibe skill for Claude Code
# Usage: curl -fsSL https://syncvibe.online/skill-install.sh | sh

set -e

SKILL_DIR="$HOME/.claude/skills/syncvibe"

# Check if syncvibe CLI is installed
if ! command -v syncvibe &>/dev/null; then
    echo "SyncVibe CLI not found. Installing..."
    if command -v brew &>/dev/null; then
        brew tap Curious1008/syncvibe && brew install syncvibe
    else
        curl -fsSL https://syncvibe.online/install.sh | sh
    fi
fi

# Create skill directory
mkdir -p "$SKILL_DIR"

# Write skill file
cat > "$SKILL_DIR/SKILL.md" << 'SKILL_EOF'
---
name: syncvibe
description: Multiplayer vibe coding -- collaborate with teammates and their AI agents in a shared chat room. Use when the user wants to work with others, share context, or coordinate tasks in real-time.
---

# SyncVibe -- Multiplayer for Your AI Coding Agent

You have access to SyncVibe's MCP tools for real-time team collaboration.
Your teammates (humans and their AI agents) are in the same chat room.

## Available MCP Tools

- **read_chat** -- Read team messages. Supports incremental reads, filtering by timestamp, and compact/json formats.
- **send_chat** -- Send a message to the team chat. All humans and agents in the room see it instantly.

## How to Collaborate

1. **Check for new messages** before starting work: call `read_chat`.
2. **Announce what you're doing**: call `send_chat` to let the team know.
3. **Respond to @mentions**: if someone tags you (e.g., @claude), they're assigning you a task.
4. **Share results**: when you finish a task, send a brief summary via `send_chat`.
5. **Coordinate, don't collide**: check if someone else is working on the same file.

## Setup (if MCP tools aren't available)

```bash
# Install: brew install syncvibe (or curl -fsSL https://syncvibe.online/install.sh | sh)
# Register MCP: claude mcp add syncvibe -- syncvibe mcp-server
# Create room: syncvibe
# Join room: syncvibe connect XXXX-YYYY
```
SKILL_EOF

# Register MCP server with Claude Code
if command -v claude &>/dev/null; then
    claude mcp add syncvibe -- syncvibe mcp-server 2>/dev/null || true
    echo "MCP server registered with Claude Code."
fi

echo ""
echo "SyncVibe skill installed to $SKILL_DIR"
echo ""
echo "Next steps:"
echo "  1. Run 'syncvibe' to create a room"
echo "  2. Share the invite code with a friend"
echo "  3. Start vibe coding together!"
echo ""
