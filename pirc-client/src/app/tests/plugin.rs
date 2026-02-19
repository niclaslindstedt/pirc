use crate::app::App;
use crate::client_command::{ClientCommand, PluginSubcommand};
use crate::config::ClientConfig;

#[test]
fn app_has_plugin_manager() {
    let config = ClientConfig::default();
    let app = App::new(config);
    assert_eq!(app.plugin_manager.plugin_count(), 0);
}

#[test]
fn plugin_list_empty() {
    let config = ClientConfig::default();
    let mut app = App::new(config);
    app.handle_plugin_command(&PluginSubcommand::List);
    // Should not crash; just outputs "No plugins loaded"
}

#[test]
fn plugin_info_not_found() {
    let config = ClientConfig::default();
    let mut app = App::new(config);
    app.handle_plugin_command(&PluginSubcommand::Info("nonexistent".into()));
    // Should not crash; outputs "not found" message
}

#[test]
fn plugin_unload_not_found() {
    let config = ClientConfig::default();
    let mut app = App::new(config);
    app.handle_plugin_command(&PluginSubcommand::Unload("ghost".into()));
    // Should not crash; outputs error message
}

#[test]
fn plugin_enable_not_found() {
    let config = ClientConfig::default();
    let mut app = App::new(config);
    app.handle_plugin_command(&PluginSubcommand::Enable("ghost".into()));
    // Should not crash; outputs error message
}

#[test]
fn plugin_disable_not_found() {
    let config = ClientConfig::default();
    let mut app = App::new(config);
    app.handle_plugin_command(&PluginSubcommand::Disable("ghost".into()));
    // Should not crash; outputs error message
}

#[test]
fn plugin_reload_not_found() {
    let config = ClientConfig::default();
    let mut app = App::new(config);
    app.handle_plugin_command(&PluginSubcommand::Reload("ghost".into()));
    // Should not crash; outputs error message
}

#[test]
fn plugin_load_invalid_path() {
    let config = ClientConfig::default();
    let mut app = App::new(config);
    app.handle_plugin_command(&PluginSubcommand::Load("/nonexistent/plugin.dylib".into()));
    // Should not crash; outputs error message
}

#[test]
fn init_plugins_disabled() {
    let mut config = ClientConfig::default();
    config.plugins.enabled = false;
    let mut app = App::new(config);
    app.init_plugins();
    assert_eq!(app.plugin_manager.plugin_count(), 0);
}

#[test]
fn init_plugins_creates_directory() {
    let dir = std::env::temp_dir().join("pirc_test_plugin_init");
    let _ = std::fs::remove_dir_all(&dir);

    let mut config = ClientConfig::default();
    config.plugins.enabled = true;
    config.plugins.plugins_dir = Some(dir.to_string_lossy().into_owned());

    let mut app = App::new(config);
    app.init_plugins();

    assert!(dir.exists());
    assert_eq!(app.plugin_manager.plugin_count(), 0);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn init_plugins_empty_dir() {
    let dir = std::env::temp_dir().join("pirc_test_plugin_init_empty");
    let _ = std::fs::create_dir_all(&dir);

    let mut config = ClientConfig::default();
    config.plugins.enabled = true;
    config.plugins.plugins_dir = Some(dir.to_string_lossy().into_owned());

    let mut app = App::new(config);
    app.init_plugins();

    assert_eq!(app.plugin_manager.plugin_count(), 0);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn dispatch_plugin_command_no_plugins() {
    let config = ClientConfig::default();
    let mut app = App::new(config);
    let handled = app.dispatch_plugin_command("nonexistent", "");
    assert!(!handled);
}

#[test]
fn dispatch_plugin_event_no_plugins() {
    let config = ClientConfig::default();
    let app = App::new(config);
    // Should not crash with no subscribed plugins.
    app.dispatch_plugin_event(
        pirc_plugin::ffi::PluginEventType::Connected,
        "test.server",
        "",
    );
}

#[test]
fn plugin_command_is_client_local() {
    // All plugin commands should return None from to_message.
    let variants = vec![
        ClientCommand::Plugin(PluginSubcommand::List),
        ClientCommand::Plugin(PluginSubcommand::Load("/p".into())),
        ClientCommand::Plugin(PluginSubcommand::Unload("n".into())),
        ClientCommand::Plugin(PluginSubcommand::Reload("n".into())),
        ClientCommand::Plugin(PluginSubcommand::Enable("n".into())),
        ClientCommand::Plugin(PluginSubcommand::Disable("n".into())),
        ClientCommand::Plugin(PluginSubcommand::Info("n".into())),
    ];

    for cmd in variants {
        assert!(
            cmd.to_message(None).is_none(),
            "expected None for {cmd:?}"
        );
    }
}
