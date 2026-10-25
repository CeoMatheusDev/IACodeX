slint::include_modules!();

use std::{fs, path::PathBuf};

fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("iacodex").join("settings.conf"))
}

fn clean(value: &str) -> String {
    value.replace('\n', " ").replace('\r', " ").replace('=', " ")
}

fn system_prefers_dark() -> bool {
    if let Ok(theme) = std::env::var("GTK_THEME") {
        if theme.to_ascii_lowercase().contains("dark") { return true; }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let path = PathBuf::from(home).join(".config/kdeglobals");
        if let Ok(contents) = fs::read_to_string(path) {
            let mut in_general = false;
            for line in contents.lines() {
                let line = line.trim();
                if line.starts_with('[') {
                    in_general = line == "[General]";
                } else if in_general {
                    if let Some(value) = line.strip_prefix("ColorScheme=") {
                        return value.to_ascii_lowercase().contains("dark");
                    }
                }
            }
        }
    }
    true // Keep the current dark default when the system theme is unknown.
}

fn main() -> Result<(), slint::PlatformError> {
    let detected_dark = system_prefers_dark();
    let app = AppWindow::new()?;
    app.set_system_dark(detected_dark);

    if let Some(path) = config_path() {
        if let Ok(contents) = fs::read_to_string(path) {
            for line in contents.lines() {
                if let Some((key, value)) = line.split_once('=') {
                    match key {
                        "mode" => app.set_active_mode(value.into()),
                        "permission" => app.set_settings_permission(value.into()),
                        "language" => app.set_settings_language(value.into()),
                        "appearance" => app.set_settings_appearance(value.into()),
                        "enter" => app.set_settings_enter(value.into()),
                        "provider" => app.set_settings_provider(value.into()),
                        "shell" => app.set_plugin_shell(value == "true"),
                        "agent_loop" => app.set_plugin_agent_loop(value == "true"),
                        "web_search" => app.set_plugin_web_search(value == "true"),
                        _ => {}
                    }
                }
            }
        }
    }

    app.on_save_settings(|mode, permission, language, appearance, enter, provider, shell, agent_loop, web_search| {
        let Some(path) = config_path() else {
            eprintln!("Cannot determine user configuration directory");
            return;
        };
        if let Some(parent) = path.parent() {
            if let Err(error) = fs::create_dir_all(parent) {
                eprintln!("Cannot create settings directory: {error}");
                return;
            }
        }
        let data = format!(
            "mode={}\npermission={}\nlanguage={}\nappearance={}\nenter={}\nprovider={}\nshell={}\nagent_loop={}\nweb_search={}\n",
            clean(&mode), clean(&permission), clean(&language), clean(&appearance),
            clean(&enter), clean(&provider), shell, agent_loop, web_search
        );
        if let Err(error) = fs::write(&path, data) {
            eprintln!("Cannot save settings to {}: {error}", path.display());
        }
    });

    app.on_send_prompt(|message| {
        println!("Prompt (interface demonstrativa): {}", message);
    });
    app.run()
}
