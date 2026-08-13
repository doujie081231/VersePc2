// easytier/state.rs — EasyTier 状态管理
// 职责：维护运行状态、模式、房间码、虚拟 IP、游戏端口等
// 对应原项目 ctx.network.terracottaStatus

use std::sync::Mutex;

use serde_json::{json, Value};

/// EasyTier 运行模式
#[derive(Clone, Debug, PartialEq)]
pub enum EasyTierMode {
    Host,
    Guest,
    Idle,
}

impl EasyTierMode {
    pub fn as_str(&self) -> &str {
        match self {
            EasyTierMode::Host => "host",
            EasyTierMode::Guest => "guest",
            EasyTierMode::Idle => "idle",
        }
    }
}

/// EasyTier 运行状态
pub struct EasyTierStatus {
    pub running: bool,
    pub mode: EasyTierMode,
    pub room_code: String,
    pub virtual_ip: String,
    pub game_port: u16,
    pub http_port: u16, // EasyTier HTTP API 端口
    pub profiles: Vec<Value>,
    pub difficulty: Option<String>,
    pub error_type: Option<String>,
    pub error_message: Option<String>,
}

impl Default for EasyTierStatus {
    fn default() -> Self {
        Self {
            running: false,
            mode: EasyTierMode::Idle,
            room_code: String::new(),
            virtual_ip: String::new(),
            game_port: 0,
            http_port: 0,
            profiles: Vec::new(),
            difficulty: None,
            error_type: None,
            error_message: None,
        }
    }
}

impl EasyTierStatus {
    /// 转成 JSON 给前端
    pub fn to_json(&self) -> Value {
        json!({
            "running": self.running,
            "mode": self.mode.as_str(),
            "roomCode": self.room_code,
            "virtualIP": self.virtual_ip,
            "gamePort": self.game_port,
            "profiles": self.profiles,
            "difficulty": self.difficulty,
            "errorType": self.error_type,
            "errorMessage": self.error_message
        })
    }
}

/// 全局状态
static STATUS: Mutex<Option<EasyTierStatus>> = Mutex::new(None);

fn init_status() {
    let mut g = STATUS.lock().unwrap();
    if g.is_none() {
        *g = Some(EasyTierStatus::default());
    }
}

/// 读取当前状态（返回 JSON 副本）
pub fn get_status() -> Value {
    init_status();
    let g = STATUS.lock().unwrap();
    g.as_ref().unwrap().to_json()
}

/// 更新状态（线程安全）
pub fn update_status(updater: impl FnOnce(&mut EasyTierStatus)) {
    init_status();
    let mut g = STATUS.lock().unwrap();
    if let Some(s) = g.as_mut() {
        updater(s);
    }
}

/// 重置为空闲状态
pub fn reset_to_idle() {
    init_status();
    let mut g = STATUS.lock().unwrap();
    if let Some(s) = g.as_mut() {
        *s = EasyTierStatus::default();
    }
}

/// 检查 EasyTier 是否已安装
pub fn is_installed() -> bool {
    let data_dir = crate::storage::resolve_data_dir();
    data_dir.join("easytier").join("easytier.exe").exists()
}

/// EasyTier 可执行文件路径
pub fn get_binary_path() -> std::path::PathBuf {
    crate::storage::resolve_data_dir().join("easytier").join("easytier.exe")
}

/// EasyTier 日志文件路径
pub fn get_log_path() -> std::path::PathBuf {
    crate::storage::resolve_data_dir().join("logs").join("easytier.log")
}
