slint::include_modules!();

use std::{fs, path::PathBuf, process::Command};
use serde_json::{json, Value};
use slint::{Model, ModelRc, VecModel};
use std::rc::Rc;

fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("iacodex").join("settings.json"))
}

fn workspaces_path() -> Option<PathBuf> {
    Some(config_path()?.with_file_name("workspaces.json"))
}

fn read_workspaces() -> Vec<String> {
    workspaces_path()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|content| serde_json::from_str::<Value>(&content).ok())
        .and_then(|value| value.get("workspaces").and_then(Value::as_array).cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|value| value.as_str().map(str::to_owned))
        .collect()
}

fn sessions_path() -> Option<PathBuf> {
    Some(config_path()?.with_file_name("sessions.json"))
}

fn read_sessions() -> Vec<(String, String)> {
    sessions_path()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| value.get("sessions").and_then(Value::as_array).cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|item| Some((
            item.get("workspace")?.as_str()?.to_owned(),
            item.get("title")?.as_str()?.to_owned(),
        )))
        .collect()
}

fn providers_path() -> Option<PathBuf> {
    Some(config_path()?.with_file_name("providers.json"))
}

fn read_providers() -> Value {
    providers_path()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .filter(|value| value.get("providers").is_some_and(Value::is_object))
        .unwrap_or_else(|| json!({"providers": {}}))
}

fn migrate_providers() {
    let (Some(settings_path), Some(provider_path)) = (config_path(), providers_path()) else { return; };
    let Ok(text) = fs::read_to_string(&settings_path) else { return; };
    let Ok(mut settings) = serde_json::from_str::<Value>(&text) else { return; };
    let Some(legacy) = settings.get("providers").filter(|v| v.is_object()).cloned() else { return; };
    let mut providers = read_providers();
    if let (Some(target), Some(source)) = (
        providers.get_mut("providers").and_then(Value::as_object_mut),
        legacy.as_object(),
    ) {
        for (key, value) in source {
            target.entry(key.clone()).or_insert_with(|| value.clone());
        }
    }
    if let Err(error) = save_json(&provider_path, &providers) {
        eprintln!("Cannot migrate providers: {error}");
        return;
    }
    if let Some(obj) = settings.as_object_mut() {
        obj.remove("providers");
        obj.insert("provider".into(), json!(""));
    }
    if let Err(error) = save_json(&settings_path, &settings) {
        eprintln!("Cannot clean up legacy provider settings: {error}");
    }
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
    let workspace_list = Rc::new(VecModel::<slint::SharedString>::from(
        read_workspaces().into_iter().map(Into::into).collect::<Vec<_>>()
    ));
    app.set_workspaces(ModelRc::from(workspace_list.clone()));
    app.set_active_workspace("".into());
    let workspaces_for_create = workspace_list.clone();
    let weak_workspace = app.as_weak();
    app.on_create_workspace(move |name| {
        let name = name.trim();
        if name.is_empty() { return; }
        let Some(path) = workspaces_path() else { return; };
        let mut names = read_workspaces();
        if !names.iter().any(|existing| existing == name) {
            names.push(name.to_owned());
            if let Err(error) = save_json(&path, &json!({"workspaces": names})) {
                eprintln!("Cannot save workspace: {error}");
                return;
            }
            workspaces_for_create.push(name.into());
        }
        if let Some(app) = weak_workspace.upgrade() {
            app.set_active_workspace(name.into());
            app.set_session_ready(false);
            app.set_has_session(false);
        }
    });
    let session_list = Rc::new(VecModel::<slint::SharedString>::from(
        read_sessions().into_iter().map(|(workspace, title)| format!("{workspace}///{title}").into()).collect::<Vec<_>>()
    ));
    app.set_session_entries(ModelRc::from(session_list.clone()));
    app.on_session_in_workspace(|entry, workspace| {
        entry.starts_with(&format!("{workspace}///"))
    });
    app.on_session_title(|entry, workspace| {
        entry.strip_prefix(&format!("{workspace}///")).unwrap_or(entry.as_str()).into()
    });
    let sessions_for_record = session_list.clone();
    app.on_record_session(move |workspace, title| {
        if workspace.is_empty() || title.is_empty() { return; }
        let Some(path) = sessions_path() else { return; };
        let mut sessions = read_sessions();
        sessions.push((workspace.to_string(), title.to_string()));
        let data = json!({"sessions": sessions.iter().map(|(workspace, title)| json!({
            "workspace": workspace, "title": title
        })).collect::<Vec<_>>()});
        if let Err(error) = save_json(&path, &data) {
            eprintln!("Cannot save session: {error}");
            return;
        }
        sessions_for_record.push(format!("{workspace}///{title}").into());
    });
    migrate_providers();
    let model_list = Rc::new(VecModel::<slint::SharedString>::from(Vec::new()));
    app.set_configured_models(ModelRc::from(model_list.clone()));

    if let Some(path) = config_path() {
        if let Some(settings) = read_config(&path) {
            let string = |key: &str| settings.get(key).and_then(Value::as_str);
            if let Some(v) = string("mode") { app.set_active_mode(v.into()); }
            if let Some(v) = string("permission") { app.set_settings_permission(v.into()); }
            if let Some(v) = string("language") { app.set_settings_language(v.into()); }
            if let Some(v) = string("appearance") { app.set_settings_appearance(v.into()); }
            if let Some(v) = string("enter") { app.set_settings_enter(v.into()); }
            if let Some(v) = settings.get("shell").and_then(Value::as_bool) { app.set_plugin_shell(v); }
            if let Some(v) = settings.get("agent_loop").and_then(Value::as_bool) { app.set_plugin_agent_loop(v); }
            if let Some(v) = settings.get("web_search").and_then(Value::as_bool) { app.set_plugin_web_search(v); }
        }
    }
    if let Some(providers) = read_providers().get("providers").and_then(Value::as_object) {
        for details in providers.values() {
            if let Some(name) = details.get("name").and_then(Value::as_str) {
                if app.get_settings_provider().is_empty() {
                    app.set_settings_provider(name.into());
                }
            }
            if let Some(models) = details.get("models").and_then(Value::as_array) {
                for model in models {
                    if let Some(id) = model.get("id").and_then(Value::as_str) {
                        if !model_list.iter().any(|existing| existing.as_str() == id) {
                            model_list.push(id.into());
                        }
                    }
                }
            }
        }
    }
    app.on_save_settings(|mode, permission, language, appearance, enter, provider, shell, agent_loop, web_search| {
        let Some(path) = config_path() else { eprintln!("Cannot determine configuration directory"); return; };
        let mut data = read_config(&path).unwrap_or_else(|| json!({}));
        if !data.is_object() { data = json!({}); }
        data["mode"] = json!(mode.as_str());
        data["permission"] = json!(permission.as_str());
        data["language"] = json!(language.as_str());
        data["appearance"] = json!(appearance.as_str());
        data["enter"] = json!(enter.as_str());
        let _ = provider;
        data["provider"] = json!("");
        data.as_object_mut().map(|obj| obj.remove("providers"));
        data["shell"] = json!(shell);
        data["agent_loop"] = json!(agent_loop);
        data["web_search"] = json!(web_search);
        if let Err(error) = save_json(&path, &data) { eprintln!("Cannot save settings to {}: {error}", path.display()); }
    });

    app.on_model_matches(|model, search| model.to_lowercase().contains(&search.to_lowercase()));
    let weak_for_selection = app.as_weak();
    app.on_select_configured_model(move |model_id| {
        let mut supported = false;
        {
            let data = read_providers();
            {
                if let Some(providers) = data.get("providers").and_then(Value::as_object) {
                    for provider in providers.values() {
                        if let Some(models) = provider.get("models").and_then(Value::as_array) {
                            for model in models {
                                if model.get("id").and_then(Value::as_str) == Some(model_id.as_str()) {
                                    supported = model.get("supports_thinking").and_then(Value::as_bool).unwrap_or(false);
                                }
                            }
                        }
                    }
                }
            }
        }
        if let Some(app) = weak_for_selection.upgrade() {
            app.set_thinking_supported(supported);
            app.set_thinking_enabled(false);
        }
    });

    let models_for_create = model_list.clone();
    let weak = app.as_weak();
    app.on_create_provider(move |id, name, url, protocol, _api_key, model_id, display, context, max_output| {
        let Some(path) = providers_path() else { return; };
        let mut data = read_providers();
        if !data.is_object() { data = json!({}); }
        let key = if id.is_empty() { name.to_lowercase().replace(' ', "-") } else { id.to_string() };
        let models = if model_id.is_empty() {
            json!([])
        } else {
            json!([{ "id": model_id.as_str(), "display_name": display.as_str(),
                "context_window": context.as_str(), "max_output_tokens": max_output.as_str(),
                "supports_thinking": false }])
        };
        if !data.get("providers").map(Value::is_object).unwrap_or(false) {
            data["providers"] = json!({});
        }
        data["providers"][&key] = json!({
            "id": key, "name": name.as_str(), "base_url": url.as_str(),
            "api_protocol": protocol.as_str(), "models": models
        });
        if let Err(error) = save_json(&path, &data) {
            eprintln!("Cannot save provider: {error}");
            return;
        }
        if !model_id.is_empty() {
            let value: slint::SharedString = model_id.clone();
            if !models_for_create.iter().any(|existing| existing == value) {
                models_for_create.push(value);
            }
        }
        if let Some(app) = weak.upgrade() {
            if !model_id.is_empty() { app.set_active_model(model_id); }
        }
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
