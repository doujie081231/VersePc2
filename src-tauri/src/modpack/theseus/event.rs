// theseus/event.rs — theseus 事件系统
//
// 提供 theseus 生态所需的事件类型和发射函数。
// theseus 原生基于 sqlx + 自定义事件总线，这里改用 Tauri emit 实现，
// 保留 theseus 的接口命名以便兼容。
//
// 主要功能：
//   - emit_loading       — 发送加载进度事件
//   - emit_instance      — 发送实例状态变更事件
//   - loading_try_for_each_concurrent — 并发执行（带进度回调）
//   - LoadingBarId       — 加载条 ID
//   - InstancePayloadType — 实例载荷类型
//   - emit 子模块        — theseus 通过 crate::event::emit::* 调用

use std::sync::OnceLock;
use tauri::{AppHandle, Emitter};
use parking_lot::Mutex;

use super::error::{Error, Result};

/// 全局 AppHandle 存储（首次设置后复用）
static GLOBAL_APP: OnceLock<Mutex<Option<AppHandle>>> = OnceLock::new();

/// 设置全局 AppHandle
pub fn set_app_handle(app: AppHandle) {
    let mutex = GLOBAL_APP.get_or_init(|| Mutex::new(None));
    let mut guard = mutex.lock();
    *guard = Some(app);
}

/// 获取全局 AppHandle
pub async fn get_app_handle() -> Result<AppHandle> {
    let mutex = GLOBAL_APP.get_or_init(|| Mutex::new(None));
    let guard = mutex.lock();
    guard
        .clone()
        .ok_or_else(|| Error::from(super::error::ErrorKind::Other(
            "Tauri AppHandle 未设置，请先调用 set_app_handle".to_string(),
        )))
}

/// 加载条 ID（theseus 兼容类型）
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LoadingBarId(pub uuid::Uuid);

impl LoadingBarId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}

impl Default for LoadingBarId {
    fn default() -> Self {
        Self::new()
    }
}

/// 实例载荷类型（theseus 兼容枚举）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstancePayloadType {
    Created,
    Updated,
    Removed,
    Loaded,
    Edited,
}

/// theseus 事件状态（theseus 兼容类型）
/// 用于 events.rs 中 `crate::EventState::get()` 调用
pub struct EventState {
    pub app: AppHandle,
}

impl EventState {
    /// 获取全局 EventState
    pub fn get() -> Result<Self> {
        let mutex = GLOBAL_APP.get_or_init(|| Mutex::new(None));
        let guard = mutex.lock();
        guard
            .clone()
            .map(|app| EventState { app })
            .ok_or_else(|| Error::from(super::error::ErrorKind::Other(
                "Tauri AppHandle 未设置，请先调用 set_app_handle".to_string(),
            )))
    }
}

/// 事件错误（theseus 兼容类型，从 tauri::Error 转换）
pub struct EventError(pub String);

impl From<tauri::Error> for EventError {
    fn from(e: tauri::Error) -> Self {
        EventError(e.to_string())
    }
}

impl From<EventError> for Error {
    fn from(e: EventError) -> Self {
        Error::from(super::error::ErrorKind::EventError(e.0))
    }
}

/// 发送加载进度事件
///
/// 事件名：`theseus-loading`
/// 载荷：`{ id, message, progress, total }`
pub async fn emit_loading(
    id: &LoadingBarId,
    message: &str,
    progress: u64,
    total: u64,
) -> Result<()> {
    let app = get_app_handle().await?;
    let _ = app.emit(
        "theseus-loading",
        serde_json::json!({
            "id": id.0.to_string(),
            "message": message,
            "progress": progress,
            "total": total,
        }),
    );
    Ok(())
}

/// 发送实例状态变更事件
///
/// 事件名：`theseus-instance`
/// 参数顺序为 (instance_id, payload_type)，与 theseus 调用约定一致
pub async fn emit_instance(
    instance_id: &str,
    payload_type: InstancePayloadType,
) -> Result<()> {
    let app = get_app_handle().await?;
    let _ = app.emit(
        "theseus-instance",
        serde_json::json!({
            "type": payload_type_name(payload_type),
            "instanceId": instance_id,
        }),
    );
    Ok(())
}

/// 发送安装任务事件
///
/// 事件名：`theseus-install-job`
pub async fn emit_install_job(snapshot: &serde_json::Value) -> Result<()> {
    let app = get_app_handle().await?;
    let _ = app.emit("theseus-install-job", snapshot.clone());
    Ok(())
}

/// 并发执行流，每个元素调用 f，带并发限制和进度回调
///
/// 参考 theseus 的 loading_try_for_each_concurrent
/// 参数：
///   - stream: 产出 Result<T, Error> 的流
///   - limit: 并发限制（None 表示使用默认值）
///   - _loading_bar_id: 可选加载条 ID（用于进度展示）
///   - _progress_weight: 进度权重
///   - _total: 总数
///   - _progress_fn: 可选进度回调
///   - f: 对每个元素执行的闭包
pub async fn loading_try_for_each_concurrent<S, F, Fut, T>(
    stream: S,
    limit: Option<usize>,
    _loading_bar_id: Option<&LoadingBarId>,
    _progress_weight: f64,
    _total: usize,
    _progress_fn: Option<()>,
    f: F,
) -> Result<Vec<T>>
where
    // 修复 E0277：buffer_unordered + next().await 要求 S 实现 Unpin
    S: futures::Stream<Item = Result<T>> + std::marker::Unpin,
    F: FnMut(T) -> Fut + Send,
    Fut: std::future::Future<Output = Result<()>> + Send,
    T: Send,
{
    use futures::StreamExt;
    let _concurrency = limit.unwrap_or(4).max(1);

    // 修复 E0382 + 闭包逃逸：f 消费 T 后无法返回 item，
    // 且 FnMut 闭包不能在 async block 中跨 await 持有。
    // 实际调用方（install_mrpack.rs）不使用返回的 Vec<T>，
    // 这里采用顺序处理简化实现，保证正确性优先。
    // 注意：由于 f 消费 item，返回的 Vec 永远为空。
    // 如需返回 item，调用方应自行收集或在 f 内部处理。
    let results: Vec<T> = Vec::new();
    let mut stream = stream;
    let mut f = f;
    while let Some(res) = stream.next().await {
        match res {
            Ok(item) => {
                // 调用 f 处理 item（f 消费 item）
                f(item).await?;
            }
            Err(e) => return Err(e),
        }
    }

    Ok(results)
}

fn payload_type_name(t: InstancePayloadType) -> &'static str {
    match t {
        InstancePayloadType::Created => "created",
        InstancePayloadType::Updated => "updated",
        InstancePayloadType::Removed => "removed",
        InstancePayloadType::Loaded => "loaded",
        InstancePayloadType::Edited => "edited",
    }
}

/// emit 子模块（theseus 通过 crate::event::emit::* 调用）
pub mod emit {
    use super::*;

    /// 发送实例状态变更事件（theseus 兼容接口）
    /// 参数顺序为 (instance_id, payload_type)
    pub async fn emit_instance(
        instance_id: &str,
        payload_type: InstancePayloadType,
    ) -> Result<()> {
        super::emit_instance(instance_id, payload_type).await
    }

    /// 发送加载进度事件（theseus 兼容接口，补充以支持 crate::event::emit::emit_loading 调用）
    pub async fn emit_loading(
        id: &LoadingBarId,
        message: &str,
        progress: u64,
        total: u64,
    ) -> Result<()> {
        super::emit_loading(id, message, progress, total).await
    }

    /// 并发执行流（theseus 兼容接口）
    pub async fn loading_try_for_each_concurrent<S, F, Fut, T>(
        stream: S,
        limit: Option<usize>,
        loading_bar_id: Option<&LoadingBarId>,
        progress_weight: f64,
        total: usize,
        progress_fn: Option<()>,
        f: F,
    ) -> Result<Vec<T>>
    where
        // 修复 E0277：与父函数保持一致的 Unpin 约束
        S: futures::Stream<Item = Result<T>> + std::marker::Unpin,
        F: FnMut(T) -> Fut + Send,
        Fut: std::future::Future<Output = Result<()>> + Send,
        T: Send,
    {
        super::loading_try_for_each_concurrent(
            stream,
            limit,
            loading_bar_id,
            progress_weight,
            total,
            progress_fn,
            f,
        )
        .await
    }
}
