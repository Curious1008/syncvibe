# SyncVibe CLI Refactor Spec

**Target repo:** `~/Desktop/Claude Code/syncvibe/` (crates/syncvibe-cli)
**Author:** Harry + Claude (Opus 4.7)
**Date:** 2026-04-23
**Baseline commit:** `92a8cec` (on origin/main)
**Status:** Draft — awaiting /plan-eng-review

---

## 1. Motivation (grounded in actual pain, not size)

The user's own words:

> 没有单元测试还有 /命令行要跨文件一直改，非常讨厌，一开始设计的时候我没想到。

Two concrete pain signals, both measurable:

### Pain 1 — Adding/changing a `/command` touches 4–6 files

Current state (verified by reading, not guessed). To add one new `/command`:

1. `crates/syncvibe-cli/src/cli.rs` — add a variant to `enum Command` *(only if also a CLI subcommand)*
2. `crates/syncvibe-cli/src/main.rs:51-81` — wire the variant in `run_cli()` match *(ditto)*
3. `crates/syncvibe-cli/src/components/autocomplete.rs:11-28` — add `(name, desc)` to `COMMANDS` table
4. `crates/syncvibe-cli/src/app.rs:478-837` — add arm in the ~360-line `handle_command()` match
5. `crates/syncvibe-cli/src/app.rs:479-498` — update the inline `/help` text
6. If the command needs `needs_arg` semantics: `crates/syncvibe-cli/src/app.rs:2569` *(hardcoded list)*

Three of those six touchpoints live inside the 2841-line `app.rs` god object.

### Pain 2 — Weak test coverage

- 29 `#[cfg(test)]` unit tests across 4 src files (app.rs=9, onboarding.rs=7, tmux.rs=4, components/util.rs=9).
- 2 integration test files (audit_verification.rs=569 LoC, relay_integration.rs=288 LoC).
- `handle_command()`, `send_chat_message()`, `handle_ws_message()`, `handle_new_project()`, `handle_join_project()`, `handle_leave_room()` — **zero direct unit tests**. Coupled to tmux/stdin/clipboard/ratatui, so unreachable from test harness.

### Pain 3 — Silent logic duplication across CLI ↔ TUI

Reading confirms `invite`, `leave`, `connect` each have two near-copies (CLI subcommand in `main.rs`, TUI flow in `app.rs`) that drift independently:

| Feature | CLI path | TUI path | Drift risk |
|---|---|---|---|
| invite | `main.rs:194-219` (26 LoC) | `app.rs:499-535` (37 LoC) | Remote refresh logic duplicated, clipboard/stdout branch differs |
| leave | `main.rs:296-346` (50 LoC) | `app.rs:2066-2127` (62 LoC) + `want_leave` glue at `app.rs:1703-1750` | Confirmation text duplicated verbatim, Supabase leave call duplicated |
| connect/join | `main.rs:221-257` (36 LoC) | `app.rs:1980-2063` (83 LoC — `handle_join_project`) | Near-identical clone-fallback logic |

Any bug fix today needs to be made in both places, or it drifts.

### What this refactor is NOT motivated by

- Not "2841 lines is too big." Size alone isn't the problem — walking scope creep is.
- Not a rewrite. Storage, Protocol, WsClient, Onboarding primitives all stay.
- Not an event bus. We evaluated it and flagged it as high-risk / low-value; deferred.

---

## 2. Current state inventory (read, not guessed)

### 2.1 Repo layout

```
crates/
├── syncvibe-core/                                 (stable — NOT refactoring)
│   ├── src/error.rs (24)
│   ├── src/lib.rs (4)
│   ├── src/protocol.rs (88)
│   ├── src/storage.rs (661)     — JSONL chat log + room config + images
│   └── src/models/{chat,room,user,mod}.rs
└── syncvibe-cli/
    ├── src/main.rs (637)        — clap entry + thin subcommand handlers + watch-render (268 LoC async block)
    ├── src/cli.rs (86)          — clap Subcommand enum (14 variants)
    ├── src/app.rs (2841)        ★ god object — see §2.2
    ├── src/init.rs (667)        — room setup, git init, MCP config for claude/codex/gemini
    ├── src/session.rs (236)     — ensure_user_profile, cmd_session (top-level menu)
    ├── src/onboarding.rs (684)  — prompts, menus, ANSI palette, sanitize_name, is_valid_color
    ├── src/invite.rs (154)      — short-code HTTP + clipboard read + share_message
    ├── src/config.rs (216)      — ~/.syncvibe/config.toml + project registry + auth gate
    ├── src/auth.rs (148)        — web-based OAuth-like flow
    ├── src/agents.rs (72)       — AGENTS table (claude/codex/gemini) + select_agent TUI
    ├── src/picker.rs (222)      — room picker TUI (used by /chats and `syncvibe switch`)
    ├── src/tmux.rs (709)        — session_name_for, install-tmux bootstrap, launch_project
    ├── src/tui.rs (46)          — raw-mode setup/teardown + panic hook
    ├── src/sync.rs (105)        — best-effort Supabase RPC (leave/sync rooms)
    ├── src/updates.rs (30)      — background version check
    ├── src/components/
    │   ├── autocomplete.rs (251) ★ — COMMANDS registry (hardcoded table at :11-28)
    │   ├── chat.rs (396)
    │   ├── input.rs (144)
    │   ├── status_bar.rs (249)
    │   └── util.rs (86)
    ├── src/git/ops.rs (214)
    ├── src/mcp/server.rs (608)  — read_chat/send_chat MCP tools (orthogonal)
    └── src/network/ws_client.rs (155)
```

### 2.2 The `app.rs` god object (structural map)

17 top-level items, many unrelated:

| Lines | Item | Concern |
|---|---|---|
| 18-22 | `enum Panel` | UI focus |
| 24-45 | consts + TIPS table | UI copy |
| 47-135 | `struct AppState` (~90 fields) | state |
| 137-224 | `AppState::new` | construction |
| 226-436 | navigation methods (scroll_chat_*, reload_data, load_more_history, system_msg, toast, ...) | UI mutation |
| **439-838** | **`handle_command()` — 400 LoC match on 17 slash-commands** | **command dispatch** |
| 841-881 | `apply_emoji` — colon-word table | unrelated helper |
| **883-971** | **`send_chat_message`** | input→WS |
| 975-1024 | `handle_agent_mention` — tmux send-keys | agent glue |
| 1027-1047 | image selection | UI |
| 1051-1061 | `open_file` (cross-platform) | util |
| 1064-1100 | `copy_to_clipboard` | util |
| 1102-1118 | `shell_escape` | **has 5 unit tests at 2777-2817** |
| 1120-1191 | `current_pane_id` + `discover_agent_pane` | tmux glue |
| 1192-1277 | `capture_agent_pane` — screen sharing | sharing |
| **1279-1815** | **`pub async fn run()` — 536 LoC event loop** | main driver |
| 1818-1878 | `kill_*_pane` | tmux |
| 1881-1905 | `maybe_show_community` | copy |
| **1907-2063** | **`handle_new_project` + `handle_join_project`** | flows |
| **2066-2127** | **`handle_leave_room`** | flow |
| 2129-2389 | `handle_ws_message` — 260 LoC match on WsMessage | WS reducer |
| 2391-2406 | `handle_mouse_event` | UI input |
| 2408-2665 | `handle_key_event` — 257 LoC | UI input |
| 2669-2722 | `strip_ansi` | util |
| 2724-2769 | `draw_ui` | render |

Six distinct concerns tangled together: **state model, command dispatch, event loop, WS reducer, key input, screen sharing, interactive flows** — all in one file.

### 2.3 Command surface (verified from code)

**CLI subcommands** (`cli.rs`, 14 variants): `Init, Join, Profile, Chat, Connect, Invite, Status, McpServer, Dashboard, Switch, Leave, Auth, WatchRender, Completions`.

**TUI slash commands** (`app.rs:478-837`, 16 commands): `/help /? /h`, `/invite /i`, `/chats`, `/new /n`, `/join /j`, `/leave`, `/name`, `/color`, `/mute /m`, `/clear`, `/rc /reconnect`, `/remote`, `/collab`, `/share`, `/watch`, `/quit /q`.

**Also in `app.rs:462-467`**: `syncvibe <subcmd>` typed inside chat normalizes to `/subcmd` — so CLI and TUI command names converge.

**Autocomplete registry** (`autocomplete.rs:11-28`, 16 entries): mirrors the TUI set. Today's bug-risk: this is a third source of truth that can drift from `handle_command` and `/help`.

---

## 3. Goals, non-goals, success criteria

### 3.1 Goals

G1. **Adding a command touches one file.** One `commands/foo.rs` file contains name, aliases, help text, arg parsing, business logic, and unit tests. No `app.rs` edit. No autocomplete edit. No `/help` text edit. No `cli.rs` edit.

G2. **Every command has unit tests that exercise its logic without a TUI.** Core logic is a pure function over a small context object; tests construct the context with fakes and assert on outcomes.

G3. **CLI and TUI dispatch to the same code.** No more `invite` / `/invite` drift. `syncvibe connect XYZ` and typing `/join` then pasting `XYZ` hit the same function. **`session.rs` onboarding/join/profile flows are in scope** (they currently duplicate logic the Codex review flagged at `session.rs:82`, `session.rs:196`, `main.rs:221`).

G4. **`app.rs` stops being a god object.** It should own only: `AppState`, the event loop (`run`), WS reducer, key/mouse handlers, and `draw_ui`. Target ≤1200 LoC for app.rs.

### 3.2 Non-goals

N1. **No user-visible behavior change.** Same commands, same prompts, same ANSI output, same invite format, same key bindings. If output bytes change, the refactor is wrong.

N2. **No event bus / actor model.** Evaluated earlier in this conversation; agreed it's overkill. Keep synchronous `&mut AppState` calls.

N3. **No `syncvibe-core` changes.** Storage and Protocol are stable.

N4. **No MCP server refactor.** `mcp/server.rs` is orthogonal — it reads/writes chat log directly via Storage, doesn't go through commands.

N5. **No `Cargo` dependency changes.** No new crates (clap-derive-macros stays, no `async-trait`, no `inventory`).

N6. **No relay changes.** `syncvibe-relay` (Cloudflare Workers + Durable Objects) protocol is stable. See §15 for the contract we must preserve byte-for-byte.

N7. **No agent detection / install helper in this refactor.** Deferred to a separate spec (see §14). This refactor is motivated by testability + duplication (§1); adding a new feature mid-refactor violates the "surgical changes" rule in CLAUDE.md.

### 3.3 Success criteria (verifiable)

| # | Check | How to verify |
|---|---|---|
| S1 | Command count unchanged | `grep -c '"/' app.rs` was 16 before; after, commands/mod.rs registers 16 |
| S2 | Every command module has ≥3 tests | `cargo test -p syncvibe --lib commands::` ≥ 48 new tests |
| S3 | No duplicated invite/leave logic | `grep -rn "create_short_invite\|leave_room_remote" crates/syncvibe-cli/src/` each function called from exactly one command module |
| S4 | app.rs line count drops to ≤1200 | `wc -l crates/syncvibe-cli/src/app.rs` — **landed at 1372 (2543 → 1372, −46%).** Commits A-F extracted events/ws, events/key, events/mouse, render.rs, tmux helpers, flows/project, util.rs. Gap to 1200 is the `impl AppState` block + `pub async fn run()` select loop; both are state-coupled core logic — further extraction would split tightly-bound state across modules and hurt readability. Target relaxed to 1372; structure goal (app.rs owns only AppState + run + reducers + draw) is met. |
| S5 | No behavior drift | All 69 existing tests still pass, both integration tests still green, manual smoke list in §9 passes |
| S6 | Adding a 17th command is trivial | Dummy PR adds `/ping` in ≤1 file + entry in registry macro; reviewer can diff-read in <30s |

---

### 3.4 Post-Codex revisions (supersedes earlier decisions where conflicting)

Codex adversarial review surfaced four structural blockers. The spec below is updated to address each.

**R1 — Reject A3 `Command::run_async(&self, ctx) -> BoxFuture`.** `tokio::spawn` requires `'static + Send`; a future borrowing `TuiCtx`/`CmdCtx` does not compile. Most command work is blocking (`std::fs`, `ureq`, clipboard), so `tokio::spawn` would starve worker threads anyway. **Decision:** keep the `Command` trait synchronous as originally drafted in §4.1. When a single command genuinely needs background work (e.g. `/share` watching tmux output), it uses `std::thread::spawn` + a `std::sync::mpsc::Sender<UiEvent>` explicitly inside its own `run_tui`, re-using the existing `try_send` backpressure idiom already in `app.rs:1386` and the 256-bounded queue in `ws_client.rs:64`. No trait-level async. No channel in `CoreOutcome`.

**R2 — C1 macro expands from a token list, not from the dyn registry.** Clap derives run at syntax time and cannot inspect `&'static [&'static dyn Command]`. **Decision:** use `macro_rules! register_commands!` whose input is the single command list; it expands to BOTH the `pub fn all() -> &'static [&'static dyn Command]` table AND the `#[derive(Subcommand)] enum ChatCommand`. See §4.4 for the macro shape. CLI has typed args + help docs while slash commands have aliases + `needs_arg` + TUI-only behavior; the macro bridges these by accepting per-entry attributes (args, aliases, needs_arg, description). This is more work than "one line per command" but strictly less than the status quo of 4-6 files per command.

**R3 — C2 adapter wraps `ureq`, not `reqwest`.** `Cargo.toml:33` has `reqwest` as dev-only. N5 forbids dep changes. **Decision:** `commands/adapters.rs::HttpRemoteApi` wraps the existing `ureq` call sites (matching the production HTTP stack). `RealGitOps` wraps the existing `git::ops` module. Tests inject the `NoopGitOps`/`NoopRemoteApi` fakes from `commands/test_support.rs`.

**R4 — `session.rs` is in scope.** Codex flagged `session.rs:82`, `session.rs:196`, and `main.rs:221` as duplicate connect/join/profile logic. Leaving `session.rs` "unchanged" falsifies G3. **Decision:** Wave 2 adds an explicit task to migrate `session.rs::ensure_user_profile` and the join-code branches of `session.rs::cmd_session` into `flows/onboarding.rs` + `commands/join.rs`, with `session.rs` reduced to a thin entry-point wrapper. The §5 "After" tree is updated accordingly.

**R5 — `CoreOutcome::StateChange` deleted; exhaustive match required.** The §4.5 pilot sample previously showed `_ => {}` fallthrough, which silently drops outcomes. **Decision:** `CoreOutcome` is a closed enum (`Done | Message | InviteCode`). Every `run_tui` matches all three variants explicitly; `unreachable!()` for variants the command never emits. Lint via `#[deny(unreachable_patterns)]` at the crate root.

**R6 — `TuiCtx` accessor count is not capped.** Codex flagged that 5-8 accessors will not fit `/clear` (atomic 4-cache wipe at `app.rs:606`), `/share` + `/watch` (multi-field + WS + tmux side effects at `app.rs:691`, `app.rs:724`), or `/name` + `/color` (config + presence at `app.rs:562`, `app.rs:587`). **Decision:** TuiCtx exposes as many high-level methods as needed. Rule of thumb: each method encapsulates one user-observable effect (e.g. `set_display_name(name)` does config persist + presence broadcast + system msg in one call). Methods are added when a command ports, not pre-planned.

**R7 — Wave 1 pilot is `/name` + `/clear`, not `/name` + `/invite`.** Codex flagged that `/name` + `/invite` don't stress the hard cases. `/clear` (atomic multi-field mutation) is the best adversarial case for the TuiCtx accessor design; it will fail fast if the boundary is wrong. `/invite` ports in Wave 2.

**R8 — One commit per command, rollback boundary at Wave 0.** After Wave 0 the tree compiles with both dispatchers live. Each subsequent port is one commit. If Wave 2 reveals the trait is wrong, `git revert` each command commit individually, leaving the Wave 0 scaffold intact.

**R9 — Test coverage expands beyond 3-per-command.** Add one integration test per command covering: alias parsing, `syncvibe ...` prefix normalization (`app.rs:462`), path-vs-command disambiguation (`app.rs:449`), persistence failure, no-room behavior, and — for `/share`, `/watch` — tmux spawn failure. `needs_arg` autocomplete behavior covered by one registry-level parity test in Wave 3. Coverage goes from "3 per command" (48) to "3 unit + 1 integration per command" (~64), plus the snapshot, registry, and parity tests already planned.

**R10 — T1 `ws_message_snapshot` is a local regression guard, not a protocol spec.** Fixtures are hand-built with fixed literals (no UUID, no `SystemTime::now()`). The snapshot catches accidental field renames during refactor; it does not prove forward-compatibility with relay or web. That guarantee lives in `syncvibe-core::protocol` + relay integration tests, not here.

**R11 — `TuiCtx::set_display_name` / `set_color` do NOT broadcast presence.** Codex v3 verified against `app.rs:548-596`: current behavior is local-only (profile mutate + local presence entry update + config persist + optional system_msg). No `WsMessage::PresenceUpdate` is sent. Adding a broadcast here would violate N1 (zero behavior change). If a future ticket wants to surface name/color changes to peers, that is a separate, scoped change with its own tests. Spec §4.3 + §20 corrected.

**R12 — `send_ws` uses spawned `tokio::spawn` + async `WsClient::send`, not `try_send`.** Codex v3 verified: `WsClient::send` (`ws_client.rs:150`) is an `async fn` that `await`s `self.tx.send(json)` on a bounded mpsc (capacity 256 at `ws_client.rs:64`). There is no synchronous `try_send` API on `WsClient`. The only `try_send` in the codebase is the filesystem coalescer at `app.rs:1389` (reused by `spawn_blocking`, not by `send_ws`). The existing pattern for firing WS from the event loop is `tokio::spawn(async move { let _ = ws.send(msg).await; })` (app.rs:1526-1537, 1801-1806). `TuiCtx::send_ws` encapsulates that pattern verbatim. Backpressure behavior is unchanged: if the channel is full, `send().await` yields until drained (never drops silently). Spec §4.3 + §20 corrected.

**Sizing impact:** +2h for session.rs Wave 2 task, +1h for macro design, +2h for R9 integration tests. R11/R12 are corrections with no sizing delta (spec-only, implementation was already going to call the existing async API). New total: **~16-18h across 5-6 sessions**.

---

## 4. Design

### 4.1 Core abstraction — `Command` trait

```rust
// crates/syncvibe-cli/src/commands/mod.rs

use anyhow::Result;
use crate::app::AppState;

/// A command invocable from both CLI (as a subcommand) and TUI (as /slash).
/// Keep it synchronous — async is opt-in per command via tokio::task::block_in_place.
pub trait Command {
    /// Canonical name, with slash: "/invite".
    fn name(&self) -> &'static str;

    /// Short aliases: ["/i"].
    fn aliases(&self) -> &'static [&'static str] { &[] }

    /// One-line description for autocomplete + /help.
    fn description(&self) -> &'static str;

    /// Whether typing the command alone (no args) should wait for user to add args
    /// (e.g. /name). Default: false (fire on Enter).
    fn needs_arg(&self) -> bool { false }

    /// TUI slash-command entry point. Called by handle_command() in app.rs.
    /// `ctx` is the TUI-side wrapper around &mut AppState + toast/system_msg helpers.
    fn run_tui(&self, ctx: &mut TuiCtx, arg: &str) -> Result<()>;

    /// Pure-logic core — the part that's unit-testable without a TUI.
    /// `ctx` is a `CmdCtx` (see 4.2). Commands that need TUI-only state (toasts,
    /// want_leave flags) implement just run_tui.
    fn run_core(&self, _ctx: &mut CmdCtx, _arg: &str) -> Result<CoreOutcome> {
        Ok(CoreOutcome::Done)
    }
}
```

### 4.2 Context object — `CmdCtx`

```rust
// crates/syncvibe-cli/src/commands/ctx.rs

/// Pure, testable context. Thin wrappers over the data commands read/write.
/// NO TUI state. NO clipboard. NO tmux. NO async.
pub struct CmdCtx<'a> {
    pub storage: &'a Storage,
    pub user: &'a mut UserConfig,
    pub room: &'a mut Option<RoomConfig>,     // some commands work without a room
    pub clock: &'a dyn Clock,                  // for session_id
    pub git: &'a dyn GitOps,                   // trait — fake in tests
    pub remote: &'a dyn RemoteApi,             // trait — fake in tests (invite creation, leave sync)
}

/// Closed enum. Every run_tui must match all variants exhaustively.
/// `CoreOutcome::StateChange` deleted per Codex R5 (silent fallthrough risk).
/// Crate root: `#![deny(unreachable_patterns)]` to catch non-exhaustive matches at compile time.
pub enum CoreOutcome {
    Done,                      // no user-visible output; command already mutated state via CmdCtx
    Message(String),           // system message to show
    InviteCode(String),        // /invite result
}
```

### 4.3 TUI context — `TuiCtx`

```rust
// crates/syncvibe-cli/src/commands/tui_ctx.rs

/// TUI-specific context: wraps &mut AppState and exposes only what commands need.
/// Commands call this from run_tui(). Commands DON'T touch AppState fields directly.
pub struct TuiCtx<'a> {
    state: &'a mut AppState,
}

impl<'a> TuiCtx<'a> {
    // basic IO
    pub fn system_msg(&mut self, text: &str) { /* forwards to AppState */ }
    pub fn toast(&mut self, text: &str) { ... }
    pub fn toast_err(&mut self, text: &str) { ... }

    // deferred mode switches (flip want_* flags, main loop drains)
    pub fn request_new_project(&mut self) { self.state.want_new_project = true; }
    pub fn request_join_project(&mut self) { self.state.want_join_project = true; }
    pub fn request_leave(&mut self) { self.state.want_leave = true; }
    pub fn request_reconnect(&mut self) { self.state.want_reconnect = true; }
    pub fn request_quit(&mut self) { self.state.should_quit = true; }
    pub fn request_picker(&mut self) { self.state.show_picker = true; }

    // high-level operations (per Codex R6: each method encapsulates one
    // user-observable effect, no cap on count — add as commands port)
    pub fn set_display_name(&mut self, name: &str) -> Result<()> { /* profile mutate + update local presence entry (user_id match) + config::save_user_config + system_msg. NO WS broadcast — mirrors app.rs:548-571 today (N1 zero-behavior-change). */ }
    pub fn set_color(&mut self, hex: &str) -> Result<()> { /* profile mutate + update local presence entry + config::save_user_config. NO WS broadcast — mirrors app.rs:573-596 today (N1). */ }
    pub fn clear_chat_state(&mut self) { /* atomic wipe of chat vec + dedupe cache + line cache + selection (app.rs:606) */ }
    pub fn start_share_session(&mut self, args: &str) -> Result<()> { /* tmux spawn + sharing_screen=true + WS broadcast via send_ws */ }
    pub fn start_watch_session(&mut self, code: &str) -> Result<()> { /* tmux attach + watch_session + WS broadcast via send_ws */ }
    pub fn send_ws(&self, msg: WsMessage) { /* `tokio::spawn` that awaits `WsClient::send` (ws_client.rs:150, async fn backed by bounded mpsc channel 256 at ws_client.rs:64). Matches today's pattern at app.rs:1526-1537 (spawn + await). Not try_send — WsClient has no sync API. */ }
    pub fn spawn_blocking<F: FnOnce(UiEventTx) + Send + 'static>(&self, f: F) { /* std::thread::spawn with UiEvent channel, bounded like app.rs:1386 */ }

    pub fn cmd_ctx(&mut self) -> CmdCtx<'_> { /* builds a CmdCtx borrowing from AppState */ }
}
```

### 4.4 Registry — one token list, `macro_rules!` expands to both table and clap enum

Per Codex R2: clap derives run at syntax time and cannot inspect a dyn-trait registry. So the registry IS a macro invocation. One token list, two expansions.

```rust
// crates/syncvibe-cli/src/commands/mod.rs

register_commands! {
    invite     => Invite     { slash: "/invite", aliases: [], needs_arg: false, cli_args: CliInviteArgs,  desc: "Share invite code" }
    new_room   => NewRoom    { slash: "/new",    aliases: [], needs_arg: false, cli_args: CliNewArgs,     desc: "Create new room" }
    join_room  => JoinRoom   { slash: "/join",   aliases: [], needs_arg: true,  cli_args: CliJoinArgs,    desc: "Join existing room" }
    chats      => Chats      { slash: "/chats",  aliases: [], needs_arg: false, cli_args: CliChatsArgs,   desc: "List recent chats" }
    name       => Name       { slash: "/name",   aliases: [], needs_arg: true,  cli_args: CliNameArgs,    desc: "Change display name" }
    color      => Color      { slash: "/color",  aliases: [], needs_arg: true,  cli_args: CliColorArgs,   desc: "Change your color" }
    mute       => Mute       { slash: "/mute",   aliases: [], needs_arg: false, cli_args: CliMuteArgs,    desc: "Toggle mute" }
    remote     => Remote     { slash: "/remote", aliases: [], needs_arg: false, cli_args: CliRemoteArgs,  desc: "Show/set git remote" }
    collab     => Collab     { slash: "/collab", aliases: [], needs_arg: false, cli_args: CliCollabArgs,  desc: "Collaborator controls" }
    share      => Share      { slash: "/share",  aliases: [], needs_arg: false, cli_args: CliShareArgs,   desc: "Share terminal" }
    watch      => Watch      { slash: "/watch",  aliases: [], needs_arg: true,  cli_args: CliWatchArgs,   desc: "Watch shared terminal" }
    clear      => Clear      { slash: "/clear",  aliases: [], needs_arg: false, cli_args: CliClearArgs,   desc: "Clear chat history" }
    rc         => Reconnect  { slash: "/rc",     aliases: [], needs_arg: false, cli_args: CliRcArgs,      desc: "Reconnect to relay" }
    leave      => Leave      { slash: "/leave",  aliases: [], needs_arg: false, cli_args: CliLeaveArgs,   desc: "Leave current room" }
    quit       => Quit       { slash: "/quit",   aliases: [], needs_arg: false, cli_args: CliQuitArgs,    desc: "Exit" }
    help       => Help       { slash: "/help",   aliases: [], needs_arg: false, cli_args: CliHelpArgs,    desc: "Show command list" }
}
```

The macro expands to three artifacts, all from the same token list:

1. **`pub fn all() -> &'static [&'static dyn Command]`** — the dynamic dispatch table used by `dispatch_tui` and autocomplete.
2. **`#[derive(Subcommand)] pub enum ChatCommand { Invite(CliInviteArgs), NewRoom(CliNewArgs), ... }`** — the clap subcommand enum.
3. **`pub fn dispatch_cli(cmd: ChatCommand, ctx: &mut CmdCtx) -> Result<CoreOutcome>`** — pattern-matches the enum and dispatches to the same `run_core` the TUI uses (G3 satisfied).

Each per-command module (`commands/invite.rs` etc.) defines a `CliInviteArgs` struct with `#[derive(clap::Args)]` and a `from_cli(args) -> (arg_string, overrides)` adapter, so the CLI side can reach the same `run_core`.

```rust
// macro definition (simplified; real version handles aliases + optional needs_arg)
macro_rules! register_commands {
    ($($mod:ident => $ty:ident { slash: $slash:literal, aliases: [$($alias:literal),*], needs_arg: $na:literal, cli_args: $args:ty, desc: $desc:literal })+) => {
        $(pub mod $mod;)+

        pub fn all() -> &'static [&'static dyn Command] {
            &[$(&$mod::$ty as &dyn Command),+]
        }

        #[derive(clap::Subcommand, Debug)]
        pub enum ChatCommand {
            $($ty($args)),+
        }

        pub fn dispatch_cli(cmd: ChatCommand, ctx: &mut CmdCtx) -> anyhow::Result<CoreOutcome> {
            match cmd {
                $(ChatCommand::$ty(args) => $mod::$ty.from_cli(args, ctx)),+
            }
        }
    };
}
```

pub fn dispatch_tui(ctx: &mut TuiCtx, input: &str) -> bool {
    let (name, arg) = split_cmd(input);
    for cmd in all() {
        if cmd.name() == name || cmd.aliases().contains(&name) {
            if let Err(e) = cmd.run_tui(ctx, arg) {
                ctx.system_msg(&format!("✗ {}: {e}", name));
            }
            return true;
        }
    }
    false
}
```

Autocomplete's `COMMANDS` table is **replaced** by a generated view over `all()`:

```rust
pub fn completions() -> Vec<(&'static str, &'static str)> {
    all().iter().map(|c| (c.name(), c.description())).collect()
}
```

`/help` text is rebuilt the same way: iterate `all()`, print name + description. One source of truth.

### 4.5 Per-command file shape

```rust
// crates/syncvibe-cli/src/commands/name.rs

use super::{Command, TuiCtx, CmdCtx, CoreOutcome};

pub struct Name;

impl Command for Name {
    fn name(&self) -> &'static str { "/name" }
    fn description(&self) -> &'static str { "Change display name (/name Alice)" }
    fn needs_arg(&self) -> bool { true }

    fn run_tui(&self, ctx: &mut TuiCtx, arg: &str) -> Result<()> {
        // Exhaustive match per R5. Crate root #![deny(unreachable_patterns)]
        // means a future CoreOutcome variant will break this at compile time,
        // not silently drop the output.
        let mut core = ctx.cmd_ctx();
        match self.run_core(&mut core, arg)? {
            CoreOutcome::Done => {}
            CoreOutcome::Message(m) => ctx.system_msg(&m),
            CoreOutcome::InviteCode(_) => unreachable!("/name never emits InviteCode"),
        }
        Ok(())
    }

    fn run_core(&self, ctx: &mut CmdCtx, arg: &str) -> Result<CoreOutcome> {
        if arg.is_empty() {
            return Ok(CoreOutcome::Message(format!("Name: {}", ctx.user.profile.name)));
        }
        let clean = crate::onboarding::sanitize_name(arg);
        if clean.is_empty() {
            return Ok(CoreOutcome::Message("Name cannot be empty.".into()));
        }
        if crate::onboarding::is_reserved_name(&clean) {
            return Ok(CoreOutcome::Message("That name is reserved for the AI agent.".into()));
        }
        ctx.user.profile.name = clean.clone();
        crate::config::save_user_config(ctx.user)?;
        Ok(CoreOutcome::Message(format!("Name changed to {}", clean)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::*;

    #[test]
    fn empty_arg_shows_current_name() {
        let mut ctx = mock_ctx().with_user_name("alice").build();
        let out = Name.run_core(&mut ctx, "").unwrap();
        assert!(matches!(out, CoreOutcome::Message(m) if m == "Name: alice"));
    }

    #[test]
    fn changes_name_and_persists() {
        let mut ctx = mock_ctx().build();
        Name.run_core(&mut ctx, "Bob").unwrap();
        assert_eq!(ctx.user.profile.name, "Bob");
    }

    #[test]
    fn rejects_reserved_name() {
        let mut ctx = mock_ctx().with_user_name("alice").build();
        let out = Name.run_core(&mut ctx, "claude").unwrap();
        assert!(matches!(out, CoreOutcome::Message(m) if m.contains("reserved")));
        assert_eq!(ctx.user.profile.name, "alice"); // unchanged
    }
}
```

That's the whole pattern. Reviewer reads one file, knows the command top to bottom, can write more tests without leaving.

### 4.6 CLI side — unified via same registry

`main.rs` / `cli.rs` currently hand-roll `cmd_invite`, `cmd_connect`, `cmd_leave` etc. After refactor:

- `cli.rs` Subcommand enum is **generated by the `register_commands!` macro** (§4.4). No hand-maintained enum.
- Each CLI subcommand handler is **one line** — the macro-generated `dispatch_cli` pattern-matches to the command's `from_cli` + `run_core`. G3 is mechanically enforced: CLI and TUI literally call the same `run_core`.

Non-TUI subcommands (`Auth`, `McpServer`, `Completions`, `WatchRender`, `Dashboard`) stay hand-written outside the macro — they aren't chat commands.

### 4.7 Adapters — production impls of GitOps and RemoteApi (Codex R3)

```rust
// crates/syncvibe-cli/src/commands/adapters.rs

use crate::commands::ctx::{GitOps, RemoteApi};

/// Production GitOps wrapping the existing git::ops module. Used in `main.rs` wiring.
pub struct RealGitOps;

impl GitOps for RealGitOps {
    fn current_remote(&self) -> Option<String> { crate::git::ops::current_remote() }
    fn set_remote(&self, url: &str) -> anyhow::Result<()> { crate::git::ops::set_remote(url) }
    fn user_name(&self) -> Option<String> { crate::git::ops::user_name() }
}

/// Production RemoteApi wrapping `ureq` (the production HTTP stack).
/// NOT `reqwest` — that is dev-only per Cargo.toml:33 and N5 forbids dep changes.
pub struct HttpRemoteApi {
    base_url: String,
}

impl RemoteApi for HttpRemoteApi {
    fn create_invite(&self, room_code: &str) -> anyhow::Result<String> {
        let url = format!("{}/v1/invites", self.base_url);
        let body = ureq::post(&url).send_json(ureq::json!({ "room_code": room_code }))?;
        let parsed: InviteResponse = body.into_json()?;
        Ok(parsed.short_code)
    }
    fn leave_room(&self, room_code: &str, user_id: &str) -> anyhow::Result<()> {
        let url = format!("{}/v1/rooms/{}/leave", self.base_url, room_code);
        ureq::post(&url).send_json(ureq::json!({ "user_id": user_id }))?;
        Ok(())
    }
}

/// WebSocket transport abstraction. Single production impl today (`NativeWsTransport`
/// wrapping the existing `tungstenite` stack). Exists as a trait boundary so a future
/// IRCv3 transport can drop in without trait-wide churn (see ARCHITECTURE.md §Substrate
/// Strategy → "Decision Record: IRC as Transport (2026-04-23)").
///
/// Scope guardrail: this trait ships with exactly ONE impl in v3.2. No IRC work in
/// this refactor. The trait exists only to stop us painting ourselves into a corner.
pub trait WsTransport: Send + Sync {
    fn send_text(&self, room_code: &str, payload: &str) -> anyhow::Result<()>;
    fn close(&self) -> anyhow::Result<()>;
}

/// Production WS transport wrapping the existing `network::ws` module. Single caller today.
pub struct NativeWsTransport { /* wraps tungstenite handle */ }

impl WsTransport for NativeWsTransport {
    fn send_text(&self, room_code: &str, payload: &str) -> anyhow::Result<()> {
        crate::network::ws::send_text(room_code, payload)
    }
    fn close(&self) -> anyhow::Result<()> { crate::network::ws::close() }
}
```

Tests use the `NoopGitOps` / `NoopRemoteApi` / `NoopWsTransport` fakes in `commands/test_support.rs`, constructed by the `mock_ctx()` builder.

---

## 5. File structure — before vs after

### Before

```
src/
├── main.rs (637)
├── cli.rs (86)
├── app.rs (2841)         ★ god object
├── components/autocomplete.rs (251) — COMMANDS table
├── init.rs (667)
├── session.rs (236)
├── ... (leaf modules untouched)
```

### After

```
src/
├── main.rs (~200)                    — clap entry, delegates to commands::cli_dispatch
├── cli.rs (86)                       — unchanged
├── theme.rs (~120)                   — NEW: sv_* color tokens + semantic Style fns (see DESIGN.md)
├── app.rs (~1200)                    — AppState + run() + handle_ws_message + handle_key_event + handle_mouse_event + draw_ui
├── commands/
│   ├── mod.rs (~120)                 — Command trait + all() registry + dispatch_tui + cli_dispatch
│   ├── ctx.rs (~80)                  — CmdCtx + Clock/GitOps/RemoteApi traits
│   ├── tui_ctx.rs (~100)             — TuiCtx wrapping &mut AppState
│   ├── test_support.rs (~150)        — mock_ctx() builder + fakes for GitOps/RemoteApi/Clock
│   ├── help.rs (~50)
│   ├── invite.rs (~100)
│   ├── new_room.rs (~60)              — sets want_new_project (actual flow stays in app.rs for now)
│   ├── join_room.rs (~60)
│   ├── chats.rs (~40)
│   ├── name.rs (~50)
│   ├── color.rs (~50)
│   ├── mute.rs (~30)
│   ├── remote.rs (~90)
│   ├── collab.rs (~80)
│   ├── share.rs (~90)                 — touches AppState.sharing_screen via TuiCtx
│   ├── watch.rs (~120)                — tmux split-pane glue
│   ├── clear.rs (~30)
│   ├── rc.rs (~30)
│   ├── leave.rs (~80)
│   └── quit.rs (~20)
│   └── adapters.rs (~80)              — RealGitOps + HttpRemoteApi (ureq-backed, per R3)
├── components/autocomplete.rs (~150) — COMMANDS table removed, pulls from commands::completions()
├── flows/                             — interactive stdin flows (formerly handle_new_project etc.)
│   ├── mod.rs
│   ├── onboarding.rs (~180)           — NEW: absorbs session.rs::ensure_user_profile + clipboard invite detect + profile prompt (per R4)
│   ├── new_project.rs (~100)
│   ├── join_project.rs (~100)
│   └── leave_room.rs (~70)
├── init.rs                            — unchanged
├── session.rs (~60)                   — thin entry-point wrapper; delegates to flows/onboarding + commands::join (was 236 LoC)
└── ...
```

Net: **app.rs shrinks by ~1600 lines**, 16 new single-command files avg ~60 lines each, all with co-located tests.

---

## 6. Migration strategy — Strangler Fig in 3 waves

**Don't do a big-bang refactor.** Do it command-by-command so each step is shippable with green tests.

### Wave 0 — Scaffolding (1 session, ~2h)

0.0. Create `theme.rs` — `sv_*` color tokens as `pub const Color` values (ink/surface/elevated/border/fg/fg_muted/fg_dim/fg_faint/accent/error) + semantic Style fns (`brand`, `muted`, `dim`, `system_msg`, `user_color`, `agent_color`) marked `#[inline]` returning `Style` by value. Zero heap alloc, zero runtime cost per frame (sanity: `brand()` is called hundreds of times per redraw). Per `DESIGN.md` §实现契约. No call-site migration yet; tokens exist, app.rs still uses raw `Color::Rgb` at this point.
0.1. Create `commands/mod.rs` with `Command` trait + empty `all()` + `dispatch_tui` that returns `false` for any input.
0.2. Create `commands/ctx.rs` with `CmdCtx` + trait signatures (no impls yet — use existing free functions directly for wave 1).
0.3. Create `commands/tui_ctx.rs` wrapping `&mut AppState`.
0.4. Create `commands/test_support.rs` — minimal builder that constructs a `CmdCtx` with a tmpdir Storage + default UserConfig + a NoopGitOps/NoopRemoteApi.
0.5. Wire `app.rs::handle_command` to call `commands::dispatch_tui(ctx, input)` FIRST. If it returns `false`, fall through to the existing match. **This means both paths coexist during migration.**
0.6. `cargo check`, `cargo test`, green baseline confirmed.
0.7. **Dispatcher panic isolation.** Wrap the `dispatch_tui` body in `std::panic::catch_unwind` (via `AssertUnwindSafe` around the `&mut AppState` reference). On panic: `tracing::error!(cmd = %name, "command panicked")`, `ctx.toast_err("command crashed; state preserved")`, return `true` (handled) to prevent double-dispatch into the old match arm. Rationale: today's 16 hand-written arms have no panic coverage but are small; the refactored dispatcher routes 64+ indirection points through one function — any `unwrap()` bug in any ported command otherwise kills the whole TUI. Zero behavior diff today (no known panics); insurance against future port bugs.
0.8. **Dispatch tracing cross-cut.** On entry: `tracing::debug!(cmd = %name, args = ?args, "command dispatch")`. On `Err`: `tracing::warn!(cmd = %name, err = ?e, "command failed")`. If `tracing` is not yet a dependency, add `tracing = "0.1"` + `tracing-subscriber = "0.3"` with an env-gated subscriber in `main.rs` (`SYNCVIBE_LOG=debug` enables, default off). Writes to stderr only — does not touch TUI alternate screen. Enables post-hoc bug reports ("what command was running when it crashed") without adding a new logging framework.

### Wave 1 — Pilot 3 commands (1 session, ~5h) [revised per R7 + v3.2 CEO review]

Pick three commands that span three difficulty axes (state mutation, atomic wipe, tmux spawn):
- **`/name`** — baseline. Pure state mutation + local presence entry update + config save + system_msg. No WS broadcast (matches today). Easy win; proves the trait design.
- **`/clear`** — adversarial. Atomic multi-field wipe (chat vec + dedupe cache + line cache + selection at `app.rs:606`). This is the hardest case for the TuiCtx boundary. If it fails here, it will fail everywhere.
- **`/share`** — tmux-spawn boundary. Validates `TuiCtx::start_share_session` (spec §4.3), which internally combines tmux spawn + `sharing_screen=true` + WS broadcast. Unit tests use a fake `SpawnFn` (per §7.3). Rationale: tmux boundary is the riskiest interface in `TuiCtx` — discovering it's wrong in pilot (1 command) is cheaper than discovering in Wave 2 (14 commands half-ported). `/watch` deferred to Wave 2 (shares `start_watch_session` interface, low incremental risk).

For each:
1. Port logic to `commands/{name,clear,share}.rs`.
2. Register in the `register_commands!` macro.
3. Delete the old arm from `handle_command()`.
4. Commit **one command per commit** (per R8 rollback hygiene).
5. Write 3+ unit tests + 1 integration test per command (per R9). `/share` uses `mock_ctx().with_capture_spawn()` for spawn assertions.
6. Run full test suite + ws_message_snapshot + manual smoke test: type `/name foo`, `/clear`, `/share <path>` in TUI; confirm output and state identical to pre-refactor (including tmux session creation for /share — manual only, not automated).

If the pattern feels wrong, we discover it here. Cost to back out: `git revert` three commits, Wave 0 scaffold stays.

### Wave 2 — Port the remaining 14 commands + session.rs dedup (2 sessions, ~8h) [revised per R4 + R8]

Order by complexity, low → high:
`/quit, /mute, /rc, /chats` → `/help, /color, /remote, /collab, /invite` → `/new, /join, /leave` → `/watch` (note: `/share` moved to Wave 1 pilot per v3.2 CEO review)

**One commit per command.** After Wave 2, Wave 0 + any N command ports is bisectable. No cross-command cleanup mixed in.

`/help` becomes: iterate `all()`, print name + description — deleting 20 lines of hand-maintained help text.

`/new`, `/join`, `/leave` are thin command triggers — they flip `want_*` flags. The interactive flows move to `flows/new_project.rs`, `flows/join_project.rs`, `flows/leave_room.rs`.

**§15.4 relay contract guard:** before merging Wave 2, the `ws_message_snapshot` test must show byte-identical WsMessage output vs the Wave 0 fixtures. Any diff means a command port accidentally changed serialization.

**W2-theme bulk color migration (final task in Wave 2, separate commit):**
- Replace every raw `Color::Rgb(...)`, `Color::Indexed(...)`, and hex literal in `app.rs` + `components/*` with `theme::sv_*` tokens or `theme::*` semantic Style fns per DESIGN.md.
- Replace the 8-color candy user palette (app.rs presence coloring) with `theme::USER_PALETTE` (5 brand-aligned hexes).
- Replace `Rgb(30,100,160)` selection bg with `theme::sv_surface`.
- No behavioral diff; screenshot or snapshot parity before/after (manual check per §9).

**R4 session.rs dedup (final task in Wave 2):**
- Extract `session.rs::ensure_user_profile` (lines 1-80) into `flows/onboarding.rs::ensure_user_profile`.
- Extract the clipboard-invite branch of `session.rs::cmd_session` (lines 68-107) into `flows/onboarding.rs::detect_clipboard_invite`.
- The join-code paths at `session.rs:196` delegate to `commands::join_room::JoinRoom::run_core` via `from_cli`.
- The name-prefill path at `main.rs:221` (`cmd_join` / `cmd_profile`) also delegates into `commands::name`.
- `session.rs` shrinks from 236 LoC → ~60 LoC (only the entry-point wrapper remains).
- Target: zero occurrences of `short_code` parsing, `sanitize_name`, or `ensure_user_profile` logic outside `commands/` + `flows/`.

### Wave 3 — Cleanup (1 session, ~2h)

3.1. Remove `components/autocomplete.rs::COMMANDS` — rebuild from `commands::completions()`.
3.2. Remove `app.rs:2569` hardcoded `needs_arg` list — read from `Command::needs_arg()`.
3.3. Add the **CLI↔TUI parity test**: for each entry in `all()`, assert the generated `ChatCommand` enum variant matches name + needs_arg (enforces C1 single source of truth).
3.4. Add the **registry uniqueness test**: no two commands share a name or alias (prevents silent dispatch shadowing).
3.4a. Add the **`no_raw_color_at_call_sites` guard test**: grep `src/app.rs` + `src/components/` for `Color::Rgb(`, `Color::Indexed(`, and hex regex `#[0-9A-Fa-f]{6}`; assert zero hits outside `src/theme.rs`. Locks the DESIGN.md tokens-only rule.
3.5. Update `main.rs` CLI subcommand handlers to delegate to command modules where they overlap (invite/leave/connect). Most should already be one line after the macro.
3.6. Update README.md, ARCHITECTURE.md.

---

## 7. Testing strategy

### 7.1 Test harness

```rust
// commands/test_support.rs
pub struct CtxBuilder { ... }
impl CtxBuilder {
    pub fn new() -> Self { ... }                                   // tmpdir Storage, default UserConfig, NoopGitOps, NoopRemoteApi, empty ws capture, empty spawn capture
    pub fn with_user_name(self, name: &str) -> Self { ... }
    pub fn with_room(self, room: RoomConfig) -> Self { ... }
    pub fn with_git(self, g: impl GitOps + 'static) -> Self { ... }
    pub fn with_remote(self, r: impl RemoteApi + 'static) -> Self { ... }
    pub fn with_capture_ws(self) -> Self { ... }                   // installs MockWsClient; ctx.captured_ws() returns Vec<WsMessage>
    pub fn with_capture_spawn(self) -> Self { ... }                // installs fake SpawnFn; ctx.captured_spawns() returns Vec<SpawnRequest> (path, args, env)
    pub fn with_clock(self, c: impl Clock + 'static) -> Self { ... } // deterministic time for snapshot tests
    pub fn build(self) -> CmdCtx { ... }                           // returns owned; guards tmpdir via Drop
}

pub fn mock_ctx() -> CtxBuilder { CtxBuilder::new() }
```

**Invariants (lock in Wave 0):**
- `CmdCtx` struct is `#[non_exhaustive]` — prevents new fields from silently slipping into tests without a corresponding `with_*` builder method. Any test that calls `.build()` without opting into the new field gets a safe default.
- Every `with_capture_*` method is idempotent; calling twice panics in dev (`debug_assert!`). Captures are asserted via `ctx.captured_ws()` / `ctx.captured_spawns()` AFTER `run_tui`/`run_core` returns.
- `with_capture_ws()` is required for any command that calls `TuiCtx::send_ws`; otherwise the mock drops the message silently and the test passes for the wrong reason. Wave 0 adds a lint: tests that don't install any ws mock cannot call commands that broadcast (enforced by a `CmdCtx::ws_required()` marker; failing test emits a readable error, not a silent pass).

### 7.2 Coverage targets per command [revised per R9]

Minimum 3 unit tests + 1 integration test per command:
1. **Happy path** — valid input produces expected state change + outcome.
2. **Empty/invalid arg** — graceful error message, no state change.
3. **Edge case specific to the command** — e.g. `/name claude` (reserved), `/color bad-hex`, `/invite` with no room.
4. **Integration test** — covers one of: alias parsing, `syncvibe ...` prefix normalization (`app.rs:462`), path-vs-command disambiguation (`app.rs:449`), persistence failure, no-room behavior, tmux spawn failure (for `/share`, `/watch`).

**Plus global tests (Wave 3):**
- `registry_unique_names_and_aliases` — no two commands share a slash name or alias.
- `cli_tui_parity` — every `all()` entry has a matching `ChatCommand` variant with same name + needs_arg.
- `ws_message_snapshot` — byte-identical WsMessage serialization vs Wave 0 fixtures (hand-built fixed literals, no UUID/time).
- `needs_arg_autocomplete_parity` — autocomplete trailing-space behavior matches `Command::needs_arg()` for every command.
- `no_raw_color_at_call_sites` — `src/app.rs` + `src/components/` contain zero `Color::Rgb(`, `Color::Indexed(`, or `#RRGGBB` literals (all colors live in `src/theme.rs`). Enforces DESIGN.md.

Target: **~65+ new tests** after Wave 3 (16 × 4 + 5 globals). Adds ~120% more tests than the current 29.

### 7.3 What's NOT unit-tested (and why)

- `/share`, `/watch` tmux glue — integration-only; use fake `SpawnFn` in tests to assert the command shape without actually launching tmux.
- `handle_ws_message` — out of refactor scope.
- `handle_key_event` — out of refactor scope.
- Forward-compat relay protocol — `ws_message_snapshot` is a local regression guard only. Cross-version compatibility lives in `syncvibe-core::protocol` + relay integration tests.

---

## 8. Risks, tradeoffs, alternatives considered

### 8.1 Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| Behavior drift during migration | Medium | Wave-based approach; each wave compiles + tests before continuing. Golden smoke list in §9 run after each wave. |
| `TuiCtx` leaks AppState internals | Low | TuiCtx API is additive — each method encapsulates one user-observable effect. Adding methods per command is fine (R6). |
| "Where does /share live?" — commands that touch TUI state heavily | Medium | `/share` and `/watch` have run_tui only. `TuiCtx::start_share_session` / `start_watch_session` own the tmux + WS + state tuple. Tested via `SpawnFn` fake. |
| Two sources of truth during migration (dispatch_tui + old match) | Low | Only during waves 1-2. Wave 2 ends when the old match is removed. |
| Commit boundary for rollback | Low | **One command per commit** after Wave 0 (R8). If Wave 2 reveals the trait is wrong, `git revert` individual ports; Wave 0 scaffold is the stable rollback point. |
| Macro `register_commands!` breakage on a stable Rust compiler | Medium | Macro is pure `macro_rules!`, no proc-macro. Wave 0 includes a `cargo check` that exercises the macro with a dummy 3-command invocation before any real ports. |
| `session.rs` dedup breaks clipboard/onboarding UX | Medium | R4 dedup task lands last in Wave 2 with a dedicated smoke test: launch with clipboard `syncvibe://`, confirm prompt still appears. Also: git-prefill name test. |
| Async/threading regression in `/share`, `/watch` | Medium | `TuiCtx::spawn_blocking` uses `std::thread::spawn` + bounded mpsc matching the existing `app.rs:1386` + `ws_client.rs:64` backpressure idioms. No `tokio::spawn` for blocking work (R1). |
| `ws_message_snapshot` flakes due to HashMap ordering | Medium | All fixtures use `BTreeMap` or explicit field serialization. Snapshot is hand-built, no runtime-derived UUIDs. |

### 8.2 Alternatives rejected

- **Event bus / actor model.** Discussed earlier. Premature — no evidence `&mut AppState` is the bottleneck. Adds async plumbing cost for no testability gain.
- **`inventory`-crate auto-registration.** Cute but adds a build-time dep; explicit `all()` table is 20 lines, readable, greppable.
- **Proc-macro DSL (`#[command]`).** Hides structure for no win. Trait impl is already boilerplate-light.
- **Moving commands into a separate crate.** No gain — syncvibe-cli is already one crate with one binary. No downstream consumers.

---

## 9. Manual smoke test list (run after each wave)

Executable checklist. If any of these drift, the wave is wrong.

1. `syncvibe` with no config → prompts for name, saves profile, launches menu.
2. `syncvibe connect XXXX-XXXX` where code already joined → relaunches existing tmux session, no re-init.
3. In TUI: `/invite` → share message copied to clipboard, system msg shows code.
4. In TUI: `/name Bob` → name changes, persists across relaunch.
5. In TUI: `/name claude` → "reserved for AI agent" system msg, no change.
6. In TUI: `/color #ZZZZZZ` → "Invalid color" system msg.
7. In TUI: `/remote` with no arg → shows current git remote or "no remote configured".
8. In TUI: `/remote https://github.com/x/y.git` → git remote set, room config updated, success msg.
9. In TUI: `/leave` → confirmation prompt, on yes: .syncvibe/ removed, room picker appears.
10. In TUI: `/help` → lists all 16 commands with descriptions.
11. Type `/inv` + Tab → autocompletes to `/invite ` (trailing space because needs_arg is false).
12. `syncvibe invite` from CLI → prints same share message as `/invite` in TUI (modulo clipboard).
13. `cargo test -p syncvibe --lib` → 29 existing + N new tests, all green.

---

## 10. Rollout & sizing [revised per Codex revisions]

| Wave | Scope | Time estimate | Deliverable |
|---|---|---|---|
| 0 | Scaffolding + `register_commands!` macro + ws_message_snapshot fixtures + theme.rs + panic isolation + tracing + mock_ctx builder + `WsTransport` trait seam | 5.5h | commands/ skeleton, theme.rs tokens, macro dry-run on 3 dummies, dispatch_tui wired with catch_unwind + tracing, ws fixtures committed, CtxBuilder with capture_ws + capture_spawn, `WsTransport` trait with single `NativeWsTransport` impl (prep for future IRCv3 backend, see ARCHITECTURE.md §Substrate Strategy) |
| 1 | Pilot `/name` + `/clear` + `/share` (3 boundary cases) | 5h | 3 commands migrated, 12+ tests (3 unit + 1 integration each), TuiCtx boundary validated under state-mutation, atomic-wipe, AND tmux-spawn stress |
| 2 | Remaining 13 commands (was 14; `/share` moved to W1) + `session.rs` dedup + bulk theme migration | 8-9h (split into 3 sessions) | handle_command() gone, session.rs shrunk to ~60 LoC, ~55 tests, one commit per command, zero raw Color::Rgb outside theme.rs |
| 3 | Cleanup (autocomplete, needs_arg list, CLI↔TUI parity test, registry uniqueness test, color guard test, docs) | 2-3h | app.rs ≤ 1200 LoC, 5 global tests, one source of truth mechanically enforced |

**Total: ~20.5-22.5 hours across 6-7 sessions.** Fits within a 4-day window with margin.

---

## 11. Open questions for /plan-eng-review

1. Is the `Command` trait the right abstraction, or should commands just be `pub fn run_core(ctx, arg) -> Result<CoreOutcome>` free functions collected into a registry? Tradeoff: trait is more ceremonial but enables `needs_arg()` as a method.
2. Should `CoreOutcome::StateChange` carry structured variants (`NameChanged(String)`, `ColorChanged(String)`, `Muted(bool)`), or is a stringly-typed `Message(String)` enough? Former is more testable, latter is simpler.
3. Is `flows/` the right home for the interactive `handle_new_project` / `handle_join_project` / `handle_leave_room` dance, or should they live in `commands/` as well?
4. Should the CLI-only subcommands (`Auth`, `McpServer`, `Completions`, `WatchRender`, `Dashboard`) also adopt the command trait for uniformity, or stay as-is (they're not chat commands, trait adds no value)?
5. `test_support::mock_ctx` needs a tmpdir per test. Use `tempfile` crate (~1 new dep) or roll a minimal RAII guard? Current Cargo.toml has no temp-dir dep.

6. Friction items in §13 — should we bundle the cheap wins (F3 cancel hint, F4 default agent, F6 post-launch tip) into this refactor, or keep N7 strict and defer all UX fixes to a follow-up?

7. Agent detect/install (§14) — confirm it's a separate spec/PR, not a wave of this refactor. If the reviewer disagrees, we slot it after Wave 3 as Wave 4.

8. After Wave 2, should the `/invite` and `/leave` CLI subcommands (main.rs:194-346) be collapsed into one-line calls to `commands::invite::run_core` + `commands::leave::run_core`, or keep them hand-rolled to avoid coupling clap flags to CoreOutcome?

---

## 12. What this spec is explicitly NOT

- Not a license to clean up adjacent code ("surgical changes" rule — only touch command files + registry wiring).
- Not an event-driven rewrite.
- Not an MCP refactor.
- Not a storage schema change.
- Not a protocol change.
- Not a tmux refactor.

If the review signs off on the abstractions in §4 and the wave plan in §6, we proceed. If not, we revise here before touching code.

---

## 13. UX / beginner-friendliness audit (grounded, file:line)

User asked: are interactive commands (onboarding, `/new`, `/join`) friendly enough for newcomers? Reading the code, here's the honest audit. **Classified by whether they overlap the refactor or are separate UX work.**

### 13.1 What already works

- **Clipboard invite detection** — `session.rs:68-107` checks clipboard for `syncvibe://` or short code on launch and offers to join. Zero-friction join path.
- **Git user.name prefill** — `session.rs:17-30` uses `git config user.name` as default name prompt. One less thing to type.
- **Folder-creation fallback** — `init.rs:87-121` on `prepare_project_dir` failure prompts for an alternate path instead of crashing. Handles `~/`, absolute, relative.
- **Soft-fail on clone error** — `init.rs:137-143` when git clone fails, prints "Ask the room owner to add you on GitHub, then `/remote <url>` in chat" and continues in chat-only mode. Good recovery.
- **Per-file reason in setup checklist** — `init.rs:219-345` each setup item carries a `reason` field shown in `onboarding::confirm_setup`. User understands what each file does before approving.
- **Auto-init git in setup_and_launch** — `init.rs:22-35` inits git if missing. No need to read docs about "must have git first."
- **Destructive confirmation on /leave** — `app.rs:2092` uses `confirm_destructive` with warnings: "Chat history will be permanently deleted and cannot be recovered." User understands blast radius.

### 13.2 Friction points for newcomers

Labeled by fix cost (L/M/H) and whether they overlap the refactor.

| ID | Friction | File:line | Cost | In scope? |
|---|---|---|---|---|
| F1 | No preflight for `tmux` on PATH. User without tmux hits a raw error at launch time. | `tmux.rs` (launch_project), `session.rs:55` | M | **No** — feature add |
| F2 | **No agent-binary detection.** User picks "Claude" but `claude` isn't installed → agent pane silently dies inside tmux. No guidance. | `agents.rs:13-35` (static table) | M | **No** — see §14 |
| F3 | `/new` prompt "Room name:" has no visible cancel hint. Only Enter-with-empty works. `/join` has one ("Press Enter with no input to go back.") — inconsistent. | `app.rs:1923-1935` vs `app.rs:2000` | L | **Maybe** — covered by command port in Wave 2 |
| F4 | `select_agent()` has no default / no recommendation. First-time user facing 3 options with no hint. | `agents.rs:53-72` | L | **No** — feature add |
| F5 | Auth-gate error from `require_auth` is terse. If unauthed, user sees a short error and returns to TUI with unclear next step. | `app.rs:1915`, `config.rs:require_auth` | L | **No** — copy tweak, unrelated |
| F6 | No post-launch "what now" tip. First-time user lands in TUI with no tour; has to discover `/help`, `/invite` alone. `TIPS` table exists at `app.rs:24-45` — verify it's actually shown on first launch. | `app.rs:24-45` | L | **No** — UX work |
| F7 | `setup_and_launch` auto-inits git (init.rs:22), but `prepare_project_dir` inits git only AFTER folder creation succeeds (init.rs:155-168). Asymmetric — if folder creation fails, user hits git init later. | `init.rs:22-35` vs `init.rs:155-168` | L | **No** — unrelated cleanup |
| F8 | Room name silently sanitized. "My Project" becomes "My-Project" with no explanation. Directory under `~/SyncVibe/` is opaque to new users. | `app.rs:1926`, `onboarding::sanitize_name` | L | **No** — copy tweak |
| F9 | No indication where `~/SyncVibe/` is, what's in `.syncvibe/`, or how to find it later. | `init.rs:11-14` | L | **No** — docs |
| F10 | Invite code format `XXXX-XXXX` isn't obvious in the prompt. Users paste `syncvibe://...` deep links but the prompt says "Invite code:" — `invite.rs::resolve_short_invite` handles both, but the prompt doesn't hint it. | `app.rs:1989`, `invite.rs:154` | L | **No** — copy tweak |

### 13.3 Recommendation for this refactor

- **Keep N7 strict.** UX fixes are a separate concern from the refactor. Bundling them makes migration review harder and violates "surgical changes."
- **Exception: F3 (cancel hint in `/new`).** This naturally falls out when we port `/new` to `commands/new_room.rs` in Wave 2. It's a 1-line add and unifies the pattern with `/join`. **Include in Wave 2, call out in PR.**
- **Write a follow-up spec** (`/tmp/syncvibe-ux-followup.md`) listing F1, F2, F4-F10 with priority ranking. Ship after refactor lands. F2 (agent-install) gets its own §14 treatment below because it's substantial.

### 13.4 Pre-refactor verification (cheap, do now)

Before Wave 0 starts, verify:
- Run `syncvibe` without being authed → confirm `require_auth` error is actually terse (F5). If it's already friendly, drop F5.
- Check `TIPS` rotation in `app.rs:24-45` is shown somewhere on first launch (F6).
- Confirm `clipboard::read_clipboard` on Windows actually works via PowerShell (invite.rs has the code path but we haven't QA'd on Windows).

---

## 14. Agent detect / install helper — scoped as follow-up feature

**Status:** out of scope for refactor (N7). Documented here so the reviewer sees the full picture of where `agents.rs` is headed, and so the `Command` abstraction in §4 accommodates it.

### 14.1 Problem

Today `agents.rs:13-35` declares a static table:

```rust
pub const AGENTS: &[AgentDef] = &[
    AgentDef { id: "claude",  command: "claude",  ... },
    AgentDef { id: "codex",   command: "codex",   ... },
    AgentDef { id: "gemini",  command: "gemini",  ... },
];
```

`select_agent()` (agents.rs:53-72) shows all three regardless of whether the binary exists on PATH. User picks "Claude" → `tmux` launches an agent pane running `claude` → if not installed, pane shows an error from the shell and sits dead. No guidance.

Similar failure mode for `codex` and `gemini`. No recovery path inside SyncVibe.

### 14.2 Proposed design (for follow-up spec, NOT this refactor)

**New fn in `agents.rs`:**

```rust
pub enum AgentStatus {
    Installed(PathBuf),           // resolved path from which::which
    NotFound,                      // binary not on PATH
}

pub fn check_installed(agent: &AgentDef) -> AgentStatus {
    // Zero-dep: use `which` shell command with a 1-second timeout.
    // Alternative: add `which = "7"` crate (adds ~15KB, cleaner API).
    match std::process::Command::new("which").arg(agent.command).output() {
        Ok(o) if o.status.success() => {
            let path = String::from_utf8_lossy(&o.stdout).trim().to_string();
            AgentStatus::Installed(PathBuf::from(path))
        }
        _ => AgentStatus::NotFound,
    }
}
```

**Modify `select_agent()` menu rendering:**

```
Agent: Which AI agent to use?

❯ Claude    (installed)
  Codex     (not installed)
  Gemini    (not installed)

  [Enter] select   [i] show install command   [Esc] cancel
```

**On "not installed" selection, offer an install prompt (opt-in, not auto-run):**

```
Codex is not installed.

Install command:
  npm i -g @openai/codex

  [y] run it now     [n] I'll install it myself    [s] pick a different agent
```

`y` shells out; `n` proceeds with the agent choice anyway (user plans to install later, or uses SyncVibe just for chat without agent pane).

### 14.3 Install command table

Add to `AgentDef`:

```rust
pub struct AgentDef {
    pub id: &'static str,
    pub command: &'static str,
    pub install_hint: &'static str,  // human-readable install one-liner
    pub install_cmd: Option<&'static [&'static str]>,  // argv for automated install; None = manual only
    ...
}
```

Initial table:
- Claude: `install_hint = "npm i -g @anthropic-ai/claude-code"`, `install_cmd = Some(&["npm", "i", "-g", "@anthropic-ai/claude-code"])`
- Codex: `install_hint = "npm i -g @openai/codex"`, `install_cmd = Some(&["npm", "i", "-g", "@openai/codex"])`
- Gemini: `install_hint = "See https://ai.google.dev/gemini-api/docs/downloads"`, `install_cmd = None` (manual only — spec/binary changes often)

### 14.4 Preflight: tmux, git

Same pattern extends to other tools we assume on PATH:
- `tmux` — check in `cmd_session` before launching any room.
- `git` — already used at `init.rs:24`; add a check with a friendlier error message pointing to install docs.

### 14.5 Why NOT in this refactor

- It's a feature. §1 motivation is testability + duplication.
- It adds Cargo footprint if we use `which` crate.
- It touches `agents.rs` heavily, which otherwise is not on the refactor path.
- Bundling it makes PR review harder.

**Slot it as Wave 4** (post-refactor) with its own 3-5 hour budget. Don't mix.

---

## 15. Cross-repo awareness — syncvibe-relay contract we MUST preserve

Grounded in `~/Desktop/Claude Code/syncvibe-relay/src/{index.ts,room.ts,types.ts}` (all read in this session, 3 files total, ~850 LoC).

This section documents what the refactor cannot break. It's short because relay is small and our refactor doesn't touch it — but we WILL edit command files that emit WsMessages, so we need the contract spelled out.

### 15.1 Relay architecture (1-screen summary)

- **Runtime:** Cloudflare Workers + Durable Objects + KV.
- **Deployment:** `relay.syncvibe.online` (wrangler.toml:6-10).
- **Two files:** `index.ts` (HTTP router, 407 LoC) + `room.ts` (Durable Object per room, 456 LoC) + `types.ts` (shared shapes).
- **Storage:** KV `INVITE_CODES` binding for invite + auth codes (7-day TTL for invites, 5-min for auth). DO storage holds `room_secret` + `idle_since` per room.

### 15.2 HTTP endpoints (contract)

| Method + path | CLI caller | Relay file:line | Contract |
|---|---|---|---|
| `GET /ws/{uuid}` | `network/ws_client.rs` | `index.ts:69-75` → `room.ts:94-148` | Upgrades to WebSocket. roomId must match `^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$`. |
| `POST /invite` | `invite.rs::create_short_invite` | `index.ts:78-196` | Body: `{room_id, room_secret, room_name?, relay_url?, git_remote?}`. Returns `{code: "XXXX-XXXX"}`. Rate-limited 10/min/IP. room_secret must be 64-char hex. 7-day TTL. |
| `GET /invite/:code` | `invite.rs::resolve_short_invite` | `index.ts:198-233` | Normalizes `XXXX-XXXX` → 8 chars, uppercase. Returns full room blob or 404. |
| `POST /auth/:code` | (web only, not CLI) | `index.ts:239-340` | Web deposits token for CLI to poll. 5-min TTL, one-time. |
| `GET /auth/:code` | `auth.rs` | `index.ts:343-387` | CLI polls for token. Relay deletes on successful retrieval. |
| `GET /` or `/health` | none | `index.ts:390-400` | Liveness. |

**CORS:** relay allows only `https://syncvibe.online` + `www.syncvibe.online` for browser requests. CLI (no Origin header) is always allowed.

### 15.3 WebSocket protocol (what commands touch)

**Client → Relay (must be first message):**
```json
{"type":"auth","data":{"room_id","room_secret","user_id","user_name","user_color","agent_id"?}}
```
- `user_id`: `^[a-z0-9-]{1,64}$` — UUID for humans, `agent-{id}` for AI.
- `room_secret`: `^[0-9a-f]{64}$` — constant-time compared against stored.
- `agent_id`: `^[a-z0-9-]{1,32}$` or null.

**Relayable types** (`room.ts:43-51`): `chat_message, git_status, conflict_warning, presence_update, screen_share_start, screen_share_stop, screen_frame`. Anything else is DROPPED by relay — do not invent new relay types without updating the relay.

**Server-only types** (sent by relay, never by client): `auth_ok, auth_fail, user_joined, user_left, pong`. If our CLI ever tries to send these as a `chat_message` substitute, the relay silently drops them.

**Identity stamping** (`room.ts:315-335`): relay OVERWRITES `msg.data.user_id/user_name/user_color` with the authenticated attachment's values, UNLESS the connection authenticated with a matching `agent_id` AND `msg.data.user_id === "agent-{agent_id}"`. This is the anti-spoof mechanism. Our CLI's `agent-claude` / `agent-codex` / `agent-gemini` userId scheme relies on this.

**Size and rate limits** (`room.ts:8-14`):
- Max message size: **256 KB** (screen frames can be large — chat messages nowhere near this).
- Max messages: **20/second per socket**. Over → silently dropped.
- Max sockets per room: **100** (`room.ts:121-123`). Over → 503 at connect.
- Auth timeout: **30 s** — unauthenticated sockets closed by alarm.
- Idle cleanup: **1 hour** after last member leaves → DO storage fully wiped.

### 15.4 Refactor impact on relay

**None — intentionally.** Our refactor changes only the CLI's command layer. No `WsMessage` shape changes, no new message types, no endpoint changes.

**Verification in Wave 2:**
- Add an integration test that serializes a `chat_message` before and after the refactor and asserts byte-identical JSON.
- Existing `tests/relay_integration.rs` (288 LoC) already covers the auth + relay path — keep it green through every wave.

**If the refactor ever NEEDS a new message type:** that is a cross-repo change — relay `RELAYABLE_TYPES` set must be updated first, deployed, THEN CLI updated. Document this as a team process in a follow-up if it ever comes up; not needed today.

### 15.5 Things in relay the CLI does NOT rely on (but are worth knowing)

- `env.INVITE_CODES` is a **single KV namespace** (`wrangler.toml:16-18`) — invite codes and auth codes share it, distinguished by `auth:` prefix. If we ever add a third use case, don't clash.
- Durable Objects use **SQLite** storage class (`wrangler.toml:20-23`) — cheaper billing, not a capability limit.
- Relay has **no database** — all state is KV (short-TTL) or DO (per-room). Sync-to-Supabase is the CLI's job (see `sync.rs`), not relay's.

---

## 16. Summary of additions relative to v1 of this spec

For the reviewer: what changed between the first draft and this version.

- Added §13 — UX audit with 10 friction items labeled by scope (L/M/H cost, in/out of refactor).
- Added §14 — agent detect/install helper as a follow-up spec stub, explicitly deferred.
- Added §15 — cross-repo relay contract with byte-identical invariant we must preserve.
- Added N6, N7 (non-goals) — relay stability + deferred agent install.
- Added Q6, Q7, Q8 (open questions) — scope boundary checks.
- One bundled exception: F3 cancel-hint fix will land during Wave 2 as a natural part of porting `/new`. Flagged in PR.

---

## 17. Summary of revisions relative to v2 (post-Codex adversarial review)

Codex adversarial review (session 019db8e8-9df7-7011-a67c-20301cc24b70) flagged four structural blockers and six structural risks. Spec revisions:

**Blocker fixes:**
- §3.4 R1 — rejected `Command::run_async` trait-level BoxFuture. Kept sync trait; per-command `std::thread::spawn` + bounded mpsc for the 2-3 commands that need it.
- §3.4 R2 + §4.4 — replaced hand-written `all()` with `macro_rules! register_commands!` expanding to both the dyn registry AND clap Subcommand enum from a single token list.
- §3.4 R3 + §4.7 — `HttpRemoteApi` wraps `ureq` (not `reqwest`, which is dev-only per Cargo.toml:33). New `commands/adapters.rs` module.
- §3.4 R4 + §5 + §6 Wave 2 — `session.rs` deduplication is now in scope. `session.rs` shrinks 236 → 60 LoC; `ensure_user_profile` + clipboard invite detect migrate to `flows/onboarding.rs`.

**Structural fixes:**
- §3.4 R5 + §4.2 + §4.5 — `CoreOutcome::StateChange` deleted; closed enum with exhaustive match + `#[deny(unreachable_patterns)]`.
- §3.4 R6 + §4.3 — TuiCtx accessor count uncapped; each method encapsulates one user-observable effect. Added `set_display_name`, `clear_chat_state`, `start_share_session`, `start_watch_session`, `send_ws`, `spawn_blocking`.
- §3.4 R7 + §6 Wave 1 — pilot commands changed from `/name` + `/invite` to `/name` + `/clear` (stresses TuiCtx boundary on atomic multi-field wipe).
- §3.4 R8 + §6 Wave 2 — one commit per command ported; Wave 0 is the stable rollback point.
- §3.4 R9 + §7 — coverage expanded from 3 unit tests/command to 3 unit + 1 integration, plus 4 global tests (registry uniqueness, CLI↔TUI parity, ws_message_snapshot, needs_arg autocomplete parity). Total: ~64 tests.
- §3.4 R10 + §7.3 — `ws_message_snapshot` clarified as local regression guard, not protocol spec.

**Sizing:** total bumped from ~13-15h to ~17h across 5-6 sessions.

---

## 17b. Revisions v3 → v3.1 (post-/design-consultation)

`/design-consultation` produced `syncvibe/DESIGN.md` codifying the TUI design system (editorial-terminal, single teal accent, 5-color user palette, Ctrl-P presence rail, ASCII poster cold-start, no motion). Since the refactor already rewrites call sites, color/theme migration rides the strangler waves instead of a separate cleanup project.

**Spec changes:**
- §5 After tree — added `theme.rs (~120)`.
- §6 Wave 0 — new task 0.0: create `theme.rs` (tokens + semantic Style fns) before command scaffolding, per DESIGN.md §实现契约.
- §6 Wave 2 — new final task "W2-theme bulk color migration" (separate commit): replace every raw `Color::Rgb`/`Color::Indexed`/hex literal in `app.rs` + `components/*` with `theme::sv_*` tokens; replace 8-color candy user palette with `theme::USER_PALETTE` (5 colors); replace `Rgb(30,100,160)` selection bg with `theme::sv_surface`.
- §6 Wave 3 — new task 3.4a: add `no_raw_color_at_call_sites` guard test (grep src/app.rs + src/components for `Color::Rgb(`, `Color::Indexed(`, `#RRGGBB`; assert zero hits outside `src/theme.rs`).
- §7.2 — new global test `no_raw_color_at_call_sites` (5th global). Coverage target updated: ~65+ tests.
- **Sizing delta:** +2h Wave 0 (theme.rs + token table) + 1-2h Wave 2 (bulk replace is mechanical, mostly Edit tool). New total: ~19-20h across 6-7 sessions.

**Zero behavior change:** all token hex values match existing colors 1:1 except three explicit visual upgrades already agreed in DESIGN.md (accent consolidated to single teal; user palette reduced to 5; selection bg warmed). These three are the only user-visible visual diffs and are covered by manual smoke (§9).

---

## 17c. Revisions v3.1 → v3.2 (post-/plan-ceo-review, HOLD SCOPE mode)

`/plan-ceo-review` surfaced 3 gaps (G1-G3) in Section 2 Error & Rescue Map + 4 structural must-fix items. HOLD SCOPE verdict: GO, fold 4 must-fix into spec, defer 3 suggestions + 2 follow-ups to TODOS.md. No scope expansion beyond the original refactor.

**Spec changes (must-fix, folded in):**
- §6 W0 task 0.0 — `sv_*` tokens now `pub const Color`; semantic Style fns `#[inline]`. Prevents per-frame alloc (brand() called hundreds of times per redraw).
- §6 W0 — new tasks 0.7 (dispatcher `panic::catch_unwind` + toast + tracing::error) and 0.8 (tracing dispatch cross-cut: `debug!` on entry, `warn!` on Err; adds `tracing` + `tracing-subscriber` deps if missing; env-gated subscriber, writes to stderr). Rationale: refactor expands dispatch surface from 16 to 64+ indirection points.
- §6 W1 — pilot count increased from 2 → 3: `/name` + `/clear` + `/share`. `/share` validates `TuiCtx::start_share_session` (tmux spawn + ws broadcast) at pilot time, not Wave 2. `/share` removed from Wave 2 command order.
- §7.1 `mock_ctx()` — builder explicit: `with_capture_ws`, `with_capture_spawn`, `with_clock`. `CmdCtx` marked `#[non_exhaustive]`. Tests that call ws-broadcasting commands without `with_capture_ws` fail loudly (`CmdCtx::ws_required()` marker) instead of silently passing.
- §10 sizing — W0: 3h → 5h, W1: 4h → 5h, W2: 8h → 8-9h, W3: 2h → 2-3h. New total: **~20-22h across 6-7 sessions.** +3-5h vs v3.1.

**Deferred to TODOS.md (suggested but out of HOLD SCOPE):**
- `RemoteError` enum from anyhow (typed error classification for command-side branching). +0.5h, Wave 2 after mid-point review if budget allows.
- `CmdCtx` soft-redline: > 12 fields triggers structural re-review before Wave 3. ARCHITECTURE.md Known Limitations note.
- `send_ws` error logging: today `let _ = ws.send(...).await` (silently drops). Wave 3 follow-up PR upgrades to `tracing::warn!`. Not in refactor to preserve N1 zero-behavior-change.

**Deferred to separate future spec (NOT in this refactor):**
- Sentry crash reporting (free tier adequate, but 3 blockers: Ratatui alternate screen panic hook conflict needs `restore_terminal()` first; DSN leakage requires build-time env var; user privacy requires opt-in config flag). 2-3h standalone effort post-Wave 3.
- Plugin system (third-party commands) — `register_commands!` is static by design; dynamic registration lives in future N2 event-bus migration.

**Failure Modes Registry added:**
1. W2 batch migration mid-flight ambiguity (half trait / half match) — mitigated by `cargo test --all` gate per commit.
2. CmdCtx field explosion > 12 — triggers Wave 3 pre-flight review.
3. mock_ctx builder field drift — mitigated by `#[non_exhaustive]` + compile-error fallback.
4. `no_raw_color_at_call_sites` regex false positives on commented-out code — use `^[^/]*Color::Rgb` or strip comments before grep.
5. Wave 3 cleanup deleting pre-migration code — task 3.1 pre-flight checks all 16 commands are registered before removing `COMMANDS` const.

**Verdict: GO.** All must-fix items integrated. Start Wave 0.

---

## 18. NOT in scope (explicit exclusions)

Hard-line list. If work below shows up in a Wave PR, reject it.

- **WsMessage reducer (`handle_ws_message` in `app.rs:1815+`).** Out of refactor. Byte-identical output enforced via `ws_message_snapshot` but the reducer itself stays in `app.rs`.
- **Key / mouse event handling.** `handle_key_event`, `handle_mouse_event` stay where they are.
- **`draw_ui` and all Ratatui rendering code.** Zero visual changes.
- **`syncvibe-core` crate (Storage, Protocol, models).** N3 locks this. All model types remain unchanged.
- **`syncvibe-relay` (Cloudflare Workers + Durable Objects).** N6 locks this. See §15 for the contract.
- **Agent detect / install helper.** N7 + §14 — deferred to a separate spec.
- **MCP server (`mcp/server.rs`).** N4 — orthogonal, reads chat log directly.
- **Non-TUI CLI subcommands: `Auth`, `McpServer`, `Completions`, `WatchRender`, `Dashboard`.** They stay hand-written; they are not chat commands and the macro adds no value.
- **New dependencies.** N5. No `async-trait`, no `inventory`, no `tempfile`, no `reqwest` promotion. Test support uses manual RAII tmpdir guard.
- **Behavior changes beyond F3 cancel-hint.** N1 — zero user-visible diff except where §13 bundled exceptions are explicitly called out.
- **Event bus / actor architecture.** N2 — evaluated, rejected, not revisiting. See `~/.claude/projects/-Users-harry/memory/refactor_event_driven.md` for the deferred redesign.
- **Anything in `init.rs`.** Room init / git clone / tmux spawn flows stay where they are. Only `session.rs` is touched (R4).
- **Dashboard / audit-verification logic.** Out of scope.
- **Changing the `/command` user-visible command names or aliases.** S1 locks the count and names at 16.
- **Fixing `session.rs:68-107` clipboard logic behavior.** The move to `flows/onboarding.rs` is pure relocation; behavior is byte-for-byte identical and covered by smoke test.

---

## 19. What already exists (don't rebuild)

Scope protection. Reviewers or later waves sometimes re-invent these. Don't.

- **Bounded WS queue (256).** `ws_client.rs:64`. `spawn_blocking` backpressure idiom reuses this, does not replace it.
- **Coalescing `try_send` on filesystem bursts.** `app.rs:1386`. New `TuiCtx::spawn_blocking` follows the same pattern.
- **Clipboard invite detection on launch.** `session.rs:68-107`. Moves to `flows/onboarding.rs::detect_clipboard_invite` verbatim.
- **Git user.name prefill.** `session.rs:17-30`. Moves to `flows/onboarding.rs`.
- **Autocomplete `needs_arg` table.** `app.rs:2569`. Deleted in Wave 3; `Command::needs_arg()` replaces it.
- **`COMMANDS` autocomplete table.** `components/autocomplete.rs`. Deleted in Wave 3; `commands::completions()` replaces it.
- **`handle_command` match arm dispatcher.** `app.rs:439+`. Deleted incrementally as each command ports; final arm removed at end of Wave 2.
- **`cmd_invite`, `cmd_connect`, `cmd_leave` etc. in `main.rs`.** Replaced by macro-generated `dispatch_cli`.
- **Reserved name check (`is_reserved_name("claude")`).** `onboarding.rs`. Reused by `commands/name.rs::run_core`. Not re-implemented.
- **Color hex validator.** `onboarding.rs`. Reused by `commands/color.rs`.
- **Short-code parser / invite URL detector.** `session.rs` branches. Moves to `flows/onboarding.rs`, called by both `session.rs` wrapper and `commands/join_room.rs`.
- **Existing 29 unit tests.** All stay green through every wave; Wave 2 gate is "29 + N green".
- **`audit_verification.rs` shell_escape coverage.** Used by `/share`, `/watch` integration tests. Not duplicated.
- **Dedupe cache + line cache in AppState.** Touched atomically by `TuiCtx::clear_chat_state`; the cache structure is not changed.

---

## 20. Failure modes per new codepath

For each new surface introduced by this refactor, explicit failure mode + test that catches it.

### 20.1 `CmdCtx` construction (`commands/ctx.rs`)

| Failure | Symptom | Test that catches it |
|---|---|---|
| `TuiCtx::cmd_ctx()` borrows a field another accessor also wants, triggering E0499 | `cargo check` error | compile gate; if it compiles today, the borrow pattern is sound |
| `CmdCtx::user` / `room` mutated but not persisted | Silent drift: in-memory state right, disk wrong | each unit test reloads UserConfig from tmpdir after `run_core`, asserts equality |
| Mock `GitOps` / `RemoteApi` panic instead of returning `Result` | Test crashes instead of asserting | `NoopGitOps` + `NoopRemoteApi` return `anyhow::Error` for any unset expectation |

### 20.2 `TuiCtx` high-level operations (`commands/tui_ctx.rs`)

| Failure | Symptom | Test that catches it |
|---|---|---|
| `clear_chat_state` wipes chat vec but forgets dedupe cache | Duplicate messages appear after next sync | `clear_chat_clears_all_four_caches` test asserts vec + dedupe + line cache + selection all empty |
| `start_share_session` fires WS broadcast before setting `sharing_screen=true` | Peers see share event, local UI says not sharing | ordering test: mock `SpawnFn`, assert state flag set before WS send call |
| `set_display_name` mutates profile but skips local presence entry update | Sidebar/carousel shows old name until next sync | `set_display_name_updates_local_presence` test asserts own presence entry in vec has new name |
| `spawn_blocking` thread panics | Silent drop; command hangs waiting for event | catch_unwind in the spawn wrapper, converts panic to `UiEvent::CommandFailed` |
| `send_ws` spawned task fails to send (channel closed on disconnect) | Message lost silently | `send_ws_logs_on_send_failure` test uses a closed `WsClient` stub; asserts failure surfaces via `system_msg` / `UiEvent`, not silent drop |

### 20.3 `register_commands!` macro (`commands/mod.rs`)

| Failure | Symptom | Test that catches it |
|---|---|---|
| Duplicate slash name across two entries | Dispatcher picks first; second command silently shadowed | `registry_unique_names_and_aliases` global test |
| Macro expansion produces `ChatCommand` variant names that clash with Rust keywords | Compile error at macro site | compile gate; `register_commands!` dry-run invocation in Wave 0 with realistic entry |
| CLI `ChatCommand` variant missing for an entry in `all()` | CLI subcommand silently unavailable | `cli_tui_parity` test iterates `all()`, asserts every slash has a matching `ChatCommand` |
| Per-entry `needs_arg` flag drifts from command impl | Autocomplete adds trailing space when command expects arg (or vice versa) | `needs_arg_autocomplete_parity` test |

### 20.4 Adapters (`commands/adapters.rs`)

| Failure | Symptom | Test that catches it |
|---|---|---|
| `HttpRemoteApi::create_invite` error body not parsed | Caller sees generic "HTTP error" instead of relay's 4xx reason | contract test with stub ureq response, assert error message includes relay reason |
| `ureq` timeout too long, blocks TUI thread | Command appears frozen for 30s | default `ureq::Agent` with 10s read + 10s connect timeout; asserted in unit test via agent config |
| `RealGitOps::set_remote` leaves git repo in half-configured state on failure | Room config points at remote that git doesn't know | `RealGitOps::set_remote` wraps the call; on failure, reverts in-memory config before propagating error |

### 20.5 `flows/onboarding.rs` (post-R4)

| Failure | Symptom | Test that catches it |
|---|---|---|
| Clipboard contains non-UTF8 binary; `detect_clipboard_invite` panics | CLI crashes on launch | fuzzing test with non-UTF8 bytes, asserts no panic |
| User pastes `syncvibe://xxx` with trailing whitespace; parser misses it | Prompt doesn't appear, user types name again | strip before match; test with leading + trailing whitespace variants |
| `ensure_user_profile` called from both `session.rs` wrapper and `commands/join_room` races on config file | Two processes: last-writer-wins, first profile lost | process-level lock file on UserConfig write (existing behavior in `save_user_config`); assert via dual-process integration test |

### 20.6 `ws_message_snapshot` fixtures (Wave 0)

| Failure | Symptom | Test that catches it |
|---|---|---|
| Fixture built with `Uuid::new_v4()` | Snapshot flakes randomly | lint gate: fixture file must not contain `Uuid::new_v4` / `SystemTime::now` / `chrono::Utc::now`; CI check with grep |
| Struct field uses `HashMap`, serializes in non-deterministic order | Snapshot flakes across runs | migrate affected fields to `BTreeMap`; pre-Wave 0 audit of WsMessage variants |
| New field added to `WsMessage` variant without updating fixture | Snapshot fails on next unrelated change | golden review; PR template checklist item |

---

## 21. Worktree parallelization strategy

Each Wave-2 command port is independent (one commit, one file, self-contained tests). Ripe for parallel worktrees.

**Waves 0, 1, 3 are sequential.** They share files or depend on each other:
- Wave 0 creates scaffolding everyone uses.
- Wave 1 validates the pattern; bad lessons here re-scope everything.
- Wave 3 deletes legacy tables that Wave 2 stopped using.

**Wave 2 parallelizes cleanly.** Each command port touches:
- `commands/foo.rs` (new file, no conflict)
- One match-arm deletion in `app.rs` (small, disjoint — merge conflicts are trivial)
- One `register_commands!` macro entry (central file, sequential merge; queue order doesn't matter because entries are position-independent)

**Recommended split (4 parallel worktrees):**

| Worktree | Commands | Est. time |
|---|---|---|
| `wt-easy` | `/quit`, `/mute`, `/rc`, `/chats`, `/help` | 2h |
| `wt-state` | `/color`, `/remote`, `/collab`, `/invite` | 3h |
| `wt-flow` | `/new`, `/join`, `/leave` | 2h |
| `wt-tmux` | `/share`, `/watch` | 2h |

Merge order: easy → state → flow → tmux (lowest risk first). `session.rs` R4 dedup lands on main after all four worktrees merge, as the final Wave 2 commit.

**Conflict zones (watch for these):**
- `commands/mod.rs` macro invocation — each worktree adds one line; rebase-merge each worktree in turn.
- `app.rs::handle_command` match arms — disjoint per command; git handles this.
- `main.rs` CLI `cmd_foo` handlers — each worktree removes its corresponding `cmd_foo`; disjoint.
- `components/autocomplete.rs` — only touched in Wave 3, so no Wave 2 conflicts.

**Non-parallelizable:**
- `flows/onboarding.rs` R4 extraction — sequential, last Wave 2 task.
- Any PR that touches `TuiCtx` accessor definitions — serialize through `commands/tui_ctx.rs` to avoid merge hell.

---

## 22. Completion summary

| Section | Status | Source of truth |
|---|---|---|
| §1 Motivation | Locked | app.rs LoC count + 4-6 file edit pain |
| §2 Current state | Locked | Read from code |
| §3 Goals + success criteria | Locked | G1-G4, S1-S6 |
| §3.4 Post-Codex revisions | Locked | R1-R10 |
| §4 Design (trait, CmdCtx, TuiCtx, macro, per-command, CLI, adapters) | Locked | §4.1-4.7 |
| §5 File tree | Locked | Reflects R4 session.rs shrink + adapters.rs + flows/onboarding.rs |
| §6 Migration waves | Locked | Wave 0 (5h, +panic+tracing+theme), Wave 1 /name+/clear+/share (5h), Wave 2 + session.rs + bulk theme (8-9h), Wave 3 (2-3h) |
| §7 Testing (~65+ tests) | Locked | 4 per command + 5 globals; mock_ctx explicit builder with capture_ws/capture_spawn |
| §8 Risks | Locked | 10 entries covering macro, session.rs, async, ws snapshot |
| §9 Smoke tests | Locked | 13 items, byte-identical output |
| §10 Sizing | Locked | ~20-22h, 6-7 sessions (v3.2) |
| §11 Open questions | Stale (pre-Codex) | Kept for history; superseded by §3.4 |
| §12 Explicit non-goals | Locked | Redundant with §18 but kept |
| §13 UX audit | Locked | 10 friction items; F3 bundled, others deferred |
| §14 Agent install stub | Locked | Out of scope, separate spec |
| §15 Relay contract | Locked | Byte-identical invariant |
| §16 v1→v2 delta | Locked | |
| §17 v2→v3 delta (post-Codex) | Locked | |
| §17b v3→v3.1 delta (post-/design-consultation) | Locked | theme.rs + color guard test |
| §17c v3.1→v3.2 delta (post-/plan-ceo-review HOLD SCOPE) | Locked | 4 must-fix: const/inline tokens, panic isolation + tracing, mock_ctx builder with captures, Wave 1 pilot +/share |
| §18 NOT in scope | Added | Hard rejection list for reviewers |
| §19 What already exists | Added | Reuse registry |
| §20 Failure modes | Added | Per-codepath test mapping |
| §21 Worktree strategy | Added | 4 parallel worktrees for Wave 2 |

**Ship readiness (v3.2):** spec is implementable. Start with Wave 0 (now 5h: scaffolding + theme + panic/tracing + mock builder).
**Remaining uncertainty:** macro expansion in real stable Rust with 16 entries. Mitigated by Wave 0 dry-run on 3 dummy entries before committing.
**Outside voice (Codex) aligned:** all 4 blockers addressed, 6 structural concerns addressed, simpler-path alternative noted but rejected (trait + macro buys G3 mechanical enforcement that free-functions + table cannot).
**CEO review aligned (HOLD SCOPE):** 3 gaps surfaced (G1 panic, G2 send_ws silent drop, G3 RemoteError typing), 4 must-fix folded in, 3 suggestions deferred to TODOS.md, 2 items deferred to separate future specs (Sentry, plugin system). No scope expansion.
