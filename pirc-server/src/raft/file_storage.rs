use std::path::{Path, PathBuf};

use serde::{de::DeserializeOwned, Serialize};
use tokio::fs;
use tokio::io::AsyncWriteExt;

use super::storage::{RaftStorage, StorageError, StorageResult};
use super::types::{LogEntry, LogIndex, NodeId, Term};

const TERM_FILE: &str = "term";
const VOTED_FOR_FILE: &str = "voted_for";
const LOG_FILE: &str = "log.jsonl";
const SNAPSHOT_FILE: &str = "snapshot.bin";
const SNAPSHOT_META_FILE: &str = "snapshot.meta";

/// File-based persistent storage for Raft state.
///
/// Uses atomic writes (write to temp file, fsync, rename) to prevent
/// corruption from crashes mid-write. Each piece of state is stored
/// in a separate file within the configured data directory.
pub struct FileStorage {
    dir: PathBuf,
}

impl FileStorage {
    /// Create a new `FileStorage` rooted at the given directory.
    ///
    /// The directory (and any parent directories) will be created if
    /// they don't already exist.
    pub async fn new(dir: impl Into<PathBuf>) -> StorageResult<Self> {
        let dir = dir.into();
        fs::create_dir_all(&dir).await?;
        Ok(Self { dir })
    }

    /// Atomically write `data` to `path`: write to a temp file in the same
    /// directory, fsync, then rename over the target.
    async fn atomic_write(&self, path: &Path, data: &[u8]) -> StorageResult<()> {
        let tmp = self.dir.join(format!(
            ".tmp.{}",
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("data")
        ));
        let mut file = fs::File::create(&tmp).await?;
        file.write_all(data).await?;
        file.sync_all().await?;
        fs::rename(&tmp, path).await?;
        Ok(())
    }

    /// Read a file's contents, returning `None` if the file doesn't exist.
    async fn read_file(&self, path: &Path) -> StorageResult<Option<Vec<u8>>> {
        match fs::read(path).await {
            Ok(data) => Ok(Some(data)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn term_path(&self) -> PathBuf {
        self.dir.join(TERM_FILE)
    }

    fn voted_for_path(&self) -> PathBuf {
        self.dir.join(VOTED_FOR_FILE)
    }

    fn log_path(&self) -> PathBuf {
        self.dir.join(LOG_FILE)
    }

    fn snapshot_path(&self) -> PathBuf {
        self.dir.join(SNAPSHOT_FILE)
    }

    fn snapshot_meta_path(&self) -> PathBuf {
        self.dir.join(SNAPSHOT_META_FILE)
    }
}

impl<T> RaftStorage<T> for FileStorage
where
    T: Serialize + DeserializeOwned + Send + Sync + 'static,
{
    async fn save_term(&self, term: Term) -> StorageResult<()> {
        let data = term.as_u64().to_string();
        self.atomic_write(&self.term_path(), data.as_bytes()).await
    }

    async fn load_term(&self) -> StorageResult<Term> {
        match self.read_file(&self.term_path()).await? {
            Some(data) => {
                let s = String::from_utf8(data).map_err(|e| {
                    StorageError::Corrupted(format!("term file not valid UTF-8: {e}"))
                })?;
                let val: u64 = s
                    .trim()
                    .parse()
                    .map_err(|e| StorageError::Corrupted(format!("term file not a number: {e}")))?;
                Ok(Term::new(val))
            }
            None => Ok(Term::default()),
        }
    }

    async fn save_voted_for(&self, node: Option<NodeId>) -> StorageResult<()> {
        let data = match node {
            Some(id) => id.as_u64().to_string(),
            None => String::new(),
        };
        self.atomic_write(&self.voted_for_path(), data.as_bytes())
            .await
    }

    async fn load_voted_for(&self) -> StorageResult<Option<NodeId>> {
        match self.read_file(&self.voted_for_path()).await? {
            Some(data) => {
                let s = String::from_utf8(data).map_err(|e| {
                    StorageError::Corrupted(format!("voted_for file not valid UTF-8: {e}"))
                })?;
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    Ok(None)
                } else {
                    let val: u64 = trimmed.parse().map_err(|e| {
                        StorageError::Corrupted(format!("voted_for file not a number: {e}"))
                    })?;
                    Ok(Some(NodeId::new(val)))
                }
            }
            None => Ok(None),
        }
    }

    async fn append_entries(&self, entries: &[LogEntry<T>]) -> StorageResult<()> {
        if entries.is_empty() {
            return Ok(());
        }

        // Read existing log file content (may not exist yet).
        let mut content = match self.read_file(&self.log_path()).await? {
            Some(data) => {
                String::from_utf8(data).map_err(|e| {
                    StorageError::Corrupted(format!("log file not valid UTF-8: {e}"))
                })?
            }
            None => String::new(),
        };

        // Append new entries as newline-delimited JSON.
        for entry in entries {
            let line = serde_json::to_string(entry)
                .map_err(|e| StorageError::Serialization(e.to_string()))?;
            content.push_str(&line);
            content.push('\n');
        }

        self.atomic_write(&self.log_path(), content.as_bytes())
            .await
    }

    async fn load_log(&self) -> StorageResult<Vec<LogEntry<T>>> {
        match self.read_file(&self.log_path()).await? {
            Some(data) => {
                let s = String::from_utf8(data).map_err(|e| {
                    StorageError::Corrupted(format!("log file not valid UTF-8: {e}"))
                })?;
                let mut entries = Vec::new();
                for (i, line) in s.lines().enumerate() {
                    if line.trim().is_empty() {
                        continue;
                    }
                    let entry: LogEntry<T> = serde_json::from_str(line).map_err(|e| {
                        StorageError::Deserialization(format!("log line {}: {e}", i + 1))
                    })?;
                    entries.push(entry);
                }
                Ok(entries)
            }
            None => Ok(Vec::new()),
        }
    }

    async fn truncate_log_from(&self, index: LogIndex) -> StorageResult<()> {
        let entries: Vec<LogEntry<T>> = self.load_log().await?;
        let keep_count = index.as_u64().saturating_sub(1) as usize;
        let kept = &entries[..std::cmp::min(keep_count, entries.len())];

        let mut content = String::new();
        for entry in kept {
            let line = serde_json::to_string(entry)
                .map_err(|e| StorageError::Serialization(e.to_string()))?;
            content.push_str(&line);
            content.push('\n');
        }

        self.atomic_write(&self.log_path(), content.as_bytes())
            .await
    }

    async fn save_snapshot(
        &self,
        snapshot: &[u8],
        last_included_index: LogIndex,
        last_included_term: Term,
    ) -> StorageResult<()> {
        // Write snapshot data.
        self.atomic_write(&self.snapshot_path(), snapshot).await?;

        // Write metadata (index and term) as a simple text format.
        let meta = format!(
            "{}\n{}",
            last_included_index.as_u64(),
            last_included_term.as_u64()
        );
        self.atomic_write(&self.snapshot_meta_path(), meta.as_bytes())
            .await
    }

    async fn load_snapshot(&self) -> StorageResult<Option<(Vec<u8>, LogIndex, Term)>> {
        let meta_data = match self.read_file(&self.snapshot_meta_path()).await? {
            Some(data) => data,
            None => return Ok(None),
        };

        let snapshot_data = match self.read_file(&self.snapshot_path()).await? {
            Some(data) => data,
            None => return Ok(None),
        };

        let meta_str = String::from_utf8(meta_data).map_err(|e| {
            StorageError::Corrupted(format!("snapshot meta not valid UTF-8: {e}"))
        })?;
        let mut lines = meta_str.lines();

        let index_str = lines.next().ok_or_else(|| {
            StorageError::Corrupted("snapshot meta missing index line".into())
        })?;
        let term_str = lines.next().ok_or_else(|| {
            StorageError::Corrupted("snapshot meta missing term line".into())
        })?;

        let index: u64 = index_str.trim().parse().map_err(|e| {
            StorageError::Corrupted(format!("snapshot meta index not a number: {e}"))
        })?;
        let term: u64 = term_str.trim().parse().map_err(|e| {
            StorageError::Corrupted(format!("snapshot meta term not a number: {e}"))
        })?;

        Ok(Some((
            snapshot_data,
            LogIndex::new(index),
            Term::new(term),
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn temp_storage() -> (FileStorage, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let storage = FileStorage::new(dir.path()).await.unwrap();
        (storage, dir)
    }

    // ---- Term persistence ----

    #[tokio::test]
    async fn save_and_load_term() {
        let (storage, _dir) = temp_storage().await;
        <FileStorage as RaftStorage<String>>::save_term(&storage, Term::new(5))
            .await
            .unwrap();
        let term = <FileStorage as RaftStorage<String>>::load_term(&storage)
            .await
            .unwrap();
        assert_eq!(term, Term::new(5));
    }

    #[tokio::test]
    async fn load_term_default_when_missing() {
        let (storage, _dir) = temp_storage().await;
        let term = <FileStorage as RaftStorage<String>>::load_term(&storage)
            .await
            .unwrap();
        assert_eq!(term, Term::default());
    }

    #[tokio::test]
    async fn save_term_overwrites_previous() {
        let (storage, _dir) = temp_storage().await;
        <FileStorage as RaftStorage<String>>::save_term(&storage, Term::new(1))
            .await
            .unwrap();
        <FileStorage as RaftStorage<String>>::save_term(&storage, Term::new(10))
            .await
            .unwrap();
        let term = <FileStorage as RaftStorage<String>>::load_term(&storage)
            .await
            .unwrap();
        assert_eq!(term, Term::new(10));
    }

    // ---- VotedFor persistence ----

    #[tokio::test]
    async fn save_and_load_voted_for_some() {
        let (storage, _dir) = temp_storage().await;
        <FileStorage as RaftStorage<String>>::save_voted_for(&storage, Some(NodeId::new(3)))
            .await
            .unwrap();
        let voted = <FileStorage as RaftStorage<String>>::load_voted_for(&storage)
            .await
            .unwrap();
        assert_eq!(voted, Some(NodeId::new(3)));
    }

    #[tokio::test]
    async fn save_and_load_voted_for_none() {
        let (storage, _dir) = temp_storage().await;
        <FileStorage as RaftStorage<String>>::save_voted_for(&storage, None)
            .await
            .unwrap();
        let voted = <FileStorage as RaftStorage<String>>::load_voted_for(&storage)
            .await
            .unwrap();
        assert_eq!(voted, None);
    }

    #[tokio::test]
    async fn load_voted_for_default_when_missing() {
        let (storage, _dir) = temp_storage().await;
        let voted = <FileStorage as RaftStorage<String>>::load_voted_for(&storage)
            .await
            .unwrap();
        assert_eq!(voted, None);
    }

    #[tokio::test]
    async fn voted_for_roundtrip_overwrite() {
        let (storage, _dir) = temp_storage().await;
        <FileStorage as RaftStorage<String>>::save_voted_for(&storage, Some(NodeId::new(1)))
            .await
            .unwrap();
        <FileStorage as RaftStorage<String>>::save_voted_for(&storage, None)
            .await
            .unwrap();
        let voted = <FileStorage as RaftStorage<String>>::load_voted_for(&storage)
            .await
            .unwrap();
        assert_eq!(voted, None);
    }

    // ---- Log persistence ----

    #[tokio::test]
    async fn append_and_load_log_entries() {
        let (storage, _dir) = temp_storage().await;
        let entries = vec![
            LogEntry {
                term: Term::new(1),
                index: LogIndex::new(1),
                command: "cmd-1".to_owned(),
            },
            LogEntry {
                term: Term::new(1),
                index: LogIndex::new(2),
                command: "cmd-2".to_owned(),
            },
        ];
        RaftStorage::<String>::append_entries(&storage, &entries)
            .await
            .unwrap();

        let loaded = RaftStorage::<String>::load_log(&storage).await.unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].command, "cmd-1");
        assert_eq!(loaded[1].command, "cmd-2");
    }

    #[tokio::test]
    async fn load_log_empty_when_missing() {
        let (storage, _dir) = temp_storage().await;
        let loaded = RaftStorage::<String>::load_log(&storage).await.unwrap();
        assert!(loaded.is_empty());
    }

    #[tokio::test]
    async fn append_entries_accumulates() {
        let (storage, _dir) = temp_storage().await;
        let entries1 = vec![LogEntry {
            term: Term::new(1),
            index: LogIndex::new(1),
            command: "a".to_owned(),
        }];
        let entries2 = vec![LogEntry {
            term: Term::new(1),
            index: LogIndex::new(2),
            command: "b".to_owned(),
        }];
        RaftStorage::<String>::append_entries(&storage, &entries1)
            .await
            .unwrap();
        RaftStorage::<String>::append_entries(&storage, &entries2)
            .await
            .unwrap();

        let loaded = RaftStorage::<String>::load_log(&storage).await.unwrap();
        assert_eq!(loaded.len(), 2);
    }

    #[tokio::test]
    async fn append_empty_entries_is_noop() {
        let (storage, _dir) = temp_storage().await;
        RaftStorage::<String>::append_entries(&storage, &[])
            .await
            .unwrap();
        let loaded = RaftStorage::<String>::load_log(&storage).await.unwrap();
        assert!(loaded.is_empty());
    }

    #[tokio::test]
    async fn truncate_log_from_middle() {
        let (storage, _dir) = temp_storage().await;
        let entries = vec![
            LogEntry {
                term: Term::new(1),
                index: LogIndex::new(1),
                command: "a".to_owned(),
            },
            LogEntry {
                term: Term::new(1),
                index: LogIndex::new(2),
                command: "b".to_owned(),
            },
            LogEntry {
                term: Term::new(2),
                index: LogIndex::new(3),
                command: "c".to_owned(),
            },
        ];
        RaftStorage::<String>::append_entries(&storage, &entries)
            .await
            .unwrap();
        RaftStorage::<String>::truncate_log_from(&storage, LogIndex::new(2))
            .await
            .unwrap();

        let loaded = RaftStorage::<String>::load_log(&storage).await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].command, "a");
    }

    #[tokio::test]
    async fn truncate_log_from_start() {
        let (storage, _dir) = temp_storage().await;
        let entries = vec![LogEntry {
            term: Term::new(1),
            index: LogIndex::new(1),
            command: "a".to_owned(),
        }];
        RaftStorage::<String>::append_entries(&storage, &entries)
            .await
            .unwrap();
        RaftStorage::<String>::truncate_log_from(&storage, LogIndex::new(1))
            .await
            .unwrap();

        let loaded = RaftStorage::<String>::load_log(&storage).await.unwrap();
        assert!(loaded.is_empty());
    }

    #[tokio::test]
    async fn truncate_log_beyond_end_is_noop() {
        let (storage, _dir) = temp_storage().await;
        let entries = vec![LogEntry {
            term: Term::new(1),
            index: LogIndex::new(1),
            command: "a".to_owned(),
        }];
        RaftStorage::<String>::append_entries(&storage, &entries)
            .await
            .unwrap();
        RaftStorage::<String>::truncate_log_from(&storage, LogIndex::new(10))
            .await
            .unwrap();

        let loaded = RaftStorage::<String>::load_log(&storage).await.unwrap();
        assert_eq!(loaded.len(), 1);
    }

    // ---- Snapshot persistence ----

    #[tokio::test]
    async fn save_and_load_snapshot() {
        let (storage, _dir) = temp_storage().await;
        let snapshot_data = b"snapshot-state-data";
        RaftStorage::<String>::save_snapshot(
            &storage,
            snapshot_data,
            LogIndex::new(10),
            Term::new(3),
        )
        .await
        .unwrap();

        let result = RaftStorage::<String>::load_snapshot(&storage)
            .await
            .unwrap();
        let (data, index, term) = result.unwrap();
        assert_eq!(data, snapshot_data);
        assert_eq!(index, LogIndex::new(10));
        assert_eq!(term, Term::new(3));
    }

    #[tokio::test]
    async fn load_snapshot_none_when_missing() {
        let (storage, _dir) = temp_storage().await;
        let result = RaftStorage::<String>::load_snapshot(&storage)
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn snapshot_overwrites_previous() {
        let (storage, _dir) = temp_storage().await;
        RaftStorage::<String>::save_snapshot(
            &storage,
            b"old",
            LogIndex::new(5),
            Term::new(1),
        )
        .await
        .unwrap();
        RaftStorage::<String>::save_snapshot(
            &storage,
            b"new",
            LogIndex::new(10),
            Term::new(2),
        )
        .await
        .unwrap();

        let (data, index, term) = RaftStorage::<String>::load_snapshot(&storage)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(data, b"new");
        assert_eq!(index, LogIndex::new(10));
        assert_eq!(term, Term::new(2));
    }

    // ---- Atomic write behavior ----

    #[tokio::test]
    async fn no_temp_files_left_after_write() {
        let (storage, dir) = temp_storage().await;
        <FileStorage as RaftStorage<String>>::save_term(&storage, Term::new(1))
            .await
            .unwrap();

        let mut entries = fs::read_dir(dir.path()).await.unwrap();
        let mut files = Vec::new();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            files.push(entry.file_name().to_string_lossy().to_string());
        }
        // No .tmp files should remain.
        assert!(!files.iter().any(|f| f.starts_with(".tmp")));
    }

    // ---- Full state roundtrip ----

    #[tokio::test]
    async fn full_state_roundtrip() {
        let (storage, _dir) = temp_storage().await;

        // Save all state.
        <FileStorage as RaftStorage<String>>::save_term(&storage, Term::new(7))
            .await
            .unwrap();
        <FileStorage as RaftStorage<String>>::save_voted_for(&storage, Some(NodeId::new(2)))
            .await
            .unwrap();
        let entries = vec![
            LogEntry {
                term: Term::new(5),
                index: LogIndex::new(1),
                command: "x".to_owned(),
            },
            LogEntry {
                term: Term::new(7),
                index: LogIndex::new(2),
                command: "y".to_owned(),
            },
        ];
        RaftStorage::<String>::append_entries(&storage, &entries)
            .await
            .unwrap();
        RaftStorage::<String>::save_snapshot(
            &storage,
            b"snap",
            LogIndex::new(1),
            Term::new(5),
        )
        .await
        .unwrap();

        // Load and verify all state.
        assert_eq!(
            <FileStorage as RaftStorage<String>>::load_term(&storage)
                .await
                .unwrap(),
            Term::new(7)
        );
        assert_eq!(
            <FileStorage as RaftStorage<String>>::load_voted_for(&storage)
                .await
                .unwrap(),
            Some(NodeId::new(2))
        );
        let loaded_log = RaftStorage::<String>::load_log(&storage).await.unwrap();
        assert_eq!(loaded_log.len(), 2);
        assert_eq!(loaded_log[0].command, "x");
        assert_eq!(loaded_log[1].command, "y");
        let (snap_data, snap_idx, snap_term) = RaftStorage::<String>::load_snapshot(&storage)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snap_data, b"snap");
        assert_eq!(snap_idx, LogIndex::new(1));
        assert_eq!(snap_term, Term::new(5));
    }
}
