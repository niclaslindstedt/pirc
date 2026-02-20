//! Client-level group chat integration tests.
//!
//! Exercises the full client-side group chat workflow: creating groups,
//! inviting clients, P2P messaging, server relay for degraded members,
//! and group state synchronization.

use pirc_common::types::GroupId;
use pirc_crypto::group_key::{GroupEncryptionState, GroupKeyManager};
use pirc_p2p::group_mesh::{GroupMesh, GroupMeshEvent, PeerConnectionState};
use pirc_p2p::group_session::{GroupSession, GroupSessionState};
use pirc_server::group_registry::GroupRegistry;

use super::{create_test_session_pair_unique, members, mock_transport};

// ── Client creates group ─────────────────────────────────────────────

#[test]
fn client_creates_group_on_server() {
    let registry = GroupRegistry::new();
    let group_id = registry.create_group("my-chat".into(), "alice".into(), 1000);

    assert!(registry.exists(group_id));
    assert_eq!(registry.group_name(group_id), Some("my-chat".into()));
    assert!(registry.is_member(group_id, "alice"));
    assert!(registry.is_admin(group_id, "alice"));
}

#[test]
fn client_creates_group_with_encryption_manager() {
    let group_id = GroupId::new(1);
    let key_mgr = GroupKeyManager::new(group_id);

    assert_eq!(key_mgr.group_id(), group_id);
    assert_eq!(key_mgr.member_count(), 0);
    // Empty group is not "all ready" — need at least one member with a session
    assert!(!key_mgr.all_ready());
}

// ── Client invites other clients ─────────────────────────────────────

#[test]
fn invite_adds_member_to_server_group() {
    let registry = GroupRegistry::new();
    let group_id = registry.create_group("my-chat".into(), "alice".into(), 1000);

    assert!(registry.add_member(group_id, "bob".into(), 1001));
    assert!(registry.add_member(group_id, "charlie".into(), 1002));

    assert!(registry.is_member(group_id, "bob"));
    assert!(registry.is_member(group_id, "charlie"));
    assert!(!registry.is_admin(group_id, "bob"));
    assert!(!registry.is_admin(group_id, "charlie"));
}

#[test]
fn invited_client_joins_mesh_as_connecting() {
    let mut mesh = GroupMesh::new("1".into());
    mesh.add_member("bob".into());
    mesh.add_member("charlie".into());

    assert_eq!(
        mesh.member_state("bob"),
        Some(PeerConnectionState::Connecting)
    );
    assert_eq!(
        mesh.member_state("charlie"),
        Some(PeerConnectionState::Connecting)
    );
    assert_eq!(mesh.member_count(), 2);
}

#[test]
fn invited_client_added_to_key_manager() {
    let group_id = GroupId::new(1);
    let mut key_mgr = GroupKeyManager::new(group_id);

    key_mgr.add_member("bob");
    key_mgr.add_member("charlie");

    assert_eq!(
        key_mgr.member_state("bob"),
        Some(GroupEncryptionState::Pending)
    );
    assert_eq!(
        key_mgr.member_state("charlie"),
        Some(GroupEncryptionState::Pending)
    );
    assert_eq!(key_mgr.member_count(), 2);
}

// ── Invited clients join group ───────────────────────────────────────

#[test]
fn invited_clients_establish_sessions_and_connect() {
    let group_id = GroupId::new(1);
    let mut key_mgr = GroupKeyManager::new(group_id);

    // Simulate key exchange completion
    let (s_to_bob, _) = create_test_session_pair_unique(0x10);
    key_mgr.add_member("bob");
    key_mgr.set_establishing("bob");
    assert_eq!(
        key_mgr.member_state("bob"),
        Some(GroupEncryptionState::Establishing)
    );

    key_mgr.set_session("bob", s_to_bob);
    assert_eq!(
        key_mgr.member_state("bob"),
        Some(GroupEncryptionState::Ready)
    );
}

#[tokio::test]
async fn full_group_setup_workflow() {
    // 1. Server creates group
    let registry = GroupRegistry::new();
    let server_group_id = registry.create_group("dev-chat".into(), "alice".into(), 1000);
    registry.add_member(server_group_id, "bob".into(), 1001);
    registry.add_member(server_group_id, "charlie".into(), 1002);

    // 2. Alice sets up local group state
    let group_id = GroupId::new(server_group_id.as_u64());
    let mut alice_keys = GroupKeyManager::new(group_id);
    let mut alice_mesh = GroupMesh::new(group_id.as_u64().to_string());
    let mut alice_session =
        GroupSession::new(group_id.as_u64().to_string(), members(&["bob", "charlie"]));

    // Add members
    alice_keys.add_member("bob");
    alice_keys.add_member("charlie");
    alice_mesh.add_member("bob".into());
    alice_mesh.add_member("charlie".into());
    alice_session.begin_establishing();

    // 3. Key exchange completes
    let (s_to_bob, _) = create_test_session_pair_unique(0x10);
    let (s_to_charlie, _) = create_test_session_pair_unique(0x20);
    alice_keys.set_session("bob", s_to_bob);
    alice_keys.set_session("charlie", s_to_charlie);
    assert!(alice_keys.all_ready());

    // 4. P2P connections established
    alice_mesh.member_connected("bob".into(), mock_transport().await);
    alice_session.member_connected("bob");
    alice_mesh.member_connected("charlie".into(), mock_transport().await);
    alice_session.member_connected("charlie");

    // 5. Verify full setup
    assert!(alice_mesh.is_fully_connected());
    assert_eq!(alice_session.state(), GroupSessionState::Active);
    assert!(alice_keys.all_ready());

    // MeshReady should be in events
    let events = alice_mesh.drain_events();
    assert!(events.iter().any(|e| matches!(e, GroupMeshEvent::MeshReady)));
}

// ── P2P messaging in group ───────────────────────────────────────────

#[test]
fn group_message_encrypted_for_all_members() {
    let group_id = GroupId::new(1);
    let mut alice_keys = GroupKeyManager::new(group_id);

    let (s_to_bob, _) = create_test_session_pair_unique(0x10);
    let (s_to_charlie, _) = create_test_session_pair_unique(0x20);
    alice_keys.add_member("bob");
    alice_keys.set_session("bob", s_to_bob);
    alice_keys.add_member("charlie");
    alice_keys.set_session("charlie", s_to_charlie);

    let plaintext = b"hello group via P2P!";
    let encrypted_map = alice_keys.encrypt_for_group(plaintext).expect("encrypt");

    assert_eq!(encrypted_map.len(), 2);
    assert!(encrypted_map.contains_key("bob"));
    assert!(encrypted_map.contains_key("charlie"));
}

#[test]
fn group_message_decrypt_round_trip() {
    let group_id = GroupId::new(1);

    // Alice sends to bob
    let mut alice_keys = GroupKeyManager::new(group_id);
    let mut bob_keys = GroupKeyManager::new(group_id);

    let (a_to_b, b_from_a) = create_test_session_pair_unique(0x10);
    alice_keys.add_member("bob");
    alice_keys.set_session("bob", a_to_b);
    bob_keys.add_member("alice");
    bob_keys.set_session("alice", b_from_a);

    let plaintext = b"P2P group message";
    let encrypted_map = alice_keys.encrypt_for_group(plaintext).expect("encrypt");

    let decrypted = bob_keys
        .decrypt_from_member("alice", &encrypted_map["bob"])
        .expect("decrypt");
    assert_eq!(decrypted, plaintext);
}

// ── Group state synchronized ─────────────────────────────────────────

#[tokio::test]
async fn group_state_consistent_across_participants() {
    let group_id = GroupId::new(1);

    // Simulate Alice's and Bob's views of the group
    let mut alice_mesh = GroupMesh::new(group_id.as_u64().to_string());
    let mut bob_mesh = GroupMesh::new(group_id.as_u64().to_string());

    // Both track each other and charlie
    alice_mesh.add_member("bob".into());
    alice_mesh.add_member("charlie".into());

    bob_mesh.add_member("alice".into());
    bob_mesh.add_member("charlie".into());

    // All connect
    alice_mesh.member_connected("bob".into(), mock_transport().await);
    alice_mesh.member_connected("charlie".into(), mock_transport().await);

    bob_mesh.member_connected("alice".into(), mock_transport().await);
    bob_mesh.member_connected("charlie".into(), mock_transport().await);

    // Both see a fully connected mesh
    assert!(alice_mesh.is_fully_connected());
    assert!(bob_mesh.is_fully_connected());

    assert_eq!(alice_mesh.member_count(), 2);
    assert_eq!(bob_mesh.member_count(), 2);
}

// ── Group with degraded member still functional ──────────────────────

#[tokio::test]
async fn group_functions_with_mixed_delivery_paths() {
    let group_id = GroupId::new(1);

    let mut alice_keys = GroupKeyManager::new(group_id);
    let mut alice_mesh = GroupMesh::new(group_id.as_u64().to_string());

    // Bob connects via P2P, charlie degrades to relay
    let (a_to_b, _) = create_test_session_pair_unique(0x10);
    let (a_to_c, _) = create_test_session_pair_unique(0x20);

    alice_keys.add_member("bob");
    alice_keys.set_session("bob", a_to_b);
    alice_mesh.add_member("bob".into());
    alice_mesh.member_connected("bob".into(), mock_transport().await);

    alice_keys.add_member("charlie");
    alice_keys.set_session("charlie", a_to_c);
    alice_mesh.add_member("charlie".into());
    alice_mesh.member_degraded("charlie", "NAT failed".into());

    // Alice can still encrypt for all members
    assert!(alice_keys.all_ready());
    let encrypted_map = alice_keys.encrypt_for_group(b"mixed delivery").expect("encrypt");
    assert_eq!(encrypted_map.len(), 2);

    // Bob gets P2P transport, charlie needs relay
    assert!(alice_mesh.get_transport("bob").is_some());
    assert!(alice_mesh.get_transport("charlie").is_none());
    assert!(alice_mesh.is_degraded());
}

// ── Multiple groups per client ───────────────────────────────────────

#[test]
fn client_manages_multiple_groups() {
    let g1 = GroupId::new(1);
    let g2 = GroupId::new(2);

    let mut g1_keys = GroupKeyManager::new(g1);
    let mut g2_keys = GroupKeyManager::new(g2);

    let (s1, _) = create_test_session_pair_unique(0x10);
    let (s2, _) = create_test_session_pair_unique(0x20);

    g1_keys.add_member("bob");
    g1_keys.set_session("bob", s1);

    g2_keys.add_member("charlie");
    g2_keys.set_session("charlie", s2);

    // Each group is independent
    assert!(g1_keys.all_ready());
    assert!(g2_keys.all_ready());

    let g1_encrypted = g1_keys.encrypt_for_group(b"group 1 msg").expect("encrypt g1");
    let g2_encrypted = g2_keys.encrypt_for_group(b"group 2 msg").expect("encrypt g2");

    assert!(g1_encrypted.contains_key("bob"));
    assert!(!g1_encrypted.contains_key("charlie"));
    assert!(g2_encrypted.contains_key("charlie"));
    assert!(!g2_encrypted.contains_key("bob"));
}

// ── Encryption state transitions ─────────────────────────────────────

#[test]
fn encryption_state_full_lifecycle() {
    let group_id = GroupId::new(1);
    let mut key_mgr = GroupKeyManager::new(group_id);

    // 1. Add member → Pending
    key_mgr.add_member("bob");
    assert_eq!(
        key_mgr.member_state("bob"),
        Some(GroupEncryptionState::Pending)
    );

    // 2. Begin key exchange → Establishing
    key_mgr.set_establishing("bob");
    assert_eq!(
        key_mgr.member_state("bob"),
        Some(GroupEncryptionState::Establishing)
    );

    // 3. Session established → Ready
    let (session, _) = create_test_session_pair_unique(0x10);
    key_mgr.set_session("bob", session);
    assert_eq!(
        key_mgr.member_state("bob"),
        Some(GroupEncryptionState::Ready)
    );
    assert!(key_mgr.has_session("bob"));

    // 4. Remove member → None
    key_mgr.remove_member("bob");
    assert_eq!(key_mgr.member_state("bob"), None);
}
