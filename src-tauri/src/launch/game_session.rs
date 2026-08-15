// launch/game_session.rs — 游戏会话与实例管理
// 职责：管理游戏运行实例、日志缓冲、状态上报
// 对应原项目 server/context.js 的 ctx.sessions.gameInstances + gameLogBuffer

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use serde_json::{json, Value};

/// 游戏实例
pub struct GameInstance {
    pub session_id: String,
    pub version_id: String,
    pub pid: u32,
    pub game_dir: String,
    pub start_time: Instant,
    /// 启动时的 Unix 毫秒时间戳（用于前端计算已运行时长）
    pub start_timestamp_ms: u64,
    pub log_buffer: Vec<String>,
    pub lan_port: Option<u16>,
    pub game_ready: bool,
    pub ready_time: Option<Instant>,
    pub load_stage: u32, // 0-5 阶段
    pub java_path: String,
    pub main_class: String,
}

fn now_ts_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl GameInstance {
    pub fn new(session_id: String, version_id: String, pid: u32, game_dir: String, java_path: String, main_class: String) -> Self {
        Self {
            session_id,
            version_id,
            pid,
            game_dir,
            start_time: Instant::now(),
            start_timestamp_ms: now_ts_ms(),
            log_buffer: Vec::new(),
            lan_port: None,
            game_ready: false,
            ready_time: None,
            load_stage: 0,
            java_path,
            main_class,
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "sessionId": self.session_id,
            "versionId": self.version_id,
            "pid": self.pid,
            "gameDir": self.game_dir,
            "startTime": self.start_timestamp_ms,
            "lanPort": self.lan_port,
            "gameReady": self.game_ready,
            "readyTime": self.ready_time.map(|t| t.elapsed().as_secs()).unwrap_or(0),
            "loadStage": self.load_stage,
            "running": true
        })
    }
}

/// 全局游戏实例表
static GAME_INSTANCES: Mutex<Option<HashMap<String, Box<GameInstance>>>> = Mutex::new(None);

fn init() {
    let mut g = GAME_INSTANCES.lock().unwrap();
    if g.is_none() {
        *g = Some(HashMap::new());
    }
}

/// 添加游戏实例
pub fn add_instance(instance: GameInstance) {
    init();
    let mut g = GAME_INSTANCES.lock().unwrap();
    if let Some(map) = g.as_mut() {
        map.insert(instance.session_id.clone(), Box::new(instance));
    }
}

/// 移除游戏实例
pub fn remove_instance(session_id: &str) -> Option<Box<GameInstance>> {
    let mut g = GAME_INSTANCES.lock().unwrap();
    if let Some(map) = g.as_mut() {
        return map.remove(session_id);
    }
    None
}

/// 获取所有游戏实例状态（给 GET /api/game/status 用）
pub fn get_all_status() -> Vec<Value> {
    let g = GAME_INSTANCES.lock().unwrap();
    if let Some(map) = g.as_ref() {
        map.values().map(|i| i.to_json()).collect()
    } else {
        Vec::new()
    }
}

/// 停止所有游戏实例
pub fn stop_all() -> Vec<u32> {
    let mut g = GAME_INSTANCES.lock().unwrap();
    let mut pids = Vec::new();
    if let Some(map) = g.as_mut() {
        for (_, instance) in map.iter() {
            pids.push(instance.pid);
        }
        map.clear();
    }
    pids
}

/// 更新游戏实例（回调式）
pub fn update_instance(session_id: &str, updater: impl FnOnce(&mut GameInstance)) {
    let mut g = GAME_INSTANCES.lock().unwrap();
    if let Some(map) = g.as_mut() {
        if let Some(instance) = map.get_mut(session_id) {
            updater(instance);
        }
    }
}

/// 获取游戏日志
pub fn get_logs(session_id: &str, count: usize, offset: usize) -> Vec<String> {
    let g = GAME_INSTANCES.lock().unwrap();
    if let Some(map) = g.as_ref() {
        if let Some(instance) = map.get(session_id) {
            let logs = &instance.log_buffer;
            let total = logs.len();
            let start = if offset > total { return Vec::new(); } else { offset };
            let end = if count == 0 { total } else { (start + count).min(total) };
            return logs[start..end].to_vec();
        }
    }
    Vec::new()
}

// ============================================================================
// 全局缓存：退出分析 + 持久化游戏日志
// 对应原项目 server/context.js 的 ctx.sessions.lastGameExitAnalysis + gameLogBuffer
// ----------------------------------------------------------------------------
// 问题背景：
//   - 游戏实例在进程退出后会被 remove_instance 移除，绑在实例上的 log_buffer 随之丢失，
//     导致崩溃分析、日志导出拿不到日志。
//   - 这里提供两个与实例生命周期无关的全局缓存：
//       1) LAST_GAME_EXIT_ANALYSIS：最近一次游戏退出的分析结果（含退出码/原因/建议/launchInfo/logBuffer）
//       2) PERSISTENT_LOG_BUFFER：全局游戏日志缓冲，实例移除后仍然保留（最多 5000 行）
// ============================================================================

static LAST_GAME_EXIT_ANALYSIS: Mutex<Option<Value>> = Mutex::new(None);
static PERSISTENT_LOG_BUFFER: Mutex<Vec<String>> = Mutex::new(Vec::new());
const PERSISTENT_LOG_MAX: usize = 5000;

/// 记录最近一次游戏退出分析结果（游戏进程退出时由 process_manager 调用）
pub fn set_exit_analysis(analysis: Value) {
    let mut slot = LAST_GAME_EXIT_ANALYSIS.lock().unwrap();
    *slot = Some(analysis);
}

/// 读取最近一次游戏退出分析结果
pub fn get_exit_analysis() -> Option<Value> {
    LAST_GAME_EXIT_ANALYSIS.lock().unwrap().clone()
}

/// 向全局日志缓冲追加一行（游戏进程输出时调用）
pub fn push_persistent_log(line: String) {
    let mut buf = PERSISTENT_LOG_BUFFER.lock().unwrap();
    buf.push(line);
    if buf.len() > PERSISTENT_LOG_MAX {
        let extra = buf.len() - PERSISTENT_LOG_MAX;
        buf.drain(0..extra);
    }
}

/// 读取全局日志缓冲尾部若干行
pub fn get_persistent_logs(count: usize) -> Vec<String> {
    let buf = PERSISTENT_LOG_BUFFER.lock().unwrap();
    if buf.is_empty() {
        return Vec::new();
    }
    let start = if count == 0 { 0 } else { buf.len().saturating_sub(count) };
    buf[start..].to_vec()
}

/// 清空全局日志缓冲（所有游戏退出后调用，与 Electron 版行为一致）
pub fn clear_persistent_logs() {
    PERSISTENT_LOG_BUFFER.lock().unwrap().clear();
}
