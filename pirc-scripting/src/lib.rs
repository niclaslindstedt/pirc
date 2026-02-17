//! # pirc-scripting
//!
//! The scripting engine for the pirc IRC client. This crate implements an
//! mIRC-inspired domain-specific language for automating IRC interactions.
//!
//! Scripts are loaded from `~/.pirc/scripts/` and support:
//!
//! - **Aliases**: custom commands (`/greet`, `/calc`, etc.)
//! - **Event handlers**: react to JOIN, PART, TEXT, and other IRC events
//! - **Timers**: periodic execution of code blocks
//! - **Variables**: local (`%var`) and global (`%%var`) variables
//! - **Control flow**: `if`/`elseif`/`else`, `while` loops
//! - **Expressions**: arithmetic, comparison, logical, and string operations
//! - **String interpolation**: `"Hello $nick"` expands built-in identifiers
//!
//! See the [`grammar`] module for the complete EBNF grammar specification
//! and example scripts.

pub mod ast;
pub mod error;
pub mod grammar;
pub mod lexer;
pub mod token;
