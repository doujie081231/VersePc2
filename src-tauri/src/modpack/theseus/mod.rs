// theseus/mod.rs — 事件基础设施入口
//
// 保留 theseus 生态中仍被本项目使用的最小集合：
//   error.rs — 错误类型（Error/ErrorKind）
//   event.rs — 事件系统（基于 Tauri emit），lib.rs 依赖 set_app_handle
//
// 原 theseus 的完整下载引擎（runner/install_mrpack 等）已由 mrpack_native
// 等模块替代，相关子模块已移除。

pub mod error;
pub mod event;

pub use error::{Error, ErrorKind, LabrinthError, Result};