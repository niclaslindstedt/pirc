use std::io::{self, Write};
use std::net::ToSocketAddrs;
use std::time::Duration;

use pirc_common::config::keys_dir;
use pirc_network::connection::AsyncTransport;
use pirc_network::{Connection, Connector, ShutdownController, ShutdownSignal};
use pirc_plugin::ffi::PluginEventType;
use pirc_plugin::manager::PluginManager;
use pirc_protocol::{Command, Message};
use tokio::sync::mpsc;
use tokio::time::Instant;
use tracing::{debug, info, warn};

use crate::encryption::EncryptionManager;
use crate::group_chat::GroupChatManager;
use crate::p2p::P2pManager;

use crate::client_command::ClientCommand;
use crate::config::ClientConfig;
use crate::connection_state::{ConnectionManager, ConnectionState};
use crate::message_handler::{self, HandlerAction};
use crate::registration::{RegistrationEvent, RegistrationState};
use crate::tui::buffer::Buffer;
use crate::tui::buffer_manager::BufferId;
use crate::tui::input::KeyEvent;
use crate::tui::message_buffer::{BufferLine, LineType};
use crate::tui::renderer::Renderer;
use crate::tui::view_coordinator::{ViewAction, ViewCoordinator};
use crate::tui::{terminal_size, InputReader, RawModeGuard, SignalHandler};

/// Registration timeout — if no RPL_WELCOME is received within this duration,
/// the connection is dropped.
const REGISTRATION_TIMEOUT: Duration = Duration::from_secs(30);

/// How often to check if we need to send a keepalive PING (seconds).
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(30);

/// If no message is received from the server within this duration, send a PING.
const KEEPALIVE_IDLE_THRESHOLD: Duration = Duration::from_secs(60);

/// If no PONG is received within this duration after sending a PING,
/// consider the connection dead.
const PONG_TIMEOUT: Duration = Duration::from_secs(120);

/// Maximum delay between reconnection attempts.
const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(60);

/// Multiplicative factor for exponential backoff.
const BACKOFF_FACTOR: f64 = 2.0;

/// Central coordinator for the pirc client.
///
/// Owns the TUI state (view coordinator + renderer), the network connection
/// manager, and the shutdown controller. The [`App::run`] method drives the
/// main `tokio::select!` event loop.
pub struct App {
    config: ClientConfig,
    connection_mgr: ConnectionManager,
    view: ViewCoordinator,
    connection: Option<Connection>,
    registration: Option<RegistrationState>,
    registration_deadline: Option<Instant>,
    shutdown_controller: ShutdownController,
    _shutdown_signal: ShutdownSignal,
    /// When we last received any message from the server.
    last_message_received: Option<Instant>,
    /// When we sent a client keepalive PING (for lag measurement).
    ping_sent_at: Option<Instant>,
    /// Measured server lag in milliseconds.
    lag_ms: Option<u32>,
    /// When to attempt the next reconnection (if reconnecting).
    reconnect_at: Option<Instant>,
    /// Current reconnect attempt number (1-indexed).
    reconnect_attempt: u32,
    /// Channels to rejoin after a successful reconnect.
    channels_to_rejoin: Vec<String>,
    /// E2E encryption manager for private messages.
    encryption: EncryptionManager,
    /// P2P connection manager for direct peer connections.
    p2p: P2pManager,
    /// Encrypted group chat manager.
    group_chat: GroupChatManager,
    /// Plugin manager for native plugins.
    plugin_manager: PluginManager,
    /// Timestamp when the process started (for startup timing metrics).
    startup_start: std::time::Instant,
}

impl App {
    /// Create a new `App` from the given configuration.
    pub fn new(config: ClientConfig) -> Self {
        let startup_start = std::time::Instant::now();
        let phase_start = std::time::Instant::now();

        let (width, height) = terminal_size().map_or((80, 24), |s| (s.cols, s.rows));

        let scrollback = config.ui.scrollback_lines as usize;
        let mut view = ViewCoordinator::new(width, height, scrollback);

        let connection_mgr = ConnectionManager::new(&config);
        view.set_nick(connection_mgr.nick().to_string());

        let (shutdown_controller, shutdown_signal) = ShutdownSignal::new();

        let tui_elapsed = phase_start.elapsed();
        debug!(elapsed_us = tui_elapsed.as_micros(), "startup: TUI state initialized");

        // Load or create encrypted identity keys from disk.
        // Uses a machine-specific passphrase derived from the nick and hostname.
        let crypto_start = std::time::Instant::now();
        let encryption = match keys_dir() {
            Some(dir) => {
                let passphrase = derive_machine_passphrase(connection_mgr.nick());
                EncryptionManager::load_or_create(&dir, &passphrase)
            }
            None => {
                warn!("Could not determine keys directory; using ephemeral keys");
                EncryptionManager::new()
            }
        };
        let crypto_elapsed = crypto_start.elapsed();
        debug!(elapsed_us = crypto_elapsed.as_micros(), "startup: encryption keys loaded");

        let p2p = P2pManager::new(&config.p2p);

        let total_new = phase_start.elapsed();
        debug!(elapsed_us = total_new.as_micros(), "startup: App::new() complete");

        Self {
            config,
            connection_mgr,
            view,
            connection: None,
            registration: None,
            registration_deadline: None,
            shutdown_controller,
            _shutdown_signal: shutdown_signal,
            last_message_received: None,
            ping_sent_at: None,
            lag_ms: None,
            reconnect_at: None,
            reconnect_attempt: 0,
            channels_to_rejoin: Vec::new(),
            encryption,
            p2p,
            group_chat: GroupChatManager::new(),
            plugin_manager: PluginManager::new(),
            startup_start,
        }
    }

    /// Run the main event loop.
    ///
    /// This enables raw terminal mode, spawns the stdin reader task, initiates
    /// the server connection, and enters the `tokio::select!` loop that handles
    /// stdin input, network messages, and shutdown concurrently.
    pub async fn run(mut self) -> io::Result<()> {
        let run_start = std::time::Instant::now();

        // Enter raw mode (alternate screen, hide cursor)
        let _guard = RawModeGuard::enable()?;

        // Install a panic hook that restores the terminal before printing
        // the panic message. Without this, a panic leaves the terminal in
        // raw mode with the alternate screen buffer active.
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // Best-effort terminal restoration
            let mut stdout = io::stdout().lock();
            let _ = stdout.write_all(b"\x1b[?25h"); // show cursor
            let _ = stdout.write_all(b"\x1b[?1049l"); // leave alternate screen
            let _ = stdout.flush();

            // Restore cooked mode — get the fd and reset termios.
            // We can't easily access the saved termios here, but leaving
            // the alternate screen is the most important part. The
            // RawModeGuard's Drop will also run after the panic unwind.
            default_hook(info);
        }));

        // Create renderer
        let (width, height) = terminal_size().map_or((80, 24), |s| (s.cols, s.rows));
        let stdout = io::stdout();
        let mut renderer = Renderer::new(width, height, stdout.lock());

        // Push initial status message
        self.view.push_status_message(BufferLine {
            timestamp: current_timestamp(&self.config.ui.timestamp_format),
            sender: None,
            content: format!(
                "pirc — connecting to {}...",
                self.connection_mgr.server_addr()
            ),
            line_type: LineType::System,
        });

        // Render TUI immediately so the user sees the interface as fast as
        // possible. Plugin loading and network connection happen after.
        self.render(&mut renderer)?;

        let tui_ready = self.startup_start.elapsed();
        debug!(elapsed_us = tui_ready.as_micros(), "startup: TUI visible");

        // Spawn stdin reader task (non-blocking, starts accepting input immediately)
        let (stdin_tx, mut stdin_rx) = mpsc::unbounded_channel::<KeyEvent>();
        std::thread::spawn(move || {
            let mut reader = InputReader::from_stdin();
            if let Ok(handler) = SignalHandler::new() {
                reader.set_signal_handler(handler);
            }
            loop {
                if let Some(event) = reader.poll_event(50) {
                    if stdin_tx.send(event).is_err() {
                        break;
                    }
                }
            }
        });

        // Initialize plugins (after initial render so TUI is already visible)
        let plugin_start = std::time::Instant::now();
        self.init_plugins();
        let plugin_elapsed = plugin_start.elapsed();
        debug!(elapsed_us = plugin_elapsed.as_micros(), "startup: plugins initialized");

        // Initiate connection (DNS resolution + TCP connect)
        let connect_start = std::time::Instant::now();
        self.initiate_connection().await;
        let connect_elapsed = connect_start.elapsed();
        debug!(elapsed_us = connect_elapsed.as_micros(), "startup: connection initiated");

        self.render(&mut renderer)?;

        // Log total startup time (process start to event loop ready)
        let total_startup = self.startup_start.elapsed();
        let run_elapsed = run_start.elapsed();
        info!(
            total_ms = total_startup.as_millis(),
            run_ms = run_elapsed.as_millis(),
            tui_ready_ms = tui_ready.as_millis(),
            "startup complete"
        );

        // Keepalive timer
        let mut keepalive_interval = tokio::time::interval(KEEPALIVE_INTERVAL);
        keepalive_interval.tick().await; // consume the immediate first tick

        // Main event loop
        loop {
            tokio::select! {
                // Stdin input
                Some(key_event) = stdin_rx.recv() => {
                    let action = self.view.handle_key(key_event);
                    let should_quit = self.dispatch_view_action(action).await;
                    self.render(&mut renderer)?;
                    if should_quit {
                        break;
                    }
                }
                // External SIGINT (e.g. `kill -INT <pid>`) — clean quit
                _ = tokio::signal::ctrl_c() => {
                    self.handle_quit(None).await;
                    break;
                }
                // Network recv
                msg = async {
                    if let Some(ref mut conn) = self.connection {
                        conn.recv().await
                    } else {
                        // No connection — park this branch forever
                        std::future::pending::<Result<Option<Message>, pirc_network::NetworkError>>().await
                    }
                } => {
                    match msg {
                        Ok(Some(msg)) => {
                            self.last_message_received = Some(Instant::now());
                            self.handle_server_message(&msg).await;
                            self.render(&mut renderer)?;
                        }
                        Ok(None) => {
                            // Connection closed (EOF)
                            self.handle_disconnect("Connection closed by server");
                            self.render(&mut renderer)?;
                        }
                        Err(e) => {
                            self.handle_disconnect(&format!("Network error: {e}"));
                            self.render(&mut renderer)?;
                        }
                    }
                }
                // Registration timeout
                _ = async {
                    match self.registration_deadline {
                        Some(deadline) => tokio::time::sleep_until(deadline).await,
                        None => std::future::pending::<()>().await,
                    }
                } => {
                    self.handle_disconnect("Registration timed out (no RPL_WELCOME within 30s)");
                    self.render(&mut renderer)?;
                }
                // Keepalive timer
                _ = keepalive_interval.tick() => {
                    self.handle_keepalive_tick().await;
                    self.render(&mut renderer)?;
                }
                // Reconnect timer
                _ = async {
                    match self.reconnect_at {
                        Some(deadline) => tokio::time::sleep_until(deadline).await,
                        None => std::future::pending::<()>().await,
                    }
                } => {
                    self.attempt_reconnect().await;
                    self.render(&mut renderer)?;
                }
            }
        }

        // Shutdown: close connection gracefully
        if let Some(ref mut conn) = self.connection {
            let _ = conn.shutdown().await;
        }
        self.shutdown_controller.shutdown();

        Ok(())
    }

    /// Attempt to connect to the configured server.
    async fn initiate_connection(&mut self) {
        let addr_str = self.connection_mgr.server_addr().to_string();

        // Resolve the address
        let addr = match addr_str.to_socket_addrs() {
            Ok(mut addrs) => {
                if let Some(a) = addrs.next() {
                    a
                } else {
                    self.push_status(&format!("Could not resolve {addr_str}"));
                    return;
                }
            }
            Err(e) => {
                self.push_status(&format!("Could not resolve {addr_str}: {e}"));
                return;
            }
        };

        // Transition to Connecting
        if self
            .connection_mgr
            .transition(ConnectionState::Connecting)
            .is_err()
        {
            return;
        }

        let connector = Connector::new();
        match connector.connect(addr).await {
            Ok(conn) => {
                self.connection = Some(conn);

                // Transition to Registering
                if self
                    .connection_mgr
                    .transition(ConnectionState::Registering)
                    .is_err()
                {
                    return;
                }

                // Build registration state from config
                let nick = self.connection_mgr.nick().to_string();
                let username = nick.clone();
                let realname = self
                    .config
                    .identity
                    .realname
                    .clone()
                    .unwrap_or_else(|| nick.clone());
                let alt_nicks = self.config.identity.alt_nicks.clone();

                let reg = RegistrationState::new(nick, alt_nicks, username, realname);

                // Send NICK and USER via the registration state
                self.send_registration_messages(&reg).await;

                self.registration = Some(reg);
                self.registration_deadline = Some(Instant::now() + REGISTRATION_TIMEOUT);

                self.push_status(&format!("Connected to {addr_str}, registering..."));
            }
            Err(e) => {
                let _ = self
                    .connection_mgr
                    .transition(ConnectionState::Disconnected);
                self.push_status(&format!("Connection failed: {e}"));
            }
        }
    }

    /// Send NICK and USER registration messages using the registration state.
    async fn send_registration_messages(&mut self, reg: &RegistrationState) {
        if let Some(ref mut conn) = self.connection {
            if let Err(e) = conn.send(reg.nick_message()).await {
                warn!("Failed to send NICK: {e}");
            }
            if let Err(e) = conn.send(reg.user_message()).await {
                warn!("Failed to send USER: {e}");
            }
        }
    }

    /// Handle a `ViewAction` from the view coordinator.
    ///
    /// Returns `true` if the app should quit.
    async fn dispatch_view_action(&mut self, action: ViewAction) -> bool {
        match action {
            ViewAction::None | ViewAction::Redraw => false,
            ViewAction::Quit(reason) => {
                self.handle_quit(reason).await;
                true
            }
            ViewAction::Command(cmd) => {
                self.handle_command(cmd).await;
                false
            }
            ViewAction::ChatMessage(text, target) => {
                self.handle_chat_message(&text, &target).await;
                false
            }
            ViewAction::CommandError(err) => {
                self.push_status(&format!("Error: {err}"));
                false
            }
        }
    }

    /// Handle the quit sequence: send QUIT to the server and disable auto-reconnect.
    async fn handle_quit(&mut self, reason: Option<String>) {
        // Disable auto-reconnect — this is an intentional quit.
        self.connection_mgr.set_auto_reconnect(false);
        self.reconnect_at = None;
        self.reconnect_attempt = 0;

        // Send QUIT message to the server if connected.
        if let Some(ref mut conn) = self.connection {
            let params = match reason {
                Some(ref r) => vec![r.clone()],
                None => vec!["pirc".to_string()],
            };
            let quit_msg = Message::new(Command::Quit, params);
            let _ = conn.send(quit_msg).await;
        }
    }

    /// Handle a client command (e.g. /join, /nick).
    ///
    /// Note: `/quit` is handled via `ViewAction::Quit` → `handle_quit()`,
    /// not through this method.
    async fn handle_command(&mut self, cmd: ClientCommand) {
        // Handle /reconnect
        if matches!(cmd, ClientCommand::Reconnect) {
            self.handle_reconnect_command().await;
            return;
        }

        // Handle /disconnect
        if matches!(cmd, ClientCommand::Disconnect) {
            self.handle_disconnect_command().await;
            return;
        }

        // Handle /encryption subcommands
        if let ClientCommand::Encryption(ref sub) = cmd {
            self.handle_encryption_command(sub);
            return;
        }

        // Handle /fingerprint
        if let ClientCommand::Fingerprint(ref nick) = cmd {
            self.handle_fingerprint_command(nick.as_deref());
            return;
        }

        // Handle /plugin subcommands
        if let ClientCommand::Plugin(ref sub) = cmd {
            let sub = sub.clone();
            self.handle_plugin_command(&sub);
            return;
        }

        // Handle /group subcommands
        if let ClientCommand::Group(ref sub) = cmd {
            let sub = sub.clone();
            self.handle_group_command(&sub).await;
            return;
        }

        // Handle /msg with E2E encryption
        if let ClientCommand::Msg(ref target, ref message) = cmd {
            if !target.starts_with('#') && !target.starts_with('&') {
                self.handle_private_msg_command(target, message).await;
                return;
            }
        }

        // Handle /query with message — route through encryption
        if let ClientCommand::Query(ref nick, Some(ref message)) = cmd {
            self.handle_private_msg_command(nick, message).await;
            return;
        }

        // Try plugin commands for unknown commands before sending to server.
        if let ClientCommand::Unknown(ref name, ref args) = cmd {
            let args_str = args.join(" ");
            if self.dispatch_plugin_command(name, &args_str) {
                return;
            }
        }

        // Determine context (current channel name) for to_message
        let context = self.current_channel_context();
        if let Some(msg) = cmd.to_message(context.as_deref()) {
            if let Some(ref mut conn) = self.connection {
                if let Err(e) = conn.send(msg).await {
                    self.push_status(&format!("Send error: {e}"));
                }
            } else {
                self.push_status("Not connected");
            }
        }
    }

    /// Handle the `/reconnect` command — manually trigger a reconnection.
    async fn handle_reconnect_command(&mut self) {
        if self.connection_mgr.is_connected() {
            self.push_status("Already connected");
            return;
        }

        // Cancel any pending reconnect timer
        self.reconnect_at = None;
        self.reconnect_attempt = 0;

        // If we're in a Reconnecting state, transition to Disconnected first
        if matches!(
            self.connection_mgr.state(),
            ConnectionState::Reconnecting { .. }
        ) {
            let _ = self
                .connection_mgr
                .transition(ConnectionState::Disconnected);
        }

        // Re-enable auto-reconnect and start fresh
        self.connection_mgr.set_auto_reconnect(true);
        self.push_status("Manual reconnect requested...");
        self.schedule_reconnect(1);
    }

    /// Handle the `/disconnect` command — disconnect and disable auto-reconnect.
    async fn handle_disconnect_command(&mut self) {
        // Disable auto-reconnect
        self.connection_mgr.set_auto_reconnect(false);

        // Cancel any pending reconnect
        self.reconnect_at = None;
        self.reconnect_attempt = 0;

        if self.connection.is_some() || self.connection_mgr.is_connected() {
            // Send QUIT if connected
            if let Some(ref mut conn) = self.connection {
                let quit_msg = Message::new(Command::Quit, vec!["Disconnected".to_string()]);
                let _ = conn.send(quit_msg).await;
            }
            self.connection = None;
            self.registration = None;
            self.registration_deadline = None;
            self.last_message_received = None;
            self.ping_sent_at = None;
            self.lag_ms = None;
            self.view.set_lag(None);
            let _ = self
                .connection_mgr
                .transition(ConnectionState::Disconnected);
            self.push_status("Disconnected (auto-reconnect disabled)");
        } else if matches!(
            self.connection_mgr.state(),
            ConnectionState::Reconnecting { .. }
        ) {
            let _ = self
                .connection_mgr
                .transition(ConnectionState::Disconnected);
            self.push_status("Reconnect cancelled (auto-reconnect disabled)");
        } else {
            self.push_status("Not connected (auto-reconnect disabled)");
        }
    }

    /// Handle a chat message (plain text sent to the active buffer's target).
    async fn handle_chat_message(
        &mut self,
        text: &str,
        target: &crate::tui::buffer_manager::BufferId,
    ) {
        use crate::tui::buffer_manager::BufferId;

        match target {
            BufferId::Status => {
                self.push_status("Cannot send messages to the status buffer");
                return;
            }
            BufferId::Query(name) => {
                // Query messages go through encryption
                let name = name.clone();
                self.handle_private_msg_command(&name, text).await;
                return;
            }
            BufferId::Channel(_) => {}
        }

        let channel = match target {
            BufferId::Channel(name) => name.clone(),
            _ => return,
        };

        // Group buffers route through encrypted fan-out, not raw PRIVMSG.
        if let Some(group_id) = group::group_id_from_buffer(target) {
            self.handle_group_chat_message(group_id, text, target).await;
            return;
        }

        let msg = Message::new(
            pirc_protocol::Command::Privmsg,
            vec![channel.clone(), text.to_string()],
        );

        if let Some(ref mut conn) = self.connection {
            if let Err(e) = conn.send(msg).await {
                self.push_status(&format!("Send error: {e}"));
                return;
            }

            // Echo the message locally
            let nick = self.connection_mgr.nick().to_string();
            self.view.push_message(
                target,
                BufferLine {
                    timestamp: current_timestamp(&self.config.ui.timestamp_format),
                    sender: Some(nick),
                    content: text.to_string(),
                    line_type: LineType::Message,
                },
            );
        } else {
            self.push_status("Not connected");
        }
    }

    /// Handle an inbound server message.
    ///
    /// During registration (Registering state), messages are first routed
    /// through the [`RegistrationState`] handler. Once registered, or for
    /// unhandled messages, the message router dispatches to the correct buffer.
    async fn handle_server_message(&mut self, msg: &Message) {
        // Handle PING at the transport level — respond with PONG immediately.
        if msg.command == Command::Ping {
            let token = msg.params.first().cloned().unwrap_or_default();
            let pong = Message::new(Command::Pong, vec![token]);
            if let Some(ref mut conn) = self.connection {
                if let Err(e) = conn.send(pong).await {
                    warn!("Failed to send PONG: {e}");
                }
            }
            return;
        }

        // Handle PONG — measure lag from our keepalive PINGs.
        if msg.command == Command::Pong {
            if let Some(sent_at) = self.ping_sent_at.take() {
                let lag = sent_at.elapsed();
                #[allow(clippy::cast_possible_truncation)]
                let lag_ms = lag.as_millis().min(u32::MAX as u128) as u32;
                self.lag_ms = Some(lag_ms);
                self.view.set_lag(Some(lag_ms));
            }
            return;
        }

        // During registration, let the registration handler process the message first.
        if self.registration.is_some() {
            let event = self.registration.as_mut().unwrap().handle_message(msg);
            match event {
                RegistrationEvent::Welcome {
                    server_name,
                    nick,
                    message,
                } => {
                    // Complete registration
                    let sname = if server_name.is_empty() {
                        self.connection_mgr.server_addr().to_string()
                    } else {
                        server_name
                    };

                    let _ = self
                        .connection_mgr
                        .transition(ConnectionState::Connected { server_name: sname });
                    self.connection_mgr.set_nick(nick.clone());
                    self.view.set_nick(nick);

                    // Clear registration state, timeout, and reconnect state
                    self.registration = None;
                    self.registration_deadline = None;
                    self.reconnect_at = None;
                    self.reconnect_attempt = 0;

                    self.push_status(&message);

                    // Notify plugins of connection.
                    self.dispatch_plugin_event(
                        PluginEventType::Connected,
                        &self.connection_mgr.server_addr().to_string(),
                        "",
                    );

                    // Rejoin channels if this was a reconnect
                    self.rejoin_channels().await;

                    // Upload pre-key bundle for E2E encryption
                    self.upload_pre_key_bundle().await;
                    return;
                }
                RegistrationEvent::Info(text) => {
                    self.push_status(&text);
                    return;
                }
                RegistrationEvent::NickRetry {
                    new_nick,
                    nick_message,
                } => {
                    self.push_status(&format!("Nick in use, trying {new_nick}..."));
                    self.connection_mgr.set_nick(new_nick.clone());
                    self.view.set_nick(new_nick);
                    if let Some(ref mut conn) = self.connection {
                        if let Err(e) = conn.send(nick_message).await {
                            warn!("Failed to send NICK retry: {e}");
                        }
                    }
                    return;
                }
                RegistrationEvent::NickError(reason) => {
                    self.push_status(&format!("Nick error: {reason}"));
                    return;
                }
                RegistrationEvent::Unhandled => {
                    // Fall through to message routing below
                }
            }
        }

        // Handle PIRC encryption-related commands before general routing.
        if self.handle_pirc_message(msg).await {
            return;
        }

        // Handle PIRC P2P signaling messages.
        if self.handle_p2p_message(msg).await {
            return;
        }

        // Handle PIRC GROUP messages.
        if self.handle_group_message(msg) {
            return;
        }

        // Dispatch IRC events to plugins.
        self.dispatch_irc_event_to_plugins(msg);

        // Route the message to the appropriate buffer(s).
        let ts = current_timestamp(&self.config.ui.timestamp_format);
        let our_nick = self.connection_mgr.nick().to_string();
        let actions = message_handler::route_message(msg, &our_nick, &ts);

        if actions.is_empty() {
            // Unhandled command — ignore silently (PING/PONG, etc.)
            return;
        }

        for action in actions {
            match action {
                HandlerAction::PushLine { target, line } => {
                    self.view.push_message(&target, line);
                }
                HandlerAction::OpenChannel(name) => {
                    // Ensure the channel buffer exists (don't switch to it)
                    self.view.buffers_mut().ensure_open(BufferId::Channel(name));
                }
                HandlerAction::UpdateNick(new_nick) => {
                    self.connection_mgr.set_nick(new_nick.clone());
                    self.view.set_nick(new_nick);
                }
            }
        }
    }

    /// Handle the keepalive timer tick.
    ///
    /// If connected and idle for longer than `KEEPALIVE_IDLE_THRESHOLD`, sends
    /// a PING. If a PING was already sent and no PONG received within
    /// `PONG_TIMEOUT`, triggers a disconnect.
    async fn handle_keepalive_tick(&mut self) {
        if !self.connection_mgr.is_connected() {
            return;
        }

        // Check for PONG timeout — if we sent a PING and haven't received a
        // PONG within the timeout window, consider the connection dead.
        if let Some(sent_at) = self.ping_sent_at {
            if sent_at.elapsed() >= PONG_TIMEOUT {
                self.handle_disconnect("Connection timed out (no PONG received)");
                return;
            }
        }

        // If we already have an outstanding PING, don't send another.
        if self.ping_sent_at.is_some() {
            return;
        }

        // Only send a keepalive PING if we've been idle long enough.
        let idle = self
            .last_message_received
            .map_or(true, |t| t.elapsed() >= KEEPALIVE_IDLE_THRESHOLD);
        if !idle {
            return;
        }

        // Send a client keepalive PING.
        let token = format!(
            "pirc-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );
        let ping = Message::new(Command::Ping, vec![token]);
        if let Some(ref mut conn) = self.connection {
            if let Err(e) = conn.send(ping).await {
                warn!("Failed to send keepalive PING: {e}");
                return;
            }
            self.ping_sent_at = Some(Instant::now());
        }
    }

    /// Handle a disconnection event.
    ///
    /// Clears connection state and, if auto-reconnect is enabled, schedules
    /// a reconnection attempt with exponential backoff. If a reconnect
    /// sequence is already in progress, continues from the current attempt.
    fn handle_disconnect(&mut self, reason: &str) {
        // Capture channels before clearing state (for auto-rejoin)
        if self.channels_to_rejoin.is_empty() {
            self.channels_to_rejoin = self.view.buffers().channel_names();
        }

        // Determine the next reconnect attempt: if we're already reconnecting,
        // continue the sequence; otherwise start fresh.
        let next_attempt = if self.reconnect_attempt > 0 {
            self.reconnect_attempt + 1
        } else {
            1
        };

        self.connection = None;
        self.registration = None;
        self.registration_deadline = None;
        self.last_message_received = None;
        self.ping_sent_at = None;
        self.lag_ms = None;
        self.view.set_lag(None);
        self.p2p.clear();
        let _ = self
            .connection_mgr
            .transition(ConnectionState::Disconnected);
        self.push_status(reason);

        // Notify plugins of disconnection.
        self.dispatch_plugin_event(PluginEventType::Disconnected, reason, "");

        // Schedule auto-reconnect if enabled
        if self.connection_mgr.auto_reconnect() {
            self.schedule_reconnect(next_attempt);
        }
    }

    /// Schedule a reconnection attempt after an exponential backoff delay
    /// with jitter to prevent thundering herd when many clients disconnect
    /// simultaneously (e.g. server failover).
    fn schedule_reconnect(&mut self, attempt: u32) {
        let base = self.config.server.reconnect_delay_secs as f64;
        let delay_secs = base * BACKOFF_FACTOR.powi((attempt as i32) - 1);
        let capped = Duration::from_secs_f64(delay_secs).min(MAX_RECONNECT_DELAY);

        // Add ±20% jitter using low-order time bits as a simple PRNG source
        // to spread reconnection attempts across the window.
        let jitter_frac = {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos();
            #[allow(clippy::cast_precision_loss)]
            let frac = (nanos % 1000) as f64 / 1000.0; // 0.0..1.0
            0.8 + frac * 0.4 // 0.8..1.2 (±20%)
        };
        let delay = Duration::from_secs_f64(capped.as_secs_f64() * jitter_frac)
            .min(MAX_RECONNECT_DELAY);

        self.reconnect_attempt = attempt;
        let _ = self
            .connection_mgr
            .transition(ConnectionState::Reconnecting { attempt });
        self.reconnect_at = Some(Instant::now() + delay);

        self.push_status(&format!(
            "Reconnecting in {}s... (attempt {attempt})",
            delay.as_secs()
        ));
        info!(
            attempt,
            delay_secs = delay.as_secs(),
            "scheduling reconnect"
        );
    }

    /// Attempt to reconnect to the server.
    async fn attempt_reconnect(&mut self) {
        self.reconnect_at = None;
        let attempt = self.reconnect_attempt;

        self.push_status(&format!("Reconnecting... (attempt {attempt})"));

        // Transition Reconnecting → Connecting
        if self
            .connection_mgr
            .transition(ConnectionState::Connecting)
            .is_err()
        {
            return;
        }

        let addr_str = self.connection_mgr.server_addr().to_string();
        let addr = match addr_str.to_socket_addrs() {
            Ok(mut addrs) => {
                if let Some(a) = addrs.next() {
                    a
                } else {
                    let _ = self
                        .connection_mgr
                        .transition(ConnectionState::Disconnected);
                    self.push_status(&format!("Could not resolve {addr_str}"));
                    self.schedule_reconnect(attempt + 1);
                    return;
                }
            }
            Err(e) => {
                let _ = self
                    .connection_mgr
                    .transition(ConnectionState::Disconnected);
                self.push_status(&format!("Could not resolve {addr_str}: {e}"));
                self.schedule_reconnect(attempt + 1);
                return;
            }
        };

        let connector = Connector::new();
        match connector.connect(addr).await {
            Ok(conn) => {
                self.connection = Some(conn);

                if self
                    .connection_mgr
                    .transition(ConnectionState::Registering)
                    .is_err()
                {
                    return;
                }

                let nick = self.connection_mgr.nick().to_string();
                let username = nick.clone();
                let realname = self
                    .config
                    .identity
                    .realname
                    .clone()
                    .unwrap_or_else(|| nick.clone());
                let alt_nicks = self.config.identity.alt_nicks.clone();

                let reg = RegistrationState::new(nick, alt_nicks, username, realname);
                self.send_registration_messages(&reg).await;
                self.registration = Some(reg);
                self.registration_deadline = Some(Instant::now() + REGISTRATION_TIMEOUT);

                self.push_status(&format!("Connected to {addr_str}, registering..."));
                info!(attempt, "reconnected, registering");
            }
            Err(e) => {
                let _ = self
                    .connection_mgr
                    .transition(ConnectionState::Disconnected);
                self.push_status(&format!("Reconnect failed: {e}"));
                self.schedule_reconnect(attempt + 1);
            }
        }
    }

    /// Rejoin channels that were open before the disconnect.
    /// Rejoin previously open channels after reconnection.
    ///
    /// Uses IRC's comma-separated JOIN syntax to batch channels into fewer
    /// messages, reducing round-trips during failover reconnection.
    async fn rejoin_channels(&mut self) {
        let channels = std::mem::take(&mut self.channels_to_rejoin);
        if channels.is_empty() {
            return;
        }

        // Batch channels into comma-separated JOIN commands.
        // IRC servers typically accept long parameter lists, but we batch in
        // groups of 10 to stay well under any line-length limits (~512 bytes).
        for chunk in channels.chunks(10) {
            let joined = chunk.join(",");
            let msg = Message::new(Command::Join, vec![joined]);
            if let Some(ref mut conn) = self.connection {
                if let Err(e) = conn.send(msg).await {
                    warn!(%e, channels = ?chunk, "failed to rejoin channels");
                }
            }
        }
        self.push_status(&format!("Rejoining {} channel(s)...", channels.len()));
    }

    /// Push a status message to the status buffer.
    fn push_status(&mut self, text: &str) {
        self.view.push_status_message(BufferLine {
            timestamp: current_timestamp(&self.config.ui.timestamp_format),
            sender: None,
            content: text.to_string(),
            line_type: LineType::System,
        });
    }

    /// Render the current view state to the terminal.
    fn render<W: io::Write>(&mut self, renderer: &mut Renderer<W>) -> io::Result<()> {
        // Sync encryption states for all open query buffers
        self.sync_encryption_states();

        self.view.render(renderer.back_buffer());

        // Render input line at the bottom
        self.render_input_line(renderer.back_buffer());

        renderer.flush()
    }

    /// Update the view coordinator with current encryption states for all query buffers.
    fn sync_encryption_states(&mut self) {
        let query_nicks: Vec<String> = self
            .view
            .buffers()
            .buffer_list()
            .into_iter()
            .filter_map(|(id, _, _, _)| {
                if let BufferId::Query(nick) = id {
                    Some(nick)
                } else {
                    None
                }
            })
            .collect();

        for nick in query_nicks {
            let status = self.encryption.encryption_status(&nick);
            self.view.set_encryption_status(nick, status);
        }
    }

    /// Render the input line into the screen buffer.
    fn render_input_line(&self, buf: &mut Buffer) {
        let width = buf.width();
        let height = buf.height();
        if height == 0 || width == 0 {
            return;
        }

        let input_row = height - 1;
        let line = self.view.input().line();
        let content = line.content();

        // Render "[nick] " prompt
        let nick = self.connection_mgr.nick();
        let prompt = format!("[{nick}] ");
        let prompt_len = prompt.len().min(width as usize);

        let style = crate::tui::style::Style::new();
        buf.write_str(0, input_row, &prompt[..prompt_len], style);

        // Render input text after prompt
        let available = (width as usize).saturating_sub(prompt_len);
        let visible_start = line.scroll_offset();
        let visible_text: String = content
            .chars()
            .skip(visible_start)
            .take(available)
            .collect();
        #[allow(clippy::cast_possible_truncation)]
        let prompt_col = prompt_len as u16; // prompt_len is at most width (u16), so truncation is safe
        buf.write_str(prompt_col, input_row, &visible_text, style);
    }

    /// Get the current channel name for command context, if the active buffer
    /// is a channel.
    fn current_channel_context(&self) -> Option<String> {
        use crate::tui::buffer_manager::BufferId;
        match self.view.buffers().active_id() {
            BufferId::Channel(name) => Some(name.clone()),
            _ => None,
        }
    }
}

/// Return the current time formatted with the given format string.
fn current_timestamp(format: &str) -> String {
    let now = std::time::SystemTime::now();
    let since_epoch = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = since_epoch.as_secs();

    // Simple HH:MM format since we don't want to pull in chrono
    if format.contains("%H") && format.contains("%M") {
        let hours = (secs / 3600) % 24;
        let minutes = (secs / 60) % 60;
        return format
            .replace("%H", &format!("{hours:02}"))
            .replace("%M", &format!("{minutes:02}"));
    }

    // Fallback: just use epoch seconds
    secs.to_string()
}

/// Derive a machine-specific passphrase from the user's nick and hostname.
///
/// Uses SHA-256 to combine the nick with the machine hostname, producing
/// a deterministic 32-byte passphrase. This avoids prompting the user for
/// a password while still providing encrypted-at-rest key storage tied to
/// this machine and user identity.
fn derive_machine_passphrase(nick: &str) -> Vec<u8> {
    use sha2::{Digest, Sha256};

    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "pirc-default-host".to_string());

    let mut hasher = Sha256::new();
    hasher.update(b"pirc-key-storage-v1:");
    hasher.update(nick.as_bytes());
    hasher.update(b"@");
    hasher.update(hostname.as_bytes());
    hasher.finalize().to_vec()
}

mod encryption;
mod group;
mod p2p;
mod plugin;

#[cfg(test)]
mod tests;
