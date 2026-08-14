<div align="center">
  <img src="frontend/img/icon.png" alt="VersePC2 Logo" width="120">
  <h1>VersePC2</h1>
  <p><b>基于 Tauri 的 Minecraft 启动器</b></p>
</div>

<p align="center">
  <img src="https://img.shields.io/badge/version-0.1.0-blue" alt="Version">
  <img src="https://img.shields.io/badge/platform-Windows-lightgrey" alt="Platform">
  <img src="https://img.shields.io/badge/license-GPL--3.0-green" alt="License">
  <img src="https://img.shields.io/badge/framework-Tauri%202-orange" alt="Framework">
</p>

---

**VersePC2** 是 VersePC 的 Tauri 重写版，用 Rust 后端 + Web 前端搭建的 Minecraft 启动器。相比 Electron 版，它更轻量、启动更快、内存占用更低，同时保留了完整的功能。

## 功能特性

### 游戏启动
- 原版、Forge、Fabric、NeoForge、OptiFine 的一键安装与启动
- 自动检测与安装 Java 运行时
- 智能内存分配与 JVM 参数优化（含 CDS / JVM 预热）
- 启动参数构建、版本独立设置（隔离、内存、命令等）
- 游戏日志实时监听与控制台输出

### 账号系统
- 微软官方账号登录（OAuth 完整流程：设备码 / 交互授权）
- 离线账号
- authlib-injector 第三方认证服务器登录
- 头像与皮肤管理（多皮肤源）

### 版本管理
- 本地 / 外部版本文件夹管理
- 跨磁盘「一键识别版本文件夹」自动扫描
- 版本独立设置（图标 / 名称 / 描述 / 收藏 / 隔离）
- 版本清理与磁盘占用管理

### 模组与整合包
- CurseForge 与 Modrinth 模组搜索、下载、安装（国内镜像提速）
- 整合包导入：CurseForge 格式 + Modrinth `.mrpack` 格式（theseus 生态）
- 模组 JAR 解析、依赖检查、批量下载
- 数据包、资源包、光影包管理

### 联机功能
- **陶瓦联机（Terracotta）**：内网穿透，无需公网 IP 即可和朋友联机，支持主机 / 客户端模式
- **局域网端口映射（Lan Portmap）**：UPnP 自动端口映射
- **私人服务器**：一键开服与管理
- **红石联机**：内网穿透联机服务

### 实用工具
- **VorTeX / 崩溃分析**：崩溃日志自动分析，定位问题原因
- **壁纸引擎**：自定义背景、视频壁纸、全景主题
- **V岛**：AI 小助手（支持多种 AI 提供商）
- **文件浏览器**：内置文件管理
- **TTS 语音合成**
- **Dino 小游戏**：离线时的小彩蛋

### 个性化与体验
- 亮色 / 暗色 / 自定义主题，支持自定义主题色
- 侧边栏滑入滑出动画
- 自定义头像、皮肤展示
- WebView2 环境自动检测

## 技术栈

| 层 | 技术 |
|----|------|
| 桌面框架 | [Tauri 2](https://tauri.app/) |
| 后端语言 | Rust（tokio 异步运行时） |
| 前端 | HTML / CSS / JavaScript + Vue 3（CDN 引入） |
| HTTP 客户端 | reqwest（rustls TLS） |
| 加密 | AES-CBC、SHA-1、SHA-256、MD5 |
| 压缩 | zip、flate2、async_zip |
| 联机 | tokio-tungstenite（WebSocket）、UPnP、EasyTier / Terracotta |
| 系统信息 | sysinfo |

## 目录结构

```
verse框架替换项目/
├── frontend/            # 前端源码（HTML / CSS / JS）
│   ├── index.html       # 主页面
│   ├── css/             # 样式（按功能拆分）
│   └── js/
│       ├── app/         # 页面逻辑（启动、版本、模组、账号等）
│       └── vue/         # Vue 页面组件
├── frontend-tauri/      # 构建时由脚本生成的前端产物（勿手动修改）
├── src-tauri/           # Rust 后端
│   ├── src/
│   │   ├── lib.rs       # Tauri 入口与命令注册
│   │   ├── main.rs
│   │   ├── api/         # HTTP-like API 路由层
│   │   ├── launch/      # 游戏启动流程（参数、进程、会话）
│   │   ├── modpack/     # 整合包导入（CurseForge / mrpack）
│   │   ├── mods/        # 模组管理
│   │   ├── modloaders/  # 加载器（Forge / Fabric / NeoForge / OptiFine）
│   │   ├── easytier/    # 陶瓦联机 / 内网穿透
│   │   ├── network/     # 网络（UPnP / 公网 IP）
│   │   ├── java.rs      # Java 检测与安装
│   │   ├── storage.rs   # 数据持久化
│   │   ├── updater.rs   # 自动更新
│   │   └── system.rs    # 环境检查（WebView2）
│   └── tauri.conf.json  # Tauri 配置
├── scripts/             # 构建脚本
├── package.json
└── update.json          # 更新清单（版本 / 下载地址 / 校验）
```

## 数据目录

所有数据跟随程序（exe）所在目录，纯便携：

```
<exe 同目录>/data/
├── settings.json            # 全局设置
├── accounts.json            # 账号
├── favorites.json           # 收藏夹
├── store.json               # KV 存储（个性化设置、主题、壁纸等）
├── external-folders.json    # 外部版本文件夹
├── versions/                # 游戏版本
├── libraries/               # 依赖库
├── assets/                  # 游戏资源
├── mods/                    # 模组
├── java/                    # Java 运行时
└── logs/                    # 日志
```

> 首次运行会自动检测旧版 Electron VersePC 的数据目录（`~/.versepc`），并迁移账号、设置、个性化配置等。

## 构建与开发

### 环境要求
- Windows 10/11（64 位）
- [Rust](https://www.rust-lang.org/)（新版稳定版）
- [Node.js](https://nodejs.org/)（≥ 18）
- WebView2 Runtime（软件会自动检测）

### 开发运行
```bash
npm install
npm run dev        # 以开发模式运行（tauri dev）
```

### 打包
```bash
npm run build          # 便携版（生成 dist/VersePC2.exe）
npm run build:setup    # NSIS 安装包
```

## 自动更新机制

- 更新清单：仓库 `update.json`（含版本号、下载 URL、文件大小、SHA-256）
- 检查：启动时自动检查，或手动触发
- 下载：多镜像加速（国内直连 + GitHub 镜像）
- 安装：下载新 exe → 写替换脚本 → 退出自身 → 脚本替换 exe → 重启
- 校验：下载后校验 SHA-256，确保文件完整

## 协议

本项目基于 [GNU General Public License v3.0](LICENSE) 发布。

```
Copyright (C) 2026 豆杰
```

---

<p align="center">
  Made with ❤️ by 豆杰
</p>