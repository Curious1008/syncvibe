use clap::{Parser, Subcommand};
use clap_complete::Shell;

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

    /// View or update your profile (name, color)
    Profile {
        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New color (hex, e.g. #FF6B6B)
        #[arg(long)]
        color: Option<String>,
    },

    /// Send a chat message without opening TUI
    Chat {
        /// The message to send
        message: String,
    },

    /// Show room invite code
    Invite,

    /// Show current room status
    Status,

    /// Start the MCP server (for AI agents)
    McpServer,

    /// Launch just the dashboard TUI (used inside tmux pane)
    Dashboard,

    /// Switch between SyncVibe rooms
    Switch,

    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
}
