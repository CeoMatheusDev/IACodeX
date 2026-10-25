slint::include_modules!();

use std::{fs, path::PathBuf, process::Command};
use serde_json::{json, Value};

fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("iacodex").join("settings.json"))
}

fn save_json(path: &std::path::Path, value: &Value) -> std::io::Result<()> {
    if let Some(parent) = path.parent() { fs::create_dir_all(parent)?; }
    let encoded = serde_json::to_vec_pretty(value)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, encoded)?;
    fs::rename(tmp, path)?;
    Ok(())
}
fn read_config(path: &std::path::Path) -> Option<Value> {
    if let Ok(contents) = fs::read_to_string(path) {
        match serde_json::from_str::<Value>(&contents) {
            Ok(value) => return Some(value),
            Err(error) => eprintln!("Invalid settings JSON ({}): {error}", path.display()),
        }
    }
    let contents = fs::read_to_string(path.with_extension("conf")).ok()?;
    let mut result = serde_json::Map::new();
    for line in contents.lines() {
        if let Some((key, value)) = line.split_once('=') {
            result.insert(key.into(), if matches!(key, "shell" | "agent_loop" | "web_search") { json!(value == "true") } else { json!(value) });
        }
    }
    let value = Value::Object(result);
    if let Err(error) = save_json(path, &value) { eprintln!("Could not migrate settings: {error}"); }
    Some(value)
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
        if let Some(settings) = read_config(&path) {
            let string = |key: &str| settings.get(key).and_then(Value::as_str);
            if let Some(v) = string("mode") { app.set_active_mode(v.into()); }
            if let Some(v) = string("permission") { app.set_settings_permission(v.into()); }
            if let Some(v) = string("language") { app.set_settings_language(v.into()); }
            if let Some(v) = string("appearance") { app.set_settings_appearance(v.into()); }
            if let Some(v) = string("enter") { app.set_settings_enter(v.into()); }
            if let Some(v) = string("provider") { app.set_settings_provider(v.into()); }
            if let Some(v) = settings.get("shell").and_then(Value::as_bool) { app.set_plugin_shell(v); }
            if let Some(v) = settings.get("agent_loop").and_then(Value::as_bool) { app.set_plugin_agent_loop(v); }
            if let Some(v) = settings.get("web_search").and_then(Value::as_bool) { app.set_plugin_web_search(v); }
        }
    }
    app.on_save_settings(|mode, permission, language, appearance, enter, provider, shell, agent_loop, web_search| {
        let Some(path) = config_path() else { eprintln!("Cannot determine configuration directory"); return; };
        let data = json!({
            "mode": mode.as_str(), "permission": permission.as_str(),
            "language": language.as_str(), "appearance": appearance.as_str(),
            "enter": enter.as_str(), "provider": provider.as_str(),
            "shell": shell, "agent_loop": agent_loop, "web_search": web_search
        });
        if let Err(error) = save_json(&path, &data) { eprintln!("Cannot save settings to {}: {error}", path.display()); }
    });
    app.on_open_configuration_file(|| {
        let Some(path) = config_path() else { eprintln!("Cannot determine configuration directory"); return; };
        if !path.exists() {
            if let Err(error) = save_json(&path, &json!({})) { eprintln!("Cannot create configuration file: {error}"); return; }
        }
        if let Err(error) = Command::new("xdg-open").arg(&path).spawn() {
            eprintln!("Cannot open {}: {error}", path.display());
        }
    });

    app.on_send_prompt(|message| {
        println!("Prompt (interface demonstrativa): {}", message);
    });
    app.run()
}
