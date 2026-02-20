//! Scripting DSL integration tests.
//!
//! Exercises the full scripting pipeline: script loading/parsing, event hooks,
//! aliases, variables, timers, and script-host interaction.
//!
//! Test modules are organized by feature area:
//! - `loading` — script loading, parsing, syntax errors, multi-script loading
//! - `events` — event hooks (JOIN/TEXT/QUIT/NICK/CONNECT/PART/DISCONNECT)
//! - `aliases` — custom commands, parameters, chaining, override
//! - `variables` — local/global variables, interpolation, persistence
//! - `timers` — scheduled/repeating timers, cancellation, callbacks
//! - `host_interaction` — script-host communication and state queries

#![allow(clippy::needless_raw_string_hashes)]

mod aliases;
mod events;
mod host_interaction;
mod loading;
mod timers;
mod variables;

use std::sync::{Arc, Mutex};
use std::time::Instant;

use pirc_scripting::ast::EventType;
use pirc_scripting::engine::ScriptEngine;
use pirc_scripting::interpreter::{
    CommandHandler, EventContext, RuntimeError, ScriptHost, ScriptRuntimeError, Value,
};

// =========================================================================
// Shared test infrastructure
// =========================================================================

/// Mock host that records commands, echo output, errors, and warnings.
#[allow(clippy::type_complexity)]
pub struct MockScriptHost {
    commands: Arc<Mutex<Vec<(String, Vec<String>)>>>,
    echoed: Arc<Mutex<Vec<String>>>,
    errors: Arc<Mutex<Vec<String>>>,
    warnings: Arc<Mutex<Vec<String>>>,
    pub nick: String,
    pub server: Option<String>,
    pub channel: Option<String>,
    pub port: u16,
}

impl Default for MockScriptHost {
    fn default() -> Self {
        Self {
            commands: Arc::new(Mutex::new(Vec::new())),
            echoed: Arc::new(Mutex::new(Vec::new())),
            errors: Arc::new(Mutex::new(Vec::new())),
            warnings: Arc::new(Mutex::new(Vec::new())),
            nick: "testbot".to_string(),
            server: Some("irc.example.com".to_string()),
            channel: Some("#test".to_string()),
            port: 6667,
        }
    }
}

impl MockScriptHost {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn commands(&self) -> Vec<(String, Vec<String>)> {
        self.commands.lock().unwrap().clone()
    }

    pub fn echoed(&self) -> Vec<String> {
        self.echoed.lock().unwrap().clone()
    }

    pub fn errors(&self) -> Vec<String> {
        self.errors.lock().unwrap().clone()
    }

    pub fn warnings(&self) -> Vec<String> {
        self.warnings.lock().unwrap().clone()
    }
}

impl CommandHandler for MockScriptHost {
    fn handle_command(&mut self, name: &str, args: &[Value]) -> Result<(), RuntimeError> {
        self.commands.lock().unwrap().push((
            name.to_string(),
            args.iter().map(ToString::to_string).collect(),
        ));
        Ok(())
    }
}

impl ScriptHost for MockScriptHost {
    fn current_nick(&self) -> &str {
        &self.nick
    }

    fn current_server(&self) -> Option<&str> {
        self.server.as_deref()
    }

    fn current_channel(&self) -> Option<&str> {
        self.channel.as_deref()
    }

    fn server_port(&self) -> u16 {
        self.port
    }

    fn echo(&mut self, text: &str) {
        self.echoed.lock().unwrap().push(text.to_string());
    }

    fn report_error(&mut self, error: &ScriptRuntimeError) {
        self.errors.lock().unwrap().push(error.to_string());
    }

    fn report_warning(&mut self, warning: &str) {
        self.warnings.lock().unwrap().push(warning.to_string());
    }
}

/// Create a `ScriptEngine` with a script loaded from source.
pub fn engine_with_script(src: &str) -> ScriptEngine {
    let mut engine = ScriptEngine::new();
    let now = Instant::now();
    engine
        .load_script(src, "test.pirc", now)
        .expect("script should parse and load");
    engine
}

/// Create an `EventContext` for a channel text message event.
pub fn text_event(nick: &str, channel: &str, text: &str) -> EventContext {
    EventContext {
        event_type: Some(EventType::Text),
        nick: Some(nick.to_string()),
        channel: Some(channel.to_string()),
        text: Some(text.to_string()),
        ..EventContext::default()
    }
}

/// Create an `EventContext` for a JOIN event.
pub fn join_event(nick: &str, channel: &str) -> EventContext {
    EventContext {
        event_type: Some(EventType::Join),
        nick: Some(nick.to_string()),
        channel: Some(channel.to_string()),
        ..EventContext::default()
    }
}

/// Create an `EventContext` for a QUIT event.
pub fn quit_event(nick: &str, message: &str) -> EventContext {
    EventContext {
        event_type: Some(EventType::Quit),
        nick: Some(nick.to_string()),
        text: Some(message.to_string()),
        ..EventContext::default()
    }
}

/// Create an `EventContext` for a NICK event.
pub fn nick_event(old_nick: &str, new_nick: &str) -> EventContext {
    EventContext {
        event_type: Some(EventType::Nick),
        nick: Some(old_nick.to_string()),
        text: Some(new_nick.to_string()),
        ..EventContext::default()
    }
}

/// Create an `EventContext` for a CONNECT event.
pub fn connect_event(server: &str) -> EventContext {
    EventContext {
        event_type: Some(EventType::Connect),
        server: Some(server.to_string()),
        ..EventContext::default()
    }
}

/// Create an `EventContext` for a PART event.
pub fn part_event(nick: &str, channel: &str, message: &str) -> EventContext {
    EventContext {
        event_type: Some(EventType::Part),
        nick: Some(nick.to_string()),
        channel: Some(channel.to_string()),
        text: Some(message.to_string()),
        ..EventContext::default()
    }
}

/// Create an `EventContext` for a DISCONNECT event.
pub fn disconnect_event(server: &str) -> EventContext {
    EventContext {
        event_type: Some(EventType::Disconnect),
        server: Some(server.to_string()),
        ..EventContext::default()
    }
}
