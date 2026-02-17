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
        atomic_write(&path, &serde_json::to_string_pretty(config)?)?;
        set_private_permissions(&path);
        Ok(())
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

/// Set file to owner-only read/write (0600) on Unix
fn set_private_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    let _ = path;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ChatMessage, RoomConfig};
    use tempfile::TempDir;

    fn make_storage() -> (TempDir, Storage) {
        let tmp = TempDir::new().unwrap();
        let sv = tmp.path().join(".syncvibe");
        fs::create_dir_all(&sv).unwrap();
        (tmp, Storage { root: sv })
    }

    fn make_msg(content: &str) -> ChatMessage {
        ChatMessage::new_user_message(
            "u1".into(),
            "Alice".into(),
            "#ff0000".into(),
            content.into(),
            "sess1".into(),
            None,
        )
    }

    // ── Storage::find ──

    #[test]
    fn find_walks_up_dirs() {
        let tmp = TempDir::new().unwrap();
        let sv = tmp.path().join(".syncvibe");
        fs::create_dir_all(&sv).unwrap();
        let deep = tmp.path().join("a/b/c");
        fs::create_dir_all(&deep).unwrap();
        let storage = Storage::find(&deep).unwrap();
        assert_eq!(storage.root(), sv);
    }

    #[test]
    fn find_fails_when_no_syncvibe() {
        let tmp = TempDir::new().unwrap();
        assert!(Storage::find(tmp.path()).is_err());
    }

    #[test]
    fn init_creates_dir() {
        let tmp = TempDir::new().unwrap();
        let storage = Storage::init(tmp.path()).unwrap();
        assert!(storage.root().exists());
    }

    #[test]
    fn init_fails_if_already_exists() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".syncvibe")).unwrap();
        assert!(Storage::init(tmp.path()).is_err());
    }

    // ── Chat JSONL ──

    #[test]
    fn chat_roundtrip_basic() {
        let (_tmp, storage) = make_storage();
        let msg = make_msg("hello");
        storage.append_chat_message(&msg).unwrap();
        let msgs = storage.read_chat_messages().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content, "hello");
        assert_eq!(msgs[0].id, msg.id);
    }

    #[test]
    fn chat_empty_file() {
        let (_tmp, storage) = make_storage();
        // No chat-log.jsonl → empty vec
        let msgs = storage.read_chat_messages().unwrap();
        assert!(msgs.is_empty());
    }

    #[test]
    fn chat_skips_corrupt_lines() {
        let (_tmp, storage) = make_storage();
        let path = storage.root.join("chat-log.jsonl");
        let good = make_msg("good");
        let good_json = serde_json::to_string(&good).unwrap();
        // Write: good line, corrupt line, empty line, another good line
        let good2 = make_msg("good2");
        let good2_json = serde_json::to_string(&good2).unwrap();
        fs::write(
            &path,
            format!("{}\nNOT_JSON_AT_ALL\n\n{}\n", good_json, good2_json),
        )
        .unwrap();
        let msgs = storage.read_chat_messages().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].content, "good");
        assert_eq!(msgs[1].content, "good2");
    }

    #[test]
    fn chat_handles_unicode() {
        let (_tmp, storage) = make_storage();
        let msg = make_msg("你好世界 🌍 café");
        storage.append_chat_message(&msg).unwrap();
        let msgs = storage.read_chat_messages().unwrap();
        assert_eq!(msgs[0].content, "你好世界 🌍 café");
    }

    #[test]
    fn chat_handles_empty_content() {
        let (_tmp, storage) = make_storage();
        let msg = make_msg("");
        storage.append_chat_message(&msg).unwrap();
        let msgs = storage.read_chat_messages().unwrap();
        assert_eq!(msgs[0].content, "");
    }

    #[test]
    fn chat_handles_newlines_in_content() {
        let (_tmp, storage) = make_storage();
        // JSON serialization escapes \n, so this should roundtrip fine
        let msg = make_msg("line1\nline2\nline3");
        storage.append_chat_message(&msg).unwrap();
        let msgs = storage.read_chat_messages().unwrap();
        assert_eq!(msgs[0].content, "line1\nline2\nline3");
    }

    #[test]
    fn chat_handles_very_long_content() {
        let (_tmp, storage) = make_storage();
        let long = "x".repeat(100_000);
        let msg = make_msg(&long);
        storage.append_chat_message(&msg).unwrap();
        let msgs = storage.read_chat_messages().unwrap();
        assert_eq!(msgs[0].content.len(), 100_000);
    }

    #[test]
    fn chat_multiple_appends() {
        let (_tmp, storage) = make_storage();
        for i in 0..50 {
            storage
                .append_chat_message(&make_msg(&format!("msg{}", i)))
                .unwrap();
        }
        let msgs = storage.read_chat_messages().unwrap();
        assert_eq!(msgs.len(), 50);
        assert_eq!(msgs[0].content, "msg0");
        assert_eq!(msgs[49].content, "msg49");
    }

    #[test]
    fn chat_filter_by_session() {
        let (_tmp, storage) = make_storage();
        let mut m1 = make_msg("a");
        m1.session_id = "s1".into();
        let mut m2 = make_msg("b");
        m2.session_id = "s2".into();
        let mut m3 = make_msg("c");
        m3.session_id = "s1".into();
        storage.append_chat_message(&m1).unwrap();
        storage.append_chat_message(&m2).unwrap();
        storage.append_chat_message(&m3).unwrap();
        let filtered = storage.read_chat_by_session("s1").unwrap();
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].content, "a");
        assert_eq!(filtered[1].content, "c");
    }

    // ── Room Config ──

    #[test]
    fn room_config_roundtrip() {
        let (_tmp, storage) = make_storage();
        let config = RoomConfig::new();
        storage.write_room_config(&config).unwrap();
        let loaded = storage.read_room_config().unwrap();
        assert_eq!(loaded.room_id, config.room_id);
        assert_eq!(loaded.room_secret, config.room_secret);
    }

    #[test]
    fn room_secret_is_64_hex_chars() {
        let config = RoomConfig::new();
        assert_eq!(config.room_secret.len(), 64);
        assert!(config.room_secret.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn room_secrets_are_unique() {
        let a = RoomConfig::new();
        let b = RoomConfig::new();
        assert_ne!(a.room_secret, b.room_secret);
        assert_ne!(a.room_id, b.room_id);
    }

    // ── Plan ──

    #[test]
    fn plan_empty_when_missing() {
        let (_tmp, storage) = make_storage();
        assert_eq!(storage.read_plan().unwrap(), "");
    }

    #[test]
    fn plan_roundtrip() {
        let (_tmp, storage) = make_storage();
        storage.write_plan("# My Plan\n\n- Step 1").unwrap();
        assert_eq!(storage.read_plan().unwrap(), "# My Plan\n\n- Step 1");
    }

    // ── Atomic Write ──

    #[test]
    fn atomic_write_creates_file() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.txt");
        atomic_write(&path, "hello").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello");
    }

    #[test]
    fn atomic_write_overwrites() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.txt");
        atomic_write(&path, "first").unwrap();
        atomic_write(&path, "second").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "second");
    }

    // ── Image Storage ──

    #[test]
    fn save_image_copies_file() {
        let (_tmp, storage) = make_storage();
        // Create a fake image file
        let src = _tmp.path().join("photo.png");
        fs::write(&src, b"fake-png-data").unwrap();
        let relative = storage.save_image(&src).unwrap();
        assert!(relative.starts_with(".syncvibe/images/"));
        assert!(relative.ends_with(".png"));
        // Verify copied file content
        let abs = storage.image_abs_path(&relative);
        assert_eq!(fs::read(&abs).unwrap(), b"fake-png-data");
    }

    #[test]
    fn save_image_preserves_extension() {
        let (_tmp, storage) = make_storage();
        let src = _tmp.path().join("photo.jpg");
        fs::write(&src, b"fake").unwrap();
        let relative = storage.save_image(&src).unwrap();
        assert!(relative.ends_with(".jpg"));
    }

    #[test]
    fn save_image_unique_names() {
        let (_tmp, storage) = make_storage();
        let src = _tmp.path().join("photo.png");
        fs::write(&src, b"fake").unwrap();
        let a = storage.save_image(&src).unwrap();
        let b = storage.save_image(&src).unwrap();
        assert_ne!(a, b); // UUID-based names
    }
}
