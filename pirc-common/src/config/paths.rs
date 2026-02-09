//! XDG-compatible path resolution for pirc configuration.
//!
//! Respects `$XDG_CONFIG_HOME` when set, otherwise falls back to `~/.pirc`.
//! The server config additionally falls back to `/etc/pirc/pircd.toml` if the
//! user-local path does not exist.

use std::path::PathBuf;

/// Returns the pirc configuration directory.
///
/// Uses `$XDG_CONFIG_HOME/pirc` if `XDG_CONFIG_HOME` is set, otherwise `~/.pirc`.
pub fn config_dir() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg).join("pirc"));
        }
    }
    dirs::home_dir().map(|home| home.join(".pirc"))
}

/// Returns the default server configuration file path.
///
/// Returns `config_dir()/pircd.toml`, with a system-wide fallback to
/// `/etc/pirc/pircd.toml`.
pub fn default_server_config_path() -> Option<PathBuf> {
    let user_path = config_dir().map(|d| d.join("pircd.toml"));
    if let Some(ref path) = user_path {
        if path.exists() {
            return user_path;
        }
    }
    let system_path = PathBuf::from("/etc/pirc/pircd.toml");
    if system_path.exists() {
        return Some(system_path);
    }
    // Return the user path even if it doesn't exist yet (for creation)
    user_path
}

/// Returns the default client configuration file path.
///
/// Returns `config_dir()/pirc.toml`.
pub fn default_client_config_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("pirc.toml"))
}

/// Returns the scripts directory path.
///
/// Returns `config_dir()/scripts/`.
pub fn scripts_dir() -> Option<PathBuf> {
    config_dir().map(|d| d.join("scripts"))
}

/// Returns the plugins directory path.
///
/// Returns `config_dir()/plugins/`.
pub fn plugins_dir() -> Option<PathBuf> {
    config_dir().map(|d| d.join("plugins"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Env var tests must run serially to avoid races.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn config_dir_uses_xdg_when_set() {
        let _guard = ENV_LOCK.lock().unwrap();
        let original = std::env::var("XDG_CONFIG_HOME").ok();

        std::env::set_var("XDG_CONFIG_HOME", "/tmp/xdg_test");
        let dir = config_dir().unwrap();
        assert_eq!(dir, PathBuf::from("/tmp/xdg_test/pirc"));

        // Restore
        match original {
            Some(val) => std::env::set_var("XDG_CONFIG_HOME", val),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }

    #[test]
    fn config_dir_falls_back_to_home_dot_pirc() {
        let _guard = ENV_LOCK.lock().unwrap();
        let original = std::env::var("XDG_CONFIG_HOME").ok();

        std::env::remove_var("XDG_CONFIG_HOME");
        let dir = config_dir().unwrap();
        let home = dirs::home_dir().unwrap();
        assert_eq!(dir, home.join(".pirc"));

        // Restore
        if let Some(val) = original {
            std::env::set_var("XDG_CONFIG_HOME", val);
        }
    }

    #[test]
    fn config_dir_ignores_empty_xdg() {
        let _guard = ENV_LOCK.lock().unwrap();
        let original = std::env::var("XDG_CONFIG_HOME").ok();

        std::env::set_var("XDG_CONFIG_HOME", "");
        let dir = config_dir().unwrap();
        let home = dirs::home_dir().unwrap();
        assert_eq!(dir, home.join(".pirc"));

        // Restore
        match original {
            Some(val) => std::env::set_var("XDG_CONFIG_HOME", val),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }

    #[test]
    fn default_client_config_path_under_config_dir() {
        let _guard = ENV_LOCK.lock().unwrap();
        let original = std::env::var("XDG_CONFIG_HOME").ok();

        std::env::set_var("XDG_CONFIG_HOME", "/tmp/xdg_test");
        let path = default_client_config_path().unwrap();
        assert_eq!(path, PathBuf::from("/tmp/xdg_test/pirc/pirc.toml"));

        match original {
            Some(val) => std::env::set_var("XDG_CONFIG_HOME", val),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }

    #[test]
    fn default_server_config_path_returns_user_path() {
        let _guard = ENV_LOCK.lock().unwrap();
        let original = std::env::var("XDG_CONFIG_HOME").ok();

        std::env::set_var("XDG_CONFIG_HOME", "/tmp/xdg_test");
        let path = default_server_config_path().unwrap();
        // Neither user nor system path likely exists, so we get the user path
        assert_eq!(path, PathBuf::from("/tmp/xdg_test/pirc/pircd.toml"));

        match original {
            Some(val) => std::env::set_var("XDG_CONFIG_HOME", val),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }

    #[test]
    fn scripts_dir_under_config_dir() {
        let _guard = ENV_LOCK.lock().unwrap();
        let original = std::env::var("XDG_CONFIG_HOME").ok();

        std::env::set_var("XDG_CONFIG_HOME", "/tmp/xdg_test");
        let path = scripts_dir().unwrap();
        assert_eq!(path, PathBuf::from("/tmp/xdg_test/pirc/scripts"));

        match original {
            Some(val) => std::env::set_var("XDG_CONFIG_HOME", val),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }

    #[test]
    fn plugins_dir_under_config_dir() {
        let _guard = ENV_LOCK.lock().unwrap();
        let original = std::env::var("XDG_CONFIG_HOME").ok();

        std::env::set_var("XDG_CONFIG_HOME", "/tmp/xdg_test");
        let path = plugins_dir().unwrap();
        assert_eq!(path, PathBuf::from("/tmp/xdg_test/pirc/plugins"));

        match original {
            Some(val) => std::env::set_var("XDG_CONFIG_HOME", val),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }
}
