use super::types::{LogEntry, LogIndex, NodeId, Term};

/// Result type for storage operations.
pub type StorageResult<T> = std::result::Result<T, StorageError>;

/// Errors that can occur during storage operations.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("deserialization error: {0}")]
    Deserialization(String),
    #[error("corrupted data: {0}")]
    Corrupted(String),
}

/// Async trait for persistent storage of Raft state.
///
/// Implementations must ensure durability: once a method returns `Ok`,
/// the data must survive process crashes.
///
/// This trait uses `async fn` (RPITIT, stable since Rust 1.75). Implementors
/// should ensure their futures are `Send` for compatibility with tokio.
pub trait RaftStorage<T: Send + Sync>: Send + Sync {
    fn save_term(
        &self,
        term: Term,
    ) -> impl std::future::Future<Output = StorageResult<()>> + Send;

    fn load_term(&self) -> impl std::future::Future<Output = StorageResult<Term>> + Send;

    fn save_voted_for(
        &self,
        node: Option<NodeId>,
    ) -> impl std::future::Future<Output = StorageResult<()>> + Send;

    fn load_voted_for(
        &self,
    ) -> impl std::future::Future<Output = StorageResult<Option<NodeId>>> + Send;

    fn append_entries(
        &self,
        entries: &[LogEntry<T>],
    ) -> impl std::future::Future<Output = StorageResult<()>> + Send;

    fn load_log(
        &self,
    ) -> impl std::future::Future<Output = StorageResult<Vec<LogEntry<T>>>> + Send;

    fn truncate_log_from(
        &self,
        index: LogIndex,
    ) -> impl std::future::Future<Output = StorageResult<()>> + Send;

    fn save_snapshot(
        &self,
        snapshot: &[u8],
        last_included_index: LogIndex,
        last_included_term: Term,
    ) -> impl std::future::Future<Output = StorageResult<()>> + Send;

    fn load_snapshot(
        &self,
    ) -> impl std::future::Future<Output = StorageResult<Option<(Vec<u8>, LogIndex, Term)>>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_error_display() {
        let io_err = StorageError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "not found",
        ));
        assert!(io_err.to_string().contains("not found"));

        let ser_err = StorageError::Serialization("bad json".into());
        assert_eq!(ser_err.to_string(), "serialization error: bad json");

        let de_err = StorageError::Deserialization("unexpected EOF".into());
        assert_eq!(de_err.to_string(), "deserialization error: unexpected EOF");

        let corrupt_err = StorageError::Corrupted("checksum mismatch".into());
        assert_eq!(corrupt_err.to_string(), "corrupted data: checksum mismatch");
    }

    #[test]
    fn storage_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::Other, "disk full");
        let storage_err: StorageError = io_err.into();
        assert!(matches!(storage_err, StorageError::Io(_)));
    }
}
