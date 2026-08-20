// launch/mod.rs — 游戏启动模块入口
// 职责：聚合启动相关子模块
// 对应原项目 server/launch/index.js

pub mod args_builder;
pub mod dep_check;
pub mod exit_code;
pub mod game_session;
pub mod java_scan;
pub mod memory;
pub mod process_manager;

pub use args_builder::build_launch_arguments;
pub use dep_check::{check_dependencies, DepCheckResult};
pub use exit_code::{analyze_exit_code, ExitAnalysis};
pub use game_session::{
    add_instance, get_all_status, get_logs, remove_instance, stop_all, update_instance,
    GameInstance,
};
pub use java_scan::{get_java_version_range, should_skip_system_scan, JavaCandidate};
pub use memory::{
    resolve_max_memory, resolve_memory_mode, should_run_memory_optimize,
    DEFAULT_LEGACY_MAX_MEMORY, MemoryMode,
};
pub use process_manager::{do_launch, kill_game, list_running_games};
