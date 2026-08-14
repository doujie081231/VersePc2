// easytier/state.rs — 陶瓦联机（Terracotta）状态管理
// 职责：维护运行状态、模式、房间码、虚拟 IP、游戏端口、原始 /state 状态等
// 对应原项目 ctx.network.terracottaStatus

use std::sync::Mutex;

use serde_json::{json, Value};

/// 最大连续崩溃自动恢复次数（对齐 electron：超过这个阈值就停止自动拉起，避免死循环）
pub const MAX_CRASH_RECOVERY: u32 = 5;

/// 陶瓦联机运行模式
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

/// 陶瓦联机运行状态
#[derive(Clone)]
pub struct EasyTierStatus {
    pub running: bool,
    pub mode: EasyTierMode,
    pub room_code: String,
    pub virtual_ip: String,
    pub game_port: u16,
    pub http_port: u16, // Terracotta HTTP API 端口
    pub player_name: String,
    pub profiles: Vec<Value>,
    pub difficulty: Option<String>,
    pub error_type: Option<String>,
    pub error_message: Option<String>,
    /// 原始 /state 响应
    pub state: Option<Value>,
    /// 原始 /state 的 index 字段
    pub state_index: i64,
    /// 崩溃恢复需要的"上次成功配置"（对齐 electron _terracottaSaved*）
    pub saved_mode: Option<EasyTierMode>,
    pub saved_room_code: String,
    pub saved_game_port: u16,
    pub saved_player_name: String,
    /// 崩溃计数（连续崩溃超过阈值则停止自动恢复）
    pub crash_count: u32,
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
            player_name: String::new(),
            profiles: Vec::new(),
            difficulty: None,
            error_type: None,
            error_message: None,
            state: None,
            state_index: -1,
            saved_mode: None,
            saved_room_code: String::new(),
            saved_game_port: 0,
            saved_player_name: String::new(),
            crash_count: 0,
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
            "httpPort": self.http_port,
            "playerName": self.player_name,
            "profiles": self.profiles,
            "difficulty": self.difficulty,
            "errorType": self.error_type,
            "errorMessage": self.error_message,
            "state": self.state,
            "stateIndex": self.state_index,
            "savedMode": self.saved_mode.as_ref().map(|m| m.as_str()),
            "crashCount": self.crash_count,
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

/// 是否正在运行（供后台轮询判断是否需要退出）
pub fn is_running() -> bool {
    init_status();
    let g = STATUS.lock().unwrap();
    g.as_ref().map(|s| s.running).unwrap_or(false)
}

/// 原始 /state 响应（供状态接口原样返回给前端）
pub fn get_raw_state() -> Option<Value> {
    init_status();
    let g = STATUS.lock().unwrap();
    g.as_ref().and_then(|s| s.state.clone())
}

/// 原始 /state 的 index
pub fn get_state_index() -> i64 {
    init_status();
    let g = STATUS.lock().unwrap();
    g.as_ref().map(|s| s.state_index).unwrap_or(-1)
}

/// 更新状态（线程安全）
pub fn update_status(updater: impl FnOnce(&mut EasyTierStatus)) {
    init_status();
    let mut g = STATUS.lock().unwrap();
    if let Some(s) = g.as_mut() {
        updater(s);
    }
}

/// 重置为空闲状态（保留安装状态 + 崩溃恢复配置）
pub fn reset_to_idle() {
    init_status();
    let mut g = STATUS.lock().unwrap();
    if let Some(s) = g.as_mut() {
        // 崩溃恢复所需字段（对齐 electron：saved_mode / saved_room / saved_port / saved_player / crash_count）
        let saved_mode = s.saved_mode.clone();
        let saved_room = std::mem::take(&mut s.saved_room_code);
        let saved_port = s.saved_game_port;
        let saved_player = std::mem::take(&mut s.saved_player_name);
        let crash_count = s.crash_count;
        *s = EasyTierStatus::default();
        s.saved_mode = saved_mode;
        s.saved_room_code = saved_room;
        s.saved_game_port = saved_port;
        s.saved_player_name = saved_player;
        s.crash_count = crash_count;
    }
}

/// 检查陶瓦联机内核是否已安装
pub fn is_installed() -> bool {
    let data_dir = crate::storage::resolve_data_dir();
    data_dir.join("terracotta").join("terracotta.exe").exists()
}

/// 陶瓦联机可执行文件路径
pub fn get_binary_path() -> std::path::PathBuf {
    crate::storage::resolve_data_dir().join("terracotta").join("terracotta.exe")
}

/// 陶瓦联机日志文件路径
pub fn get_log_path() -> std::path::PathBuf {
    crate::storage::resolve_data_dir().join("logs").join("terracotta.log")
}