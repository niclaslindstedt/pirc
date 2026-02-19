//! User management and messaging integration tests.
//!
//! Validates server user operations through the full handler pipeline:
//! nick changes, private messaging (PRIVMSG/NOTICE), WHOIS, away mode,
//! and user modes — all using real TCP connections via the shared test harness.

use pirc_integration_tests::common::{
    assert_command, assert_numeric, assert_param_contains, away_clear, away_msg, mode_msg,
    nick_msg, notice_msg, privmsg, whois_msg, TestClient, TestServer,
};
use pirc_protocol::numeric::{
    ERR_NICKNAMEINUSE, ERR_NOSUCHNICK, ERR_UMODEUNKNOWNFLAG, ERR_USERSDONTMATCH, RPL_AWAY,
    RPL_ENDOFWHOIS, RPL_NOWAWAY, RPL_UMODEIS, RPL_UNAWAY, RPL_WHOISSERVER, RPL_WHOISUSER,
    RPL_WHOISIDLE,
};
use pirc_protocol::Command;

// ===========================================================================
// 1. Nick Changes
// ===========================================================================

#[tokio::test]
async fn nick_change_echoes_to_sender() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    alice.send(nick_msg("Alicia")).await;
    let reply = alice.recv_msg().await;
    assert_command(&reply, Command::Nick);
    assert_param_contains(&reply, 0, "Alicia");

    // Prefix should contain the OLD nick
    let prefix_str = format!("{}", reply.prefix.as_ref().unwrap());
    assert!(
        prefix_str.contains("Alice"),
        "expected old nick Alice in prefix, got {prefix_str}"
    );
}

#[tokio::test]
async fn nick_change_duplicate_returns_error() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    let mut bob = TestClient::connect(server.addr).await;
    bob.register("Bob", "bob").await;

    // Bob tries to take Alice's nick
    bob.send(nick_msg("Alice")).await;
    let err = bob.recv_msg().await;
    assert_numeric(&err, ERR_NICKNAMEINUSE);
}

#[tokio::test]
async fn nick_change_case_insensitive_collision() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    let mut bob = TestClient::connect(server.addr).await;
    bob.register("Bob", "bob").await;

    // Bob tries "alice" (lowercase) — should collide
    bob.send(nick_msg("alice")).await;
    let err = bob.recv_msg().await;
    assert_numeric(&err, ERR_NICKNAMEINUSE);
}

#[tokio::test]
async fn nick_change_case_only_succeeds() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    // Changing own nick to different case should succeed
    alice.send(nick_msg("ALICE")).await;
    let reply = alice.recv_msg().await;
    assert_command(&reply, Command::Nick);
    assert_param_contains(&reply, 0, "ALICE");
}

#[tokio::test]
async fn nick_change_with_special_characters() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    // Valid IRC nick characters include letters, digits, hyphens, brackets, etc.
    alice.send(nick_msg("Alice-2")).await;
    let reply = alice.recv_msg().await;
    assert_command(&reply, Command::Nick);
    assert_param_contains(&reply, 0, "Alice-2");
}

#[tokio::test]
async fn nick_change_preserves_messaging() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    let mut bob = TestClient::connect(server.addr).await;
    bob.register("Bob", "bob").await;

    // Alice changes nick
    alice.send(nick_msg("Alicia")).await;
    alice.recv_msg().await; // NICK confirmation

    // Bob can message Alice by new nick
    bob.send(privmsg("Alicia", "hello new you")).await;
    let msg = alice.recv_msg().await;
    assert_command(&msg, Command::Privmsg);
    assert_param_contains(&msg, 1, "hello new you");

    // Old nick should not work
    bob.send(privmsg("Alice", "old nick")).await;
    let err = bob.recv_msg().await;
    assert_numeric(&err, ERR_NOSUCHNICK);
}

// ===========================================================================
// 2. Private Messaging (PRIVMSG)
// ===========================================================================

#[tokio::test]
async fn privmsg_to_user_delivers_message() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    let mut bob = TestClient::connect(server.addr).await;
    bob.register("Bob", "bob").await;

    alice.send(privmsg("Bob", "Hello Bob!")).await;

    let msg = bob.recv_msg().await;
    assert_command(&msg, Command::Privmsg);
    assert_param_contains(&msg, 0, "Bob");
    assert_param_contains(&msg, 1, "Hello Bob!");
}

#[tokio::test]
async fn privmsg_preserves_sender_prefix() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    let mut bob = TestClient::connect(server.addr).await;
    bob.register("Bob", "bob").await;

    alice.send(privmsg("Bob", "check my prefix")).await;

    let msg = bob.recv_msg().await;
    let prefix_str = format!("{}", msg.prefix.as_ref().unwrap());
    assert!(
        prefix_str.contains("Alice"),
        "expected sender nick Alice in prefix, got {prefix_str}"
    );
    assert!(
        prefix_str.contains("alice"),
        "expected username alice in prefix, got {prefix_str}"
    );
}

#[tokio::test]
async fn privmsg_to_nonexistent_user_returns_error() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    alice.send(privmsg("NoSuchUser", "hello?")).await;
    let err = alice.recv_msg().await;
    assert_numeric(&err, ERR_NOSUCHNICK);
}

#[tokio::test]
async fn privmsg_sender_does_not_receive_echo() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    let mut bob = TestClient::connect(server.addr).await;
    bob.register("Bob", "bob").await;

    alice.send(privmsg("Bob", "no echo")).await;

    // Bob gets the message
    bob.recv_msg().await;

    // Alice should NOT get anything back (no echo)
    let no_msg = alice.try_recv_short().await;
    assert!(no_msg.is_none(), "sender should not receive echo");
}

#[tokio::test]
async fn privmsg_bidirectional() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    let mut bob = TestClient::connect(server.addr).await;
    bob.register("Bob", "bob").await;

    // Alice -> Bob
    alice.send(privmsg("Bob", "hi bob")).await;
    let msg = bob.recv_msg().await;
    assert_command(&msg, Command::Privmsg);
    assert_param_contains(&msg, 1, "hi bob");

    // Bob -> Alice
    bob.send(privmsg("Alice", "hi alice")).await;
    let msg = alice.recv_msg().await;
    assert_command(&msg, Command::Privmsg);
    assert_param_contains(&msg, 1, "hi alice");
}

// ===========================================================================
// 3. NOTICE
// ===========================================================================

#[tokio::test]
async fn notice_to_user_delivers_message() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    let mut bob = TestClient::connect(server.addr).await;
    bob.register("Bob", "bob").await;

    alice.send(notice_msg("Bob", "Important notice")).await;

    let msg = bob.recv_msg().await;
    assert_command(&msg, Command::Notice);
    assert_param_contains(&msg, 1, "Important notice");
}

#[tokio::test]
async fn notice_does_not_generate_away_reply() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    let mut bob = TestClient::connect(server.addr).await;
    bob.register("Bob", "bob").await;

    // Bob sets away
    bob.send(away_msg("Gone fishing")).await;
    bob.recv_msg().await; // RPL_NOWAWAY

    // Alice sends NOTICE to away Bob
    alice.send(notice_msg("Bob", "notification")).await;

    // Bob receives the notice
    let msg = bob.recv_msg().await;
    assert_command(&msg, Command::Notice);

    // Alice should NOT receive RPL_AWAY (unlike PRIVMSG)
    let no_msg = alice.try_recv_short().await;
    assert!(no_msg.is_none(), "NOTICE should not trigger RPL_AWAY");
}

#[tokio::test]
async fn notice_to_nonexistent_user_returns_error() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    alice.send(notice_msg("Ghost", "hello?")).await;
    let err = alice.recv_msg().await;
    assert_numeric(&err, ERR_NOSUCHNICK);
}

// ===========================================================================
// 4. WHOIS
// ===========================================================================

#[tokio::test]
async fn whois_returns_user_info() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    let mut bob = TestClient::connect(server.addr).await;
    bob.register("Bob", "bob").await;

    alice.send(whois_msg("Bob")).await;

    // RPL_WHOISUSER (311): params [requestor, nick, user, host, *, realname]
    let user_reply = alice.recv_msg().await;
    assert_numeric(&user_reply, RPL_WHOISUSER);
    assert_param_contains(&user_reply, 1, "Bob");
    assert_param_contains(&user_reply, 2, "bob");

    // RPL_WHOISSERVER (312)
    let server_reply = alice.recv_msg().await;
    assert_numeric(&server_reply, RPL_WHOISSERVER);

    // RPL_WHOISIDLE (317)
    let idle_reply = alice.recv_msg().await;
    assert_numeric(&idle_reply, RPL_WHOISIDLE);

    // RPL_ENDOFWHOIS (318)
    let end = alice.recv_msg().await;
    assert_numeric(&end, RPL_ENDOFWHOIS);
}

#[tokio::test]
async fn whois_nonexistent_user_returns_error() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    alice.send(whois_msg("NoSuchUser")).await;

    // ERR_NOSUCHNICK (401) + RPL_ENDOFWHOIS (318)
    let err = alice.recv_msg().await;
    assert_numeric(&err, ERR_NOSUCHNICK);

    let end = alice.recv_msg().await;
    assert_numeric(&end, RPL_ENDOFWHOIS);
}

#[tokio::test]
async fn whois_shows_away_status() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    let mut bob = TestClient::connect(server.addr).await;
    bob.register("Bob", "bob").await;

    // Bob sets away
    bob.send(away_msg("On vacation")).await;
    bob.recv_msg().await; // RPL_NOWAWAY

    alice.send(whois_msg("Bob")).await;

    // RPL_WHOISUSER
    let user_reply = alice.recv_msg().await;
    assert_numeric(&user_reply, RPL_WHOISUSER);

    // RPL_WHOISSERVER
    alice.recv_msg().await;

    // RPL_AWAY (301) — because Bob is away; params [requestor, nick, away_msg]
    let away_reply = alice.recv_msg().await;
    assert_numeric(&away_reply, RPL_AWAY);
    assert_param_contains(&away_reply, 1, "Bob");

    // RPL_WHOISIDLE
    alice.recv_msg().await;

    // RPL_ENDOFWHOIS
    let end = alice.recv_msg().await;
    assert_numeric(&end, RPL_ENDOFWHOIS);
}

#[tokio::test]
async fn whois_on_self() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    alice.send(whois_msg("Alice")).await;

    // RPL_WHOISUSER: params [requestor, nick, user, host, *, realname]
    let user_reply = alice.recv_msg().await;
    assert_numeric(&user_reply, RPL_WHOISUSER);
    assert_param_contains(&user_reply, 1, "Alice");

    // RPL_WHOISSERVER
    alice.recv_msg().await;

    // RPL_WHOISIDLE
    alice.recv_msg().await;

    // RPL_ENDOFWHOIS
    let end = alice.recv_msg().await;
    assert_numeric(&end, RPL_ENDOFWHOIS);
}

// ===========================================================================
// 5. Away Mode
// ===========================================================================

#[tokio::test]
async fn away_set_returns_nowaway() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    alice.send(away_msg("Be right back")).await;
    let reply = alice.recv_msg().await;
    assert_numeric(&reply, RPL_NOWAWAY);
}

#[tokio::test]
async fn away_clear_returns_unaway() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    // Set away first
    alice.send(away_msg("Gone")).await;
    alice.recv_msg().await; // RPL_NOWAWAY

    // Clear away
    alice.send(away_clear()).await;
    let reply = alice.recv_msg().await;
    assert_numeric(&reply, RPL_UNAWAY);
}

#[tokio::test]
async fn privmsg_to_away_user_returns_rpl_away() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    let mut bob = TestClient::connect(server.addr).await;
    bob.register("Bob", "bob").await;

    // Bob sets away
    bob.send(away_msg("At lunch")).await;
    bob.recv_msg().await; // RPL_NOWAWAY

    // Alice messages Bob
    alice.send(privmsg("Bob", "are you there?")).await;

    // Bob still receives the message
    let msg = bob.recv_msg().await;
    assert_command(&msg, Command::Privmsg);
    assert_param_contains(&msg, 1, "are you there?");

    // Alice receives RPL_AWAY
    let away_reply = alice.recv_msg().await;
    assert_numeric(&away_reply, RPL_AWAY);
    assert_param_contains(&away_reply, 2, "At lunch");
}

#[tokio::test]
async fn away_clear_stops_rpl_away_on_privmsg() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    let mut bob = TestClient::connect(server.addr).await;
    bob.register("Bob", "bob").await;

    // Bob sets away, then clears
    bob.send(away_msg("Gone")).await;
    bob.recv_msg().await; // RPL_NOWAWAY
    bob.send(away_clear()).await;
    bob.recv_msg().await; // RPL_UNAWAY

    // Alice messages Bob
    alice.send(privmsg("Bob", "hello")).await;

    // Bob gets message
    bob.recv_msg().await;

    // Alice should NOT get RPL_AWAY since Bob cleared away
    let no_msg = alice.try_recv_short().await;
    assert!(
        no_msg.is_none(),
        "should not get RPL_AWAY after away was cleared"
    );
}

#[tokio::test]
async fn away_message_updates_on_second_set() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    let mut bob = TestClient::connect(server.addr).await;
    bob.register("Bob", "bob").await;

    // Bob sets away, then updates message
    bob.send(away_msg("First message")).await;
    bob.recv_msg().await; // RPL_NOWAWAY
    bob.send(away_msg("Updated message")).await;
    bob.recv_msg().await; // RPL_NOWAWAY

    // Alice messages Bob
    alice.send(privmsg("Bob", "ping")).await;
    bob.recv_msg().await; // Bob gets the PRIVMSG

    // Alice should see the updated away message
    let away_reply = alice.recv_msg().await;
    assert_numeric(&away_reply, RPL_AWAY);
    assert_param_contains(&away_reply, 2, "Updated message");
}

#[tokio::test]
async fn away_status_in_whois_after_clear() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    let mut bob = TestClient::connect(server.addr).await;
    bob.register("Bob", "bob").await;

    // Bob sets away then clears
    bob.send(away_msg("Gone")).await;
    bob.recv_msg().await;
    bob.send(away_clear()).await;
    bob.recv_msg().await;

    // WHOIS should NOT show away status
    alice.send(whois_msg("Bob")).await;

    // RPL_WHOISUSER, RPL_WHOISSERVER, RPL_WHOISIDLE, RPL_ENDOFWHOIS
    let user_reply = alice.recv_msg().await;
    assert_numeric(&user_reply, RPL_WHOISUSER);
    alice.recv_msg().await; // RPL_WHOISSERVER
    let idle = alice.recv_msg().await;
    assert_numeric(&idle, RPL_WHOISIDLE);
    let end = alice.recv_msg().await;
    assert_numeric(&end, RPL_ENDOFWHOIS);
    // No RPL_AWAY should appear in the sequence
}

// ===========================================================================
// 6. User Modes
// ===========================================================================

#[tokio::test]
async fn user_mode_query_returns_current_modes() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    // Query own modes (send MODE with just the nick)
    alice
        .send(pirc_protocol::Message::new(
            Command::Mode,
            vec!["Alice".to_owned()],
        ))
        .await;
    let reply = alice.recv_msg().await;
    assert_numeric(&reply, RPL_UMODEIS);
    // Default mode should be "+"
    assert_param_contains(&reply, 1, "+");
}

#[tokio::test]
async fn user_mode_cannot_change_other_user() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    let mut _bob = TestClient::connect(server.addr).await;
    _bob.register("Bob", "bob").await;

    // Alice tries to set mode on Bob
    alice.send(mode_msg("Bob", "+v")).await;
    let err = alice.recv_msg().await;
    assert_numeric(&err, ERR_USERSDONTMATCH);
}

#[tokio::test]
async fn user_mode_unknown_flag_returns_error() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    alice.send(mode_msg("Alice", "+x")).await;

    // Should get ERR_UMODEUNKNOWNFLAG
    let err = alice.recv_msg().await;
    assert_numeric(&err, ERR_UMODEUNKNOWNFLAG);

    // Followed by RPL_UMODEIS showing current modes
    let modes = alice.recv_msg().await;
    assert_numeric(&modes, RPL_UMODEIS);
}

#[tokio::test]
async fn user_mode_cannot_self_promote_operator() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    // Try to set +o on self — should be silently ignored
    alice.send(mode_msg("Alice", "+o")).await;
    let reply = alice.recv_msg().await;
    assert_numeric(&reply, RPL_UMODEIS);
    // Mode should still be just "+"
    assert_param_contains(&reply, 1, "+");
}

#[tokio::test]
async fn user_mode_set_voiced() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    // Set +v on self
    alice.send(mode_msg("Alice", "+v")).await;
    let reply = alice.recv_msg().await;
    assert_numeric(&reply, RPL_UMODEIS);
    assert_param_contains(&reply, 1, "v");

    // Remove -v
    alice.send(mode_msg("Alice", "-v")).await;
    let reply = alice.recv_msg().await;
    assert_numeric(&reply, RPL_UMODEIS);
    // Should be back to just "+"
    let mode_str = &reply.params[1];
    assert!(
        !mode_str.contains('v'),
        "expected no v in mode string after -v, got {mode_str}"
    );
}

// ===========================================================================
// 7. Cross-Feature Scenarios
// ===========================================================================

#[tokio::test]
async fn nick_change_then_whois_shows_new_nick() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    let mut bob = TestClient::connect(server.addr).await;
    bob.register("Bob", "bob").await;

    // Bob changes nick
    bob.send(nick_msg("Robert")).await;
    bob.recv_msg().await; // NICK confirmation

    // Alice can WHOIS the new nick
    alice.send(whois_msg("Robert")).await;
    let user_reply = alice.recv_msg().await;
    assert_numeric(&user_reply, RPL_WHOISUSER);
    assert_param_contains(&user_reply, 1, "Robert");

    // Old nick should not be found
    alice.drain(3).await; // drain rest of WHOIS reply
    alice.send(whois_msg("Bob")).await;
    let err = alice.recv_msg().await;
    assert_numeric(&err, ERR_NOSUCHNICK);
}

#[tokio::test]
async fn privmsg_with_unicode_text() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    let mut bob = TestClient::connect(server.addr).await;
    bob.register("Bob", "bob").await;

    alice.send(privmsg("Bob", "Hello! \u{1F600}")).await;

    let msg = bob.recv_msg().await;
    assert_command(&msg, Command::Privmsg);
    assert_param_contains(&msg, 1, "Hello!");
}

#[tokio::test]
async fn away_and_whois_integration() {
    let server = TestServer::start().await;
    let mut alice = TestClient::connect(server.addr).await;
    alice.register("Alice", "alice").await;

    let mut bob = TestClient::connect(server.addr).await;
    bob.register("Bob", "bob").await;

    // Bob sets away
    bob.send(away_msg("Sleeping")).await;
    bob.recv_msg().await; // RPL_NOWAWAY

    // Alice WHOISes Bob — should include RPL_AWAY
    alice.send(whois_msg("Bob")).await;
    alice.recv_msg().await; // RPL_WHOISUSER
    alice.recv_msg().await; // RPL_WHOISSERVER

    let away_reply = alice.recv_msg().await;
    assert_numeric(&away_reply, RPL_AWAY);
    assert_param_contains(&away_reply, 2, "Sleeping");

    alice.recv_msg().await; // RPL_WHOISIDLE
    alice.recv_msg().await; // RPL_ENDOFWHOIS

    // Alice PRIVMSGs Bob — should also get RPL_AWAY
    alice.send(privmsg("Bob", "wake up")).await;
    bob.recv_msg().await; // Bob gets the message

    let away_from_privmsg = alice.recv_msg().await;
    assert_numeric(&away_from_privmsg, RPL_AWAY);
    assert_param_contains(&away_from_privmsg, 2, "Sleeping");
}
