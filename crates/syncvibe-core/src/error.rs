use thiserror::Error;

#[derive(Error, Debug)]
pub enum SyncVibeError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("TOML deserialize error: {0}")]
    TomlDeserialize(#[from] toml::de::Error),

    #[error("TOML serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error("Not in a SyncVibe room (no .syncvibe/ directory found)")]
    NotInRoom,

    #[error("Room already initialized")]
    RoomAlreadyExists,

    #[error("Not in a git repository")]
    NotInGitRepo,

    #[error("User config not found. Run `syncvibe join` first.")]
    NoUserConfig,

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, SyncVibeError>;
