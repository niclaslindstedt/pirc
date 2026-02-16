//! Group P2P session state machine for managing mesh connections among
//! group chat members.
//!
//! Tracks the P2P connectivity state for each member in a group chat and
//! emits events as members connect and disconnect.

use std::collections::{HashMap, HashSet};

/// State of a group P2P session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupSessionState {
    /// Group session is being created, not yet active.
    Creating,
    /// Establishing P2P connections to group members.
    Establishing,
    /// All members are connected via P2P.
    Active,
    /// Some P2P connections failed; those members use server relay.
    Degraded,
}

/// Events emitted by a [`GroupSession`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupSessionEvent {
    /// A member has connected via P2P.
    MemberConnected {
        /// Nickname of the connected member.
        member: String,
    },
    /// A member has disconnected from P2P.
    MemberDisconnected {
        /// Nickname of the disconnected member.
        member: String,
    },
    /// A member's P2P connection failed; falling back to relay.
    MemberFallbackToRelay {
        /// Nickname of the member using relay.
        member: String,
        /// Reason the P2P connection failed.
        reason: String,
    },
    /// The group session state has changed.
    StateChanged {
        /// The new state.
        new_state: GroupSessionState,
    },
}

/// Manages the P2P mesh connections for a group chat.
///
/// Tracks which members have established P2P connections and which are
/// falling back to server-relayed messaging. Emits events for state
/// transitions that the application layer can react to.
pub struct GroupSession {
    /// The group identifier (as a string for flexibility with the P2P layer).
    group_id: String,
    /// Current session state.
    state: GroupSessionState,
    /// All expected members of the group (excluding self).
    expected_members: HashSet<String>,
    /// Members with active P2P connections.
    connected_members: HashSet<String>,
    /// Members that have fallen back to relay.
    relay_members: HashSet<String>,
    /// Per-member P2P session identifiers (opaque, for correlation).
    member_sessions: HashMap<String, String>,
    /// Pending outbound events.
    events: Vec<GroupSessionEvent>,
}

impl GroupSession {
    /// Creates a new `GroupSession` for the given group.
    ///
    /// `expected_members` should contain the nicknames of all other group
    /// members (not including the local user).
    #[must_use]
    pub fn new(group_id: String, expected_members: HashSet<String>) -> Self {
        Self {
            group_id,
            state: GroupSessionState::Creating,
            expected_members,
            connected_members: HashSet::new(),
            relay_members: HashSet::new(),
            member_sessions: HashMap::new(),
            events: Vec::new(),
        }
    }

    /// Returns the group identifier.
    #[must_use]
    pub fn group_id(&self) -> &str {
        &self.group_id
    }

    /// Returns the current session state.
    #[must_use]
    pub fn state(&self) -> GroupSessionState {
        self.state
    }

    /// Returns the set of members with active P2P connections.
    #[must_use]
    pub fn connected_members(&self) -> &HashSet<String> {
        &self.connected_members
    }

    /// Returns the set of members using server relay.
    #[must_use]
    pub fn relay_members(&self) -> &HashSet<String> {
        &self.relay_members
    }

    /// Returns the set of expected members.
    #[must_use]
    pub fn expected_members(&self) -> &HashSet<String> {
        &self.expected_members
    }

    /// Drains all pending outbound events.
    pub fn drain_events(&mut self) -> Vec<GroupSessionEvent> {
        std::mem::take(&mut self.events)
    }

    /// Begins establishing P2P connections to group members.
    ///
    /// Transitions from `Creating` to `Establishing`.
    pub fn begin_establishing(&mut self) {
        if self.state == GroupSessionState::Creating {
            self.transition(GroupSessionState::Establishing);
        }
    }

    /// Records that a member has connected via P2P.
    pub fn member_connected(&mut self, member: &str) {
        if !self.expected_members.contains(member) {
            return;
        }
        self.connected_members.insert(member.to_owned());
        self.relay_members.remove(member);
        self.events.push(GroupSessionEvent::MemberConnected {
            member: member.to_owned(),
        });
        self.recalculate_state();
    }

    /// Records that a member has disconnected from P2P.
    pub fn member_disconnected(&mut self, member: &str) {
        if self.connected_members.remove(member) {
            self.events.push(GroupSessionEvent::MemberDisconnected {
                member: member.to_owned(),
            });
            self.recalculate_state();
        }
    }

    /// Records that a member's P2P connection failed and they're using relay.
    pub fn member_fallback_to_relay(&mut self, member: &str, reason: String) {
        if !self.expected_members.contains(member) {
            return;
        }
        self.connected_members.remove(member);
        self.relay_members.insert(member.to_owned());
        self.events.push(GroupSessionEvent::MemberFallbackToRelay {
            member: member.to_owned(),
            reason,
        });
        self.recalculate_state();
    }

    /// Associates an opaque session ID with a member for correlation.
    pub fn set_member_session(&mut self, member: &str, session_id: String) {
        self.member_sessions.insert(member.to_owned(), session_id);
    }

    /// Returns the session ID for a member, if any.
    #[must_use]
    pub fn get_member_session(&self, member: &str) -> Option<&str> {
        self.member_sessions.get(member).map(String::as_str)
    }

    /// Adds a new expected member to the group.
    pub fn add_expected_member(&mut self, member: String) {
        self.expected_members.insert(member);
        self.recalculate_state();
    }

    /// Removes a member from the group entirely.
    pub fn remove_member(&mut self, member: &str) {
        self.expected_members.remove(member);
        self.connected_members.remove(member);
        self.relay_members.remove(member);
        self.member_sessions.remove(member);
        self.recalculate_state();
    }

    /// Recalculates the session state based on current connectivity.
    fn recalculate_state(&mut self) {
        if self.expected_members.is_empty() {
            self.transition(GroupSessionState::Active);
            return;
        }

        let all_accounted = self.connected_members.len() + self.relay_members.len()
            == self.expected_members.len();
        let all_connected = self.connected_members.len() == self.expected_members.len();

        let new_state = if all_connected {
            GroupSessionState::Active
        } else if all_accounted {
            GroupSessionState::Degraded
        } else {
            GroupSessionState::Establishing
        };

        self.transition(new_state);
    }

    /// Transitions to a new state, emitting an event if the state changed.
    fn transition(&mut self, new_state: GroupSessionState) {
        if self.state != new_state {
            self.state = new_state;
            self.events.push(GroupSessionEvent::StateChanged {
                new_state,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn members(names: &[&str]) -> HashSet<String> {
        names.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn new_session_starts_creating() {
        let session = GroupSession::new("g1".into(), members(&["alice", "bob"]));
        assert_eq!(session.state(), GroupSessionState::Creating);
        assert_eq!(session.group_id(), "g1");
        assert!(session.connected_members().is_empty());
        assert!(session.relay_members().is_empty());
        assert_eq!(session.expected_members().len(), 2);
    }

    #[test]
    fn begin_establishing_transitions() {
        let mut session = GroupSession::new("g1".into(), members(&["alice"]));
        session.begin_establishing();
        assert_eq!(session.state(), GroupSessionState::Establishing);
        let events = session.drain_events();
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            GroupSessionEvent::StateChanged {
                new_state: GroupSessionState::Establishing
            }
        ));
    }

    #[test]
    fn member_connected_transitions_to_active() {
        let mut session = GroupSession::new("g1".into(), members(&["alice"]));
        session.begin_establishing();
        session.drain_events();

        session.member_connected("alice");
        assert_eq!(session.state(), GroupSessionState::Active);
        assert!(session.connected_members().contains("alice"));

        let events = session.drain_events();
        assert!(events
            .iter()
            .any(|e| matches!(e, GroupSessionEvent::MemberConnected { member } if member == "alice")));
        assert!(events
            .iter()
            .any(|e| matches!(e, GroupSessionEvent::StateChanged { new_state: GroupSessionState::Active })));
    }

    #[test]
    fn partial_connections_stay_establishing() {
        let mut session = GroupSession::new("g1".into(), members(&["alice", "bob"]));
        session.begin_establishing();
        session.drain_events();

        session.member_connected("alice");
        assert_eq!(session.state(), GroupSessionState::Establishing);
    }

    #[test]
    fn all_connected_transitions_to_active() {
        let mut session = GroupSession::new("g1".into(), members(&["alice", "bob"]));
        session.begin_establishing();
        session.member_connected("alice");
        session.member_connected("bob");
        assert_eq!(session.state(), GroupSessionState::Active);
    }

    #[test]
    fn fallback_to_relay_transitions_to_degraded() {
        let mut session = GroupSession::new("g1".into(), members(&["alice", "bob"]));
        session.begin_establishing();
        session.member_connected("alice");
        session.member_fallback_to_relay("bob", "timeout".into());
        assert_eq!(session.state(), GroupSessionState::Degraded);
        assert!(session.relay_members().contains("bob"));
    }

    #[test]
    fn member_disconnected_emits_event() {
        let mut session = GroupSession::new("g1".into(), members(&["alice"]));
        session.begin_establishing();
        session.member_connected("alice");
        session.drain_events();

        session.member_disconnected("alice");
        let events = session.drain_events();
        assert!(events
            .iter()
            .any(|e| matches!(e, GroupSessionEvent::MemberDisconnected { member } if member == "alice")));
    }

    #[test]
    fn member_disconnected_nonexistent_is_noop() {
        let mut session = GroupSession::new("g1".into(), members(&["alice"]));
        session.member_disconnected("ghost");
        assert!(session.drain_events().is_empty());
    }

    #[test]
    fn connected_ignores_unknown_member() {
        let mut session = GroupSession::new("g1".into(), members(&["alice"]));
        session.member_connected("ghost");
        assert!(session.connected_members().is_empty());
    }

    #[test]
    fn member_session_tracking() {
        let mut session = GroupSession::new("g1".into(), members(&["alice"]));
        assert!(session.get_member_session("alice").is_none());
        session.set_member_session("alice", "sess-123".into());
        assert_eq!(session.get_member_session("alice"), Some("sess-123"));
    }

    #[test]
    fn add_expected_member() {
        let mut session = GroupSession::new("g1".into(), members(&["alice"]));
        session.begin_establishing();
        session.member_connected("alice");
        assert_eq!(session.state(), GroupSessionState::Active);

        session.add_expected_member("bob".into());
        assert_eq!(session.state(), GroupSessionState::Establishing);
    }

    #[test]
    fn remove_member() {
        let mut session = GroupSession::new("g1".into(), members(&["alice", "bob"]));
        session.begin_establishing();
        session.member_connected("alice");

        session.remove_member("bob");
        assert_eq!(session.state(), GroupSessionState::Active);
        assert!(!session.expected_members().contains("bob"));
    }

    #[test]
    fn empty_expected_members_is_active() {
        let mut session = GroupSession::new("g1".into(), HashSet::new());
        session.begin_establishing();
        session.drain_events();

        // With no expected members, recalculate should set Active
        session.add_expected_member("alice".into());
        session.remove_member("alice");
        assert_eq!(session.state(), GroupSessionState::Active);
    }

    #[test]
    fn drain_events_clears_events() {
        let mut session = GroupSession::new("g1".into(), members(&["alice"]));
        session.begin_establishing();
        assert!(!session.drain_events().is_empty());
        assert!(session.drain_events().is_empty());
    }

    #[test]
    fn group_session_state_equality() {
        assert_eq!(GroupSessionState::Creating, GroupSessionState::Creating);
        assert_eq!(
            GroupSessionState::Establishing,
            GroupSessionState::Establishing
        );
        assert_eq!(GroupSessionState::Active, GroupSessionState::Active);
        assert_eq!(GroupSessionState::Degraded, GroupSessionState::Degraded);
        assert_ne!(GroupSessionState::Creating, GroupSessionState::Active);
    }

    #[test]
    fn group_session_event_clone() {
        let event = GroupSessionEvent::MemberConnected {
            member: "alice".into(),
        };
        let cloned = event.clone();
        assert_eq!(event, cloned);
    }

    #[test]
    fn reconnect_after_relay_transitions_from_degraded() {
        let mut session = GroupSession::new("g1".into(), members(&["alice"]));
        session.begin_establishing();
        session.member_fallback_to_relay("alice", "timeout".into());
        assert_eq!(session.state(), GroupSessionState::Degraded);

        // Alice reconnects via P2P
        session.member_connected("alice");
        assert_eq!(session.state(), GroupSessionState::Active);
        assert!(!session.relay_members().contains("alice"));
    }
}
