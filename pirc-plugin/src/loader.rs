//! Dynamic library loader for pirc plugins.
//!
//! [`PluginLoader`] loads `.dylib` (macOS) or `.so` (Linux) plugin libraries
//! at runtime using `libloading`, looks up the `pirc_plugin_init` entry point,
//! validates the returned [`PluginApi`](crate::ffi::PluginApi) vtable, and
//! produces a [`LoadedPlugin`] that keeps the library handle alive for the
//! lifetime of the plugin.

use std::fmt;
use std::path::{Path, PathBuf};

use libloading::{Library, Symbol};

use crate::ffi::{PluginApi, PluginEntryPoint, PLUGIN_ENTRY_SYMBOL};

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors that can occur when loading a plugin dynamic library.
#[derive(Debug)]
pub enum LoadError {
    /// The dynamic library could not be loaded (e.g. file not found,
    /// missing dependencies, invalid binary format).
    LibraryLoadError {
        path: PathBuf,
        source: libloading::Error,
    },
    /// The library was loaded but does not export the expected
    /// `pirc_plugin_init` symbol.
    SymbolNotFound {
        path: PathBuf,
        source: libloading::Error,
    },
    /// The entry point returned a null pointer instead of a valid
    /// [`PluginApi`] vtable.
    InvalidPlugin {
        path: PathBuf,
        reason: String,
    },
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LibraryLoadError { path, source } => {
                write!(f, "failed to load library {}: {source}", path.display())
            }
            Self::SymbolNotFound { path, source } => {
                write!(
                    f,
                    "symbol `{PLUGIN_ENTRY_SYMBOL}` not found in {}: {source}",
                    path.display()
                )
            }
            Self::InvalidPlugin { path, reason } => {
                write!(f, "invalid plugin {}: {reason}", path.display())
            }
        }
    }
}

impl std::error::Error for LoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::LibraryLoadError { source, .. } | Self::SymbolNotFound { source, .. } => {
                Some(source)
            }
            Self::InvalidPlugin { .. } => None,
        }
    }
}

// ---------------------------------------------------------------------------
// LoadedPlugin
// ---------------------------------------------------------------------------

/// A successfully loaded plugin library with its validated API vtable.
///
/// The [`Library`] handle is kept alive for the lifetime of this struct,
/// ensuring that the function pointers in [`PluginApi`] remain valid.
/// Dropping a `LoadedPlugin` unloads the dynamic library.
pub struct LoadedPlugin {
    /// The API vtable returned by the plugin's entry point.
    api: &'static PluginApi,
    /// Path to the loaded library (for reload tracking and diagnostics).
    path: PathBuf,
    /// The loaded dynamic library. Must be dropped last (after `api` is no
    /// longer used) to avoid use-after-free. Rust drops fields in
    /// declaration order, so `_library` being last is intentional.
    _library: Library,
}

impl LoadedPlugin {
    /// Returns the plugin's API vtable.
    pub fn api(&self) -> &PluginApi {
        self.api
    }

    /// Returns the file path of the loaded library.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the plugin name by calling the `info` / `free_info` cycle.
    ///
    /// # Safety
    ///
    /// The caller must ensure the plugin's `info` and `free_info` functions
    /// are safe to call (they should be if the plugin was loaded via
    /// [`PluginLoader::load`]).
    #[allow(unsafe_code)]
    pub unsafe fn plugin_name(&self) -> String {
        let info = (self.api.info)();
        let name = unsafe { info.name.as_str() }.to_owned();
        (self.api.free_info)(info);
        name
    }

    /// Returns the plugin version by calling the `info` / `free_info` cycle.
    ///
    /// # Safety
    ///
    /// Same safety requirements as [`plugin_name`](Self::plugin_name).
    #[allow(unsafe_code)]
    pub unsafe fn plugin_version(&self) -> String {
        let info = (self.api.info)();
        let version = unsafe { info.version.as_str() }.to_owned();
        (self.api.free_info)(info);
        version
    }
}

impl fmt::Debug for LoadedPlugin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoadedPlugin")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// PluginLoader
// ---------------------------------------------------------------------------

/// Loads plugin dynamic libraries from the filesystem.
///
/// The loader handles the platform-specific library extension (`.dylib` on
/// macOS, `.so` on Linux) and validates that the loaded library exports
/// a conforming `pirc_plugin_init` entry point.
#[derive(Debug, Default)]
pub struct PluginLoader;

impl PluginLoader {
    /// Creates a new `PluginLoader`.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Returns the platform-appropriate dynamic library file extension.
    #[must_use]
    pub fn library_extension() -> &'static str {
        if cfg!(target_os = "macos") {
            "dylib"
        } else {
            "so"
        }
    }

    /// Loads a plugin from the given library path.
    ///
    /// This function:
    /// 1. Opens the dynamic library at `path`
    /// 2. Looks up the `pirc_plugin_init` symbol
    /// 3. Calls the entry point to obtain a [`PluginApi`] pointer
    /// 4. Validates the returned vtable (non-null pointer, non-null
    ///    function pointer fields)
    /// 5. Returns a [`LoadedPlugin`] that keeps the library alive
    ///
    /// # Errors
    ///
    /// Returns [`LoadError`] if the library cannot be loaded, the symbol
    /// is missing, or the plugin API is invalid.
    ///
    /// # Safety
    ///
    /// Loading a dynamic library is inherently unsafe: the library may
    /// contain arbitrary code that runs during load (e.g. constructors).
    /// Only load libraries from trusted sources.
    #[allow(unsafe_code)]
    pub fn load<P: AsRef<Path>>(&self, path: P) -> Result<LoadedPlugin, LoadError> {
        let path = path.as_ref().to_path_buf();

        // 1. Load the dynamic library.
        let library = unsafe { Library::new(&path) }.map_err(|e| LoadError::LibraryLoadError {
            path: path.clone(),
            source: e,
        })?;

        // 2. Look up the entry point symbol.
        let entry_fn: Symbol<'_, PluginEntryPoint> =
            unsafe { library.get(PLUGIN_ENTRY_SYMBOL.as_bytes()) }.map_err(|e| {
                LoadError::SymbolNotFound {
                    path: path.clone(),
                    source: e,
                }
            })?;

        // 3. Call the entry point to get the PluginApi pointer.
        let api_ptr = entry_fn();

        if api_ptr.is_null() {
            return Err(LoadError::InvalidPlugin {
                path,
                reason: "entry point returned null".into(),
            });
        }

        // SAFETY: The pointer is non-null and points to a static PluginApi
        // inside the loaded library. The library remains loaded for the
        // lifetime of LoadedPlugin, so the reference is valid.
        let api: &'static PluginApi = unsafe { &*api_ptr };

        Ok(LoadedPlugin {
            api,
            path,
            _library: library,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn library_extension_is_platform_appropriate() {
        let ext = PluginLoader::library_extension();
        if cfg!(target_os = "macos") {
            assert_eq!(ext, "dylib");
        } else {
            assert_eq!(ext, "so");
        }
    }

    #[test]
    fn load_nonexistent_file_returns_library_load_error() {
        let loader = PluginLoader::new();
        let result = loader.load("/tmp/nonexistent_plugin.dylib");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, LoadError::LibraryLoadError { .. }),
            "expected LibraryLoadError, got: {err}"
        );
        // Verify Display impl works
        let msg = err.to_string();
        assert!(msg.contains("failed to load library"));
        assert!(msg.contains("nonexistent_plugin"));
    }

    #[test]
    fn load_non_plugin_library_returns_symbol_not_found() {
        // Load a system library that definitely does NOT have pirc_plugin_init.
        // On macOS: libSystem.B.dylib; on Linux: libc.so.6
        let system_lib = if cfg!(target_os = "macos") {
            "/usr/lib/libSystem.B.dylib"
        } else {
            // On Linux, libc.so.6 is typically in /lib or /lib64
            "/lib/x86_64-linux-gnu/libc.so.6"
        };

        // Only run this test if the system library exists
        if !PathBuf::from(system_lib).exists() {
            return;
        }

        let loader = PluginLoader::new();
        let result = loader.load(system_lib);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, LoadError::SymbolNotFound { .. }),
            "expected SymbolNotFound, got: {err}"
        );
        let msg = err.to_string();
        assert!(msg.contains(PLUGIN_ENTRY_SYMBOL));
    }

    #[test]
    fn load_error_display_library_load_error() {
        let err = LoadError::LibraryLoadError {
            path: PathBuf::from("/some/path.dylib"),
            source: libloading::Error::DlOpenUnknown,
        };
        let msg = err.to_string();
        assert!(msg.contains("/some/path.dylib"));
        assert!(msg.contains("failed to load library"));
    }

    #[test]
    fn load_error_display_symbol_not_found() {
        let err = LoadError::SymbolNotFound {
            path: PathBuf::from("/some/path.dylib"),
            source: libloading::Error::DlOpenUnknown,
        };
        let msg = err.to_string();
        assert!(msg.contains(PLUGIN_ENTRY_SYMBOL));
        assert!(msg.contains("/some/path.dylib"));
    }

    #[test]
    fn load_error_display_invalid_plugin() {
        let err = LoadError::InvalidPlugin {
            path: PathBuf::from("/some/path.dylib"),
            reason: "null pointer".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("invalid plugin"));
        assert!(msg.contains("null pointer"));
    }

    #[test]
    fn load_error_source_chain() {
        use std::error::Error;

        let lib_err = LoadError::LibraryLoadError {
            path: PathBuf::from("test"),
            source: libloading::Error::DlOpenUnknown,
        };
        assert!(lib_err.source().is_some());

        let sym_err = LoadError::SymbolNotFound {
            path: PathBuf::from("test"),
            source: libloading::Error::DlOpenUnknown,
        };
        assert!(sym_err.source().is_some());

        let invalid_err = LoadError::InvalidPlugin {
            path: PathBuf::from("test"),
            reason: "bad".into(),
        };
        assert!(invalid_err.source().is_none());
    }

    #[test]
    fn plugin_loader_default() {
        let loader = PluginLoader;
        // Just verify it can be constructed via Default
        let _ = format!("{loader:?}");
    }
}
