//! Top-level script engine that coordinates all scripting subsystems.
//!
//! The [`ScriptEngine`] ties together the interpreter, event dispatcher,
//! alias registry, timer manager, and builtin registry into a single
//! public API for loading, managing, and executing scripts.

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use crate::ast::{EventType, TopLevelItem};
use crate::error::{ScriptError, SemanticWarning};
use crate::interpreter::{
    BuiltinContext, Environment, EventContext, EventDispatcher, FunctionRegistry, Interpreter,
    RegexState, RuntimeError, ScriptHost, ScriptRuntimeError, TimerManager, Value,
};
use crate::interpreter::command::AliasRegistry;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::semantic::SemanticAnalyzer;

/// Information about a loaded script.
#[derive(Debug, Clone)]
struct LoadedScript {
    /// The original source code.
    source: String,
    /// Alias names registered by this script (lowercased).
    aliases: Vec<String>,
    /// Timer names registered by this script (lowercased).
    timers: Vec<String>,
}

/// Errors that can occur during script loading.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum LoadError {
    /// The script source could not be parsed.
    #[error("script error in '{filename}': {error}")]
    Script {
        /// The filename of the script.
        filename: String,
        /// The underlying parse/lex/semantic error.
        error: ScriptError,
    },

    /// Semantic analysis found fatal errors.
    #[error("semantic errors in '{filename}': {errors:?}")]
    Semantic {
        /// The filename of the script.
        filename: String,
        /// The semantic errors.
        errors: Vec<String>,
    },

    /// An I/O error occurred reading a script file.
    #[error("I/O error reading '{filename}': {message}")]
    Io {
        /// The filename that failed to read.
        filename: String,
        /// The error message.
        message: String,
    },
}

/// Result of loading a script, containing warnings even on success.
#[derive(Debug, Clone)]
pub struct LoadResult {
    /// Warnings produced during semantic analysis (non-fatal).
    pub warnings: Vec<SemanticWarning>,
}

/// The top-level scripting engine.
///
/// Coordinates the interpreter, event dispatcher, alias registry,
/// timer manager, and builtin context. Provides the public API for
/// loading scripts and dispatching events/commands.
///
/// Client interaction goes through the [`ScriptHost`] trait, which is
/// passed to methods that execute scripts. The host provides command
/// dispatch, client state, output, and error reporting.
pub struct ScriptEngine {
    /// Shared variable environment (global variables persist across scripts).
    env: Environment,
    /// Event handler dispatcher.
    events: EventDispatcher,
    /// Alias registry.
    aliases: AliasRegistry,
    /// Timer manager.
    timers: TimerManager,
    /// Built-in identifier context.
    builtin_ctx: BuiltinContext,
    /// Built-in function registry.
    functions: FunctionRegistry,
    /// Regex capture state.
    regex_state: RegexState,
    /// Loaded scripts indexed by filename.
    scripts: HashMap<String, LoadedScript>,
}

impl ScriptEngine {
    /// Creates a new empty script engine.
    #[must_use]
    pub fn new() -> Self {
        Self {
            env: Environment::new(),
            events: EventDispatcher::new(),
            aliases: AliasRegistry::new(),
            timers: TimerManager::new(),
            builtin_ctx: BuiltinContext::new(),
            functions: FunctionRegistry::new(),
            regex_state: RegexState::new(),
            scripts: HashMap::new(),
        }
    }

    // ── Script loading ────────────────────────────────────────────────

    /// Loads a script from source code.
    ///
    /// Parses, analyzes, and registers all aliases, events, and timers
    /// defined in the script. If a script with the same filename is
    /// already loaded, it is unloaded first.
    ///
    /// # Errors
    ///
    /// Returns a [`LoadError`] if the script cannot be parsed or has
    /// fatal semantic errors. Warnings are returned in the success result.
    pub fn load_script(
        &mut self,
        source: &str,
        filename: &str,
        now: Instant,
    ) -> Result<LoadResult, LoadError> {
        // Unload any previously loaded version of this script
        if self.scripts.contains_key(filename) {
            self.unload_script(filename);
        }

        // 1. Lex
        let mut lexer = Lexer::new(source);
        let tokens = lexer.tokenize().map_err(|e| LoadError::Script {
            filename: filename.to_string(),
            error: ScriptError::Lex(e),
        })?;

        // 2. Parse
        let mut parser = Parser::new(tokens, source);
        let script = parser.parse().map_err(|e| LoadError::Script {
            filename: filename.to_string(),
            error: ScriptError::Parse(e),
        })?;

        // 3. Semantic analysis
        let analyzer = SemanticAnalyzer::new(source);
        let result = analyzer.analyze(&script);

        if result.has_errors() {
            return Err(LoadError::Semantic {
                filename: filename.to_string(),
                errors: result.errors.iter().map(ToString::to_string).collect(),
            });
        }

        let warnings = result.warnings;

        // 4. Register aliases, events, and timers
        let mut loaded = LoadedScript {
            source: source.to_string(),
            aliases: Vec::new(),
            timers: Vec::new(),
        };

        for item in &script.items {
            match item {
                TopLevelItem::Alias(alias) => {
                    self.aliases.register(alias);
                    loaded.aliases.push(alias.name.to_lowercase());
                }
                TopLevelItem::Event(event) => {
                    self.events.register_with_source(event, Some(filename));
                }
                TopLevelItem::Timer(timer) => {
                    // Evaluate interval and repetitions as constants
                    let interval_secs = Self::eval_const_expr(&timer.interval);
                    let repetitions = Self::eval_const_expr(&timer.repetitions);

                    let duration =
                        std::time::Duration::from_secs_f64(interval_secs.max(0.1));
                    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                    let reps = if repetitions <= 0.0 {
                        0u64
                    } else {
                        repetitions as u64
                    };

                    self.timers
                        .register(&timer.name, duration, reps, timer.body.clone(), now);
                    loaded.timers.push(timer.name.to_lowercase());
                }
            }
        }

        self.scripts.insert(filename.to_string(), loaded);

        Ok(LoadResult { warnings })
    }

    /// Loads a script from a file.
    ///
    /// # Errors
    ///
    /// Returns a [`LoadError`] on I/O failure, parse error, or semantic errors.
    pub fn load_script_file(
        &mut self,
        path: &Path,
        now: Instant,
    ) -> Result<LoadResult, LoadError> {
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let source = std::fs::read_to_string(path).map_err(|e| LoadError::Io {
            filename: filename.clone(),
            message: e.to_string(),
        })?;

        self.load_script(&source, &filename, now)
    }

    /// Loads all `*.pirc` files from a directory.
    ///
    /// Returns a list of `(filename, result)` pairs for each file attempted.
    /// Files that fail to load are included as errors; successfully loaded
    /// scripts are included as `Ok(LoadResult)`.
    pub fn load_scripts_dir(
        &mut self,
        dir: &Path,
        now: Instant,
    ) -> Vec<(String, Result<LoadResult, LoadError>)> {
        let mut results = Vec::new();

        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) => {
                results.push((
                    dir.display().to_string(),
                    Err(LoadError::Io {
                        filename: dir.display().to_string(),
                        message: e.to_string(),
                    }),
                ));
                return results;
            }
        };

        let mut paths: Vec<_> = entries
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("pirc") {
                    Some(path)
                } else {
                    None
                }
            })
            .collect();

        // Sort for deterministic load order
        paths.sort();

        for path in paths {
            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();
            let result = self.load_script_file(&path, now);
            results.push((filename, result));
        }

        results
    }

    /// Unloads a script, removing all its registered aliases, events, and timers.
    pub fn unload_script(&mut self, filename: &str) {
        if let Some(loaded) = self.scripts.remove(filename) {
            // Remove aliases
            for alias_name in &loaded.aliases {
                self.aliases.remove(alias_name);
            }

            // Remove event handlers tagged with this filename
            self.events.remove_by_source(filename);

            // Remove timers
            for timer_name in &loaded.timers {
                self.timers.remove(timer_name);
            }
        }
    }

    /// Reloads a script by unloading and re-loading from its stored source.
    ///
    /// # Errors
    ///
    /// Returns a [`LoadError`] if the script is not currently loaded or
    /// if re-parsing fails.
    pub fn reload_script(
        &mut self,
        filename: &str,
        now: Instant,
    ) -> Result<LoadResult, LoadError> {
        let source = self
            .scripts
            .get(filename)
            .map(|s| s.source.clone())
            .ok_or_else(|| LoadError::Io {
                filename: filename.to_string(),
                message: "script not loaded".to_string(),
            })?;

        self.load_script(&source, filename, now)
    }

    // ── Public API ────────────────────────────────────────────────────

    /// Populates built-in identifiers from the [`ScriptHost`] state.
    ///
    /// Called before each execution to ensure `$me`, `$server`, `$chan`, and
    /// `$port` reflect the current client state.
    fn sync_builtins_from_host(&mut self, host: &dyn ScriptHost) {
        let nick = host.current_nick();
        self.builtin_ctx
            .set("me", Value::String(nick.to_string()));

        if let Some(server) = host.current_server() {
            self.builtin_ctx
                .set("server", Value::String(server.to_string()));
        }
        if let Some(channel) = host.current_channel() {
            self.builtin_ctx
                .set("chan", Value::String(channel.to_string()));
        }

        let port = host.server_port();
        self.builtin_ctx
            .set("port", Value::Int(i64::from(port)));
    }

    /// Dispatches an event to all matching handlers.
    ///
    /// Runtime errors in handlers are reported via [`ScriptHost::report_error`]
    /// rather than being propagated.
    pub fn dispatch_event(
        &mut self,
        event_type: EventType,
        context: &EventContext,
        host: &mut dyn ScriptHost,
    ) {
        self.sync_builtins_from_host(host);

        let mut echo_output = Vec::new();

        let result = self.events.dispatch_full(
            event_type,
            context,
            &mut self.env,
            host,
            &self.functions,
            &self.regex_state,
            Some(&self.builtin_ctx),
            Some(&mut echo_output),
            Some(&self.aliases),
            Some(&mut self.timers),
        );

        // Flush echo output through the host
        for line in &echo_output {
            host.echo(line);
        }

        if let Err(e) = result {
            host.report_error(&ScriptRuntimeError {
                error: e,
                filename: None,
                context: "event handler".to_string(),
            });
        }
    }

    /// Executes a named alias with the given argument text.
    ///
    /// Returns `true` if the alias was found and executed.
    pub fn execute_alias(
        &mut self,
        name: &str,
        args: &str,
        host: &mut dyn ScriptHost,
    ) -> bool {
        let body = match self.aliases.get(name) {
            Some(body) => body.to_vec(),
            None => return false,
        };

        self.sync_builtins_from_host(host);

        // Create a builtin context with $0-$9 populated from arguments
        let mut alias_ctx = self.builtin_ctx.clone();
        alias_ctx.set_event_text(args);

        self.env.push_scope();

        let mut echo_output = Vec::new();
        let mut interp = Interpreter::with_context(
            &mut self.env,
            host,
            &alias_ctx,
            &self.functions,
            &self.regex_state,
        );
        interp.set_aliases(&self.aliases);
        interp.set_timer_manager(&mut self.timers);
        interp.set_echo_output(&mut echo_output);

        // Use exec_stmts; catch Return as normal completion
        let result = interp.exec_stmts(&body);

        self.env.pop_scope();

        // Flush echo output through the host
        for line in &echo_output {
            host.echo(line);
        }

        match result {
            Ok(_) | Err(RuntimeError::Return(_)) => {}
            Err(e) => host.report_error(&ScriptRuntimeError {
                error: e,
                filename: None,
                context: format!("alias '{name}'"),
            }),
        }

        true
    }

    /// Processes user input: checks for alias match first, then falls through
    /// to the command handler.
    ///
    /// Input is expected without a leading `/`. For example, `"greet alice"`
    /// would try alias `greet` with args `"alice"`.
    ///
    /// Returns `true` if the input was handled by an alias.
    pub fn execute_command(
        &mut self,
        input: &str,
        host: &mut dyn ScriptHost,
    ) -> bool {
        let input = input.trim();
        if input.is_empty() {
            return false;
        }

        let (name, args) = match input.split_once(char::is_whitespace) {
            Some((name, args)) => (name, args.trim()),
            None => (input, ""),
        };

        self.execute_alias(name, args, host)
    }

    /// Advances timers and fires any that are due.
    ///
    /// Runtime errors in timer bodies are reported via [`ScriptHost::report_error`].
    pub fn tick_timers(&mut self, now: Instant, host: &mut dyn ScriptHost) {
        self.sync_builtins_from_host(host);

        let fired = self.timers.tick(now);

        for timer in &fired {
            let mut echo_output = Vec::new();

            let result = {
                self.env.push_scope();

                let mut interp = Interpreter::with_context(
                    &mut self.env,
                    host,
                    &self.builtin_ctx,
                    &self.functions,
                    &self.regex_state,
                );
                interp.set_aliases(&self.aliases);
                interp.set_timer_manager(&mut self.timers);
                interp.set_echo_output(&mut echo_output);
                let result = interp.exec_stmts(&timer.body);

                self.env.pop_scope();

                match result {
                    Ok(_) | Err(RuntimeError::Return(_)) => Ok(()),
                    Err(e) => Err(e),
                }
            };

            // Flush echo output through the host
            for line in &echo_output {
                host.echo(line);
            }

            if let Err(e) = result {
                host.report_error(&ScriptRuntimeError {
                    error: e,
                    filename: None,
                    context: format!("timer '{}'", timer.name),
                });
            }
        }
    }

    /// Returns the names of all registered aliases.
    #[must_use]
    pub fn list_aliases(&self) -> Vec<String> {
        let mut names = self.aliases.alias_names();
        names.sort();
        names
    }

    /// Returns the names of all active timers.
    #[must_use]
    pub fn list_timers(&self) -> Vec<String> {
        let mut names = self.timers.timer_names();
        names.sort();
        names
    }

    /// Returns the filenames of all loaded scripts.
    #[must_use]
    pub fn list_scripts(&self) -> Vec<String> {
        let mut names: Vec<String> = self.scripts.keys().cloned().collect();
        names.sort();
        names
    }

    /// Sets a built-in identifier value.
    pub fn set_builtin(&mut self, name: &str, value: Value) {
        self.builtin_ctx.set(name, value);
    }

    /// Returns a reference to the alias registry.
    #[must_use]
    pub fn aliases(&self) -> &AliasRegistry {
        &self.aliases
    }

    /// Returns a reference to the event dispatcher.
    #[must_use]
    pub fn events(&self) -> &EventDispatcher {
        &self.events
    }

    /// Returns a reference to the timer manager.
    #[must_use]
    pub fn timers(&self) -> &TimerManager {
        &self.timers
    }

    /// Returns the number of loaded scripts.
    #[must_use]
    pub fn script_count(&self) -> usize {
        self.scripts.len()
    }

    // ── Internal helpers ──────────────────────────────────────────────

    /// Evaluates a constant expression (integer or number literal) to f64.
    /// Returns 0.0 for non-constant expressions.
    #[allow(clippy::cast_precision_loss)]
    fn eval_const_expr(expr: &crate::ast::Expression) -> f64 {
        match expr {
            crate::ast::Expression::IntLiteral { value, .. } => *value as f64,
            crate::ast::Expression::NumberLiteral { value, .. } => *value,
            _ => 0.0,
        }
    }
}

impl Default for ScriptEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
