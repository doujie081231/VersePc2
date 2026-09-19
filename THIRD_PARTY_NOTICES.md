# 第三方组件声明（Third-Party Notices）

VersePC2 基于 GPL-3.0-only 发布，同时分发/集成了以下第三方组件。本文件列出其来源与许可证。

## 前端内嵌库（frontend/js）

| 组件 | 文件 | 许可证 |
|---|---|---|
| Vue.js | `js/vue.global.prod.js` | MIT（原文件头保留版权声明） |
| marked | `js/marked.min.js` | MIT（原文件头保留版权声明） |
| three.js | `js/three.bundle.js` | MIT |
| skinview3d | `js/skinview3d.bundle.js` | MIT |
| html-to-image | `js/vendor/html-to-image.bundle.js` | MIT |

## 后端移植/集成代码（src-tauri）

| 组件 | 位置 | 说明 |
|---|---|---|
| theseus 整合包下载引擎 | `src/modpack/theseus/` | 移植自 Modrinth theseus（[modrinth/code](https://github.com/modrinth/code)，v0.7.x 时期），原仓库以 **GPL-3.0** 发布；移植目录已随附 `LICENSE`（GPL-3.0 全文）与 `NOTICE`（来源/作者说明） |
| CurseForge / Modrinth API 客户端 | `src/modpack/`、`src/api/mods.rs` | 官方公开 API 的使用，遵循平台服务条款 |
| Wallpaper Engine Web SDK 兼容层 | `src/wallpaper_engine.rs` | 基于公开格式文档的互操作实现 |

## Rust 依赖

`src-tauri/Cargo.lock` 中列出的全部 crate 均为 MIT / Apache-2.0 / BSD 等宽松许可证，许可证文本随 cargo registry 分发。

---

**关于 GPL-3.0 的说明**：本项目以 GPL-3.0-only 许可发布（见根目录 LICENSE）。上表宽松许可证组件与 GPLv3 兼容；theseus 移植代码本身即为 GPL-3.0，同许可合并兼容。
