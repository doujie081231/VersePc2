// modloaders/mod.rs — 模组加载器模块入口
// 职责：聚合所有加载器子模块，对外暴露统一接口
//
// 架构说明：
//   每个加载器（fabric/forge/neoforge/optifine）独立成模块，
//   shared.rs 提供共享的 HTTP 工具（fetch_json、fetch_text、fetch_with_racing）。
//   fabric_api.rs 单独管理 Fabric API 模组查询（走 Modrinth API）。
//   安装逻辑暂未迁移，下次迁移。
//
//

pub mod fabric;
pub mod fabric_api;
pub mod forge;
pub mod neoforge;
pub mod optifine;
pub mod shared;
