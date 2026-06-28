// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use serde_json::{json, Value};
use swarm_core::{build_manager, keyconfig, AgentMsg, SessionManager};
use tauri::ipc::Channel;
use tauri::State;
use tokio::sync::Mutex;
use uuid::Uuid;

/// Shared application state.
struct Inner {
    manager: Option<SessionManager>,
    workspace: PathBuf,
}

struct AppState(Mutex<Inner>);

impl AppState {
    async fn manager(&self) -> Result<SessionManager, String> {
        self.0
            .lock()
            .await
            .manager
            .clone()
            .ok_or_else(|| "No API key configured. Open Settings and add your DeepSeek key.".into())
    }
}

fn parse_id(id: &str) -> Result<Uuid, String> {
    Uuid::parse_str(id).map_err(|e| format!("bad session id: {e}"))
}

// ---- commands -------------------------------------------------------------

#[tauri::command]
async fn app_status(state: State<'_, AppState>) -> Result<Value, String> {
    let inner = state.0.lock().await;
    Ok(json!({
        "hasKey": keyconfig::has_key(),
        "connected": inner.manager.is_some(),
        "workspace": inner.workspace.display().to_string(),
        "maskedKey": std::env::var(keyconfig::KEY_VAR).ok().map(|k| keyconfig::mask(&k)),
        "name": keyconfig::get("DISPLAY_NAME"),
    }))
}

#[tauri::command]
async fn set_name(name: String) -> Result<(), String> {
    keyconfig::save("DISPLAY_NAME", name.trim()).map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_key(state: State<'_, AppState>, key: String) -> Result<(), String> {
    keyconfig::save_key(key.trim()).map_err(|e| e.to_string())?;
    let workspace = state.0.lock().await.workspace.clone();
    let mgr = build_manager(workspace).map_err(|e| e.to_string())?;
    state.0.lock().await.manager = Some(mgr);
    Ok(())
}

#[tauri::command]
async fn set_workspace(state: State<'_, AppState>, path: String) -> Result<String, String> {
    let p = PathBuf::from(&path)
        .canonicalize()
        .map_err(|e| format!("invalid path: {e}"))?;
    {
        let mut inner = state.0.lock().await;
        inner.workspace = p.clone();
        if keyconfig::has_key() {
            inner.manager = Some(build_manager(p.clone()).map_err(|e| e.to_string())?);
        }
    }
    Ok(p.display().to_string())
}

#[tauri::command]
async fn create_session(state: State<'_, AppState>, title: String) -> Result<String, String> {
    let m = state.manager().await?;
    Ok(m.create(title).await.to_string())
}

#[tauri::command]
async fn list_sessions(state: State<'_, AppState>) -> Result<Vec<Value>, String> {
    let m = state.manager().await?;
    Ok(m.list()
        .await
        .into_iter()
        .map(|(id, title, turns)| json!({ "id": id.to_string(), "title": title, "turns": turns }))
        .collect())
}

#[tauri::command]
async fn session_status(state: State<'_, AppState>, id: String) -> Result<Value, String> {
    let m = state.manager().await?;
    let (model, temp, (used, max)) = m
        .status(parse_id(&id)?)
        .await
        .ok_or_else(|| "no such session".to_string())?;
    Ok(json!({ "model": model, "temp": temp, "used": used, "max": max }))
}

#[tauri::command]
async fn set_model(state: State<'_, AppState>, id: String, model: String) -> Result<(), String> {
    let m = state.manager().await?;
    m.set_model(parse_id(&id)?, model).await;
    Ok(())
}

#[tauri::command]
async fn set_temp(state: State<'_, AppState>, id: String, temp: f32) -> Result<(), String> {
    let m = state.manager().await?;
    m.set_temperature(parse_id(&id)?, temp).await;
    Ok(())
}

/// Send a message; live agent events are streamed back over `onEvent`. The
/// returned promise resolves with the final assistant text when done.
#[tauri::command]
async fn send(
    state: State<'_, AppState>,
    id: String,
    text: String,
    on_event: Channel<AgentMsg>,
) -> Result<String, String> {
    let m = state.manager().await?;
    let uuid = parse_id(&id)?;

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AgentMsg>();
    let forward = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let _ = on_event.send(msg);
        }
    });

    let result = m.send_observed(uuid, text, tx).await.map_err(|e| e.to_string());
    let _ = forward.await;
    result
}

fn main() {
    keyconfig::load_into_env();
    let workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let manager = if keyconfig::has_key() {
        build_manager(workspace.clone()).ok()
    } else {
        None
    };

    tauri::Builder::default()
        .manage(AppState(Mutex::new(Inner { manager, workspace })))
        .invoke_handler(tauri::generate_handler![
            app_status,
            set_key,
            set_name,
            set_workspace,
            create_session,
            list_sessions,
            session_status,
            set_model,
            set_temp,
            send
        ])
        .run(tauri::generate_context!())
        .expect("error while running Swarm-code");
}
