//! Built-in function implementations for the pirc scripting language.
//!
//! Functions are called from `$name(args)` expressions in scripts.
//! Each function receives evaluated [`Value`] arguments and returns a [`Value`].

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use regex::Regex;

use super::RuntimeError;
use super::value::Value;

/// Type alias for a built-in function implementation.
///
/// All functions share this signature even if some can't fail,
/// for uniform dispatch from the registry.
type BuiltinFn = fn(&[Value], &RegexState) -> Result<Value, RuntimeError>;

/// Stores the capture groups from the most recent `$regex` call.
#[derive(Debug, Clone, Default)]
pub struct RegexState {
    /// Capture groups from the last `$regex()` call.
    /// Index 0 is the full match, 1+ are capture groups.
    captures: Arc<Mutex<Vec<String>>>,
}

impl RegexState {
    /// Creates a new empty regex state.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores capture groups from a regex match.
    fn set_captures(&self, captures: Vec<String>) {
        if let Ok(mut state) = self.captures.lock() {
            *state = captures;
        }
    }

    /// Gets a capture group by index (0 = full match, 1+ = groups).
    fn get_capture(&self, index: usize) -> Option<String> {
        self.captures
            .lock()
            .ok()
            .and_then(|state| state.get(index).cloned())
    }
}

/// Registry of built-in functions.
pub struct FunctionRegistry {
    functions: HashMap<&'static str, (BuiltinFn, Arity)>,
}

/// Arity specification for a function.
#[derive(Debug, Clone, Copy)]
enum Arity {
    /// Exact number of arguments.
    Exact(usize),
}

impl Default for FunctionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl FunctionRegistry {
    /// Creates a new registry with all built-in functions registered.
    #[must_use]
    pub fn new() -> Self {
        let mut functions = HashMap::new();

        // Text manipulation
        functions.insert("len", (fn_len as BuiltinFn, Arity::Exact(1)));
        functions.insert("left", (fn_left as BuiltinFn, Arity::Exact(2)));
        functions.insert("right", (fn_right as BuiltinFn, Arity::Exact(2)));
        functions.insert("mid", (fn_mid as BuiltinFn, Arity::Exact(3)));
        functions.insert("upper", (fn_upper as BuiltinFn, Arity::Exact(1)));
        functions.insert("lower", (fn_lower as BuiltinFn, Arity::Exact(1)));
        functions.insert("replace", (fn_replace as BuiltinFn, Arity::Exact(3)));
        functions.insert("find", (fn_find as BuiltinFn, Arity::Exact(2)));
        functions.insert("token", (fn_token as BuiltinFn, Arity::Exact(3)));
        functions.insert("numtok", (fn_numtok as BuiltinFn, Arity::Exact(2)));
        functions.insert("strip", (fn_strip as BuiltinFn, Arity::Exact(1)));
        functions.insert("chr", (fn_chr as BuiltinFn, Arity::Exact(1)));
        functions.insert("asc", (fn_asc as BuiltinFn, Arity::Exact(1)));

        // Regex
        functions.insert("regex", (fn_regex as BuiltinFn, Arity::Exact(2)));
        functions.insert("regml", (fn_regml as BuiltinFn, Arity::Exact(1)));

        Self { functions }
    }

    /// Calls a built-in function by name with the given arguments.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::UnknownFunction`] if the function is not found,
    /// or [`RuntimeError::TypeError`] if the wrong number of arguments is provided.
    pub fn call(
        &self,
        name: &str,
        args: &[Value],
        regex_state: &RegexState,
    ) -> Result<Value, RuntimeError> {
        let (func, arity) = self
            .functions
            .get(name)
            .ok_or_else(|| RuntimeError::UnknownFunction(name.to_string()))?;

        match arity {
            Arity::Exact(n) => {
                if args.len() != *n {
                    return Err(RuntimeError::TypeError(format!(
                        "${name}: expected {n} argument{}, got {}",
                        if *n == 1 { "" } else { "s" },
                        args.len()
                    )));
                }
            }
        }

        func(args, regex_state)
    }
}

// ── Text manipulation functions ─────────────────────────────────────────
//
// All functions share the `BuiltinFn` signature for uniform dispatch.
// Functions that cannot fail still return `Result` for this reason.

/// `$len(str)` — string length.
#[allow(clippy::unnecessary_wraps)]
fn fn_len(args: &[Value], _: &RegexState) -> Result<Value, RuntimeError> {
    let s = args[0].to_string();
    #[allow(clippy::cast_possible_wrap)]
    Ok(Value::Int(s.len() as i64))
}

/// `$left(str, n)` — first n characters.
fn fn_left(args: &[Value], _: &RegexState) -> Result<Value, RuntimeError> {
    let s = args[0].to_string();
    let n = to_usize(&args[1], "left")?;
    let result: String = s.chars().take(n).collect();
    Ok(Value::String(result))
}

/// `$right(str, n)` — last n characters.
fn fn_right(args: &[Value], _: &RegexState) -> Result<Value, RuntimeError> {
    let s = args[0].to_string();
    let n = to_usize(&args[1], "right")?;
    let chars: Vec<char> = s.chars().collect();
    let start = chars.len().saturating_sub(n);
    let result: String = chars[start..].iter().collect();
    Ok(Value::String(result))
}

/// `$mid(str, start, len)` — substring from start position with given length.
fn fn_mid(args: &[Value], _: &RegexState) -> Result<Value, RuntimeError> {
    let s = args[0].to_string();
    let start = to_usize(&args[1], "mid")?;
    let len = to_usize(&args[2], "mid")?;
    let result: String = s.chars().skip(start).take(len).collect();
    Ok(Value::String(result))
}

/// `$upper(str)` — convert to uppercase.
#[allow(clippy::unnecessary_wraps)]
fn fn_upper(args: &[Value], _: &RegexState) -> Result<Value, RuntimeError> {
    Ok(Value::String(args[0].to_string().to_uppercase()))
}

/// `$lower(str)` — convert to lowercase.
#[allow(clippy::unnecessary_wraps)]
fn fn_lower(args: &[Value], _: &RegexState) -> Result<Value, RuntimeError> {
    Ok(Value::String(args[0].to_string().to_lowercase()))
}

/// `$replace(str, find, replacement)` — replace all occurrences.
#[allow(clippy::unnecessary_wraps)]
fn fn_replace(args: &[Value], _: &RegexState) -> Result<Value, RuntimeError> {
    let s = args[0].to_string();
    let find = args[1].to_string();
    let replacement = args[2].to_string();
    Ok(Value::String(s.replace(&find, &replacement)))
}

/// `$find(str, substr)` — find index of substring (-1 if not found).
#[allow(clippy::unnecessary_wraps)]
fn fn_find(args: &[Value], _: &RegexState) -> Result<Value, RuntimeError> {
    let s = args[0].to_string();
    let substr = args[1].to_string();
    #[allow(clippy::cast_possible_wrap)]
    let index = s.find(&substr).map_or(-1_i64, |i| i as i64);
    Ok(Value::Int(index))
}

/// `$token(str, n, delimiter)` — get nth token (1-based).
fn fn_token(args: &[Value], _: &RegexState) -> Result<Value, RuntimeError> {
    let s = args[0].to_string();
    let n = to_i64(&args[1], "token")?;
    let delim = args[2].to_string();

    if n < 1 {
        return Ok(Value::Null);
    }

    let index = to_usize_checked(n - 1, "token")?;
    let delim_str = if delim.is_empty() { " " } else { &delim };
    let tokens: Vec<&str> = s.split(delim_str).collect();
    Ok(tokens
        .get(index)
        .map_or(Value::Null, |t| Value::String((*t).to_string())))
}

/// `$numtok(str, delimiter)` — count tokens.
#[allow(clippy::unnecessary_wraps)]
fn fn_numtok(args: &[Value], _: &RegexState) -> Result<Value, RuntimeError> {
    let s = args[0].to_string();
    let delim = args[1].to_string();
    let delim_str = if delim.is_empty() { " " } else { &delim };
    if s.is_empty() {
        return Ok(Value::Int(0));
    }
    #[allow(clippy::cast_possible_wrap)]
    let count = s.split(delim_str).count() as i64;
    Ok(Value::Int(count))
}

/// `$strip(str)` — trim whitespace from both ends.
#[allow(clippy::unnecessary_wraps)]
fn fn_strip(args: &[Value], _: &RegexState) -> Result<Value, RuntimeError> {
    Ok(Value::String(args[0].to_string().trim().to_string()))
}

/// `$chr(n)` — character from Unicode code point.
fn fn_chr(args: &[Value], _: &RegexState) -> Result<Value, RuntimeError> {
    let n = to_i64(&args[0], "chr")?;
    if n < 0 {
        return Err(RuntimeError::TypeError(
            "$chr: code point must be non-negative".to_string(),
        ));
    }
    let code = u32::try_from(n).map_err(|_| {
        RuntimeError::TypeError(format!("$chr: code point too large: {n}"))
    })?;
    let ch = char::from_u32(code).ok_or_else(|| {
        RuntimeError::TypeError(format!("$chr: invalid code point {n}"))
    })?;
    Ok(Value::String(ch.to_string()))
}

/// `$asc(char)` — Unicode code point from first character.
fn fn_asc(args: &[Value], _: &RegexState) -> Result<Value, RuntimeError> {
    let s = args[0].to_string();
    let ch = s.chars().next().ok_or_else(|| {
        RuntimeError::TypeError("$asc: empty string".to_string())
    })?;
    Ok(Value::Int(i64::from(u32::from(ch))))
}

// ── Regex functions ─────────────────────────────────────────────────────

/// `$regex(str, pattern)` — returns 1 if pattern matches, 0 if not.
/// Stores capture groups in the regex state for `$regml`.
fn fn_regex(args: &[Value], regex_state: &RegexState) -> Result<Value, RuntimeError> {
    let s = args[0].to_string();
    let pattern = args[1].to_string();

    let re = Regex::new(&pattern).map_err(|e| {
        RuntimeError::TypeError(format!("$regex: invalid pattern: {e}"))
    })?;

    if let Some(caps) = re.captures(&s) {
        let mut groups = Vec::new();
        for i in 0..caps.len() {
            groups.push(
                caps.get(i)
                    .map_or_else(String::new, |m| m.as_str().to_string()),
            );
        }
        regex_state.set_captures(groups);
        Ok(Value::Int(1))
    } else {
        regex_state.set_captures(Vec::new());
        Ok(Value::Int(0))
    }
}

/// `$regml(n)` — return nth capture group from last `$regex` call.
/// 0 = full match, 1+ = capture groups.
fn fn_regml(args: &[Value], regex_state: &RegexState) -> Result<Value, RuntimeError> {
    let n = to_i64(&args[0], "regml")?;
    if n < 0 {
        return Err(RuntimeError::TypeError(
            "$regml: index must be non-negative".to_string(),
        ));
    }
    let index = to_usize_checked(n, "regml")?;
    Ok(regex_state
        .get_capture(index)
        .map_or(Value::Null, Value::String))
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Coerces a [`Value`] to a non-negative [`usize`] for use as a string index.
fn to_usize(val: &Value, fn_name: &str) -> Result<usize, RuntimeError> {
    let n = to_i64(val, fn_name)?;
    if n < 0 {
        return Err(RuntimeError::TypeError(format!(
            "${fn_name}: argument must be non-negative, got {n}"
        )));
    }
    to_usize_checked(n, fn_name)
}

/// Converts a non-negative `i64` to `usize` safely.
fn to_usize_checked(n: i64, fn_name: &str) -> Result<usize, RuntimeError> {
    usize::try_from(n).map_err(|_| {
        RuntimeError::TypeError(format!("${fn_name}: value {n} out of range"))
    })
}

/// Coerces a [`Value`] to an `i64`.
fn to_i64(val: &Value, fn_name: &str) -> Result<i64, RuntimeError> {
    match val {
        Value::Int(n) => Ok(*n),
        Value::Number(n) => {
            #[allow(clippy::cast_possible_truncation)]
            Ok(*n as i64)
        }
        Value::String(s) => s.parse::<i64>().map_err(|_| {
            RuntimeError::TypeError(format!("${fn_name}: expected number, got string \"{s}\""))
        }),
        _ => Err(RuntimeError::TypeError(format!(
            "${fn_name}: expected number, got {}",
            val.type_name()
        ))),
    }
}
