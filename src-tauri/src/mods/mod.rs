// mods/mod.rs — 模组管理模块入口
// 职责：JAR 文件解析、图标提取与缓存、已安装模组列表扫描、更新检查
// 对应原项目 server/mods.js
//
// JAR 解析流程：
//   1. 用 zip crate 打开 JAR 文件
//   2. 优先读 fabric.mod.json（Fabric 模组）
//   3. 其次读 META-INF/mods.toml（Forge 1.13+）
//   4. 最后读 META-INF/neoforge.mods.toml（NeoForge）
//   5. 提取图标文件，按 MD5(jarPath + iconPath) 缓存到 icon-cache/ 目录
//
// 已安装模组列表扫描：
//   - 版本隔离：versions/<id>/mods/
//   - 共享：gameDir/mods/
//   - .minecraft：home/.minecraft/mods/
//   - 检测重复模组 + 冲突模组组

pub mod jar;
pub mod list;
pub mod update;

// re-export 主要接口，外部通过 mods::xxx 调用
pub use list::{get_installed_mods, resolve_saves_dir};
pub use update::check_mod_updates;
