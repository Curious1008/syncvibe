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
];

/// Find an agent by id.
pub fn find(id: &str) -> Option<&'static AgentDef> {
    AGENTS.iter().find(|a| a.id == id)
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
        Some(idx) => Ok(AGENTS[idx].id.to_string()),
        None => anyhow::bail!("Cancelled."),
    }
}

/// Check if a message contains any known agent mention (for TUI highlighting).
/// Returns the display name of the matched agent, or None.
pub fn find_mentioned_agent(content_lower: &str) -> Option<&'static str> {
    if content_lower.contains("@agent") {
        return Some("agent");
    }
    for agent in AGENTS {
        if agent.mentions.iter().any(|m| content_lower.contains(m)) {
            return Some(agent.name);
        }
    }
    None
}
