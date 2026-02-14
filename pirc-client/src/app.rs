use std::io;
use std::net::ToSocketAddrs;
use std::time::Duration;

use pirc_network::connection::AsyncTransport;
use pirc_network::{Connection, Connector, ShutdownController, ShutdownSignal};
use pirc_protocol::Message;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tracing::warn;

use crate::client_command::ClientCommand;
use crate::config::ClientConfig;
use crate::connection_state::{ConnectionManager, ConnectionState};
use crate::registration::{RegistrationEvent, RegistrationState};
use crate::tui::buffer::Buffer;
use crate::tui::input::KeyEvent;
use crate::tui::message_buffer::{BufferLine, LineType};
use crate::tui::renderer::Renderer;
use crate::tui::view_coordinator::{ViewAction, ViewCoordinator};
use crate::tui::{terminal_size, InputReader, RawModeGuard, SignalHandler};

/// Registration timeout — if no RPL_WELCOME is received within this duration,
/// the connection is dropped.
const REGISTRATION_TIMEOUT: Duration = Duration::from_secs(30);

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
}

impl App {
    /// Create a new `App` from the given configuration.
    pub fn new(config: ClientConfig) -> Self {
        let (width, height) = terminal_size().map_or((80, 24), |s| (s.cols, s.rows));

        let scrollback = config.ui.scrollback_lines as usize;
        let mut view = ViewCoordinator::new(width, height, scrollback);

        let connection_mgr = ConnectionManager::new(&config);
        view.set_nick(connection_mgr.nick().to_string());

        let (shutdown_controller, shutdown_signal) = ShutdownSignal::new();

        Self {
            config,
            connection_mgr,
            view,
            connection: None,
            registration: None,
            registration_deadline: None,
            shutdown_controller,
            _shutdown_signal: shutdown_signal,
        }
    }

    /// Run the main event loop.
    ///
    /// This enables raw terminal mode, spawns the stdin reader task, initiates
    /// the server connection, and enters the `tokio::select!` loop that handles
    /// stdin input, network messages, and shutdown concurrently.
    pub async fn run(mut self) -> io::Result<()> {
        // Enter raw mode (alternate screen, hide cursor)
        let _guard = RawModeGuard::enable()?;

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

        // Initial render
        self.render(&mut renderer)?;

        // Spawn stdin reader task
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

        // Initiate connection
        self.initiate_connection().await;
        self.render(&mut renderer)?;

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
            ViewAction::Quit => true,
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

    /// Handle a client command (e.g. /join, /quit, /nick).
    async fn handle_command(&mut self, cmd: ClientCommand) {
        // Quit is special
        if matches!(cmd, ClientCommand::Quit(_)) {
            // Send QUIT message if connected
            if let Some(msg) = cmd.to_message(None) {
                if let Some(ref mut conn) = self.connection {
                    let _ = conn.send(msg).await;
                }
            }
            return;
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

    /// Handle a chat message (plain text sent to the active buffer's target).
    async fn handle_chat_message(
        &mut self,
        text: &str,
        target: &crate::tui::buffer_manager::BufferId,
    ) {
        use crate::tui::buffer_manager::BufferId;

        let channel = match target {
            BufferId::Status => {
                self.push_status("Cannot send messages to the status buffer");
                return;
            }
            BufferId::Channel(name) | BufferId::Query(name) => name.clone(),
        };

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
    /// unhandled messages, the default handler displays them in the status
    /// buffer. Full message routing (T110) will refine this further.
    async fn handle_server_message(&mut self, msg: &Message) {
        // During registration, let the registration handler process the message first.
        if self.registration.is_some() {
            let event = self.registration.as_mut().unwrap().handle_message(msg);
            match event {
                RegistrationEvent::Welcome { server_name, nick, message } => {
                    // Complete registration
                    let sname = if server_name.is_empty() {
                        self.connection_mgr.server_addr().to_string()
                    } else {
                        server_name
                    };

                    let _ = self.connection_mgr.transition(ConnectionState::Connected {
                        server_name: sname,
                    });
                    self.connection_mgr.set_nick(nick.clone());
                    self.view.set_nick(nick);

                    // Clear registration state and timeout
                    self.registration = None;
                    self.registration_deadline = None;

                    self.push_status(&message);
                    return;
                }
                RegistrationEvent::Info(text) => {
                    self.push_status(&text);
                    return;
                }
                RegistrationEvent::NickRetry { new_nick, nick_message } => {
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
                    // Fall through to default handling below
                }
            }
        }

        // Default: display the raw message in the status buffer
        let content = format!("{msg}");
        self.view.push_status_message(BufferLine {
            timestamp: current_timestamp(&self.config.ui.timestamp_format),
            sender: None,
            content,
            line_type: LineType::System,
        });
    }

    /// Handle a disconnection event.
    fn handle_disconnect(&mut self, reason: &str) {
        self.connection = None;
        self.registration = None;
        self.registration_deadline = None;
        let _ = self
            .connection_mgr
            .transition(ConnectionState::Disconnected);
        self.push_status(reason);
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
        self.view.render(renderer.back_buffer());

        // Render input line at the bottom
        self.render_input_line(renderer.back_buffer());

        renderer.flush()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ClientConfig;

    #[test]
    fn app_new_creates_with_defaults() {
        let config = ClientConfig::default();
        let app = App::new(config);
        assert!(app.connection.is_none());
        assert!(!app.connection_mgr.is_connected());
    }

    #[test]
    fn app_new_uses_config_nick() {
        let mut config = ClientConfig::default();
        config.identity.nick = Some("testuser".to_string());
        let app = App::new(config);
        assert_eq!(app.connection_mgr.nick(), "testuser");
    }

    #[test]
    fn current_timestamp_formats_hm() {
        let ts = current_timestamp("%H:%M");
        assert!(ts.contains(':'), "timestamp should contain a colon: {ts}");
        assert_eq!(ts.len(), 5, "HH:MM should be 5 chars: {ts}");
    }

    #[test]
    fn current_timestamp_fallback() {
        let ts = current_timestamp("custom");
        // Should be epoch seconds since format doesn't match
        assert!(ts.parse::<u64>().is_ok());
    }

    #[test]
    fn handle_disconnect_clears_connection() {
        let mut config = ClientConfig::default();
        config.identity.nick = Some("user".to_string());
        let mut app = App::new(config);

        // Manually force into Connecting then Registering states
        app.connection_mgr
            .transition(ConnectionState::Connecting)
            .unwrap();
        app.connection_mgr
            .transition(ConnectionState::Registering)
            .unwrap();
        app.connection_mgr
            .transition(ConnectionState::Connected {
                server_name: "test".into(),
            })
            .unwrap();

        app.handle_disconnect("test disconnect");
        assert!(app.connection.is_none());
        assert!(!app.connection_mgr.is_connected());
    }

    #[test]
    fn push_status_adds_to_status_buffer() {
        let config = ClientConfig::default();
        let mut app = App::new(config);
        app.push_status("hello world");
        assert!(app.view.buffers().get(&crate::tui::buffer_manager::BufferId::Status).unwrap().len() > 0);
    }

    #[test]
    fn dispatch_quit_returns_true() {
        let config = ClientConfig::default();
        let mut app = App::new(config);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(app.dispatch_view_action(ViewAction::Quit));
        assert!(result);
    }

    #[test]
    fn dispatch_none_returns_false() {
        let config = ClientConfig::default();
        let mut app = App::new(config);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(app.dispatch_view_action(ViewAction::None));
        assert!(!result);
    }

    #[test]
    fn dispatch_redraw_returns_false() {
        let config = ClientConfig::default();
        let mut app = App::new(config);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(app.dispatch_view_action(ViewAction::Redraw));
        assert!(!result);
    }

    #[test]
    fn dispatch_command_error_returns_false() {
        let config = ClientConfig::default();
        let mut app = App::new(config);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let err = crate::client_command::CommandError::MissingArgument {
            command: "join".into(),
            argument: "channel".into(),
        };
        let result = rt.block_on(app.dispatch_view_action(ViewAction::CommandError(err)));
        assert!(!result);
    }

    #[test]
    fn handle_server_message_rpl_welcome() {
        let mut config = ClientConfig::default();
        config.identity.nick = Some("testuser".to_string());
        let mut app = App::new(config);

        // Must be in Registering state to transition to Connected
        app.connection_mgr
            .transition(ConnectionState::Connecting)
            .unwrap();
        app.connection_mgr
            .transition(ConnectionState::Registering)
            .unwrap();

        // Set up registration state (as initiate_connection would)
        app.registration = Some(RegistrationState::new(
            "testuser".into(),
            vec![],
            "testuser".into(),
            "testuser".into(),
        ));
        app.registration_deadline = Some(Instant::now() + REGISTRATION_TIMEOUT);

        let msg = Message::with_prefix(
            pirc_protocol::Prefix::Server("irc.test.net".into()),
            pirc_protocol::Command::Numeric(1),
            vec![
                "testuser".into(),
                "Welcome to the test network!".into(),
            ],
        );

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(app.handle_server_message(&msg));
        assert!(app.connection_mgr.is_connected());
        assert_eq!(app.connection_mgr.server_name(), Some("irc.test.net"));
        assert!(app.registration.is_none());
        assert!(app.registration_deadline.is_none());
    }

    #[test]
    fn handle_server_message_rpl_welcome_updates_nick() {
        let mut config = ClientConfig::default();
        config.identity.nick = Some("mynick".to_string());
        let mut app = App::new(config);

        app.connection_mgr
            .transition(ConnectionState::Connecting)
            .unwrap();
        app.connection_mgr
            .transition(ConnectionState::Registering)
            .unwrap();

        app.registration = Some(RegistrationState::new(
            "mynick".into(),
            vec![],
            "mynick".into(),
            "mynick".into(),
        ));

        let msg = Message::with_prefix(
            pirc_protocol::Prefix::Server("irc.test.net".into()),
            pirc_protocol::Command::Numeric(1),
            vec![
                "servernick".into(),
                "Welcome!".into(),
            ],
        );

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(app.handle_server_message(&msg));
        assert_eq!(app.connection_mgr.nick(), "servernick");
    }

    #[test]
    fn handle_server_message_info_numerics() {
        let mut config = ClientConfig::default();
        config.identity.nick = Some("user".to_string());
        let mut app = App::new(config);

        app.connection_mgr
            .transition(ConnectionState::Connecting)
            .unwrap();
        app.connection_mgr
            .transition(ConnectionState::Registering)
            .unwrap();
        app.registration = Some(RegistrationState::new(
            "user".into(), vec![], "user".into(), "user".into(),
        ));

        let initial_count = app
            .view
            .buffers()
            .get(&crate::tui::buffer_manager::BufferId::Status)
            .unwrap()
            .len();

        let msg = Message::new(
            pirc_protocol::Command::Numeric(2),
            vec!["user".into(), "Your host is irc.test.net".into()],
        );

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(app.handle_server_message(&msg));

        let new_count = app
            .view
            .buffers()
            .get(&crate::tui::buffer_manager::BufferId::Status)
            .unwrap()
            .len();
        assert_eq!(new_count, initial_count + 1);
    }

    #[test]
    fn handle_server_message_nick_in_use() {
        let mut config = ClientConfig::default();
        config.identity.nick = Some("mynick".to_string());
        config.identity.alt_nicks = vec!["alt1".into()];
        let mut app = App::new(config);

        app.connection_mgr
            .transition(ConnectionState::Connecting)
            .unwrap();
        app.connection_mgr
            .transition(ConnectionState::Registering)
            .unwrap();
        app.registration = Some(RegistrationState::new(
            "mynick".into(),
            vec!["alt1".into()],
            "mynick".into(),
            "mynick".into(),
        ));

        let msg = Message::new(
            pirc_protocol::Command::Numeric(433),
            vec!["*".into(), "mynick".into(), "Nickname is already in use".into()],
        );

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(app.handle_server_message(&msg));

        // Nick should have been updated to alt1
        assert_eq!(app.connection_mgr.nick(), "alt1");
        // Still registering
        assert!(!app.connection_mgr.is_connected());
        assert!(app.registration.is_some());
    }

    #[test]
    fn handle_server_message_generic() {
        let config = ClientConfig::default();
        let mut app = App::new(config);

        let msg = Message::new(
            pirc_protocol::Command::Ping,
            vec!["server".into()],
        );

        let initial_count = app
            .view
            .buffers()
            .get(&crate::tui::buffer_manager::BufferId::Status)
            .unwrap()
            .len();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(app.handle_server_message(&msg));

        let new_count = app
            .view
            .buffers()
            .get(&crate::tui::buffer_manager::BufferId::Status)
            .unwrap()
            .len();

        assert_eq!(new_count, initial_count + 1);
    }

    #[test]
    fn handle_disconnect_clears_registration() {
        let mut config = ClientConfig::default();
        config.identity.nick = Some("user".to_string());
        let mut app = App::new(config);

        app.connection_mgr
            .transition(ConnectionState::Connecting)
            .unwrap();
        app.connection_mgr
            .transition(ConnectionState::Registering)
            .unwrap();
        app.registration = Some(RegistrationState::new(
            "user".into(), vec![], "user".into(), "user".into(),
        ));
        app.registration_deadline = Some(Instant::now() + REGISTRATION_TIMEOUT);

        app.handle_disconnect("test disconnect");
        assert!(app.registration.is_none());
        assert!(app.registration_deadline.is_none());
        assert!(app.connection.is_none());
    }

    #[test]
    fn render_input_line_works() {
        let mut config = ClientConfig::default();
        config.identity.nick = Some("nick".to_string());
        let app = App::new(config);

        let mut buf = Buffer::new(80, 24);
        app.render_input_line(&mut buf);

        // Check that the prompt is rendered
        let cell = buf.get(0, 23);
        assert_eq!(cell.ch, '[');
    }
}
