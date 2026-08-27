// server_host_daemon.rs — 独立开服守护进程
// 职责：用 axum 在本地 127.0.0.1 提供一个 HTTP/SSE 服务，
// 复用 server_host 的传输无关核心（host_create/host_start/...），
// 由 --data-dir 共享主程序同一份 versions/libraries/servers 数据。

use std::sync::{Arc, Mutex};

use axum::{
    extract::State,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::server_host::{self, HostSink};

/// 事件扇出：每个 SSE 连接注入一个单向通道，日志/状态推给所有在线连接。
type EventBus = Arc<Mutex<Vec<mpsc::UnboundedSender<String>>>>;

/// 守护进程所用的 HostSink 实现：把日志/状态序列化成 JSON 后广播给 SSE 订阅者。
struct DaemonHostSink {
    events: EventBus,
}

impl DaemonHostSink {
    fn push(&self, payload: Value) {
        let text = payload.to_string();
        let mut live = Vec::new();
        let mut guard = self.events.lock().unwrap();
        for tx in guard.iter() {
            if tx.send(text.clone()).is_ok() {
                live.push(tx.clone());
            }
        }
        *guard = live;
    }
}

impl HostSink for DaemonHostSink {
    fn emit_log(&self, id: &str, line: &str, stream: &str) {
        self.push(json!({
            "type": "log",
            "id": id,
            "line": line.replace('\r', ""),
            "stream": stream,
            "ts": now_ms()
        }));
    }
    fn emit_status(&self, id: &str, status: &str, extra: Value) {
        let mut payload = json!({
            "type": "status",
            "id": id,
            "status": status,
            "ts": now_ms()
        });
        if let (Some(dst), Some(src)) = (payload.as_object_mut(), extra.as_object()) {
            for (k, v) in src {
                dst.insert(k.clone(), v.clone());
            }
        }
        self.push(payload);
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn get_str(v: &Value, key: &str) -> String {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("").to_string()
}

#[derive(Clone)]
struct AppState {
    sink: Arc<dyn HostSink>,
    events: EventBus,
}

// ============== HTTP 处理器 ==============

async fn api_list(State(_): State<AppState>) -> Json<Value> {
    Json(server_host::host_list().await)
}

async fn api_status(State(_): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    let sid = get_str(&body, "serverId");
    let id = if sid.is_empty() { None } else { Some(sid) };
    Json(server_host::host_status(id).await)
}

async fn api_create(State(st): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    let version_id = get_str(&body, "versionId");
    let name = get_str(&body, "name");
    let port = body.get("port").and_then(|v| v.as_u64()).unwrap_or(25565) as u16;
    let options = body.get("options").cloned().or_else(|| Some(json!({})));
    Json(server_host::host_create(&st.sink, version_id, name, port, options).await)
}

async fn api_start(State(st): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    Json(server_host::host_start(&st.sink, get_str(&body, "serverId")).await)
}

async fn api_stop(State(st): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    Json(server_host::host_stop(&st.sink, get_str(&body, "serverId")).await)
}

async fn api_command(State(st): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    Json(server_host::host_command(&st.sink, get_str(&body, "serverId"), get_str(&body, "cmd")).await)
}

async fn api_delete(State(_): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    Json(server_host::host_delete(get_str(&body, "serverId")).await)
}

async fn api_sync(State(st): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    Json(server_host::host_sync_mods(&st.sink, get_str(&body, "serverId"), get_str(&body, "clientVersionId")).await)
}

async fn api_resolve(State(_): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    Json(server_host::host_resolve_version(get_str(&body, "versionId")).await)
}

async fn api_detect(State(_): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    Json(server_host::host_detect_loader(get_str(&body, "versionId")).await)
}

/// SSE 事件流：连接后持续推送开服日志/状态事件。
async fn api_stream(State(st): State<AppState>) -> impl IntoResponse {
    let (tx, rx) = mpsc::unbounded_channel::<String>();
    st.events.lock().unwrap().push(tx);
    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv()
            .await
            .map(|s| (Ok::<_, std::io::Error>(Event::default().data(s)), rx))
    });
    let stream = Box::pin(stream);
    Sse::new(stream).keep_alive(KeepAlive::default())
}

// ============== 启动 ==============

/// 启动守护进程。绑定 127.0.0.1:<port>，注入共享数据目录，阻塞直到被取消。
pub async fn run(data_dir: std::path::PathBuf, port: u16) -> Result<(), Box<dyn std::error::Error>> {
    crate::storage::override_data_dir(data_dir);

    let events: EventBus = Arc::new(Mutex::new(Vec::new()));
    let sink: Arc<dyn HostSink> = Arc::new(DaemonHostSink { events: events.clone() });
    let state = AppState { sink, events };

    let app = Router::new()
        .route("/api/host/list", get(api_list))
        .route("/api/host/status", post(api_status))
        .route("/api/host/create", post(api_create))
        .route("/api/host/start", post(api_start))
        .route("/api/host/stop", post(api_stop))
        .route("/api/host/command", post(api_command))
        .route("/api/host/delete", post(api_delete))
        .route("/api/host/sync", post(api_sync))
        .route("/api/host/resolve", post(api_resolve))
        .route("/api/host/detect", post(api_detect))
        .route("/api/host/stream", get(api_stream))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    eprintln!("[verse-server-host] 守护进程已就绪: http://127.0.0.1:{}/api/host/stream", port);
    axum::serve(listener, app).await?;
    Ok(())
}