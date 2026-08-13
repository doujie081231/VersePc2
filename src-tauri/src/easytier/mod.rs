// easytier/mod.rs — EasyTier 虚拟局域网模块入口
// 职责：EasyTier 子进程管理、状态查询、HTTP 控制、日志收集
// 对应原项目 server/terracotta/ (Terracotta) 模块
//
// EasyTier 是一款开源 P2P 虚拟局域网工具，本项目通过子进程方式调用其 CLI：
//   - 主机模式：easytier-core --network <room> --listener tcp://0.0.0.0:<port>
//   - 客户端模式：easytier-core --network <room> --peers <invite_code>
//
// 启动后通过 HTTP API (默认 11010) 查询状态、节点、日志

pub mod process;
pub mod state;

pub use process::*;
pub use state::*;
