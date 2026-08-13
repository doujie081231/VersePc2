// download/mod.rs — 下载核心模块入口
// 聚合镜像源管理 + 单流下载 + 批量下载 + 资源下载会话

pub mod batch;
pub mod chunked;
pub mod mirror;
pub mod resources_session;
pub mod single;

pub use batch::{download_asset_objects, select_asset_sources, AssetObject};
pub use single::{download_with_mirror, compute_sha1, DownloadProgress, ProgressCb};
