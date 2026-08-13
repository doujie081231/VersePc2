// modloaders/mod.rs — 模组加载器模块入口
// 职责：聚合所有加载器子模块，对外暴露统一接口
// 对应原项目 server/modloaders/index.js（仅版本查询部分）
//
// 架构说明：
//   每个加载器（fabric/forge/neoforge/optifine）独立成模块，
//   shared.rs 提供共享的 HTTP 工具（fetch_json、fetch_text、fetch_with_racing）。
//   fabric_api.rs 单独管理 Fabric API 模组查询（走 Modrinth API）。
//   安装逻辑（install*）暂未迁移，下次迁移。
//
// 与原项目对应关系：
//   - getFabricLoaderVersions / getFabricLoaderVersionsForGame  → fabric.rs
//   - getNeoForgeVersionsForGame                                  → neoforge.rs
//   - Forge 版本查询（路由层内联）                                  → forge.rs
//   - OptiFine 版本查询（路由层内联）                               → optifine.rs
//   - Fabric API 版本查询（路由层内联）                             → fabric_api.rs
//   - 共享 HTTP 工具（http-client.js）                             → shared.rs

pub mod fabric;
pub mod fabric_api;
pub mod forge;
pub mod neoforge;
pub mod optifine;
pub mod shared;
