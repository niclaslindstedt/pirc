//! Built-in identifier registry and context for the pirc interpreter.
//!
//! Provides context-dependent identifiers (`$nick`, `$chan`, etc.),
//! static constants (`$null`, `$true`, `$false`, `$cr`, `$lf`, `$crlf`, `$tab`),
//! and numeric parameters (`$0`–`$9`).

use std::collections::HashMap;

use super::value::Value;

/// Context for built-in identifiers populated per-event.
///
/// Event handlers receive a context that maps identifier names to values.
/// Static builtins (`$null`, `$true`, etc.) are resolved without context.
#[derive(Debug, Clone, Default)]
pub struct BuiltinContext {
    /// Context-dependent identifiers (e.g., `nick`, `chan`, `target`, `text`).
    identifiers: HashMap<String, Value>,
    /// The full event text line (`$0`).
    event_text: Option<String>,
}

impl BuiltinContext {
    /// Creates a new empty context.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }


    /// Sets a context identifier (without the `$` prefix).
    pub fn set(&mut self, name: &str, value: Value) {
        self.identifiers.insert(name.to_string(), value);
    }

    /// Sets the event text used for `$0`–`$9` parameter splitting.
    pub fn set_event_text(&mut self, text: &str) {
        self.event_text = Some(text.to_string());
        self.identifiers
            .insert("text".to_string(), Value::String(text.to_string()));
    }

    /// Resolves a built-in identifier name (without `$` prefix) to a value.
    ///
    /// Resolution order:
    /// 1. Static constants (`null`, `true`, `false`, `cr`, `lf`, `crlf`, `tab`)
    /// 2. Numeric parameters (`0`–`9`) from event text
    /// 3. Context-dependent identifiers
    /// 4. `Null` for unknown identifiers
    #[must_use]
    pub fn resolve(&self, name: &str) -> Value {
        // 1. Static constants
        if let Some(val) = resolve_static(name) {
            return val;
        }

        // 2. Numeric parameters ($0–$9)
        if let Some(val) = self.resolve_numeric_param(name) {
            return val;
        }

        // 3. Context-dependent identifiers
        if let Some(val) = self.identifiers.get(name) {
            return val.clone();
        }

        // 4. Unknown → Null
        Value::Null
    }

    /// Resolves `$0`–`$9` from the event text.
    ///
    /// `$0` = full event text line.
    /// `$1`–`$9` = space-separated tokens from the event text (1-based).
    fn resolve_numeric_param(&self, name: &str) -> Option<Value> {
        let n: usize = name.parse().ok()?;
        if n > 9 {
            return None;
        }

        let text = self.event_text.as_deref().unwrap_or("");

        if n == 0 {
            return Some(Value::String(text.to_string()));
        }

        let tokens: Vec<&str> = text.split_whitespace().collect();
        tokens
            .get(n - 1)
            .map_or(Some(Value::Null), |t| Some(Value::String((*t).to_string())))
    }
}

/// Resolves static built-in constants that don't depend on context.
fn resolve_static(name: &str) -> Option<Value> {
    match name {
        "null" => Some(Value::Null),
        "true" => Some(Value::Bool(true)),
        "false" => Some(Value::Bool(false)),
        "cr" => Some(Value::String("\r".to_string())),
        "lf" => Some(Value::String("\n".to_string())),
        "crlf" => Some(Value::String("\r\n".to_string())),
        "tab" => Some(Value::String("\t".to_string())),
        _ => None,
    }
}
