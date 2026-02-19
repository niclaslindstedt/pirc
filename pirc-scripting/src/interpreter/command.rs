//! Alias registry and built-in command definitions.
//!
//! Provides [`AliasRegistry`] for storing alias definitions with
//! case-insensitive lookup. Built-in command handling and alias execution
//! are integrated into the [`Interpreter`](super::Interpreter).

use std::collections::HashMap;

use crate::ast::{AliasDefinition, Statement};

/// Stores alias definitions for case-insensitive lookup.
///
/// Aliases are registered from parsed [`AliasDefinition`] AST nodes and can be
/// invoked by name. Names are stored lowercased for case-insensitive matching.
#[derive(Debug, Clone, Default)]
pub struct AliasRegistry {
    /// Alias bodies indexed by lowercased name.
    aliases: HashMap<String, Vec<Statement>>,
}

impl AliasRegistry {
    /// Creates a new empty alias registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an alias from a parsed [`AliasDefinition`] AST node.
    ///
    /// If an alias with the same name already exists, it is replaced.
    pub fn register(&mut self, alias: &AliasDefinition) {
        self.aliases
            .insert(alias.name.to_lowercase(), alias.body.clone());
    }

    /// Looks up an alias body by name (case-insensitive).
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&[Statement]> {
        self.aliases.get(&name.to_lowercase()).map(Vec::as_slice)
    }

    /// Returns the number of registered aliases.
    #[must_use]
    pub fn len(&self) -> usize {
        self.aliases.len()
    }

    /// Returns `true` if no aliases are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.aliases.is_empty()
    }

    /// Returns `true` if an alias with the given name exists (case-insensitive).
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.aliases.contains_key(&name.to_lowercase())
    }

    /// Removes an alias by name (case-insensitive).
    ///
    /// Returns `true` if an alias was removed.
    pub fn remove(&mut self, name: &str) -> bool {
        self.aliases.remove(&name.to_lowercase()).is_some()
    }

    /// Returns the names of all registered aliases (lowercased).
    #[must_use]
    pub fn alias_names(&self) -> Vec<String> {
        self.aliases.keys().cloned().collect()
    }
}
