## SyncVibe Collaboration

This project uses SyncVibe for team coordination. All shared state lives in `.syncvibe/`.

### Before starting work
- Read `.syncvibe/plan.md` for the shared project plan.
- Read `.syncvibe/tasks.json` for current task assignments and status.
- Read `.syncvibe/chat-log.jsonl` (last 20 lines) for recent team discussions.

### Tasks
- Tasks are stored in `.syncvibe/tasks.json` as a JSON object with `tasks` array and `version` counter.
- To create a task: read the file, append to the `tasks` array, increment `version`, write back.
- Each task has: `id` (UUID), `title`, `status` (pending/in_progress/completed), `assigned_to`, `assigned_name`, `created_by`, `created_name`, `created_at`, `updated_at`.
- To claim a task: set `status` to `in_progress` and fill `assigned_to`/`assigned_name`.

### Chat
- Chat is append-only JSONL in `.syncvibe/chat-log.jsonl`. One JSON object per line.
- To send a message: append a line with `{"id":"<uuid>","user_id":"...","user_name":"...","user_color":"...","content":"...","message_type":"user","thread_id":null,"session_id":"...","timestamp":"..."}`.
- If SyncVibe MCP server is available, use `read_chat` for smart filtered/incremental reads.

### Plan
- If SyncVibe MCP server is available, use `read_plan`/`update_plan` tools (they handle metadata tracking).
- Otherwise, read/write `.syncvibe/plan.md` directly.
