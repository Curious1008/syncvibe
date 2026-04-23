# Design System — SyncVibe CLI (TUI)

> Companion to [syncvibe-web/DESIGN.md](../syncvibe-web/DESIGN.md). The web is the
> shadow; the TUI is the object. This is the canonical form of the editorial-terminal
> language because it IS a terminal.

## Memorable thing

**"In tmux, open SyncVibe — in two seconds you know this is a craftsman's tool, not a chat app."**

Every visual decision below serves that sentence. Serious software, shared by serious
builders. Every cell on the grid earns its place.

## Product context

- **What this is:** Real-time collaboration chat + screen share, lives inside tmux.
- **Who it's for:** Developers working with AI agents (Claude, Codex, Gemini) and each other.
- **Space:** Terminal-native collab tools (think: tmux, mosh, abduco, zellij) crossed with Slack/Discord.
- **Project type:** Rust TUI (Ratatui), dark-only, truecolor-first with 256-color fallback.

## Aesthetic direction

- **Direction:** Editorial-terminal — canonical form.
- **Decoration level:** Minimal. Typography (case + weight + box-drawing) does all the work.
- **Mood:** Quiet competence. One signal color. No shouting.
- **Reference:** syncvibe-web `.sv-terminal`, `.sv-ascii`, `.sv-caret` — but native, not imitated.

### Anti-slop rules (TUI flavor)

Never:
- Use `Color::Cyan`, `Color::DarkGray`, `Color::Magenta`, `Color::Yellow` at call sites. All color goes through `theme::sv_*`.
- Use raw `Color::Rgb(...)` at call sites. Define it as a token first.
- Use multiple accent colors. One teal; everything else is neutral.
- Decorative emoji in any UI copy, toasts, or status messages (ASCII glyphs from box-drawing range are fine).
- Bouncy / animated loaders. Terminals don't do subpixel easing — fake motion looks cheap.
- "Card" style backgrounds inside the TUI (no rounded-looking fills, no nested panels). Flat surfaces only.
- Gradients of any kind (ANSI gradients via 256-color stepping).

Always:
- Semantic tokens (`theme::sv_accent` not `Color::Cyan`).
- Box-drawing for structure (`╭─ ─╮ │ ╰─ ─╯`), not ASCII fakes (`+-- --+ |`).
- Lowercase handles (`harry: ` not `Harry: `) — IRC/Slack/Discord convention.
- Single accent, used sparingly: caret, focus ring, own-@mentions, new-count badge.

## Color tokens

All tokens in `crates/syncvibe-cli/src/theme.rs`. Truecolor primary; 256-color fallback automatic via Ratatui's color downgrade.

| Token            | Hex       | Use                                          |
|------------------|-----------|----------------------------------------------|
| `sv_ink`         | `#0A0A0A` | Default bg. Never override.                  |
| `sv_surface`     | `#141414` | Alt-row / hover / subtle elevation.          |
| `sv_elevated`    | `#1A1A1A` | Selected message bg.                         |
| `sv_border`      | `#262626` | All hairlines, pane splits, input frames.    |
| `sv_fg`          | `#EDEDED` | Primary text.                                |
| `sv_fg_muted`    | `#A1A1A1` | Timestamps, sender labels.                   |
| `sv_fg_dim`      | `#6E6E6E` | System meta (`joined room`, `left`).         |
| `sv_fg_faint`    | `#4A4A4A` | Placeholder, disabled, decorative dividers.  |
| `sv_accent`      | `#4ECDC4` | Caret, focus, own-@mentions, new-count.      |
| `sv_error`       | `#E5484D` | Destructive signals only (kick, conn fail).  |

`sv_accent` is rare and meaningful. If it's on more than ~5% of visible cells, the system is wrong.

## User chat color palette

Replaces the 8-color candy set at `main.rs:109`. Aligned to brand, readable on `#0A0A0A`, distinguishable at low terminal brightness.

| Hex       | Name    | Notes                                     |
|-----------|---------|-------------------------------------------|
| `#4ECDC4` | teal    | Brand. Reserved by convention for self.   |
| `#E8845C` | amber   | Shared with Claude agent color.           |
| `#9B85E8` | violet  |                                           |
| `#5CB888` | sage    |                                           |
| `#D8B84D` | mustard |                                           |

5 colors is enough for typical 3-6 person rooms. If the room fills past 5, colors cycle.

## Agent colors (fixed, recognizable)

| Agent   | Hex       | Notes                                              |
|---------|-----------|----------------------------------------------------|
| Claude  | `#E8845C` | Matches amber in user palette (deliberate alias).  |
| Codex   | `#4FD88C` | Desaturated from `#00FF88` (too neon).             |
| Gemini  | `#4285F4` | Google blue. Keep for recognition.                 |

Agent handles render with a single-glyph prefix (e.g. `◆` Claude, `◇` Codex, `●` Gemini) so users can spot an agent message without reading the name.

## Typography (case, weight, modifier)

Font is the user's terminal monospace. Hierarchy comes from case + weight + modifier, not size.

| Level        | Rule                                                              |
|--------------|-------------------------------------------------------------------|
| Brand        | `SYNCVIBE` — uppercase, bold, `sv_accent`. Once per screen.       |
| Section label| UPPERCASE, `sv_fg_muted`. E.g. `PRESENCE`, `ROOM`.                |
| Content      | Mixed case, `sv_fg`.                                              |
| Own @mention | Bold, `sv_accent`.                                                |
| New-count    | Bold, `sv_accent`. E.g. `↓ 3 new from @harry`.                    |
| Sender label | lowercase, color from user palette + `: `.                        |
| Timestamp    | `sv_fg_muted`, `11:42a` lower, `Thu 23 Apr` full-date dividers.   |
| System msg   | Italic, `sv_fg_dim`.                                              |
| Placeholder  | `sv_fg_faint`.                                                    |
| Clickable    | Underline, no color change. URLs, `/help` hints.                  |

Never combine bold + uppercase + accent. Pick one; you have three ways to yell.

## Layout

### Baseline (default)

```
┌────────────────────────────────────────────────┐
│  SYNCVIBE  rain-ibis-42 · 3 online             │ 1 row: status bar
├────────────────────────────────────────────────┤
│                                                │
│  chat area (fill, scrolls up)                  │
│                                                │
│                                                │
├────────────────────────────────────────────────┤
│ › type a message or /command                   │ 3 rows: input
│                                                │
└────────────────────────────────────────────────┘
```

- Status bar: `SYNCVIBE` brand (teal bold) · room name · `N online`. Keep slim. No right-aligned decoration.
- Chat area: see [Density](#density) below.
- Input: 3 rows = top border, prompt + cursor, bottom border. Prompt is `›` (`sv_fg_dim`); caret is teal reverse-block.

### Presence rail (toggle `Ctrl-P`, OFF by default)

```
┌─────────────────────────────────────────┬──────┐
│  SYNCVIBE  rain-ibis-42 · 3 online      │      │
├─────────────────────────────────────────┤PRES- │
│                                         │ENCE  │
│  chat area                              │      │
│                                         │• har │
│                                         │• sam │
│                                         │◆ cld │
├─────────────────────────────────────────┴──────┤
│ › …                                            │
└────────────────────────────────────────────────┘
```

- 16 columns, right side. Opens/closes with `Ctrl-P`.
- Section label `PRESENCE` in uppercase muted.
- `•` glyph for humans (color from user palette). Agent glyph prefix (`◆ ◇ ●`) for agents.
- Handle truncated to 10 chars.
- Off by default because TMUX pane real estate is sacred. Most sessions don't need ambient awareness.

## Density

- Same-sender consecutive: 0 blank lines.
- Different-sender: 1 blank line.
- Gap of 5+ minutes between messages: faint `──` divider line with compact timestamp, `sv_fg_dim`.
- Midnight boundary: full-date divider, `── Thursday, 23 Apr ──` centered, `sv_fg_dim`.
- Selected message: `sv_elevated` background + 1-cell `sv_accent` left border. Replaces current cold `Rgb(30, 100, 160)`.

## Motion

Terminals render full cells, no subpixel easing. Motion = state transitions, not animations.

- **Caret:** 0.55ch blinking reverse-block in `sv_accent`. 530ms period. Matches web `.sv-caret`.
- **Toasts:** Appear with single-frame bottom slide-up. 4s linger. Fade to `sv_fg_dim` for 200ms, then gone.
- **Typing indicator (new):** `@sam is typing…` dim. Trailing dots cycle 330ms.
- **New-message badge:** When chat is scrolled up and messages arrive, 1-row sticky at chat bottom: `↓ 3 new from @harry · Enter to jump`. Teal. Dismiss on scroll-to-bottom.
- **Connection state:** `●` in status bar. Teal = online, faint = offline, amber = reconnecting. No spinner.

## ASCII poster (empty-room cold start)

Rendered in place of chat when the room is empty and user just joined. Vanishes on first message.

```
     ╭──────────────────────────────╮
     │       S Y N C V I B E        │
     │                              │
     │   room: rain-ibis-42         │
     │   invite copied to clipboard │
     │                              │
     │   try: /share · /watch · /ai │
     ╰──────────────────────────────╯
```

- Centered in chat area. ~60 cols wide, auto-center on narrower terminals with margin 2.
- Box-drawing in `sv_accent`. `S Y N C V I B E` in `sv_accent` bold. Body text in `sv_fg_muted`.
- Letter-spacing on brand via single-space separator.
- 5 rows of visible content cost. Earns it back as onboarding + identity anchor.

## Theme module (implementation contract)

`crates/syncvibe-cli/src/theme.rs`:

```rust
use ratatui::style::{Color, Modifier, Style};

// tokens
pub const SV_INK: Color       = Color::Rgb(0x0A, 0x0A, 0x0A);
pub const SV_SURFACE: Color   = Color::Rgb(0x14, 0x14, 0x14);
pub const SV_ELEVATED: Color  = Color::Rgb(0x1A, 0x1A, 0x1A);
pub const SV_BORDER: Color    = Color::Rgb(0x26, 0x26, 0x26);
pub const SV_FG: Color        = Color::Rgb(0xED, 0xED, 0xED);
pub const SV_FG_MUTED: Color  = Color::Rgb(0xA1, 0xA1, 0xA1);
pub const SV_FG_DIM: Color    = Color::Rgb(0x6E, 0x6E, 0x6E);
pub const SV_FG_FAINT: Color  = Color::Rgb(0x4A, 0x4A, 0x4A);
pub const SV_ACCENT: Color    = Color::Rgb(0x4E, 0xCD, 0xC4);
pub const SV_ERROR: Color     = Color::Rgb(0xE5, 0x48, 0x4D);

// semantic styles
pub fn brand() -> Style { Style::default().fg(SV_ACCENT).add_modifier(Modifier::BOLD) }
pub fn section_label() -> Style { Style::default().fg(SV_FG_MUTED) }
pub fn body() -> Style { Style::default().fg(SV_FG) }
pub fn own_mention() -> Style { Style::default().fg(SV_ACCENT).add_modifier(Modifier::BOLD) }
pub fn timestamp() -> Style { Style::default().fg(SV_FG_MUTED) }
pub fn system() -> Style { Style::default().fg(SV_FG_DIM).add_modifier(Modifier::ITALIC) }
pub fn placeholder() -> Style { Style::default().fg(SV_FG_FAINT) }
pub fn selected_bg() -> Style { Style::default().bg(SV_ELEVATED) }
pub fn error() -> Style { Style::default().fg(SV_ERROR) }

// user palette (index by user_id hash or explicit choice)
pub const USER_PALETTE: &[&str] = &[
    "#4ECDC4", "#E8845C", "#9B85E8", "#5CB888", "#D8B84D",
];

// agent colors (stable, recognizable)
pub const AGENT_CLAUDE: &str  = "#E8845C";
pub const AGENT_CODEX: &str   = "#4FD88C";
pub const AGENT_GEMINI: &str  = "#4285F4";
```

Every component must `use crate::theme`. No raw `Color::Cyan` / `Color::Rgb(...)` at call sites after the theme is in place.

## Migration (ties into the strangler fig refactor at `/tmp/syncvibe-refactor-spec.md`)

The design system lands as part of the refactor, not a separate project:

| Wave    | Scope                                                                      |
|---------|----------------------------------------------------------------------------|
| Wave 0  | Add `theme.rs`. No call sites updated yet. Ship as a scaffolding commit.   |
| Wave 1  | Pilot commands (`/name`, `/clear`) use theme tokens at every new call site.|
| Wave 2  | Bulk command porting replaces scattered colors as they pass through.       |
| Wave 3  | Final sweep: grep for `Color::Cyan`, `Color::DarkGray`, raw `Color::Rgb(`. Zero hits or the PR bounces. |

**New test** (add to refactor spec §7 globals): `no_raw_color_at_call_sites`:
```rust
#[test]
fn no_raw_color_outside_theme() {
    let src = std::fs::read_to_string("src/components").unwrap(); // walk dir
    for forbidden in &["Color::Cyan", "Color::DarkGray", "Color::Magenta", "Color::Yellow", "Color::Rgb("] {
        // except theme.rs itself
        assert!(!src.contains(forbidden), "raw color at call site: {forbidden}");
    }
}
```

(If that's too brittle, use a lint via `clippy.toml` disallowed-types list.)

## Decisions log

| Date       | Decision                                                            | Rationale                                                                                          |
|------------|---------------------------------------------------------------------|----------------------------------------------------------------------------------------------------|
| 2026-04-23 | Create initial design system for SyncVibe CLI TUI.                  | No DESIGN.md existed for CLI. Colors scattered across components. Refactor window = right moment.  |
| 2026-04-23 | Memorable thing: "craftsman's tool, not a chat app."                | User-confirmed.                                                                                    |
| 2026-04-23 | Inherit web palette tokens (teal accent, dark neutrals).            | Web already canonicalized editorial-terminal. TUI is the source form, not a departure.             |
| 2026-04-23 | Single teal accent only. Kill scattered Cyan/Magenta/Yellow.        | Risk R1 confirmed. Coherence over category variety.                                                |
| 2026-04-23 | Presence rail `Ctrl-P`, default OFF.                                | Risk R2 confirmed. TMUX pane real estate is sacred. Opt-in ambient awareness.                     |
| 2026-04-23 | Replace 8-color candy palette with 5 brand-aligned.                 | Risk R3 confirmed. Cohesion beats personal expression at room size 3-6.                           |
| 2026-04-23 | ASCII poster on empty room cold start.                              | Risk R4 confirmed. 5-row cost, identity + onboarding return.                                      |
| 2026-04-23 | Selection bg: warm `sv_elevated` + 1-cell teal left border.         | Risk R5 confirmed. Replaces cold `Rgb(30, 100, 160)` Windows-95 feel.                             |
