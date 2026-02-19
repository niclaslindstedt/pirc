//! C FFI ABI types and entry points for the plugin system.
//!
//! All types in this module use `#[repr(C)]` to ensure a stable ABI across
//! dynamic library boundaries. Plugins implement the [`PluginApi`] vtable
//! and receive a [`PluginHostApi`] vtable from the host for callbacks.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::ptr;

// ---------------------------------------------------------------------------
// FfiString — safe C string passing across the FFI boundary
// ---------------------------------------------------------------------------

/// A C-compatible string for passing text across the FFI boundary.
///
/// The string is owned by whichever side allocated it.  Call [`FfiString::free`]
/// to release memory when the receiver is done with it.
#[repr(C)]
pub struct FfiString {
    /// Pointer to a null-terminated UTF-8 C string.
    pub ptr: *mut c_char,
    /// Length of the string in bytes (not including the null terminator).
    pub len: usize,
}

impl FfiString {
    /// Creates an [`FfiString`] from a Rust `&str`.
    ///
    /// # Panics
    ///
    /// Panics if the string contains interior null bytes.
    #[must_use]
    pub fn new(s: &str) -> Self {
        let cstring = CString::new(s).expect("string must not contain interior null bytes");
        let len = cstring.as_bytes().len();
        let ptr = cstring.into_raw();
        Self { ptr, len }
    }

    /// Converts the [`FfiString`] back into a Rust [`String`], consuming it.
    ///
    /// # Safety
    ///
    /// The `ptr` must have been allocated by [`FfiString::new`] (i.e. via
    /// [`CString::into_raw`]) and must not have been freed yet.
    #[allow(unsafe_code)]
    pub unsafe fn into_string(self) -> String {
        if self.ptr.is_null() {
            return String::new();
        }
        let cstring = unsafe { CString::from_raw(self.ptr) };
        cstring.into_string().unwrap_or_default()
    }

    /// Returns the string as a `&str` without consuming it.
    ///
    /// # Safety
    ///
    /// The `ptr` must point to a valid, null-terminated C string that was
    /// allocated by [`FfiString::new`].
    #[allow(unsafe_code)]
    pub unsafe fn as_str(&self) -> &str {
        if self.ptr.is_null() {
            return "";
        }
        let cstr = unsafe { CStr::from_ptr(self.ptr) };
        cstr.to_str().unwrap_or("")
    }

    /// Frees the memory backing this [`FfiString`].
    ///
    /// # Safety
    ///
    /// The `ptr` must have been allocated by [`FfiString::new`] and must
    /// not have been freed already.
    #[allow(unsafe_code)]
    pub unsafe fn free(self) {
        if !self.ptr.is_null() {
            drop(unsafe { CString::from_raw(self.ptr) });
        }
    }

    /// Creates an empty [`FfiString`] with a null pointer.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            ptr: ptr::null_mut(),
            len: 0,
        }
    }

    /// Returns `true` if the string pointer is null (i.e. the string is empty).
    #[must_use]
    pub fn is_null(&self) -> bool {
        self.ptr.is_null()
    }
}

// ---------------------------------------------------------------------------
// PluginStatus / PluginResult
// ---------------------------------------------------------------------------

/// Status code for plugin operations.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginStatus {
    /// The operation succeeded.
    Ok = 0,
    /// The operation failed.
    Error = 1,
}

/// Result type returned by plugin lifecycle functions.
///
/// A plain `#[repr(C)]` struct with explicit ownership semantics:
/// - When `status` is [`PluginStatus::Ok`], `error_message` must be
///   [`FfiString::empty()`] (null pointer).
/// - When `status` is [`PluginStatus::Error`], the caller **owns**
///   `error_message` and must free it via [`FfiString::free`].
#[repr(C)]
pub struct PluginResult {
    /// Whether the operation succeeded or failed.
    pub status: PluginStatus,
    /// Error message when `status` is [`PluginStatus::Error`].
    /// Must be [`FfiString::empty()`] when `status` is [`PluginStatus::Ok`].
    /// The caller owns this string and must call [`FfiString::free`] on it.
    pub error_message: FfiString,
}

impl PluginResult {
    /// Creates a successful result.
    #[must_use]
    pub fn ok() -> Self {
        Self {
            status: PluginStatus::Ok,
            error_message: FfiString::empty(),
        }
    }

    /// Creates an error result with the given message.
    ///
    /// # Panics
    ///
    /// Panics if `msg` contains interior null bytes.
    #[must_use]
    pub fn error(msg: &str) -> Self {
        Self {
            status: PluginStatus::Error,
            error_message: FfiString::new(msg),
        }
    }

    /// Returns `true` if the result represents success.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.status == PluginStatus::Ok
    }

    /// Returns `true` if the result represents an error.
    #[must_use]
    pub fn is_error(&self) -> bool {
        self.status == PluginStatus::Error
    }
}

// ---------------------------------------------------------------------------
// PluginCapability — sandboxing flags
// ---------------------------------------------------------------------------

/// Capabilities a plugin may request. Used for sandboxing: the host checks
/// that a plugin only uses APIs matching its declared capabilities.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluginCapability {
    /// Plugin may read its own configuration values.
    ReadConfig,
    /// Plugin may register and unregister commands.
    RegisterCommands,
    /// Plugin may subscribe to and unsubscribe from events.
    HookEvents,
    /// Plugin may send messages to channels and users.
    SendMessages,
    /// Plugin may make outbound network requests.
    AccessNetwork,
}

// ---------------------------------------------------------------------------
// PluginInfo
// ---------------------------------------------------------------------------

/// Metadata about a plugin, returned by the plugin's `info` entry point.
///
/// The `capabilities` and `capabilities_len` fields together form a borrowed
/// slice of [`PluginCapability`] values. The host must call
/// [`PluginApi::free_info`] when it is done reading the info to allow the
/// plugin to release any allocated memory (including the `FfiString` fields
/// and the capabilities array if it was heap-allocated).
#[repr(C)]
pub struct PluginInfo {
    /// Human-readable plugin name.
    pub name: FfiString,
    /// Semantic version string (e.g. "1.0.0").
    pub version: FfiString,
    /// Short description of what the plugin does.
    pub description: FfiString,
    /// Author name or identifier.
    pub author: FfiString,
    /// Pointer to an array of requested [`PluginCapability`] values.
    ///
    /// # Lifetime contract
    ///
    /// This pointer must remain valid until the host calls
    /// [`PluginApi::free_info`]. The plugin may point this at a `'static`
    /// array or a heap allocation — the host must not assume either.
    /// Together with `capabilities_len`, this forms a borrowed slice:
    /// the host may read `capabilities_len` elements starting at this
    /// pointer, but must not write to or free the memory directly.
    pub capabilities: *const PluginCapability,
    /// Number of elements in the `capabilities` array.
    pub capabilities_len: usize,
}

// ---------------------------------------------------------------------------
// PluginEventType / PluginEvent
// ---------------------------------------------------------------------------

/// Types of events the host can deliver to plugins.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluginEventType {
    /// A message was received in a channel or query.
    MessageReceived,
    /// A user joined a channel.
    UserJoined,
    /// A user parted (left) a channel.
    UserParted,
    /// A user quit the network.
    UserQuit,
    /// A user changed their nick.
    NickChanged,
    /// The client connected to a server.
    Connected,
    /// The client disconnected from a server.
    Disconnected,
    /// A command was executed (e.g. `/foo`).
    CommandExecuted,
}

/// An event delivered from the host to a plugin.
#[repr(C)]
pub struct PluginEvent {
    /// What kind of event this is.
    pub event_type: PluginEventType,
    /// Primary payload (e.g. the message text, the nick, the channel name).
    pub data: FfiString,
    /// Secondary payload (e.g. the channel for a join, the new nick).
    pub source: FfiString,
}

// ---------------------------------------------------------------------------
// PluginApi — vtable implemented by the plugin
// ---------------------------------------------------------------------------

/// The vtable a plugin exposes to the host.
///
/// A plugin dynamic library must export a function:
/// ```c
/// const PluginApi* pirc_plugin_init(void);
/// ```
/// that returns a pointer to a static `PluginApi` instance.
#[repr(C)]
pub struct PluginApi {
    /// Returns metadata about the plugin.
    ///
    /// The host must call [`free_info`](Self::free_info) when it is done
    /// reading the returned [`PluginInfo`].
    pub info: extern "C" fn() -> PluginInfo,

    /// Frees a [`PluginInfo`] previously returned by [`info`](Self::info).
    ///
    /// The plugin is responsible for releasing any memory it allocated for
    /// the `FfiString` fields and the capabilities array. The host must
    /// call this exactly once for each [`PluginInfo`] obtained from `info`.
    pub free_info: extern "C" fn(info: PluginInfo),

    /// Initialises the plugin.  Called once after loading.
    /// The host passes its own callback vtable so the plugin can call back.
    pub init: extern "C" fn(host: *const PluginHostApi) -> PluginResult,

    /// Called when the plugin is enabled (after init or after a disable/enable cycle).
    pub on_enable: extern "C" fn() -> PluginResult,

    /// Called when the plugin is disabled but not yet unloaded.
    pub on_disable: extern "C" fn() -> PluginResult,

    /// Called when the plugin is about to be unloaded.  Clean-up happens here.
    pub shutdown: extern "C" fn() -> PluginResult,

    /// Called when an event the plugin subscribed to is fired.
    pub on_event: extern "C" fn(event: *const PluginEvent) -> PluginResult,
}

// ---------------------------------------------------------------------------
// PluginHostApi — vtable provided by the host to plugins
// ---------------------------------------------------------------------------

/// The callback vtable the host provides to each plugin at init time.
///
/// Plugins call these functions to interact with the IRC client.
#[repr(C)]
pub struct PluginHostApi {
    /// Register a new command (e.g. `/myplugin`).
    /// `name` is the command name (without the leading `/`).
    /// `callback` is called when the user types the command.
    pub register_command: extern "C" fn(
        name: FfiString,
        callback: extern "C" fn(args: FfiString) -> PluginResult,
    ) -> PluginResult,

    /// Unregister a previously registered command.
    pub unregister_command: extern "C" fn(name: FfiString) -> PluginResult,

    /// Subscribe to an event type.
    pub hook_event: extern "C" fn(event_type: PluginEventType) -> PluginResult,

    /// Unsubscribe from an event type.
    pub unhook_event: extern "C" fn(event_type: PluginEventType) -> PluginResult,

    /// Print a message to the user's active window.
    pub echo: extern "C" fn(message: FfiString),

    /// Write a log message at the given level (0=error, 1=warn, 2=info, 3=debug).
    pub log: extern "C" fn(level: u32, message: FfiString),

    /// Look up a config value by key. Returns an empty [`FfiString`] if not found.
    pub get_config_value: extern "C" fn(key: FfiString) -> FfiString,
}

// ---------------------------------------------------------------------------
// Entry point type alias
// ---------------------------------------------------------------------------

/// Signature of the entry point a plugin dynamic library must export.
///
/// ```c
/// const PluginApi* pirc_plugin_init(void);
/// ```
pub type PluginEntryPoint = extern "C" fn() -> *const PluginApi;

/// The symbol name the host looks for when loading a plugin library.
pub const PLUGIN_ENTRY_SYMBOL: &str = "pirc_plugin_init";

// ---------------------------------------------------------------------------
// Send/Sync safety for FFI pointer types
// ---------------------------------------------------------------------------

// SAFETY: The raw pointers in these types are only accessed on a single thread
// at a time, and the host ensures proper synchronisation. The FFI contract
// guarantees that pointer targets remain valid for the lifetime of the struct.
#[allow(unsafe_code)]
unsafe impl Send for FfiString {}
#[allow(unsafe_code)]
unsafe impl Send for PluginInfo {}
#[allow(unsafe_code)]
unsafe impl Send for PluginEvent {}
#[allow(unsafe_code)]
unsafe impl Send for PluginResult {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem;

    #[test]
    fn ffi_string_round_trip() {
        let original = "Hello, plugin world!";
        let ffi = FfiString::new(original);
        assert!(!ffi.is_null());
        assert_eq!(ffi.len, original.len());

        #[allow(unsafe_code)]
        let recovered = unsafe { ffi.into_string() };
        assert_eq!(recovered, original);
    }

    #[test]
    fn ffi_string_empty() {
        let ffi = FfiString::empty();
        assert!(ffi.is_null());
        assert_eq!(ffi.len, 0);

        #[allow(unsafe_code)]
        let recovered = unsafe { ffi.into_string() };
        assert_eq!(recovered, "");
    }

    #[test]
    fn ffi_string_as_str() {
        let original = "test string";
        let ffi = FfiString::new(original);

        #[allow(unsafe_code)]
        let s = unsafe { ffi.as_str() };
        assert_eq!(s, original);

        // Clean up
        #[allow(unsafe_code)]
        unsafe {
            ffi.free();
        }
    }

    #[test]
    fn ffi_string_free_null_is_safe() {
        let ffi = FfiString::empty();
        // Freeing a null FfiString should not panic or crash.
        #[allow(unsafe_code)]
        unsafe {
            ffi.free();
        }
    }

    #[test]
    fn type_sizes_are_stable() {
        // FfiString: pointer + usize
        assert_eq!(
            mem::size_of::<FfiString>(),
            mem::size_of::<*mut c_char>() + mem::size_of::<usize>()
        );

        // PluginStatus is a simple C enum (no data variants)
        assert!(mem::size_of::<PluginStatus>() <= mem::size_of::<u32>() * 2);

        // PluginResult is a struct: PluginStatus + FfiString (with padding)
        assert!(
            mem::size_of::<PluginResult>()
                >= mem::size_of::<PluginStatus>() + mem::size_of::<FfiString>()
        );

        // PluginCapability is a simple C enum — should be the size of a c_int
        // or similar (repr(C) enums without data use the smallest integer that
        // can represent all variants, which on most platforms is i32/u32).
        assert!(mem::size_of::<PluginCapability>() <= mem::size_of::<u32>() * 2);

        // PluginEventType likewise
        assert!(mem::size_of::<PluginEventType>() <= mem::size_of::<u32>() * 2);

        // Basic sanity: these structs are non-zero sized
        assert!(mem::size_of::<PluginApi>() > 0);
        assert!(mem::size_of::<PluginHostApi>() > 0);
        assert!(mem::size_of::<PluginInfo>() > 0);
        assert!(mem::size_of::<PluginEvent>() > 0);
        assert!(mem::size_of::<PluginResult>() > 0);
    }

    #[test]
    fn plugin_capability_values_are_distinct() {
        let caps = [
            PluginCapability::ReadConfig,
            PluginCapability::RegisterCommands,
            PluginCapability::HookEvents,
            PluginCapability::SendMessages,
            PluginCapability::AccessNetwork,
        ];
        for (i, a) in caps.iter().enumerate() {
            for (j, b) in caps.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b);
                }
            }
        }
    }

    #[test]
    fn plugin_event_type_values_are_distinct() {
        let types = [
            PluginEventType::MessageReceived,
            PluginEventType::UserJoined,
            PluginEventType::UserParted,
            PluginEventType::UserQuit,
            PluginEventType::NickChanged,
            PluginEventType::Connected,
            PluginEventType::Disconnected,
            PluginEventType::CommandExecuted,
        ];
        for (i, a) in types.iter().enumerate() {
            for (j, b) in types.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b);
                }
            }
        }
    }

    #[test]
    fn plugin_result_ok() {
        let result = PluginResult::ok();
        assert!(result.is_ok());
        assert!(!result.is_error());
        assert_eq!(result.status, PluginStatus::Ok);
        assert!(result.error_message.is_null());
    }

    #[test]
    fn plugin_result_error_with_message() {
        let msg = "something went wrong";
        let result = PluginResult::error(msg);
        assert!(!result.is_ok());
        assert!(result.is_error());
        assert_eq!(result.status, PluginStatus::Error);
        assert!(!result.error_message.is_null());

        #[allow(unsafe_code)]
        let recovered = unsafe { result.error_message.into_string() };
        assert_eq!(recovered, msg);
    }

    #[test]
    fn plugin_status_values() {
        assert_ne!(PluginStatus::Ok, PluginStatus::Error);
        assert_eq!(PluginStatus::Ok as u32, 0);
        assert_eq!(PluginStatus::Error as u32, 1);
    }

    #[test]
    fn plugin_event_ffi_strings_can_be_freed_via_destructure() {
        // Mirrors the cleanup pattern used in dispatch_command/dispatch_event:
        // create a PluginEvent with FfiString allocations, then destructure
        // and free the strings after the FFI call returns.
        let event = PluginEvent {
            event_type: PluginEventType::CommandExecuted,
            data: FfiString::new("test-command"),
            source: FfiString::new("arg1 arg2"),
        };

        // Verify strings are valid before cleanup.
        assert!(!event.data.is_null());
        assert!(!event.source.is_null());

        // Destructure and free — the same pattern used in dispatch.rs.
        let PluginEvent { data, source, .. } = event;
        #[allow(unsafe_code)]
        unsafe {
            data.free();
            source.free();
        }
    }
}
