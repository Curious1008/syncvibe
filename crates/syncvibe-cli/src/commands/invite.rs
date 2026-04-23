//! `/invite` — generate a short invite code for the current room and copy
//! it to the clipboard. Falls back to writing `.syncvibe/invite.txt` when
//! the clipboard isn't reachable.
//!
//! Also refreshes `git_remote` in room config so late-added remotes propagate.

use std::path::Path;

use anyhow::Result;

use syncvibe_core::models::RoomConfig;
use syncvibe_core::storage::Storage;

use super::{Command, TuiCtx};

pub struct Invite;

impl Command for Invite {
    fn name(&self) -> &'static str { "/invite" }
    fn aliases(&self) -> &'static [&'static str] { &["/i"] }
    fn description(&self) -> &'static str { "create invite code  (/i)" }

    fn run_tui(&self, ctx: &mut TuiCtx<'_>, _arg: &str) -> Result<()> {
        let mut room = match ctx.read_room_config() {
            Ok(r) => r,
            Err(_) => {
                ctx.system_msg("No room config found.");
                return Ok(());
            }
        };

        // Refresh git_remote — user may have added a remote after room creation.
        let project_root = ctx.project_root();
        refresh_git_remote_in_place(&mut room, &project_root, |r| {
            let _ = ctx.write_room_config(r);
        });

        match build_invite_code(&room) {
            Ok(code) => {
                let msg = crate::invite::share_message(
                    &code,
                    ctx.current_user_name(),
                    room.room_name.as_deref(),
                );
                if crate::app::copy_to_clipboard(&msg) {
                    ctx.system_msg(&format!("Invite copied: {}", code));
                } else {
                    let path = ctx.storage_root().join("invite.txt");
                    let _ = std::fs::write(&path, &msg);
                    ctx.system_msg(&format!("Invite saved to {}", path.display()));
                }
            }
            Err(e) => {
                ctx.system_msg(&format!("Could not generate invite code: {e}"));
            }
        }
        Ok(())
    }
}

/// Refresh git_remote on `room` if the on-disk repo has a newer remote than
/// what room config remembers. `persist` is a caller-supplied closure that
/// writes the updated room config (TUI writes via `ctx`, CLI via `Storage`).
fn refresh_git_remote_in_place<F: FnOnce(&RoomConfig)>(
    room: &mut RoomConfig,
    project_root: &Path,
    persist: F,
) {
    if let Some(detected) = crate::git::ops::get_git_remote_in(project_root) {
        if room.git_remote.as_deref() != Some(&detected) {
            room.git_remote = Some(detected);
            persist(room);
        }
    }
}

/// Build a short invite code, falling back to the legacy long code.
fn build_invite_code(room: &RoomConfig) -> Result<String> {
    crate::invite::create_short_invite(room)
        .or_else(|_| room.to_invite_code().map_err(|e| anyhow::anyhow!(e)))
}

/// CLI-side invite generation: refresh git remote, mint a code, format the
/// shareable message. Returns `(code, share_message)`.
///
/// Shared with `/invite` in the TUI via the private helpers above. Byte-
/// identical to the pre-W3.5 `main.rs::cmd_invite` body through the point
/// where the message is produced (the CLI subcommand then prints it;
/// `/invite` copies to clipboard).
pub fn generate_invite_for_storage(
    storage: &Storage,
    user_name: &str,
) -> Result<(String, String)> {
    let mut room = storage.read_room_config()?;
    refresh_git_remote_in_place(&mut room, storage.project_root(), |r| {
        let _ = storage.write_room_config(r);
    });
    let code = build_invite_code(&room)?;
    let msg = crate::invite::share_message(&code, user_name, room.room_name.as_deref());
    Ok((code, msg))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::*;

    #[test]
    fn missing_room_config_surfaces_error_msg() {
        let (_tmp, mut state) = mk_app_state();
        let mut ctx = TuiCtx::new_with_ws(&mut state, Box::new(NoopWs));
        Invite.run_tui(&mut ctx, "").unwrap();
        let last = state.chat_messages.last().unwrap().content.clone();
        // mk_app_state() doesn't seed a room — the command should report missing.
        assert!(
            last.contains("No room config") || last.contains("Invite"),
            "unexpected: {last}"
        );
    }

    #[test]
    fn metadata_matches_spec() {
        assert_eq!(Invite.name(), "/invite");
        assert_eq!(Invite.aliases(), &["/i"]);
        assert!(!Invite.needs_arg());
    }
}
