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
        set_private_dir_permissions(&root);
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn project_root(&self) -> &Path {
        self.root.parent().unwrap_or(&self.root)
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

    // --- Chat Log (JSONL) ---

    pub fn append_chat_message(&self, msg: &ChatMessage) -> Result<()> {
        let path = self.root.join("chat-log.jsonl");
        let line = serde_json::to_string(msg)?;
        let created = !path.exists();
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        if created {
            set_private_permissions(&path);
        }
        // Advisory file lock with bounded retry
        let mut locked = false;
        for _ in 0..20 {
            match file.try_lock_exclusive() {
                Ok(()) => { locked = true; break; }
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(5)),
            }
        }
        if !locked {
            return Err(SyncVibeError::Other("Could not acquire chat log file lock".into()));
        }
        writeln!(file, "{}", line)?;
        file.flush()?;
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

    /// Read only new messages appended after `offset` bytes.
    /// Returns (new_messages, new_byte_offset).
    pub fn read_chat_from_offset(&self, offset: u64) -> Result<(Vec<ChatMessage>, u64)> {
        let path = self.root.join("chat-log.jsonl");
        if !path.exists() {
            return Ok((Vec::new(), 0));
        }
        let file = fs::File::open(&path)?;
        let file_len = file.metadata()?.len();
        if offset >= file_len {
            return Ok((Vec::new(), file_len));
        }
        let mut file = file;
        use std::io::Seek;
        file.seek(std::io::SeekFrom::Start(offset))?;
        let reader = std::io::BufReader::new(file);
        let mut messages = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str(&line) {
                Ok(msg) => messages.push(msg),
                Err(_) => continue,
            }
        }
        Ok((messages, file_len))
    }

    /// Get the current byte size of the chat log file.
    pub fn chat_log_size(&self) -> u64 {
        let path = self.root.join("chat-log.jsonl");
        fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
    }

    // --- Digest ---

    /// Write chat digest markdown to .syncvibe/chat-digest.md (atomic write)
    pub fn write_chat_digest(&self, content: &str) -> Result<PathBuf> {
        let path = self.root.join("chat-digest.md");
        atomic_write(&path, content)?;
        Ok(path)
    }

    /// Return the project-relative path of the digest file
    pub fn chat_digest_relative_path(&self) -> String {
        ".syncvibe/chat-digest.md".to_string()
    }

    // --- Images ---

    /// Save an image file into .syncvibe/images/, return the relative path
    pub fn save_image(&self, source_path: &Path) -> Result<String> {
        let images_dir = self.root.join("images");
        fs::create_dir_all(&images_dir)?;

        let raw_ext = source_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("png");
        let ext = match raw_ext.to_lowercase().as_str() {
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" => raw_ext,
            _ => "png",
        };
        let filename = format!("{}.{}", uuid::Uuid::new_v4(), ext);
        let dest = images_dir.join(&filename);
        fs::copy(source_path, &dest)?;

        Ok(format!(".syncvibe/images/{}", filename))
    }

    /// Get absolute path for a .syncvibe/ relative image path.
    /// Validates the path stays within the project root to prevent path traversal.
    pub fn image_abs_path(&self, relative: &str) -> PathBuf {
        let candidate = self.project_root().join(relative);
        // Normalize and verify the path doesn't escape project root
        let root = self.project_root().canonicalize().unwrap_or_else(|_| self.project_root().to_path_buf());
        let resolved = candidate.canonicalize().unwrap_or(candidate);
        if resolved.starts_with(&root) {
            resolved
        } else {
            // Path traversal attempt — return a safe fallback
            root.join(".syncvibe").join("images").join("invalid")
        }
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
        if let Err(e) = fs::set_permissions(path, fs::Permissions::from_mode(0o600)) {
            eprintln!(
                "Warning: could not set permissions on {}: {}",
                path.display(),
                e
            );
        }
    }
    let _ = path;
}

/// Set directory to owner-only (0700) on Unix
fn set_private_dir_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = fs::set_permissions(path, fs::Permissions::from_mode(0o700)) {
            eprintln!(
                "Warning: could not set permissions on {}: {}",
                path.display(),
                e
            );
        }
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

    // ── Incremental Read ──

    #[test]
    fn chat_from_offset_reads_only_new() {
        let (_tmp, storage) = make_storage();
        storage.append_chat_message(&make_msg("first")).unwrap();
        storage.append_chat_message(&make_msg("second")).unwrap();
        let offset = storage.chat_log_size();

        storage.append_chat_message(&make_msg("third")).unwrap();
        storage.append_chat_message(&make_msg("fourth")).unwrap();

        let (new_msgs, new_offset) = storage.read_chat_from_offset(offset).unwrap();
        assert_eq!(new_msgs.len(), 2);
        assert_eq!(new_msgs[0].content, "third");
        assert_eq!(new_msgs[1].content, "fourth");
        assert!(new_offset > offset);
    }

    #[test]
    fn chat_from_offset_empty_when_caught_up() {
        let (_tmp, storage) = make_storage();
        storage.append_chat_message(&make_msg("msg")).unwrap();
        let offset = storage.chat_log_size();

        let (new_msgs, new_offset) = storage.read_chat_from_offset(offset).unwrap();
        assert!(new_msgs.is_empty());
        assert_eq!(new_offset, offset);
    }

    #[test]
    fn chat_from_offset_zero_reads_all() {
        let (_tmp, storage) = make_storage();
        storage.append_chat_message(&make_msg("a")).unwrap();
        storage.append_chat_message(&make_msg("b")).unwrap();

        let (msgs, offset) = storage.read_chat_from_offset(0).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(offset, storage.chat_log_size());
    }

    #[test]
    fn chat_from_offset_no_file() {
        let (_tmp, storage) = make_storage();
        let (msgs, offset) = storage.read_chat_from_offset(0).unwrap();
        assert!(msgs.is_empty());
        assert_eq!(offset, 0);
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
