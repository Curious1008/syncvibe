use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "syncvibe", about = "Terminal-native collaboration for vibe coding")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Initialize a new SyncVibe room in the current project
    Init,

    /// Join an existing SyncVibe room (set up your profile)
    Join {
        /// Your display name
        #[arg(long)]
        name: Option<String>,

        /// Your color (hex, e.g. #FF6B6B)
        #[arg(long)]
        color: Option<String>,
    },

    /// Send a chat message without opening TUI
    Chat {
        /// The message to send
        message: String,
    },

    /// Print invite instructions
    Invite,

    /// Start the MCP server (for AI agents)
    McpServer,

    /// Launch just the dashboard TUI (used inside tmux pane)
    Dashboard,

    /// Switch between SyncVibe projects
    Switch,
}
