use std::fs;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use fs2::FileExt;

use crate::error::{Result, SyncVibeError};
use crate::models::*;

const SYNCVIBE_DIR: &str = ".syncvibe";

/// Storage manager for .syncvibe/ directory
pub struct Storage {
    root: PathBuf,
}

impl Storage {
    /// Find .syncvibe/ directory by walking up from the given path
    pub fn find(start: &Path) -> Result<Self> {
        let mut current = start.to_path_buf();
        loop {
            let candidate = current.join(SYNCVIBE_DIR);
            if candidate.is_dir() {
                return Ok(Self { root: candidate });
            }
            if !current.pop() {
                return Err(SyncVibeError::NotInRoom);
            }
        }
    }

    /// Create a new .syncvibe/ directory at the given project root
    pub fn init(project_root: &Path) -> Result<Self> {
        let root = project_root.join(SYNCVIBE_DIR);
        if root.exists() {
            return Err(SyncVibeError::RoomAlreadyExists);
        }
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn project_root(&self) -> &Path {
        self.root.parent().unwrap()
    }

    // --- Room Config ---

    pub fn read_room_config(&self) -> Result<RoomConfig> {
        let path = self.root.join("room.json");
        let content = fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&content)?)
    }

    pub fn write_room_config(&self, config: &RoomConfig) -> Result<()> {
        let path = self.root.join("room.json");
        atomic_write(&path, &serde_json::to_string_pretty(config)?)
    }

    // --- Plan ---

    pub fn read_plan(&self) -> Result<String> {
        let path = self.root.join("plan.md");
        if !path.exists() {
            return Ok(String::new());
        }
        Ok(fs::read_to_string(&path)?)
    }

    pub fn write_plan(&self, content: &str) -> Result<()> {
        let path = self.root.join("plan.md");
        atomic_write(&path, content)
    }

    pub fn read_plan_meta(&self) -> Result<Option<PlanMeta>> {
        let path = self.root.join("plan-meta.json");
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)?;
        Ok(Some(serde_json::from_str(&content)?))
    }

    pub fn write_plan_meta(&self, meta: &PlanMeta) -> Result<()> {
        let path = self.root.join("plan-meta.json");
        atomic_write(&path, &serde_json::to_string_pretty(meta)?)
    }

    // --- Tasks ---

    pub fn read_task_board(&self) -> Result<TaskBoard> {
        let path = self.root.join("tasks.json");
        if !path.exists() {
            return Ok(TaskBoard::default());
        }
        let content = fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&content)?)
    }

    pub fn write_task_board(&self, board: &TaskBoard) -> Result<()> {
        let path = self.root.join("tasks.json");
        atomic_write(&path, &serde_json::to_string_pretty(board)?)
    }

    // --- Chat Log (JSONL) ---

    pub fn append_chat_message(&self, msg: &ChatMessage) -> Result<()> {
        let path = self.root.join("chat-log.jsonl");
        let line = serde_json::to_string(msg)?;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        // Advisory file lock to prevent concurrent write corruption
        file.lock_exclusive()?;
        writeln!(file, "{}", line)?;
        file.unlock()?;
        Ok(())
    }

    pub fn read_chat_messages(&self) -> Result<Vec<ChatMessage>> {
        let path = self.root.join("chat-log.jsonl");
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = fs::File::open(&path)?;
        let reader = std::io::BufReader::new(file);
        let mut messages = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            // Skip corrupt lines instead of failing entirely
            match serde_json::from_str(&line) {
                Ok(msg) => messages.push(msg),
                Err(_) => continue,
            }
        }
        Ok(messages)
    }

    /// Read messages filtered by session_id
    pub fn read_chat_by_session(&self, session_id: &str) -> Result<Vec<ChatMessage>> {
        Ok(self
            .read_chat_messages()?
            .into_iter()
            .filter(|m| m.session_id == session_id)
            .collect())
    }

    /// Read messages filtered by thread_id (task)
    pub fn read_chat_by_task(&self, task_id: &str) -> Result<Vec<ChatMessage>> {
        Ok(self
            .read_chat_messages()?
            .into_iter()
            .filter(|m| m.thread_id.as_deref() == Some(task_id))
            .collect())
    }

    /// Read messages since a given timestamp
    pub fn read_chat_since(
        &self,
        since: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<ChatMessage>> {
        Ok(self
            .read_chat_messages()?
            .into_iter()
            .filter(|m| m.timestamp >= since)
            .collect())
    }

    // --- Images ---

    /// Save an image file into .syncvibe/images/, return the relative path
    pub fn save_image(&self, source_path: &Path) -> Result<String> {
        let images_dir = self.root.join("images");
        fs::create_dir_all(&images_dir)?;

        let ext = source_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("png");
        let filename = format!("{}.{}", uuid::Uuid::new_v4(), ext);
        let dest = images_dir.join(&filename);
        fs::copy(source_path, &dest)?;

        Ok(format!(".syncvibe/images/{}", filename))
    }

    /// Get absolute path for a .syncvibe/ relative image path
    pub fn image_abs_path(&self, relative: &str) -> PathBuf {
        self.project_root().join(relative)
    }
}

/// Write to a temp file then rename, preventing partial writes
fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&tmp, content)?;
    fs::rename(&tmp, path)?;
    Ok(())
}
