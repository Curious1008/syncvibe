## SyncVibe Collaboration

This project uses SyncVibe for team coordination. All shared state lives in `.syncvibe/`.

### Before starting work
- Read `.syncvibe/chat-log.jsonl` (last 20 lines) for recent team discussions.

### Chat
- Chat is append-only JSONL in `.syncvibe/chat-log.jsonl`. One JSON object per line.
- To send a message: append a line with `{"id":"<uuid>","user_id":"...","user_name":"...","user_color":"...","content":"...","message_type":"user","thread_id":null,"session_id":"...","timestamp":"..."}`.
- If SyncVibe MCP server is available, use `read_chat` for smart filtered/incremental reads.

### Cross-Repo Awareness
- SyncVibe has 3 components: **CLI** (Rust, this repo), **Relay** (Cloudflare Workers, syncvibe-relay), **Web** (React/Vite, syncvibe-web)
- When modifying features, consider the impact on all 3 components (e.g. protocol changes affect CLI + relay, auth changes affect all 3)

### UI Consistency
- Any UI changes must match the existing visual style and design language
- Follow established patterns for colors, spacing, typography, and component structure

### Release Checklist
Before every version release:
1. **README.md** — Update to reflect any new features, commands, or changes
2. **ARCHITECTURE.md** — Update if architecture, protocols, or components changed
3. **Release notes** — Write clear changelog with bullet points for each release (never leave it as just a "Full Changelog" link)
