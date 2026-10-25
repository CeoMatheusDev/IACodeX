mod agent;
mod llm;

slint::include_modules!();

use std::{fs, path::PathBuf, process::Command};
use serde_json::{json, Value};
use slint::{Model, ModelRc, VecModel};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use tokio_util::sync::CancellationToken;
use agent::{AgentConfig, AgentEvent, AgentLoop};
use llm::openai_compatible::OpenAiCompatible;

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
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        use std::io::Write;
        let mut file = fs::OpenOptions::new().write(true).create(true).truncate(true)
            .mode(0o600).open(&tmp)?;
        file.write_all(&encoded)?;
        file.sync_all()?;
    }
    #[cfg(not(unix))]
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
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Tokio runtime could not start");
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

    let weak_for_edit = app.as_weak();
    app.on_edit_provider(move |name| {
        let data = read_providers();
        let Some((id, provider)) = data.get("providers").and_then(Value::as_object)
            .and_then(|providers| providers.iter().find(|(_, p)| p.get("name").and_then(Value::as_str) == Some(name.as_str()))) else { return; };
        if let Some(ui) = weak_for_edit.upgrade() {
            ui.set_provider_id(id.as_str().into());
            ui.set_provider_name(name);
            ui.set_provider_url(provider.get("base_url").and_then(Value::as_str).unwrap_or("").into());
            ui.set_provider_protocol(provider.get("api_protocol").and_then(Value::as_str).unwrap_or("OpenAI-compatible").into());
            ui.set_provider_api_key("".into()); // Empty means retain existing secret on save.
            ui.set_provider_custom(true);
            ui.set_provider_model(provider.get("models").and_then(Value::as_array)
                .and_then(|models| models.first()).and_then(|m| m.get("id")).and_then(Value::as_str).unwrap_or("").into());
            ui.set_provider_model_added(false);
            ui.set_provider_editing_existing(true);
            let saved = provider.get("models").and_then(Value::as_array).cloned().unwrap_or_default();
            ui.set_saved_provider_models(ModelRc::from(Rc::new(VecModel::<slint::SharedString>::from(
                saved.iter().filter_map(|m| m.get("id").and_then(Value::as_str).map(Into::into)).collect::<Vec<_>>()
            ))));
            ui.set_discovered_models(ModelRc::from(Rc::new(VecModel::<slint::SharedString>::from(Vec::new()))));
            ui.set_provider_status("Leave API key empty to keep the saved key.".into());
            ui.set_provider_editor(true);
        }
    });

    let weak_for_model_edit = app.as_weak();
    app.on_edit_provider_model(move |provider_id, model_id| {
        let providers = read_providers();
        let model = providers.get("providers").and_then(|p| p.get(provider_id.as_str()))
            .and_then(|p| p.get("models")).and_then(Value::as_array)
            .and_then(|models| models.iter().find(|m| m.get("id").and_then(Value::as_str) == Some(model_id.as_str())));
        if let (Some(ui), Some(model)) = (weak_for_model_edit.upgrade(), model) {
            ui.set_provider_original_model(model_id.clone());
            ui.set_provider_model(model_id);
            ui.set_provider_model_display(model.get("display_name").and_then(Value::as_str).unwrap_or("").into());
            ui.set_provider_model_context(model.get("context_window").and_then(Value::as_str).unwrap_or("").into());
            ui.set_provider_model_max_output(model.get("max_output_tokens").and_then(Value::as_str).unwrap_or("").into());
            ui.set_provider_model_added(true);
            ui.set_provider_model_expanded(true);
        }
    });

    let weak_for_discovery = app.as_weak();
    let discovery_handle = runtime.handle().clone();
    app.on_fetch_provider_models(move |url, key, protocol| {
        let url = url.to_string();
        let mut key = key.to_string();
        let protocol = protocol.to_string();
        let weak = weak_for_discovery.clone();
        if key.is_empty() {
            let providers = read_providers();
            if let Some(items) = providers.get("providers").and_then(Value::as_object) {
                if let Some(provider) = items.values().find(|p| p.get("base_url").and_then(Value::as_str) == Some(url.as_str())) {
                    key = provider.get("api_key").and_then(Value::as_str).unwrap_or("").to_string();
                }
            }
        }
        discovery_handle.spawn(async move {
            let result: Result<Vec<String>, String> = async {
                if protocol != "OpenAI-compatible" { return Err("Only OpenAI-compatible discovery is supported.".into()); }
                if url.trim().is_empty() { return Err("Base URL is required.".into()); }
                let endpoint = format!("{}/models", url.trim_end_matches('/'));
                let http = reqwest::Client::builder().timeout(std::time::Duration::from_secs(15))
                    .build().map_err(|e| e.to_string())?;
                let mut request = http.get(endpoint);
                if !key.is_empty() { request = request.bearer_auth(key); }
                let response = request.send().await.map_err(|e| e.to_string())?;
                let status = response.status();
                if !status.is_success() { return Err(format!("HTTP {status}: {}", response.text().await.unwrap_or_default().chars().take(150).collect::<String>())); }
                let json: Value = response.json().await.map_err(|e| e.to_string())?;
                let mut ids = json.get("data").and_then(Value::as_array)
                    .ok_or_else(|| "Response does not contain a data array.".to_string())?
                    .iter().filter_map(|model| model.get("id").and_then(Value::as_str).map(str::to_owned))
                    .collect::<Vec<_>>();
                ids.sort();
                ids.dedup();
                if ids.is_empty() { return Err("Provider returned no models.".into()); }
                Ok(ids)
            }.await;
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak.upgrade() {
                    ui.set_provider_loading(false);
                    match result {
                        Ok(ids) => {
                            ui.set_provider_health(1);
                            ui.set_provider_status(format!("Connected: {} models available. Click a model to select.", ids.len()).into());
                            ui.set_discovered_models(ModelRc::from(Rc::new(VecModel::<slint::SharedString>::from(
                                ids.into_iter().map(Into::into).collect::<Vec<_>>()
                            ))));
                        }
                        Err(error) => {
                            ui.set_provider_health(2);
                            ui.set_provider_status(format!("Connection failed: {error}").into());
                        }
                    }
                }
            });
        });
    });

    let models_for_create = model_list.clone();
    let weak = app.as_weak();
    app.on_create_provider(move |id, name, url, protocol, api_key, model_id, display, context, max_output| {
        let Some(path) = providers_path() else { return; };
        let mut data = read_providers();
        if !data.is_object() { data = json!({}); }
        let key = if id.is_empty() { name.to_lowercase().replace(' ', "-") } else { id.to_string() };

        let mut models = data.get("providers").and_then(|p| p.get(&key))
            .and_then(|p| p.get("models")).and_then(Value::as_array).cloned().unwrap_or_default();
        if let Some(ui) = weak.upgrade() {
            for discovered in ui.get_discovered_models().iter() {
                let discovered = discovered.to_string();
                if !models.iter().any(|m| m.get("id").and_then(Value::as_str) == Some(discovered.as_str())) {
                    models.push(json!({"id": discovered, "display_name": discovered,
                        "context_window": "", "max_output_tokens": "", "supports_thinking": false}));
                }
            }
        }
        let original_id = weak.upgrade().map(|ui| ui.get_provider_original_model().to_string()).unwrap_or_default();
        if !original_id.is_empty() && original_id != model_id.as_str() {
            models.retain(|m| m.get("id").and_then(Value::as_str) != Some(original_id.as_str()));
        }
        if !model_id.is_empty() {
            let old = models.iter().find(|m| m.get("id").and_then(Value::as_str) == Some(model_id.as_str()));
            let display_value = if display.is_empty() { old.and_then(|m| m.get("display_name")).and_then(Value::as_str).unwrap_or(model_id.as_str()) } else { display.as_str() };
            let context_value = if context.is_empty() { old.and_then(|m| m.get("context_window")).and_then(Value::as_str).unwrap_or("") } else { context.as_str() };
            let output_value = if max_output.is_empty() { old.and_then(|m| m.get("max_output_tokens")).and_then(Value::as_str).unwrap_or("") } else { max_output.as_str() };
            let entry = json!({"id": model_id.as_str(), "display_name": display_value,
                "context_window": context_value, "max_output_tokens": output_value,
                "supports_thinking": false});
            if let Some(existing) = models.iter_mut().find(|m| m.get("id").and_then(Value::as_str) == Some(model_id.as_str())) {
                *existing = entry;
            } else { models.push(entry); }
        }
        if !data.get("providers").map(Value::is_object).unwrap_or(false) {
            data["providers"] = json!({});
        }
        let previous_key = data["providers"][&key].get("api_key")
            .and_then(Value::as_str).unwrap_or("").to_owned();
        let saved_key = if api_key.is_empty() { previous_key } else { api_key.to_string() };
        data["providers"][&key] = json!({
            "id": key, "name": name.as_str(), "base_url": url.as_str(),
            "api_protocol": protocol.as_str(), "api_key": saved_key, "models": models
        });
        if let Err(error) = save_json(&path, &data) {
            eprintln!("Cannot save provider: {error}");
            return;
        }
        models_for_create.set_vec(
            read_providers().get("providers").and_then(Value::as_object).into_iter()
                .flat_map(|providers| providers.values())
                .flat_map(|provider| provider.get("models").and_then(Value::as_array).into_iter().flatten())
                .filter_map(|m| m.get("id").and_then(Value::as_str).map(Into::into))
                .collect::<Vec<slint::SharedString>>()
        );
        if let Some(app) = weak.upgrade() {
            if !model_id.is_empty() { app.set_active_model(model_id); }
            app.set_provider_health(0);
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

    type SharedAgent = Arc<tokio::sync::Mutex<AgentLoop<OpenAiCompatible>>>;
    let agents: Arc<Mutex<HashMap<String, SharedAgent>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let agents_for_send = Arc::clone(&agents);
    let active_cancel: Arc<Mutex<Option<CancellationToken>>> = Arc::new(Mutex::new(None));
    let cancel_for_button = Arc::clone(&active_cancel);
    app.on_cancel_prompt(move || {
        if let Ok(guard) = cancel_for_button.lock() {
            if let Some(token) = guard.as_ref() { token.cancel(); }
        }
    });

    let weak_for_send = app.as_weak();
    let cancellation_for_send = Arc::clone(&active_cancel);
    let handle = runtime.handle().clone();
    app.on_send_prompt(move |message| {
        let Some(ui) = weak_for_send.upgrade() else { return; };
        let selected_model = ui.get_active_model().to_string();
        let workspace = ui.get_active_workspace().to_string();
        let title = if ui.get_has_session() {
            ui.get_session_name().to_string()
        } else {
            message.to_string()
        };
        let session_key = format!("{workspace}///{title}");
        let providers = read_providers();
        let selected = providers.get("providers").and_then(Value::as_object)
            .and_then(|items| items.values().find_map(|provider| {
                let models = provider.get("models")?.as_array()?;
                let model = models.iter().find(|item| item.get("id").and_then(Value::as_str) == Some(selected_model.as_str()))?;
                Some((provider, model))
            }));
        let Some((provider, _model)) = selected else {
            ui.set_response_text("Select a configured model in the chat before sending.".into());
            ui.set_is_streaming(false);
            return;
        };
        if provider.get("api_protocol").and_then(Value::as_str) != Some("OpenAI-compatible") {
            ui.set_response_text("This provider protocol is not supported yet.".into());
            ui.set_is_streaming(false);
            return;
        }
        let url = provider.get("base_url").and_then(Value::as_str).unwrap_or("").to_owned();
        let saved_key = provider.get("api_key").and_then(Value::as_str)
            .filter(|key| !key.is_empty()).map(str::to_owned);
        let env_key = std::env::var("DEEPSEEK_API_KEY").ok().filter(|key| !key.is_empty());
        let insecure = saved_key.is_some();
        let key = saved_key.or(env_key);
        let Some(key) = key.filter(|key| !key.is_empty()) else {
            ui.set_response_text("Configure a chave API em Settings > Providers antes de enviar.".into());
            ui.set_is_streaming(false);
            return;
        };
        if insecure {
            ui.set_response_text("Aviso: chave salva em providers.json (texto simples).\n".into());
        }
        let token = CancellationToken::new();
        if let Ok(mut guard) = cancellation_for_send.lock() {
            if let Some(previous) = guard.replace(token.clone()) { previous.cancel(); }
        }
        let weak = weak_for_send.clone();
        let agents_for_task = Arc::clone(&agents_for_send);
        handle.spawn(async move {
            let agent = {
                let mut map = match agents_for_task.lock() {
                    Ok(map) => map,
                    Err(_) => return,
                };
                if let Some(existing) = map.get(&session_key) {
                    Arc::clone(existing)
                } else {
                    let client = match OpenAiCompatible::new(&url, key, selected_model.clone()) {
                        Ok(client) => client,
                        Err(error) => {
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = weak.upgrade() {
                                    ui.set_response_text(error.to_string().into());
                                    ui.set_is_streaming(false);
                                }
                            });
                            return;
                        }
                    };
                    let created = Arc::new(tokio::sync::Mutex::new(AgentLoop::new(client, AgentConfig {
                        model: selected_model,
                        system_prompt: "Você é um assistente de código conciso.".into(),
                        max_iterations: 1,
                    })));
                    map.insert(session_key, Arc::clone(&created));
                    created
                }
            };
            let mut agent = agent.lock().await;
            let mut events = agent.run(message.to_string(), token).await;
            while let Some(event) = events.recv().await {
                let weak = weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = weak.upgrade() {
                        match event {
                            AgentEvent::Started => { ui.set_is_streaming(true); }
                            AgentEvent::TextDelta(delta) => {
                                let mut text = ui.get_response_text().to_string();
                                text.push_str(&delta);
                                ui.set_response_text(text.into());
                            }
                            AgentEvent::Cancelled => {
                                ui.set_is_streaming(false);
                            }
                            AgentEvent::Completed => {
                                ui.set_is_streaming(false);
                            }
                            AgentEvent::Failed(message) => {
                                let text = format!("{}\\nError: {message}", ui.get_response_text());
                                ui.set_response_text(text.into());
                                ui.set_is_streaming(false);
                            }
                        }
                    }
                });
            }
        });
    });
    app.run()
}
