use pirc_common::RaftError;
use pirc_protocol::{Command, Message, PircSubcommand};
use serde::{Deserialize, Serialize};

use super::types::{LogEntry, LogIndex, NodeId, Term};

/// `RequestVote` RPC (sent by candidates to gather votes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestVote {
    pub term: Term,
    pub candidate_id: NodeId,
    pub last_log_index: LogIndex,
    pub last_log_term: Term,
}

/// Response to a `RequestVote` RPC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestVoteResponse {
    pub term: Term,
    pub vote_granted: bool,
}

/// `AppendEntries` RPC (sent by leader to replicate log entries and as heartbeat).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppendEntries<T> {
    pub term: Term,
    pub leader_id: NodeId,
    pub prev_log_index: LogIndex,
    pub prev_log_term: Term,
    pub entries: Vec<LogEntry<T>>,
    pub leader_commit: LogIndex,
}

/// Response to an `AppendEntries` RPC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppendEntriesResponse {
    pub term: Term,
    pub success: bool,
    pub match_index: LogIndex,
}

/// Envelope for all Raft RPC message types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RaftMessage<T> {
    RequestVote(RequestVote),
    RequestVoteResponse(RequestVoteResponse),
    AppendEntries(AppendEntries<T>),
    AppendEntriesResponse(AppendEntriesResponse),
}

impl<T: Serialize> RaftMessage<T> {
    /// Encode this Raft message into a PIRC CLUSTER RAFT protocol message.
    pub fn to_protocol_message(&self) -> Result<Message, RaftError> {
        let json = serde_json::to_string(self).map_err(|e| RaftError::InvalidRpc {
            message: e.to_string(),
        })?;
        Ok(Message::new(
            Command::Pirc(PircSubcommand::ClusterRaft),
            vec![json],
        ))
    }
}

impl<T: for<'de> Deserialize<'de>> RaftMessage<T> {
    /// Decode a PIRC CLUSTER RAFT protocol message into a Raft message.
    pub fn from_protocol_message(msg: &Message) -> Result<Self, RaftError> {
        match &msg.command {
            Command::Pirc(PircSubcommand::ClusterRaft) => {
                let payload = msg.params.first().ok_or_else(|| RaftError::InvalidRpc {
                    message: "missing raft payload".into(),
                })?;
                serde_json::from_str(payload).map_err(|e| RaftError::InvalidRpc {
                    message: e.to_string(),
                })
            }
            _ => Err(RaftError::InvalidRpc {
                message: format!("expected PIRC CLUSTER RAFT, got {:?}", msg.command),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- RequestVote ----

    #[test]
    fn request_vote_serde_roundtrip() {
        let rv = RequestVote {
            term: Term::new(5),
            candidate_id: NodeId::new(1),
            last_log_index: LogIndex::new(10),
            last_log_term: Term::new(4),
        };
        let json = serde_json::to_string(&rv).unwrap();
        let deserialized: RequestVote = serde_json::from_str(&json).unwrap();
        assert_eq!(rv, deserialized);
    }

    // ---- RequestVoteResponse ----

    #[test]
    fn request_vote_response_serde_roundtrip() {
        let resp = RequestVoteResponse {
            term: Term::new(5),
            vote_granted: true,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: RequestVoteResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, deserialized);
    }

    // ---- AppendEntries ----

    #[test]
    fn append_entries_empty_serde_roundtrip() {
        let ae: AppendEntries<String> = AppendEntries {
            term: Term::new(3),
            leader_id: NodeId::new(1),
            prev_log_index: LogIndex::new(5),
            prev_log_term: Term::new(2),
            entries: vec![],
            leader_commit: LogIndex::new(4),
        };
        let json = serde_json::to_string(&ae).unwrap();
        let deserialized: AppendEntries<String> = serde_json::from_str(&json).unwrap();
        assert_eq!(ae, deserialized);
    }

    #[test]
    fn append_entries_with_entries_serde_roundtrip() {
        let ae = AppendEntries {
            term: Term::new(3),
            leader_id: NodeId::new(1),
            prev_log_index: LogIndex::new(5),
            prev_log_term: Term::new(2),
            entries: vec![
                LogEntry {
                    term: Term::new(3),
                    index: LogIndex::new(6),
                    command: "cmd1".to_owned(),
                },
                LogEntry {
                    term: Term::new(3),
                    index: LogIndex::new(7),
                    command: "cmd2".to_owned(),
                },
            ],
            leader_commit: LogIndex::new(5),
        };
        let json = serde_json::to_string(&ae).unwrap();
        let deserialized: AppendEntries<String> = serde_json::from_str(&json).unwrap();
        assert_eq!(ae, deserialized);
    }

    // ---- AppendEntriesResponse ----

    #[test]
    fn append_entries_response_serde_roundtrip() {
        let resp = AppendEntriesResponse {
            term: Term::new(3),
            success: true,
            match_index: LogIndex::new(7),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: AppendEntriesResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, deserialized);
    }

    // ---- RaftMessage envelope ----

    #[test]
    fn raft_message_request_vote_serde_roundtrip() {
        let msg: RaftMessage<String> = RaftMessage::RequestVote(RequestVote {
            term: Term::new(1),
            candidate_id: NodeId::new(2),
            last_log_index: LogIndex::new(0),
            last_log_term: Term::new(0),
        });
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: RaftMessage<String> = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, deserialized);
    }

    #[test]
    fn raft_message_append_entries_serde_roundtrip() {
        let msg = RaftMessage::AppendEntries(AppendEntries {
            term: Term::new(2),
            leader_id: NodeId::new(1),
            prev_log_index: LogIndex::new(3),
            prev_log_term: Term::new(1),
            entries: vec![LogEntry {
                term: Term::new(2),
                index: LogIndex::new(4),
                command: "test".to_owned(),
            }],
            leader_commit: LogIndex::new(3),
        });
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: RaftMessage<String> = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, deserialized);
    }

    // ---- Protocol message conversion ----

    #[test]
    fn raft_message_to_protocol_message() {
        let msg: RaftMessage<String> = RaftMessage::RequestVote(RequestVote {
            term: Term::new(1),
            candidate_id: NodeId::new(2),
            last_log_index: LogIndex::new(0),
            last_log_term: Term::new(0),
        });
        let proto = msg.to_protocol_message().unwrap();
        assert_eq!(proto.command, Command::Pirc(PircSubcommand::ClusterRaft));
        assert_eq!(proto.params.len(), 1);
        // The payload should be valid JSON
        let _: RaftMessage<String> = serde_json::from_str(&proto.params[0]).unwrap();
    }

    #[test]
    fn raft_message_protocol_roundtrip() {
        let original: RaftMessage<String> =
            RaftMessage::AppendEntriesResponse(AppendEntriesResponse {
                term: Term::new(5),
                success: true,
                match_index: LogIndex::new(10),
            });
        let proto = original.to_protocol_message().unwrap();
        let decoded: RaftMessage<String> = RaftMessage::from_protocol_message(&proto).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn raft_message_from_wrong_command() {
        let msg = Message::new(Command::Ping, vec!["test".to_owned()]);
        let result: Result<RaftMessage<String>, _> = RaftMessage::from_protocol_message(&msg);
        assert!(result.is_err());
    }

    #[test]
    fn raft_message_from_missing_payload() {
        let msg = Message::new(Command::Pirc(PircSubcommand::ClusterRaft), vec![]);
        let result: Result<RaftMessage<String>, _> = RaftMessage::from_protocol_message(&msg);
        assert!(result.is_err());
    }

    #[test]
    fn raft_message_from_invalid_json() {
        let msg = Message::new(
            Command::Pirc(PircSubcommand::ClusterRaft),
            vec!["not json".to_owned()],
        );
        let result: Result<RaftMessage<String>, _> = RaftMessage::from_protocol_message(&msg);
        assert!(result.is_err());
    }
}
