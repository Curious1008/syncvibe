//! Interactive CLI flows extracted from `session.rs`.
//!
//! Each flow is a coherent chunk of terminal I/O (prompt → validate → persist
//! → launch). Keeping them out of `session.rs` shrinks the entry-point file
//! and makes each flow independently reviewable/testable.
//!
//! Contract: flows may call into `commands::` for pure logic, but never the
//! other way around. Commands stay headless (no stdin/stdout prompts).

pub mod onboarding;
