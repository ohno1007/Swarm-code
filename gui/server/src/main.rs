// Swarm-code desktop: a pure-Rust local web server that embeds the Material 3
// UI and exposes swarm-core over HTTP + NDJSON streaming. Being pure Rust, it
// cross-compiles from Linux to a self-contained Windows `.exe` (no system
// WebView / MSVC needed — it opens the user's browser).
//
// Keeps a console window: it shows the local URL and acts as the app's
// lifecycle (close it / Ctrl+C to stop the server).

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use swarm_core::{build_manager, keyconfig, AgentMsg, SessionManager};
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(rust_embed::RustEmbed)]
#[folder = "../dist"]
struct Assets;

struct Inner {
    manager: Option<SessionManager>,
    workspace: PathBuf,
}

struct AppState {
    inner: Mutex<Inner>,
}

impl AppState {
    async fn manager(&self) -> Result<SessionManager, Error> {
        self.inner
            .lock()
            .await
            .manager
            .clone()
            .ok_or_else(|| Error::bad("No API key configured. Open Settings and add your DeepSeek key."))
    }
}

/// Simple error → HTTP mapping.
struct Error(StatusCode, String);
impl Error {
    fn bad(msg: impl Into<String>) -> Self {
        Error(StatusCode::BAD_REQUEST, msg.into())
    }
}
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (self.0, self.1).into_response()
    }
}
type ApiResult = Result<Json<Value>, Error>;

fn parse_id(id: &str) -> Result<Uuid, Error> {
    Uuid::parse_str(id).map_err(|e| Error::bad(format!("bad session id: {e}")))
}

// ---- request bodies -------------------------------------------------------

#[derive(Deserialize, Default)]
struct KeyReq {
    key: String,
}
#[derive(Deserialize, Default)]
struct WorkspaceReq {
    path: String,
}
#[derive(Deserialize, Default)]
struct TitleReq {
    #[serde(default)]
    title: String,
}
#[derive(Deserialize)]
struct IdReq {
    id: String,
}
#[derive(Deserialize)]
struct ModelReq {
    id: String,
    model: String,
}
#[derive(Deserialize)]
struct TempReq {
    id: String,
    temp: f32,
}
#[derive(Deserialize)]
struct SendReq {
    id: String,
    text: String,
}

// ---- handlers -------------------------------------------------------------

async fn app_status(State(s): State<Arc<AppState>>) -> ApiResult {
    let inner = s.inner.lock().await;
    Ok(Json(json!({
        "hasKey": keyconfig::has_key(),
        "connected": inner.manager.is_some(),
        "workspace": inner.workspace.display().to_string(),
        "maskedKey": std::env::var(keyconfig::KEY_VAR).ok().map(|k| keyconfig::mask(&k)),
    })))
}

async fn set_key(State(s): State<Arc<AppState>>, Json(req): Json<KeyReq>) -> ApiResult {
    keyconfig::save_key(req.key.trim()).map_err(|e| Error::bad(e.to_string()))?;
    let workspace = s.inner.lock().await.workspace.clone();
    let mgr = build_manager(workspace).map_err(|e| Error::bad(e.to_string()))?;
    s.inner.lock().await.manager = Some(mgr);
    Ok(Json(json!({ "ok": true })))
}

async fn set_workspace(State(s): State<Arc<AppState>>, Json(req): Json<WorkspaceReq>) -> ApiResult {
    let p = PathBuf::from(&req.path)
        .canonicalize()
        .map_err(|e| Error::bad(format!("invalid path: {e}")))?;
    let mut inner = s.inner.lock().await;
    inner.workspace = p.clone();
    if keyconfig::has_key() {
        inner.manager = Some(build_manager(p.clone()).map_err(|e| Error::bad(e.to_string()))?);
    }
    Ok(Json(json!({ "workspace": p.display().to_string() })))
}

async fn create_session(State(s): State<Arc<AppState>>, Json(req): Json<TitleReq>) -> ApiResult {
    let m = s.manager().await?;
    let title = if req.title.is_empty() { "session".into() } else { req.title };
    Ok(Json(json!(m.create(title).await.to_string())))
}

async fn list_sessions(State(s): State<Arc<AppState>>) -> ApiResult {
    let m = s.manager().await?;
    let list: Vec<Value> = m
        .list()
        .await
        .into_iter()
        .map(|(id, title, turns)| json!({ "id": id.to_string(), "title": title, "turns": turns }))
        .collect();
    Ok(Json(json!(list)))
}

async fn session_status(State(s): State<Arc<AppState>>, Json(req): Json<IdReq>) -> ApiResult {
    let m = s.manager().await?;
    let (model, temp, (used, max)) = m
        .status(parse_id(&req.id)?)
        .await
        .ok_or_else(|| Error::bad("no such session"))?;
    Ok(Json(json!({ "model": model, "temp": temp, "used": used, "max": max })))
}

async fn set_model(State(s): State<Arc<AppState>>, Json(req): Json<ModelReq>) -> ApiResult {
    let m = s.manager().await?;
    m.set_model(parse_id(&req.id)?, req.model).await;
    Ok(Json(json!({ "ok": true })))
}

async fn set_temp(State(s): State<Arc<AppState>>, Json(req): Json<TempReq>) -> ApiResult {
    let m = s.manager().await?;
    m.set_temperature(parse_id(&req.id)?, req.temp).await;
    Ok(Json(json!({ "ok": true })))
}

/// Stream agent events as NDJSON; the final line carries the result/error.
async fn send(State(s): State<Arc<AppState>>, Json(req): Json<SendReq>) -> Response {
    let manager = match s.manager().await {
        Ok(m) => m,
        Err(e) => return e.into_response(),
    };
    let id = match parse_id(&req.id) {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };
    let text = req.text;

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AgentMsg>();
    let stream = async_stream::stream! {
        let handle = tokio::spawn(async move { manager.send_observed(id, text, tx).await });

        while let Some(msg) = rx.recv().await {
            let v = serde_json::to_value(&msg).unwrap_or(Value::Null);
            let line = json!({ "t": "msg", "msg": v }).to_string();
            yield Ok::<_, std::convert::Infallible>(axum::body::Bytes::from(line + "\n"));
        }

        let final_line = match handle.await {
            Ok(Ok(result)) => json!({ "t": "done", "result": result }),
            Ok(Err(e)) => json!({ "t": "error", "error": e.to_string() }),
            Err(e) => json!({ "t": "error", "error": format!("task failed: {e}") }),
        };
        yield Ok(axum::body::Bytes::from(final_line.to_string() + "\n"));
    };

    Response::builder()
        .header(header::CONTENT_TYPE, "application/x-ndjson")
        .body(Body::from_stream(stream))
        .unwrap()
}

// ---- static assets (embedded) --------------------------------------------

async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match Assets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            ([(header::CONTENT_TYPE, mime.as_ref())], content.data).into_response()
        }
        None => match Assets::get("index.html") {
            Some(content) => (
                [(header::CONTENT_TYPE, "text/html")],
                content.data,
            )
                .into_response(),
            None => (StatusCode::NOT_FOUND, "not found").into_response(),
        },
    }
}

#[tokio::main]
async fn main() {
    keyconfig::load_into_env();
    let workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let manager = if keyconfig::has_key() {
        build_manager(workspace.clone()).ok()
    } else {
        None
    };
    let state = Arc::new(AppState {
        inner: Mutex::new(Inner { manager, workspace }),
    });

    let app = Router::new()
        .route("/api/app_status", post(app_status))
        .route("/api/set_key", post(set_key))
        .route("/api/set_workspace", post(set_workspace))
        .route("/api/create_session", post(create_session))
        .route("/api/list_sessions", post(list_sessions))
        .route("/api/session_status", post(session_status))
        .route("/api/set_model", post(set_model))
        .route("/api/set_temp", post(set_temp))
        .route("/api/send", post(send))
        .route("/", get(static_handler))
        .fallback(get(static_handler))
        .with_state(state);

    // Bind to a free loopback port.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let addr: SocketAddr = listener.local_addr().unwrap();
    let url = format!("http://{addr}");
    println!("\n  Swarm-code");
    println!("  Open in your browser:  {url}");
    println!("  (this window keeps the app running — close it or press Ctrl+C to stop)\n");
    let _ = open::that(&url);

    axum::serve(listener, app).await.expect("server error");
}
