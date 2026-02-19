//! Event dispatch system for routing IRC events to matching script handlers.
//!
//! The [`EventDispatcher`] registers event handlers from parsed scripts and
//! dispatches incoming events to all matching handlers in registration order.
//! Pattern matching uses case-insensitive glob-style patterns.

use std::collections::HashMap;

use crate::ast::{EventHandler, EventType, Statement};

use super::builtins::BuiltinContext;
use super::environment::Environment;
use super::functions::{FunctionRegistry, RegexState};
use super::value::Value;
use super::{CommandHandler, Interpreter, RuntimeError};

/// Context for an incoming IRC event, providing event-specific data.
///
/// When an event is dispatched, the context fields are used to populate
/// built-in identifiers (`$nick`, `$chan`, `$text`, etc.) in the handler scope.
#[derive(Debug, Clone, Default)]
pub struct EventContext {
    /// Which event type fired.
    pub event_type: Option<EventType>,
    /// The nickname of the user who triggered the event.
    pub nick: Option<String>,
    /// The channel where the event occurred (if applicable).
    pub channel: Option<String>,
    /// The message text.
    pub text: Option<String>,
    /// The full parameter string for `$0`–`$9` splitting.
    pub raw_params: Option<String>,
    /// The client's own nickname.
    pub me: Option<String>,
    /// The server hostname.
    pub server: Option<String>,
    /// The event target.
    pub target: Option<String>,
}

impl EventContext {
    /// Creates a new empty event context.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Populates a [`BuiltinContext`] from this event context.
    #[must_use]
    pub fn to_builtin_context(&self) -> BuiltinContext {
        let mut ctx = BuiltinContext::new();

        if let Some(ref nick) = self.nick {
            ctx.set("nick", Value::String(nick.clone()));
        }
        if let Some(ref channel) = self.channel {
            ctx.set("chan", Value::String(channel.clone()));
        }
        if let Some(ref me) = self.me {
            ctx.set("me", Value::String(me.clone()));
        }
        if let Some(ref server) = self.server {
            ctx.set("server", Value::String(server.clone()));
        }
        if let Some(ref target) = self.target {
            ctx.set("target", Value::String(target.clone()));
        }

        // Set $0–$9 via event text. Use raw_params if available, else text.
        let event_text = self
            .raw_params
            .as_deref()
            .or(self.text.as_deref())
            .unwrap_or("");
        ctx.set_event_text(event_text);

        ctx
    }
}

/// A registered handler entry: pattern + body statements.
#[derive(Debug, Clone)]
struct HandlerEntry {
    /// The glob pattern (stored lowercased for case-insensitive matching).
    pattern: String,
    /// The handler body statements.
    body: Vec<Statement>,
}

/// Dispatches IRC events to matching script event handlers.
///
/// Handlers are registered from parsed [`EventHandler`] AST nodes and stored
/// per event type. When an event arrives, all handlers with matching patterns
/// are executed in registration order, each in a fresh local scope.
#[derive(Debug, Clone, Default)]
pub struct EventDispatcher {
    /// Handlers grouped by event type, in registration order.
    handlers: HashMap<EventType, Vec<HandlerEntry>>,
}

impl EventDispatcher {
    /// Creates a new empty dispatcher.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an event handler from a parsed [`EventHandler`] AST node.
    pub fn register(&mut self, handler: &EventHandler) {
        let entry = HandlerEntry {
            pattern: handler.pattern.to_lowercase(),
            body: handler.body.clone(),
        };
        self.handlers
            .entry(handler.event_type)
            .or_default()
            .push(entry);
    }

    /// Returns the number of registered handlers across all event types.
    #[must_use]
    pub fn handler_count(&self) -> usize {
        self.handlers.values().map(Vec::len).sum()
    }

    /// Dispatches an event to all matching handlers.
    ///
    /// For each handler whose pattern matches the match text (derived from the
    /// event context), a fresh local scope is created, built-in identifiers
    /// are populated, and the handler body is executed.
    ///
    /// # Errors
    ///
    /// Returns the first [`RuntimeError`] encountered during handler execution
    /// (except `Return`, which is caught and treated as normal completion).
    pub fn dispatch(
        &self,
        event_type: EventType,
        context: &EventContext,
        env: &mut Environment,
        cmd_handler: &mut dyn CommandHandler,
        functions: &FunctionRegistry,
        regex_state: &RegexState,
    ) -> Result<(), RuntimeError> {
        let Some(handlers) = self.handlers.get(&event_type) else {
            return Ok(());
        };

        let match_text = Self::match_text_for(event_type, context);
        let builtin_ctx = context.to_builtin_context();

        for handler in handlers {
            if !glob_match(&handler.pattern, &match_text.to_lowercase()) {
                continue;
            }

            // Fresh local scope per handler
            env.push_scope();

            let mut interp =
                Interpreter::with_context(env, cmd_handler, &builtin_ctx, functions, regex_state);
            let result = interp.exec_stmts(&handler.body);

            env.pop_scope();

            // Catch Return as normal completion; propagate other errors
            match result {
                Ok(_) | Err(RuntimeError::Return(_)) => {}
                Err(e) => return Err(e),
            }
        }

        Ok(())
    }

    /// Determines the text to match against handler patterns for a given event.
    ///
    /// For channel-oriented events, the match text is the channel name.
    /// For text-oriented events, it is the message text.
    /// For events without a natural match target, it defaults to `*`.
    fn match_text_for(event_type: EventType, context: &EventContext) -> String {
        match event_type {
            // Text-oriented events: match against message text
            EventType::Text | EventType::Notice | EventType::Action | EventType::Ctcp => context
                .text
                .clone()
                .or_else(|| context.channel.clone())
                .unwrap_or_default(),

            // Channel-oriented events: match against channel name
            EventType::Join
            | EventType::Part
            | EventType::Kick
            | EventType::Topic
            | EventType::Mode
            | EventType::Invite => context.channel.clone().unwrap_or_default(),

            // Nick-oriented events: match against nickname
            EventType::Nick | EventType::Quit => context.nick.clone().unwrap_or_default(),

            // Numeric: match against raw_params or text
            EventType::Numeric => context
                .raw_params
                .clone()
                .or_else(|| context.text.clone())
                .unwrap_or_default(),

            // Connection events: match against server
            EventType::Connect | EventType::Disconnect => {
                context.server.clone().unwrap_or_default()
            }
        }
    }
}

/// Matches a string against a glob-style pattern (case-insensitive).
///
/// Supports:
/// - `*` — matches zero or more characters
/// - `?` — matches exactly one character
/// - All other characters are matched literally
///
/// Both pattern and text should be pre-lowercased for case-insensitive matching.
fn glob_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    glob_match_inner(&pattern, &text)
}

/// Recursive glob matcher with backtracking.
fn glob_match_inner(pattern: &[char], text: &[char]) -> bool {
    let mut pat_idx = 0;
    let mut txt_idx = 0;
    let mut wildcard_pat = None; // Pattern index after last `*`
    let mut wildcard_txt = 0; // Text index to retry after last `*`

    while txt_idx < text.len() {
        if pat_idx < pattern.len()
            && (pattern[pat_idx] == '?' || pattern[pat_idx] == text[txt_idx])
        {
            // Exact match or `?` wildcard
            pat_idx += 1;
            txt_idx += 1;
        } else if pat_idx < pattern.len() && pattern[pat_idx] == '*' {
            // `*` wildcard: save state and try matching zero chars
            wildcard_pat = Some(pat_idx);
            wildcard_txt = txt_idx;
            pat_idx += 1;
        } else if let Some(wp) = wildcard_pat {
            // Mismatch: backtrack to last `*` and consume one more char
            pat_idx = wp + 1;
            wildcard_txt += 1;
            txt_idx = wildcard_txt;
        } else {
            return false;
        }
    }

    // Consume remaining `*` patterns
    while pat_idx < pattern.len() && pattern[pat_idx] == '*' {
        pat_idx += 1;
    }

    pat_idx == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{CommandStatement, EventHandler, EventType, Statement};
    use crate::interpreter::{CommandHandler as CmdHandler, RuntimeError, Value};
    use crate::token::Span;

    // ── Glob pattern matching tests ─────────────────────────────────────

    #[test]
    fn glob_star_matches_everything() {
        assert!(glob_match("*", ""));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("*", "#channel"));
        assert!(glob_match("*", "hello world"));
    }

    #[test]
    fn glob_exact_match() {
        assert!(glob_match("hello", "hello"));
        assert!(!glob_match("hello", "world"));
        assert!(!glob_match("hello", "hell"));
        assert!(!glob_match("hello", "helloo"));
    }

    #[test]
    fn glob_case_insensitive() {
        // Both inputs should be lowercased before calling glob_match
        assert!(glob_match("#channel", "#channel"));
        assert!(glob_match("hello", "hello"));
    }

    #[test]
    fn glob_star_prefix() {
        assert!(glob_match("*hello", "hello"));
        assert!(glob_match("*hello", "say hello"));
        assert!(!glob_match("*hello", "hello world"));
    }

    #[test]
    fn glob_star_suffix() {
        assert!(glob_match("hello*", "hello"));
        assert!(glob_match("hello*", "hello world"));
        assert!(!glob_match("hello*", "say hello"));
    }

    #[test]
    fn glob_star_both_sides() {
        assert!(glob_match("*hello*", "hello"));
        assert!(glob_match("*hello*", "say hello world"));
        assert!(glob_match("*hello*", "say hello"));
        assert!(glob_match("*hello*", "hello world"));
        assert!(!glob_match("*hello*", "helo"));
    }

    #[test]
    fn glob_question_mark() {
        assert!(glob_match("h?llo", "hello"));
        assert!(glob_match("h?llo", "hallo"));
        assert!(!glob_match("h?llo", "hllo"));
        assert!(!glob_match("h?llo", "heello"));
    }

    #[test]
    fn glob_channel_pattern() {
        assert!(glob_match("#test", "#test"));
        assert!(!glob_match("#test", "#other"));
        assert!(glob_match("#*", "#test"));
        assert!(glob_match("#*", "#anything"));
        assert!(!glob_match("#*", "noprefix"));
    }

    #[test]
    fn glob_empty_pattern_and_text() {
        assert!(glob_match("", ""));
        assert!(!glob_match("", "something"));
        assert!(glob_match("*", ""));
    }

    #[test]
    fn glob_multiple_stars() {
        assert!(glob_match("*a*b*", "aabb"));
        assert!(glob_match("*a*b*", "xaxbx"));
        assert!(!glob_match("*a*b*", "ba"));
    }

    #[test]
    fn glob_consecutive_stars() {
        assert!(glob_match("**", "anything"));
        assert!(glob_match("***", ""));
        assert!(glob_match("a**b", "ab"));
        assert!(glob_match("a**b", "aXYZb"));
    }

    // ── EventContext tests ──────────────────────────────────────────────

    #[test]
    fn event_context_to_builtin_context_populates_identifiers() {
        let ctx = EventContext {
            event_type: Some(EventType::Text),
            nick: Some("alice".to_string()),
            channel: Some("#test".to_string()),
            text: Some("hello world".to_string()),
            raw_params: None,
            me: Some("bot".to_string()),
            server: Some("irc.example.com".to_string()),
            target: Some("#test".to_string()),
        };

        let builtin = ctx.to_builtin_context();
        assert_eq!(
            builtin.resolve("nick"),
            Value::String("alice".to_string())
        );
        assert_eq!(
            builtin.resolve("chan"),
            Value::String("#test".to_string())
        );
        assert_eq!(builtin.resolve("me"), Value::String("bot".to_string()));
        assert_eq!(
            builtin.resolve("server"),
            Value::String("irc.example.com".to_string())
        );
        assert_eq!(
            builtin.resolve("target"),
            Value::String("#test".to_string())
        );
    }

    #[test]
    fn event_context_populates_numeric_params() {
        let ctx = EventContext {
            text: Some("hello world foo".to_string()),
            raw_params: None,
            ..EventContext::default()
        };

        let builtin = ctx.to_builtin_context();
        assert_eq!(
            builtin.resolve("0"),
            Value::String("hello world foo".to_string())
        );
        assert_eq!(
            builtin.resolve("1"),
            Value::String("hello".to_string())
        );
        assert_eq!(
            builtin.resolve("2"),
            Value::String("world".to_string())
        );
        assert_eq!(builtin.resolve("3"), Value::String("foo".to_string()));
        assert_eq!(builtin.resolve("4"), Value::Null);
    }

    #[test]
    fn event_context_raw_params_takes_precedence() {
        let ctx = EventContext {
            text: Some("not this".to_string()),
            raw_params: Some("use this instead".to_string()),
            ..EventContext::default()
        };

        let builtin = ctx.to_builtin_context();
        assert_eq!(
            builtin.resolve("0"),
            Value::String("use this instead".to_string())
        );
        assert_eq!(builtin.resolve("1"), Value::String("use".to_string()));
    }

    #[test]
    fn event_context_empty_produces_defaults() {
        let ctx = EventContext::default();
        let builtin = ctx.to_builtin_context();
        assert_eq!(builtin.resolve("nick"), Value::Null);
        assert_eq!(builtin.resolve("chan"), Value::Null);
        assert_eq!(builtin.resolve("0"), Value::String(String::new()));
    }

    // ── EventDispatcher tests ───────────────────────────────────────────

    /// Test command handler that records command calls.
    struct TestCmdHandler {
        calls: Vec<(String, Vec<Value>)>,
    }

    impl TestCmdHandler {
        fn new() -> Self {
            Self { calls: vec![] }
        }
    }

    impl CmdHandler for TestCmdHandler {
        fn handle_command(&mut self, name: &str, args: &[Value]) -> Result<(), RuntimeError> {
            self.calls.push((name.to_string(), args.to_vec()));
            Ok(())
        }
    }

    fn make_echo_handler(event_type: EventType, pattern: &str) -> EventHandler {
        EventHandler {
            event_type,
            pattern: pattern.to_string(),
            body: vec![Statement::Command(CommandStatement {
                name: "echo".to_string(),
                args: vec![crate::ast::Expression::StringLiteral {
                    value: format!("matched:{pattern}"),
                    span: Span::new(0, 1),
                }],
                span: Span::new(0, 1),
            })],
            span: Span::new(0, 1),
        }
    }

    #[test]
    fn dispatcher_no_handlers_is_noop() {
        let dispatcher = EventDispatcher::new();
        let ctx = EventContext {
            event_type: Some(EventType::Text),
            text: Some("hello".to_string()),
            ..EventContext::default()
        };
        let mut env = Environment::new();
        let mut cmd = TestCmdHandler::new();
        let funcs = FunctionRegistry::new();
        let regex = RegexState::new();

        let result = dispatcher.dispatch(EventType::Text, &ctx, &mut env, &mut cmd, &funcs, &regex);
        assert!(result.is_ok());
        assert!(cmd.calls.is_empty());
    }

    #[test]
    fn dispatcher_matches_catch_all() {
        let mut dispatcher = EventDispatcher::new();
        dispatcher.register(&make_echo_handler(EventType::Text, "*"));

        let ctx = EventContext {
            event_type: Some(EventType::Text),
            text: Some("hello world".to_string()),
            ..EventContext::default()
        };
        let mut env = Environment::new();
        let mut cmd = TestCmdHandler::new();
        let funcs = FunctionRegistry::new();
        let regex = RegexState::new();

        dispatcher
            .dispatch(EventType::Text, &ctx, &mut env, &mut cmd, &funcs, &regex)
            .unwrap();
        assert_eq!(cmd.calls.len(), 1);
        assert_eq!(cmd.calls[0].0, "echo");
    }

    #[test]
    fn dispatcher_skips_non_matching_pattern() {
        let mut dispatcher = EventDispatcher::new();
        dispatcher.register(&make_echo_handler(EventType::Text, "*goodbye*"));

        let ctx = EventContext {
            event_type: Some(EventType::Text),
            text: Some("hello world".to_string()),
            ..EventContext::default()
        };
        let mut env = Environment::new();
        let mut cmd = TestCmdHandler::new();
        let funcs = FunctionRegistry::new();
        let regex = RegexState::new();

        dispatcher
            .dispatch(EventType::Text, &ctx, &mut env, &mut cmd, &funcs, &regex)
            .unwrap();
        assert!(cmd.calls.is_empty());
    }

    #[test]
    fn dispatcher_multiple_handlers_all_fire() {
        let mut dispatcher = EventDispatcher::new();
        dispatcher.register(&make_echo_handler(EventType::Text, "*"));
        dispatcher.register(&make_echo_handler(EventType::Text, "*hello*"));

        let ctx = EventContext {
            event_type: Some(EventType::Text),
            text: Some("hello world".to_string()),
            ..EventContext::default()
        };
        let mut env = Environment::new();
        let mut cmd = TestCmdHandler::new();
        let funcs = FunctionRegistry::new();
        let regex = RegexState::new();

        dispatcher
            .dispatch(EventType::Text, &ctx, &mut env, &mut cmd, &funcs, &regex)
            .unwrap();
        assert_eq!(cmd.calls.len(), 2);
        assert_eq!(cmd.calls[0].1[0], Value::String("matched:*".to_string()));
        assert_eq!(
            cmd.calls[1].1[0],
            Value::String("matched:*hello*".to_string())
        );
    }

    #[test]
    fn dispatcher_only_fires_matching_event_type() {
        let mut dispatcher = EventDispatcher::new();
        dispatcher.register(&make_echo_handler(EventType::Text, "*"));
        dispatcher.register(&make_echo_handler(EventType::Join, "*"));

        let ctx = EventContext {
            event_type: Some(EventType::Text),
            text: Some("hello".to_string()),
            ..EventContext::default()
        };
        let mut env = Environment::new();
        let mut cmd = TestCmdHandler::new();
        let funcs = FunctionRegistry::new();
        let regex = RegexState::new();

        dispatcher
            .dispatch(EventType::Text, &ctx, &mut env, &mut cmd, &funcs, &regex)
            .unwrap();
        // Only the Text handler fires, not the Join handler
        assert_eq!(cmd.calls.len(), 1);
    }

    #[test]
    fn dispatcher_channel_pattern_matches_join() {
        let mut dispatcher = EventDispatcher::new();
        dispatcher.register(&make_echo_handler(EventType::Join, "#test"));
        dispatcher.register(&make_echo_handler(EventType::Join, "#other"));

        let ctx = EventContext {
            event_type: Some(EventType::Join),
            channel: Some("#test".to_string()),
            nick: Some("alice".to_string()),
            ..EventContext::default()
        };
        let mut env = Environment::new();
        let mut cmd = TestCmdHandler::new();
        let funcs = FunctionRegistry::new();
        let regex = RegexState::new();

        dispatcher
            .dispatch(EventType::Join, &ctx, &mut env, &mut cmd, &funcs, &regex)
            .unwrap();
        assert_eq!(cmd.calls.len(), 1);
        assert_eq!(
            cmd.calls[0].1[0],
            Value::String("matched:#test".to_string())
        );
    }

    #[test]
    fn dispatcher_case_insensitive_matching() {
        let mut dispatcher = EventDispatcher::new();
        dispatcher.register(&make_echo_handler(EventType::Text, "*HELLO*"));

        let ctx = EventContext {
            event_type: Some(EventType::Text),
            text: Some("say Hello World".to_string()),
            ..EventContext::default()
        };
        let mut env = Environment::new();
        let mut cmd = TestCmdHandler::new();
        let funcs = FunctionRegistry::new();
        let regex = RegexState::new();

        dispatcher
            .dispatch(EventType::Text, &ctx, &mut env, &mut cmd, &funcs, &regex)
            .unwrap();
        assert_eq!(cmd.calls.len(), 1);
    }

    #[test]
    fn dispatcher_handler_gets_fresh_scope() {
        // Use a handler body that sets a local variable
        let handler = EventHandler {
            event_type: EventType::Text,
            pattern: "*".to_string(),
            body: vec![Statement::VarDecl(crate::ast::VarDeclStatement {
                name: "x".to_string(),
                global: false,
                value: crate::ast::Expression::IntLiteral {
                    value: 42,
                    span: Span::new(0, 1),
                },
                span: Span::new(0, 1),
            })],
            span: Span::new(0, 1),
        };

        let mut dispatcher = EventDispatcher::new();
        dispatcher.register(&handler);

        let ctx = EventContext {
            event_type: Some(EventType::Text),
            text: Some("test".to_string()),
            ..EventContext::default()
        };
        let mut env = Environment::new();
        let mut cmd = TestCmdHandler::new();
        let funcs = FunctionRegistry::new();
        let regex = RegexState::new();

        // Dispatch twice
        dispatcher
            .dispatch(EventType::Text, &ctx, &mut env, &mut cmd, &funcs, &regex)
            .unwrap();
        dispatcher
            .dispatch(EventType::Text, &ctx, &mut env, &mut cmd, &funcs, &regex)
            .unwrap();

        // The variable should not leak into the outer scope
        assert!(env.get_local("x").is_none());
    }

    #[test]
    fn dispatcher_handler_count() {
        let mut dispatcher = EventDispatcher::new();
        assert_eq!(dispatcher.handler_count(), 0);

        dispatcher.register(&make_echo_handler(EventType::Text, "*"));
        assert_eq!(dispatcher.handler_count(), 1);

        dispatcher.register(&make_echo_handler(EventType::Join, "*"));
        assert_eq!(dispatcher.handler_count(), 2);

        dispatcher.register(&make_echo_handler(EventType::Text, "*hello*"));
        assert_eq!(dispatcher.handler_count(), 3);
    }

    #[test]
    fn dispatcher_handlers_fire_in_registration_order() {
        let mut dispatcher = EventDispatcher::new();
        dispatcher.register(&make_echo_handler(EventType::Text, "*"));

        // Second handler echoes a different message
        let handler2 = EventHandler {
            event_type: EventType::Text,
            pattern: "*".to_string(),
            body: vec![Statement::Command(CommandStatement {
                name: "echo".to_string(),
                args: vec![crate::ast::Expression::StringLiteral {
                    value: "second".to_string(),
                    span: Span::new(0, 1),
                }],
                span: Span::new(0, 1),
            })],
            span: Span::new(0, 1),
        };
        dispatcher.register(&handler2);

        let ctx = EventContext {
            event_type: Some(EventType::Text),
            text: Some("hi".to_string()),
            ..EventContext::default()
        };
        let mut env = Environment::new();
        let mut cmd = TestCmdHandler::new();
        let funcs = FunctionRegistry::new();
        let regex = RegexState::new();

        dispatcher
            .dispatch(EventType::Text, &ctx, &mut env, &mut cmd, &funcs, &regex)
            .unwrap();
        assert_eq!(cmd.calls.len(), 2);
        assert_eq!(cmd.calls[0].1[0], Value::String("matched:*".to_string()));
        assert_eq!(cmd.calls[1].1[0], Value::String("second".to_string()));
    }

    #[test]
    fn dispatcher_return_in_handler_does_not_stop_others() {
        let mut dispatcher = EventDispatcher::new();

        // First handler returns early
        let handler1 = EventHandler {
            event_type: EventType::Text,
            pattern: "*".to_string(),
            body: vec![
                Statement::Command(CommandStatement {
                    name: "echo".to_string(),
                    args: vec![crate::ast::Expression::StringLiteral {
                        value: "first".to_string(),
                        span: Span::new(0, 1),
                    }],
                    span: Span::new(0, 1),
                }),
                Statement::Return(crate::ast::ReturnStatement {
                    value: None,
                    span: Span::new(0, 1),
                }),
            ],
            span: Span::new(0, 1),
        };
        dispatcher.register(&handler1);
        dispatcher.register(&make_echo_handler(EventType::Text, "*"));

        let ctx = EventContext {
            event_type: Some(EventType::Text),
            text: Some("hi".to_string()),
            ..EventContext::default()
        };
        let mut env = Environment::new();
        let mut cmd = TestCmdHandler::new();
        let funcs = FunctionRegistry::new();
        let regex = RegexState::new();

        dispatcher
            .dispatch(EventType::Text, &ctx, &mut env, &mut cmd, &funcs, &regex)
            .unwrap();
        // Both handlers should fire: return in first doesn't stop second
        assert_eq!(cmd.calls.len(), 2);
    }

    #[test]
    fn dispatcher_builtin_identifiers_accessible_in_handler() {
        // Handler body: echo $nick
        let handler = EventHandler {
            event_type: EventType::Text,
            pattern: "*".to_string(),
            body: vec![Statement::Command(CommandStatement {
                name: "echo".to_string(),
                args: vec![crate::ast::Expression::BuiltinId {
                    name: "nick".to_string(),
                    span: Span::new(0, 1),
                }],
                span: Span::new(0, 1),
            })],
            span: Span::new(0, 1),
        };

        let mut dispatcher = EventDispatcher::new();
        dispatcher.register(&handler);

        let ctx = EventContext {
            event_type: Some(EventType::Text),
            nick: Some("alice".to_string()),
            text: Some("hi".to_string()),
            ..EventContext::default()
        };
        let mut env = Environment::new();
        let mut cmd = TestCmdHandler::new();
        let funcs = FunctionRegistry::new();
        let regex = RegexState::new();

        dispatcher
            .dispatch(EventType::Text, &ctx, &mut env, &mut cmd, &funcs, &regex)
            .unwrap();
        assert_eq!(cmd.calls.len(), 1);
        assert_eq!(cmd.calls[0].1[0], Value::String("alice".to_string()));
    }

    #[test]
    fn dispatcher_connect_event_matches_server() {
        let mut dispatcher = EventDispatcher::new();
        dispatcher.register(&make_echo_handler(EventType::Connect, "irc.example.com"));
        dispatcher.register(&make_echo_handler(EventType::Connect, "other.server.com"));

        let ctx = EventContext {
            event_type: Some(EventType::Connect),
            server: Some("irc.example.com".to_string()),
            ..EventContext::default()
        };
        let mut env = Environment::new();
        let mut cmd = TestCmdHandler::new();
        let funcs = FunctionRegistry::new();
        let regex = RegexState::new();

        dispatcher
            .dispatch(EventType::Connect, &ctx, &mut env, &mut cmd, &funcs, &regex)
            .unwrap();
        assert_eq!(cmd.calls.len(), 1);
    }

    #[test]
    fn dispatcher_nick_event_matches_nick() {
        let mut dispatcher = EventDispatcher::new();
        dispatcher.register(&make_echo_handler(EventType::Nick, "alice"));

        let ctx = EventContext {
            event_type: Some(EventType::Nick),
            nick: Some("alice".to_string()),
            ..EventContext::default()
        };
        let mut env = Environment::new();
        let mut cmd = TestCmdHandler::new();
        let funcs = FunctionRegistry::new();
        let regex = RegexState::new();

        dispatcher
            .dispatch(EventType::Nick, &ctx, &mut env, &mut cmd, &funcs, &regex)
            .unwrap();
        assert_eq!(cmd.calls.len(), 1);
    }

    #[test]
    fn dispatcher_part_event_matches_channel() {
        let mut dispatcher = EventDispatcher::new();
        dispatcher.register(&make_echo_handler(EventType::Part, "#general"));

        let ctx = EventContext {
            event_type: Some(EventType::Part),
            channel: Some("#general".to_string()),
            nick: Some("bob".to_string()),
            ..EventContext::default()
        };
        let mut env = Environment::new();
        let mut cmd = TestCmdHandler::new();
        let funcs = FunctionRegistry::new();
        let regex = RegexState::new();

        dispatcher
            .dispatch(EventType::Part, &ctx, &mut env, &mut cmd, &funcs, &regex)
            .unwrap();
        assert_eq!(cmd.calls.len(), 1);
    }
}
