//! Integration test: a plugin whose `init()` fails must NOT be stored.
//!
//! This lives in an integration test (separate binary) because each
//! `declare_plugin!` invocation emits a `#[no_mangle] pirc_plugin_init`
//! symbol, and only one can exist per binary.

use pirc_plugin::ffi::{
    FfiString, PluginEventType, PluginHostApi, PluginResult,
};
use pirc_plugin::plugin::{Plugin, PluginError, PluginHost};

// -- Mock host API functions -------------------------------------------------

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

// -- A plugin whose init() always fails --------------------------------------

#[derive(Default)]
struct FailingPlugin;

#[allow(clippy::unnecessary_literal_bound)]
impl Plugin for FailingPlugin {
    fn name(&self) -> &str {
        "failing-plugin"
    }
    fn version(&self) -> &str {
        "0.1.0"
    }
    fn init(&mut self, _host: &dyn PluginHost) -> Result<(), PluginError> {
        Err(PluginError::InitFailed("intentional failure".into()))
    }
}

pirc_plugin::declare_plugin!(FailingPlugin);

#[test]
#[allow(unsafe_code)]
fn failed_init_does_not_store_plugin() {
    let api_ptr = __pirc_plugin_ffi::pirc_plugin_init();
    let api = unsafe { &*api_ptr };

    let host_api = mock_host_api();
    let result = (api.init)(&raw const host_api);
    assert!(result.is_error());
    let msg = unsafe { result.error_message.into_string() };
    assert!(msg.contains("init failed"));

    // The plugin should NOT be stored, so subsequent lifecycle
    // calls must return "plugin not initialised" errors.
    let result = (api.on_enable)();
    assert!(result.is_error());
    let msg = unsafe { result.error_message.into_string() };
    assert_eq!(msg, "plugin not initialised");

    let result = (api.on_disable)();
    assert!(result.is_error());
    let msg = unsafe { result.error_message.into_string() };
    assert_eq!(msg, "plugin not initialised");

    let result = (api.shutdown)();
    assert!(result.is_error());
    let msg = unsafe { result.error_message.into_string() };
    assert_eq!(msg, "plugin not initialised");
}
