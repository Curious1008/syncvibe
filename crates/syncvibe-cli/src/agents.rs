use anyhow::Result;

use crate::onboarding::{self, DIM, R, TEAL};

pub struct AgentDef {
    pub id: &'static str,
    pub name: &'static str,
    pub command: &'static str,
    pub color: &'static str,
    pub mentions: &'static [&'static str],
}

pub const AGENTS: &[AgentDef] = &[
    AgentDef {
        id: "claude",
        name: "Claude",
        command: "claude",
        color: "#E8845C",
        mentions: &["@claude", "@claude-code"],
    },
    AgentDef {
        id: "codex",
        name: "Codex",
        command: "codex",
        color: "#00FF88",
        mentions: &["@codex", "@openai"],
    },
    AgentDef {
        id: "gemini",
        name: "Gemini",
        command: "gemini",
        color: "#4285F4",
        mentions: &["@gemini", "@gemini-cli"],
    },
];

/// Find an agent by id.
pub fn find(id: &str) -> Option<&'static AgentDef> {
    AGENTS.iter().find(|a| a.id == id)
}

/// Owner qualifier inside a `@claude(Alice)` or `@claude(Alice#7af)` mention.
///
/// `suffix` is the last chars of the owner's `user_id` — needed when two
/// people in the same room share the same `user_name` (the name field is
/// user-editable so collisions are possible).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MentionOwner {
    pub name: String,
    pub suffix: Option<String>,
}

/// Parsed @mention token.
///
/// `keyword` is lowercased including the `@` prefix (e.g. `@claude`).
/// `owner` is `None` for bare `@claude`, or `Some(MentionOwner)` when the
/// mention was written as `@claude(Alice)` or `@claude(Alice#7af)`.
///
/// Used by remote and local trigger paths to decide whether THIS
/// machine's agent pane should fire — bare mentions preserve the old
/// broadcast behavior, qualified mentions route by owner (strict).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mention {
    pub keyword: String,
    pub owner: Option<MentionOwner>,
}

/// Length of the user_id suffix used to disambiguate duplicate user_names.
/// UUIDv4 is hex, so 4 chars give 16^4 = 65 536 buckets — ample for the
/// handful of owners of a single agent in a single room.
pub const OWNER_SUFFIX_LEN: usize = 4;

/// Extract the suffix that goes into `@claude(Name#suffix)` from a user_id.
pub fn owner_suffix(user_id: &str) -> String {
    let chars: Vec<char> = user_id.chars().collect();
    let start = chars.len().saturating_sub(OWNER_SUFFIX_LEN);
    chars[start..].iter().collect()
}

/// Extract all `@name` / `@name(owner)` tokens from a chat message.
///
/// Walks chars (not bytes) so CJK owner names work. `(` must follow the
/// word with no whitespace in between for the owner qualifier to attach.
/// Inside the parens, `name#suffix` splits on the first `#`; the suffix
/// is stored lower-cased to match the hex user_id representation.
pub fn extract_mentions(content: &str) -> Vec<Mention> {
    let chars: Vec<char> = content.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '@' {
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        while i < chars.len()
            && (chars[i].is_alphanumeric() || chars[i] == '-' || chars[i] == '_')
        {
            i += 1;
        }
        if i == start + 1 {
            continue;
        }
        let keyword: String = chars[start..i].iter().collect::<String>().to_lowercase();
        let mut owner = None;
        if i < chars.len() && chars[i] == '(' {
            let paren_start = i + 1;
            let mut j = paren_start;
            while j < chars.len() && chars[j] != ')' {
                j += 1;
            }
            if j < chars.len() && chars[j] == ')' {
                let inside: String = chars[paren_start..j].iter().collect();
                let trimmed = inside.trim();
                if !trimmed.is_empty() {
                    let (name_part, suffix_part) = match trimmed.split_once('#') {
                        Some((n, s)) => (n.trim().to_string(), Some(s.trim().to_lowercase())),
                        None => (trimmed.to_string(), None),
                    };
                    if !name_part.is_empty() {
                        owner = Some(MentionOwner {
                            name: name_part,
                            suffix: suffix_part.filter(|s| !s.is_empty()),
                        });
                    }
                }
                i = j + 1;
            }
        }
        out.push(Mention { keyword, owner });
    }
    out
}

/// Default agent (Claude).
pub fn default() -> &'static AgentDef {
    &AGENTS[0]
}

/// Resolve agent from optional id, falling back to default.
pub fn for_room(agent_id: Option<&str>) -> &'static AgentDef {
    agent_id.and_then(find).unwrap_or_else(default)
}

/// Interactive agent selection menu. Returns agent id.
pub fn select_agent() -> Result<String> {
    let items: Vec<onboarding::MenuItem> = AGENTS
        .iter()
        .map(|a| onboarding::MenuItem {
            label: a.name.to_string(),
            hint: format!("({})", a.command),
        })
        .collect();

    println!("  {TEAL}Agent:{R} {DIM}Which AI agent to use?{R}");
    println!();

    match onboarding::select_menu(&items)? {
        Some(idx) => {
            println!();
            Ok(AGENTS[idx].id.to_string())
        }
        None => anyhow::bail!("Cancelled."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner_name(m: &Mention) -> Option<&str> {
        m.owner.as_ref().map(|o| o.name.as_str())
    }
    fn owner_suffix_of(m: &Mention) -> Option<&str> {
        m.owner.as_ref().and_then(|o| o.suffix.as_deref())
    }

    #[test]
    fn bare_mention_has_no_owner() {
        let m = extract_mentions("hey @claude fix this");
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].keyword, "@claude");
        assert!(m[0].owner.is_none());
    }

    #[test]
    fn qualified_mention_extracts_owner() {
        let m = extract_mentions("@claude(Alice) please");
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].keyword, "@claude");
        assert_eq!(owner_name(&m[0]), Some("Alice"));
        assert_eq!(owner_suffix_of(&m[0]), None);
    }

    #[test]
    fn qualified_mention_with_suffix() {
        let m = extract_mentions("@claude(Alice#7AF) please");
        assert_eq!(owner_name(&m[0]), Some("Alice"));
        assert_eq!(owner_suffix_of(&m[0]), Some("7af")); // lowercased for hex match
    }

    #[test]
    fn mention_keyword_is_lowercased() {
        let m = extract_mentions("@CLAUDE(alice)");
        assert_eq!(m[0].keyword, "@claude");
        assert_eq!(owner_name(&m[0]), Some("alice"));
    }

    #[test]
    fn cjk_owner_names_preserved() {
        let m = extract_mentions("@claude(小明) 请修复");
        assert_eq!(m.len(), 1);
        assert_eq!(owner_name(&m[0]), Some("小明"));
    }

    #[test]
    fn bare_at_sign_is_ignored() {
        let m = extract_mentions("email me @ work");
        assert!(m.is_empty());
    }

    #[test]
    fn multiple_mentions_both_captured() {
        let m = extract_mentions("@claude(Alice) vs @codex");
        assert_eq!(m.len(), 2);
        assert_eq!(owner_name(&m[0]), Some("Alice"));
        assert_eq!(m[1].keyword, "@codex");
        assert!(m[1].owner.is_none());
    }

    #[test]
    fn unclosed_paren_does_not_swallow_rest_of_line() {
        let m = extract_mentions("@claude(Alice no close");
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].keyword, "@claude");
        assert!(m[0].owner.is_none());
    }

    #[test]
    fn whitespace_between_word_and_paren_detaches_owner() {
        let m = extract_mentions("@claude (Alice)");
        assert_eq!(m.len(), 1);
        assert!(m[0].owner.is_none());
    }

    #[test]
    fn owner_suffix_takes_last_n_chars() {
        assert_eq!(owner_suffix("550e8400-e29b-41d4-a716-446655440000"), "0000");
        assert_eq!(owner_suffix("abc"), "abc"); // shorter than suffix len
    }
}
