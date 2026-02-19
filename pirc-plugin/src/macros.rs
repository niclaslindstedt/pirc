//! Macros for bridging safe Rust [`Plugin`](crate::plugin::Plugin) implementations
//! to the raw C FFI ABI.

/// Declares a plugin by generating the `extern "C"` entry point and FFI bridge.
///
/// This macro takes a type that implements [`Plugin`](crate::plugin::Plugin)
/// (constructed via `Default`) and generates:
///
/// - A static plugin instance protected by a mutex
/// - An FFI host adapter wrapping [`PluginHostApi`](crate::ffi::PluginHostApi)
///   into a safe [`PluginHost`](crate::plugin::PluginHost)
/// - All `extern "C"` functions required by [`PluginApi`](crate::ffi::PluginApi)
/// - The `pirc_plugin_init` entry point symbol
///
/// # Example
///
/// ```rust,ignore
/// use pirc_plugin::plugin::{Plugin, PluginHost, PluginError};
///
/// struct MyPlugin;
///
/// impl Default for MyPlugin {
///     fn default() -> Self { Self }
/// }
///
/// impl Plugin for MyPlugin {
///     fn name(&self) -> &str { "my-plugin" }
///     fn version(&self) -> &str { "0.1.0" }
///     fn init(&mut self, _host: &dyn PluginHost) -> Result<(), PluginError> { Ok(()) }
/// }
///
/// pirc_plugin::declare_plugin!(MyPlugin);
/// ```
#[macro_export]
macro_rules! declare_plugin {
    ($plugin_ty:ty) => {
        // Keep all generated items in a hidden module to avoid polluting the
        // plugin author's namespace while still exporting the entry point.
        #[doc(hidden)]
        #[allow(clippy::items_after_statements)]
        mod __pirc_plugin_ffi {
            use super::*;
            use std::sync::Mutex;

            use $crate::ffi::{
                FfiString, PluginApi, PluginEvent as FfiPluginEvent,
                PluginHostApi, PluginInfo, PluginResult,
            };
            use $crate::plugin::{
                LogLevel, Plugin, PluginError, PluginEvent, PluginHost,
            };

            /// Global plugin instance, created on first access.
            static PLUGIN: Mutex<Option<$plugin_ty>> = Mutex::new(None);

            // No-op FFI command callback used by `register_command`. The safe
            // Plugin trait dispatches commands via `on_command` instead.
            extern "C" fn noop_callback(_args: FfiString) -> PluginResult {
                PluginResult::ok()
            }

            /// Adapter that wraps a raw [`PluginHostApi`] pointer into a safe
            /// [`PluginHost`] implementation.
            struct FfiHostAdapter {
                host: *const PluginHostApi,
            }

            // SAFETY: The host pointer is provided by the host and remains valid
            // for the plugin's lifetime. The host ensures single-threaded access.
            #[allow(unsafe_code)]
            unsafe impl Send for FfiHostAdapter {}
            #[allow(unsafe_code)]
            unsafe impl Sync for FfiHostAdapter {}

            #[allow(unsafe_code)]
            impl PluginHost for FfiHostAdapter {
                fn register_command(
                    &self,
                    name: &str,
                    _description: &str,
                ) -> Result<(), PluginError> {
                    let host = unsafe { &*self.host };
                    let ffi_name = FfiString::new(name);
                    let result = (host.register_command)(ffi_name, noop_callback);
                    if result.is_ok() {
                        Ok(())
                    } else {
                        let msg = unsafe { result.error_message.into_string() };
                        Err(PluginError::Other(msg))
                    }
                }

                fn unregister_command(&self, name: &str) {
                    let host = unsafe { &*self.host };
                    let ffi_name = FfiString::new(name);
                    let _ = (host.unregister_command)(ffi_name);
                }

                fn hook_event(
                    &self,
                    event_type: $crate::ffi::PluginEventType,
                ) -> Result<(), PluginError> {
                    let host = unsafe { &*self.host };
                    let result = (host.hook_event)(event_type);
                    if result.is_ok() {
                        Ok(())
                    } else {
                        let msg = unsafe { result.error_message.into_string() };
                        Err(PluginError::Other(msg))
                    }
                }

                fn unhook_event(&self, event_type: $crate::ffi::PluginEventType) {
                    let host = unsafe { &*self.host };
                    let _ = (host.unhook_event)(event_type);
                }

                fn echo(&self, text: &str) {
                    let host = unsafe { &*self.host };
                    let ffi_text = FfiString::new(text);
                    (host.echo)(ffi_text);
                }

                fn log(&self, level: LogLevel, msg: &str) {
                    let host = unsafe { &*self.host };
                    let ffi_msg = FfiString::new(msg);
                    (host.log)(level as u32, ffi_msg);
                }

                fn get_config_value(&self, key: &str) -> Option<String> {
                    let host = unsafe { &*self.host };
                    let ffi_key = FfiString::new(key);
                    let ffi_val = (host.get_config_value)(ffi_key);
                    if ffi_val.is_null() {
                        None
                    } else {
                        Some(unsafe { ffi_val.into_string() })
                    }
                }
            }

            // -- Helper to convert Result<(), PluginError> to PluginResult ----

            fn to_ffi_result(r: Result<(), PluginError>) -> PluginResult {
                match r {
                    Ok(()) => PluginResult::ok(),
                    Err(e) => PluginResult::error(&e.to_string()),
                }
            }

            // -- extern "C" functions for PluginApi ---------------------------

            extern "C" fn plugin_info() -> PluginInfo {
                let guard = PLUGIN.lock().unwrap_or_else(|e| e.into_inner());
                let plugin = guard.as_ref().expect("plugin not initialised");

                let caps = plugin.capabilities();
                PluginInfo {
                    name: FfiString::new(plugin.name()),
                    version: FfiString::new(plugin.version()),
                    description: FfiString::new(plugin.description()),
                    author: FfiString::new(plugin.author()),
                    capabilities: if caps.is_empty() {
                        std::ptr::null()
                    } else {
                        caps.as_ptr()
                    },
                    capabilities_len: caps.len(),
                }
            }

            #[allow(unsafe_code)]
            extern "C" fn plugin_free_info(info: PluginInfo) {
                // Free the FfiString fields; capabilities points into plugin
                // memory and must not be freed here.
                unsafe {
                    info.name.free();
                    info.version.free();
                    info.description.free();
                    info.author.free();
                }
            }

            #[allow(unsafe_code)]
            extern "C" fn plugin_init(host: *const PluginHostApi) -> PluginResult {
                // Create the plugin instance.
                let mut plugin = <$plugin_ty>::default();

                // Build the safe host adapter.
                let adapter = FfiHostAdapter { host };

                // Call the plugin's init method.
                let result = plugin.init(&adapter);

                // Only store the plugin instance if init succeeded.
                // A failed init must not leave a half-initialised plugin
                // accessible to subsequent lifecycle calls.
                if result.is_ok() {
                    let mut guard =
                        PLUGIN.lock().unwrap_or_else(|e| e.into_inner());
                    *guard = Some(plugin);
                }

                to_ffi_result(result)
            }

            extern "C" fn plugin_on_enable() -> PluginResult {
                let mut guard = PLUGIN.lock().unwrap_or_else(|e| e.into_inner());
                match guard.as_mut() {
                    Some(plugin) => to_ffi_result(plugin.on_enable()),
                    None => PluginResult::error("plugin not initialised"),
                }
            }

            extern "C" fn plugin_on_disable() -> PluginResult {
                let mut guard = PLUGIN.lock().unwrap_or_else(|e| e.into_inner());
                match guard.as_mut() {
                    Some(plugin) => to_ffi_result(plugin.on_disable()),
                    None => PluginResult::error("plugin not initialised"),
                }
            }

            extern "C" fn plugin_shutdown() -> PluginResult {
                let mut guard = PLUGIN.lock().unwrap_or_else(|e| e.into_inner());
                match guard.as_mut() {
                    Some(plugin) => to_ffi_result(plugin.shutdown()),
                    None => PluginResult::error("plugin not initialised"),
                }
            }

            #[allow(unsafe_code)]
            extern "C" fn plugin_on_event(
                event: *const FfiPluginEvent,
            ) -> PluginResult {
                if event.is_null() {
                    return PluginResult::error("null event pointer");
                }
                let ffi_event = unsafe { &*event };
                let safe_event = PluginEvent {
                    event_type: ffi_event.event_type,
                    data: unsafe { ffi_event.data.as_str() }.to_owned(),
                    source: unsafe { ffi_event.source.as_str() }.to_owned(),
                };

                let mut guard = PLUGIN.lock().unwrap_or_else(|e| e.into_inner());
                match guard.as_mut() {
                    Some(plugin) => to_ffi_result(plugin.on_event(&safe_event)),
                    None => PluginResult::error("plugin not initialised"),
                }
            }

            /// The static [`PluginApi`] vtable returned by the entry point.
            static PLUGIN_API: PluginApi = PluginApi {
                info: plugin_info,
                free_info: plugin_free_info,
                init: plugin_init,
                on_enable: plugin_on_enable,
                on_disable: plugin_on_disable,
                shutdown: plugin_shutdown,
                on_event: plugin_on_event,
            };

            /// The entry point symbol that the host looks for when loading the
            /// plugin dynamic library.
            #[no_mangle]
            #[allow(unsafe_code)]
            pub extern "C" fn pirc_plugin_init() -> *const PluginApi {
                &PLUGIN_API
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use crate::ffi::{
        FfiString, PluginEventType, PluginHostApi, PluginResult,
    };
    use crate::plugin::{Plugin, PluginError, PluginHost};

    // -- Mock host API functions (defined at module level for clippy) ----------

    extern "C" fn mock_register_command(
        _name: FfiString,
        _cb: extern "C" fn(FfiString) -> PluginResult,
    ) -> PluginResult {
        PluginResult::ok()
    }
    extern "C" fn mock_unregister_command(_name: FfiString) -> PluginResult {
        PluginResult::ok()
    }
    extern "C" fn mock_hook_event(_et: PluginEventType) -> PluginResult {
        PluginResult::ok()
    }
    extern "C" fn mock_unhook_event(_et: PluginEventType) -> PluginResult {
        PluginResult::ok()
    }
    extern "C" fn mock_echo(_msg: FfiString) {}
    extern "C" fn mock_log(_level: u32, _msg: FfiString) {}
    extern "C" fn mock_get_config_value(_key: FfiString) -> FfiString {
        FfiString::empty()
    }

    fn mock_host_api() -> PluginHostApi {
        PluginHostApi {
            register_command: mock_register_command,
            unregister_command: mock_unregister_command,
            hook_event: mock_hook_event,
            unhook_event: mock_unhook_event,
            echo: mock_echo,
            log: mock_log,
            get_config_value: mock_get_config_value,
        }
    }

    // -- A test plugin for macro expansion ------------------------------------

    #[derive(Default)]
    struct TestPlugin {
        init_called: bool,
    }

    #[allow(clippy::unnecessary_literal_bound)]
    impl Plugin for TestPlugin {
        fn name(&self) -> &str {
            "test-plugin"
        }
        fn version(&self) -> &str {
            "0.1.0"
        }
        fn description(&self) -> &str {
            "A test plugin"
        }
        fn author(&self) -> &str {
            "tester"
        }
        fn init(&mut self, _host: &dyn PluginHost) -> Result<(), PluginError> {
            self.init_called = true;
            Ok(())
        }
    }

    declare_plugin!(TestPlugin);

    #[test]
    fn macro_generates_entry_point() {
        let api_ptr = __pirc_plugin_ffi::pirc_plugin_init();
        assert!(!api_ptr.is_null());
    }

    #[test]
    #[allow(unsafe_code)]
    fn macro_generated_api_info_returns_metadata() {
        let api_ptr = __pirc_plugin_ffi::pirc_plugin_init();
        let api = unsafe { &*api_ptr };

        let host_api = mock_host_api();
        let result = (api.init)(&raw const host_api);
        assert!(result.is_ok());

        let info = (api.info)();
        unsafe {
            assert_eq!(info.name.as_str(), "test-plugin");
            assert_eq!(info.version.as_str(), "0.1.0");
            assert_eq!(info.description.as_str(), "A test plugin");
            assert_eq!(info.author.as_str(), "tester");
        }
        assert_eq!(info.capabilities_len, 0);

        (api.free_info)(info);
    }

    #[test]
    #[allow(unsafe_code)]
    fn macro_generated_lifecycle_works() {
        let api_ptr = __pirc_plugin_ffi::pirc_plugin_init();
        let api = unsafe { &*api_ptr };

        let host_api = mock_host_api();
        let result = (api.init)(&raw const host_api);
        assert!(result.is_ok());

        let result = (api.on_enable)();
        assert!(result.is_ok());

        let result = (api.on_disable)();
        assert!(result.is_ok());

        let result = (api.shutdown)();
        assert!(result.is_ok());
    }

    #[test]
    #[allow(unsafe_code)]
    fn macro_generated_on_event_handles_null() {
        let api_ptr = __pirc_plugin_ffi::pirc_plugin_init();
        let api = unsafe { &*api_ptr };

        let result = (api.on_event)(std::ptr::null());
        assert!(result.is_error());
        let msg = unsafe { result.error_message.into_string() };
        assert_eq!(msg, "null event pointer");
    }

}
