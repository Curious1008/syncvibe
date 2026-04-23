# Changelog

All notable changes to SyncVibe are documented here.

This project follows [Semantic Versioning](https://semver.org/) and the [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) conventions.

---

## [0.5.0] - 2026-04-24

First release aligned to the new positioning: **SyncVibe is a remote pair-teaching tool for terminal-based AI coding agents**. The experienced user stays on their own machine, the learner stays on theirs, and the teacher drives the **learner's own agent** via chat. The loop — chat, terminal view, agent trigger, all co-located — is the product.

This release is a foundations push. Most of the work is invisible to end users: a full refactor of the slash-command surface into a registry, a two-row status bar, agent-mention disambiguation that handles same-agent ambiguity and username collisions, and a large test expansion. Shipping as 0.5.0 (not 0.4.8) because the command dispatch, mention syntax, and status bar layout are all structurally new under the hood, and we want the version number to say so.

### Added

- **Agent mention disambiguation.** `@claude` still works when exactly one teammate runs Claude; ambiguous cases now require `@claude(Alice)` to name the owner. Names collide? The TUI auto-appends a 4-hex-char suffix from the user id, e.g. `@claude(Alice#7af)` vs `@claude(Alice#b2c)`. Tab-completion shows the disambiguated form. Routing on both trigger sites (local + remote WebSocket) navigates the full `MentionOwner { name, suffix }` struct.
- **Two-row status bar.** Row 0 carries the brand, version, online dot, live-share marker, and toast slot. Row 1 carries the agent rail (unique agents in the room), a spacer, a carousel of other users, and the "me" pill on the right. Narrow terminals collapse the version and overflowing users into `+N`.
- **Room name in the tmux pane title.** The left pane's tmux title now shows the room name instead of the hard-coded `SyncVibe Chat`, so the brand is stamped once (status bar Row 0) rather than twice.
- **Ctrl+C-twice to exit.** First press shows a full-width red banner in the chat surface; a second press within two seconds quits. Prevents accidental session kills.
- **Cancellable prompts.** Onboarding prompts (`/new`, `/join`, display name, etc.) accept Esc to cancel cleanly without aborting the session.
- **Test expansion.** 115 unit tests in the CLI crate (up from 88), plus `ws_message_snapshot` regression guard (§7.2), `needs_arg_autocomplete_parity` guard, `cli_tui_parity` guard, and a 3-test-minimum floor for each slash command. Username-collision behavior has explicit coverage.

### Changed

- **Slash commands ported to a registry.** Every slash command (`/help`, `/invite`, `/new`, `/join`, `/leave`, `/chats`, `/share`, `/watch`, `/name`, `/color`, `/remote`, `/collab`, `/mute`, `/clear`, `/rc`, `/quit`) now lives under `commands/` as a discrete file with its own spec, handler, and tests. `/help`, `/autocomplete`, and `needs_arg` are driven off the registry — no more drift between what the TUI claims to support and what it actually dispatches.
- **Event loop split.** `app.rs` no longer owns key handling, mouse handling, or WebSocket message dispatch. Those moved to `events/key.rs`, `events/mouse.rs`, and `events/ws.rs`. Rendering moved to `render.rs`. The refactor is a prerequisite for the eventual event-bus architecture (tracked separately).
- **Color discipline.** All call sites use `theme::sv_*` tokens instead of raw `Color::Rgb(...)` or ad-hoc `Style::default().fg(...)`. A compile-time guard test fails CI if a new raw color sneaks in.
- **Flows extracted.** `/new`, `/join`, and `/leave` lifecycle logic moved into `flows/project.rs` and `commands/{new,join,leave}` helpers; `ensure_user_profile` moved into `flows/onboarding`. Menu branches for invite / join-code / create-room were extracted from the onboarding mega-function.
- **tmux helpers consolidated.** All tmux shell-outs now live in `tmux.rs`; `shell_escape` has a single canonical definition; `capture_agent_pane`, `open_file`, and `copy_to_clipboard` moved into their rightful homes.
- **Dependency bumps.** `rustls-webpki` 0.103.10 → 0.103.13. Rust 1.95 compliance: clippy idiom fixes, `cargo fmt --all`, `allow(dead_code)` applied narrowly to refactor scaffolding.

### Fixed

- Snapshot tests now catch silent WebSocket message-type changes that would have broken relay compatibility.
- `needs_arg` parity test closes the window where a command could declare "needs arg" in the registry but be tab-completed without one.

### Removed

- `AppState::project_name`. The status bar no longer needs it (the tmux pane title carries the room name), and no other code path reads it. `git::ops::repo_name()` is gone with it.

### Internal

- Refactor spec v3.2 landed. Wave 0 through Wave 3 plus Stage 4 file extractions are complete; `app.rs` is down from ~4.5k LoC to ~1.3k and the shape is ready for the event-bus refactor that unblocks plugin surface area.
- Documentation reorganized to reflect the registry architecture (§3.3, §7.2).

---

## [0.4.6] - prior

Prior releases are tagged but not documented here. Run `git log v0.4.6` for history.

[0.5.0]: https://github.com/Curious1008/syncvibe/compare/v0.4.6...v0.5.0
[0.4.6]: https://github.com/Curious1008/syncvibe/releases/tag/v0.4.6
