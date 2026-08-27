// install/session.rs — 安装会话管理
// 职责：管理安装会话状态、进度上报、取消机制

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

/// 安装阶段
#[derive(Clone, Debug, PartialEq)]
pub enum InstallStage {
    Preparing,
    VersionJson,
    ClientJar,
    Libraries,
    Natives,
    Assets,
    Loader,
    Finalizing,
    Completed,
    Failed,
    Cancelled,
}

impl InstallStage {
    fn as_str(&self) -> &str {
        match self {
            InstallStage::Preparing => "preparing",
            InstallStage::VersionJson => "version_json",
            InstallStage::ClientJar => "client_jar",
            InstallStage::Libraries => "libraries",
            InstallStage::Natives => "natives",
            InstallStage::Assets => "assets",
            InstallStage::Loader => "loader",
            InstallStage::Finalizing => "finalizing",
            InstallStage::Completed => "completed",
            InstallStage::Failed => "failed",
            InstallStage::Cancelled => "cancelled",
        }
    }
}

/// 安装会话状态
pub struct InstallSession {
    pub session_id: String,
    pub version_id: String,
    pub stage: InstallStage,
    pub progress: u32,
    pub message: String,
    pub current_file: String,
    pub total_files: u32,
    pub completed_files: u32,
    pub speed: u64,
    pub bytes_downloaded: u64,
    pub total_bytes: u64,
    pub errors: Vec<String>,
    pub cancel_flag: Arc<AtomicBool>,
}

impl InstallSession {
    fn new(session_id: String, version_id: String) -> Self {
        Self {
            session_id,
            version_id,
            stage: InstallStage::Preparing,
            progress: 0,
            message: "准备中...".to_string(),
            current_file: String::new(),
            total_files: 0,
            completed_files: 0,
            speed: 0,
            bytes_downloaded: 0,
            total_bytes: 0,
            errors: Vec::new(),
            cancel_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 转成 JSON 给前端
    fn to_json(&self) -> Value {
        json!({
            "sessionId": self.session_id,
            "versionId": self.version_id,
            "status": self.stage.as_str(),
            "stage": self.stage.as_str(),
            "progress": self.progress,
            "message": self.message,
            "currentFile": self.current_file,
            "totalFiles": self.total_files,
            "completedFiles": self.completed_files,
            "speed": self.speed,
            "bytesDownloaded": self.bytes_downloaded,
            "totalBytes": self.total_bytes,
            "errors": self.errors,
        })
    }
}

/// 全局会话表
static SESSIONS: Mutex<Option<HashMap<String, Arc<Mutex<InstallSession>>>>> = Mutex::new(None);

/// 初始化会话表（懒加载）
fn init_sessions() {
    let mut g = SESSIONS.lock().unwrap();
    if g.is_none() {
        *g = Some(HashMap::new());
    }
}

/// 创建新会话，返回 session_id 和取消标志
pub fn create_session(version_id: &str) -> (String, Arc<AtomicBool>) {
    init_sessions();
    let session_id = format!("inst-{}", uuid_like());
    let session = Arc::new(Mutex::new(InstallSession::new(session_id.clone(), version_id.to_string())));
    let cancel_flag = session.lock().unwrap().cancel_flag.clone();

    let mut g = SESSIONS.lock().unwrap();
    if let Some(map) = g.as_mut() {
        map.insert(session_id.clone(), session);
    }
    (session_id, cancel_flag)
}

/// 更新会话状态并推送进度事件给前端
pub fn update_session(app: &AppHandle, session_id: &str, updater: impl FnOnce(&mut InstallSession)) {
    let json_val = {
        let g = SESSIONS.lock().unwrap();
        if let Some(map) = g.as_ref() {
            if let Some(session_arc) = map.get(session_id) {
                let mut session = session_arc.lock().unwrap();
                updater(&mut session);
                session.to_json()
            } else {
                return;
            }
        } else {
            return;
        }
    };
    // 推送给前端
    let _ = app.emit("install-progress", &json_val);
}

/// 获取会话当前状态（给 GET /api/install-progress 用）
pub fn get_session_status(session_id: &str) -> Option<Value> {
    let g = SESSIONS.lock().unwrap();
    if let Some(map) = g.as_ref() {
        if let Some(session_arc) = map.get(session_id) {
            let session = session_arc.lock().unwrap();
            return Some(session.to_json());
        }
    }
    None
}

/// 取消会话
pub fn cancel_session(session_id: &str) -> bool {
    let g = SESSIONS.lock().unwrap();
    if let Some(map) = g.as_ref() {
        if let Some(session_arc) = map.get(session_id) {
            let mut session = session_arc.lock().unwrap();
            session.cancel_flag.store(true, Ordering::SeqCst);
            session.stage = InstallStage::Cancelled;
            session.message = "已取消".to_string();
            return true;
        }
    }
    false
}

/// 删除会话（安装完成后清理）
pub fn remove_session(session_id: &str) {
    let mut g = SESSIONS.lock().unwrap();
    if let Some(map) = g.as_mut() {
        map.remove(session_id);
    }
}

/// 检查是否已取消
pub fn is_cancelled(cancel_flag: &Arc<AtomicBool>) -> bool {
    cancel_flag.load(Ordering::SeqCst)
}

/// 生成简易唯一 ID（不用 uuid crate，省依赖）
fn uuid_like() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:x}", now)
}
