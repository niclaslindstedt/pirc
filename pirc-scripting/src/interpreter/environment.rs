//! Variable environment with lexical scoping for the pirc interpreter.

use std::collections::HashMap;

use super::value::Value;

/// Variable environment with local scope stack and global storage.
///
/// Local variables use a scope stack — `push_scope` creates a new scope and
/// `pop_scope` removes it. Variable lookup walks the stack from innermost
/// scope outward. Global variables (`%%name`) live in a separate flat map.
#[derive(Debug, Clone)]
pub struct Environment {
    /// Stack of local variable scopes (innermost last).
    local_scopes: Vec<HashMap<String, Value>>,
    /// Global variable storage.
    globals: HashMap<String, Value>,
}

impl Default for Environment {
    fn default() -> Self {
        Self::new()
    }
}

impl Environment {
    /// Creates a new environment with one empty local scope.
    #[must_use]
    pub fn new() -> Self {
        Self {
            local_scopes: vec![HashMap::new()],
            globals: HashMap::new(),
        }
    }

    /// Pushes a new local scope onto the stack.
    pub fn push_scope(&mut self) {
        self.local_scopes.push(HashMap::new());
    }

    /// Pops the innermost local scope.
    ///
    /// # Panics
    ///
    /// Panics if there is only one scope left (the root scope).
    pub fn pop_scope(&mut self) {
        assert!(
            self.local_scopes.len() > 1,
            "cannot pop the root scope"
        );
        self.local_scopes.pop();
    }

    /// Looks up a local variable by searching scopes from innermost to
    /// outermost. Returns `None` if not found.
    #[must_use]
    pub fn get_local(&self, name: &str) -> Option<Value> {
        for scope in self.local_scopes.iter().rev() {
            if let Some(val) = scope.get(name) {
                return Some(val.clone());
            }
        }
        None
    }

    /// Sets a local variable in the current (innermost) scope.
    pub fn set_local(&mut self, name: &str, value: Value) {
        if let Some(scope) = self.local_scopes.last_mut() {
            scope.insert(name.to_string(), value);
        }
    }

    /// Updates an existing local variable in the scope where it was defined.
    /// If the variable is not found in any scope, sets it in the current scope.
    pub fn update_local(&mut self, name: &str, value: Value) {
        for scope in self.local_scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.to_string(), value);
                return;
            }
        }
        // Fallback: set in current scope
        self.set_local(name, value);
    }

    /// Gets a global variable. Returns `None` if not set.
    #[must_use]
    pub fn get_global(&self, name: &str) -> Option<Value> {
        self.globals.get(name).cloned()
    }

    /// Sets a global variable.
    pub fn set_global(&mut self, name: &str, value: Value) {
        self.globals.insert(name.to_string(), value);
    }
}
