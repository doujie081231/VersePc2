// download/resources_session.rs — 资源下载会话管理
// 职责：管理资源（模组/整合包/资源包/光影包/数据包）下载会话状态、进度上报、取消
//
// 与 install/session.rs 模式一致，但字段贴合资源下载场景：
//   - phase: download / install / completed
//   - fileName / packName / mcVersion / projectType
//   - files: 单文件进度列表
//   - stageHistory: 阶段历史（整合包场景）
//
// 推送事件名：resource-download-progress（前端监听后更新 UI）

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

/// 资源下载阶段
#[derive(Clone, Debug, PartialEq)]
pub enum ResourceStage {
    Downloading,
    Install,
    Completed,
    Failed,
    Cancelled,
}

impl ResourceStage {
    fn as_str(&self) -> &str {
        match self {
            ResourceStage::Downloading => "downloading",
            ResourceStage::Install => "install",
            ResourceStage::Completed => "completed",
            ResourceStage::Failed => "failed",
            ResourceStage::Cancelled => "cancelled",
        }
    }
}

/// 单文件进度
#[derive(Clone, Default)]
pub struct FileProgress {
    pub name: String,
    pub status: String, // downloading / completed / failed
    pub progress: u32,
    pub size: u64,
}

/// 资源下载会话
pub struct ResourceSession {
    pub session_id: String,
    pub status: ResourceStage,
    pub progress: u32,
    pub message: String,
    pub file_name: String,
    pub total_size: u64,
    pub downloaded: u64,
    pub download_speed: u64,
    pub bytes_downloaded: u64,
    pub project_type: String, // mod / modpack / resourcepack / shader / datapack
    pub project_id: String,
    pub pack_name: String,
    pub mc_version: String,
    pub phase: String, // download / install
    pub current_file: String,
    pub files: Vec<FileProgress>,
    pub warning: Option<String>,
    pub cancel_flag: Arc<AtomicBool>,
}

impl ResourceSession {
    fn new(session_id: String, file_name: String, total_size: u64, project_type: String, project_id: String) -> Self {
        Self {
            session_id,
            status: ResourceStage::Downloading,
            progress: 0,
            message: "下载中..".to_string(),
            file_name: file_name.clone(),
            total_size,
            downloaded: 0,
            download_speed: 0,
            bytes_downloaded: 0,
            project_type,
            project_id,
            pack_name: String::new(),
            mc_version: String::new(),
            phase: "download".to_string(),
            current_file: file_name.clone(),
            files: vec![FileProgress {
                name: file_name,
                status: "downloading".to_string(),
                progress: 0,
                size: total_size,
            }],
            warning: None,
            cancel_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 转成 JSON 给前端
    fn to_json(&self) -> Value {
        json!({
            "sessionId": self.session_id,
            "status": self.status.as_str(),
            "progress": self.progress,
            "message": self.message,
            "fileName": self.file_name,
            "totalSize": self.total_size,
            "downloaded": self.downloaded,
            "downloadSpeed": self.download_speed,
            "bytesDownloaded": self.bytes_downloaded,
            "projectType": self.project_type,
            "projectId": self.project_id,
            "packName": self.pack_name,
            "mcVersion": self.mc_version,
            "phase": self.phase,
            "currentFile": self.current_file,
            "files": self.files.iter().map(|f| json!({
                "name": f.name,
                "status": f.status,
                "progress": f.progress,
                "size": f.size,
            })).collect::<Vec<_>>(),
            "warning": self.warning,
        })
    }
}

/// 全局会话表
static SESSIONS: Mutex<Option<HashMap<String, Arc<Mutex<ResourceSession>>>>> = Mutex::new(None);

fn init_sessions() {
    let mut g = SESSIONS.lock().unwrap();
    if g.is_none() {
        *g = Some(HashMap::new());
    }
}

/// 创建新会话，返回 (session_id, cancel_flag)
pub fn create_session(
    file_name: &str,
    total_size: u64,
    project_type: &str,
    project_id: &str,
) -> (String, Arc<AtomicBool>) {
    init_sessions();
    let session_id = format!("res-{}", uuid_like());
    let session = Arc::new(Mutex::new(ResourceSession::new(
        session_id.clone(),
        file_name.to_string(),
        total_size,
        project_type.to_string(),
        project_id.to_string(),
    )));
    let cancel_flag = session.lock().unwrap().cancel_flag.clone();

    let mut g = SESSIONS.lock().unwrap();
    if let Some(map) = g.as_mut() {
        map.insert(session_id.clone(), session);
    }
    (session_id, cancel_flag)
}

/// 更新会话状态（线程安全），并推送事件给前端
pub fn update_session(app: &AppHandle, session_id: &str, updater: impl FnOnce(&mut ResourceSession)) {
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
    let _ = app.emit("resource-download-progress", &json_val);
}

/// 仅更新会话状态（不推送事件，用于后台频繁进度更新）
pub fn update_session_silent(session_id: &str, updater: impl FnOnce(&mut ResourceSession)) {
    let g = SESSIONS.lock().unwrap();
    if let Some(map) = g.as_ref() {
        if let Some(session_arc) = map.get(session_id) {
            let mut session = session_arc.lock().unwrap();
            updater(&mut session);
        }
    }
}

/// 获取会话当前状态
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
            session.status = ResourceStage::Cancelled;
            session.message = "已取消".to_string();
            return true;
        }
    }
    false
}

/// 删除会话
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

/// 生成简易唯一 ID
fn uuid_like() -> String {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:x}", now)
}
