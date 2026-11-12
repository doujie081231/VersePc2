# 便携版 release 打不开（localhost 拒绝连接）修复记录

## 现象
双击 VersePC2.exe（或桌面快捷方式）后窗口里是 WebView2 的错误页：
`localhost 拒绝连接 ERR_CONNECTION_REFUSED`，界面用不了。

## 根因（两个，必须一起修）
1. **缺少 Tauri 生产构建特性**
   `src-tauri/Cargo.toml` 里没有 `[features]` 块，因此编译时 `tauri-macros` 走
   `dev: cfg!(not(feature = "custom-protocol"))` 的 dev 分支，
   `generate_context!()` 不嵌入 `frontend-dist`，窗口直接去加载
   `build.devUrl = http://localhost:1420`。没有 vite dev server → 连接被拒。
   修复：补上
   ```toml
   [features]
   default = []
   custom-protocol = ["tauri/custom-protocol"]
   ```
   并用 `cargo build --release --features custom-protocol` 构建（或 `npx tauri build`）。
2. **工作副本缺 188 个前端静态资源（约 63 MB）**
   `frontend/public` 少了 23 个 CSS（themes.css / accounts.css / panorama、icons、
   fonts、js/three.bundle.js、models、plugins 等）。缺 CSS 时首页只有半套样式，
   深色主题变白底、按钮无样式，看起来像"白屏/打不开"。
   修复：从上游仓库补齐（`git clone --depth 1` 后按缺失文件逐个复制，保留本地改动）。

## 构建
```
scripts/build-release.bat
  → npx vite build                 # 输出 frontend-dist
  → cargo build --manifest-path src-tauri/Cargo.toml --release --features custom-protocol
  → src-tauri/target/release/verse-tauri.exe（约 68.6 MB，与官方 68.4 MB 同级）
```

## 验证（CDP 实测，不是看窗口存在）
- 页面 URL 必须是 `http://tauri.localhost/`（不是 localhost:1420），
  localStorage 里只有 `http://tauri.localhost` 的 origin 记录。
- `document.styleSheets` 中 0 规则的表数量 = 0（32 个 CSS 全部加载）。
- 首页内文包含 主页/下载/资源/模组/…/云盘/反馈 完整导航。
- 编辑布局 → 「+」→ 选择器 19 项，含「下载自定义文件」，加完 `layout.visible.download = true`。
- 命名管道 `\\.\pipe\VersePC2_ipc_<USERNAME>`：`{action:"ping"}` 返回
  `{ok:true,pid,version:"1.8.6"}`；`{action:"download",url}` 返回 `{accepted:true,pending:1}`，
  前端 2 秒内把队列抽干（pending 回 0）并自动把下载控件挂到主页、填好地址。
- 真实下载：`POST /api/download-custom`（带 savePath）→ 状态 `completed 100%`，
  磁盘文件大小与源一致（512 KB 校验通过）。

## 备注
- 桌面快捷方式指向 `E:\yx\mc\Verse启动器\VersePC2.exe`，官方原版备份为
  `VersePC2.exe.官方原版.bak`。
- 应用是便携式：数据目录跟随 exe（`data/`），WebView2 用户数据在
  `%LOCALAPPDATA%\com.versepc.launcher\EBWebView`。


## 追加：为什么"修好后加号又不见了"（第二次事故）
用户在 v1.8.9 上点「编辑布局」看不到「+」。排查发现安装目录里的 exe 已被
**上游自动更新替换成官方 v1.8.9**（71,697,920 B，sha 5e8099e0…），
官方版本当然没有本项目的「+ 控件库」和下载控件。

机制：`updater.rs` 检测到远端 update.json（当时是 v1.8.9）比本地 1.8.6 新，
就**后台自动下载 68 MB 安装包**，并在**程序退出时静默安装替换自身 exe**。
只要版本号低于上游，任何一次启动都可能把定制版本覆盖掉。

### 处理
1. 版本号提到 **1.9.0**（高于远端 1.8.9），检测逻辑 `remote <= current` 直接判定"无更新"。
2. `updater.rs` 增加两个常量闸门，默认关闭静默行为：
   - `AUTO_DOWNLOAD_ON_DETECT = false`：不再"检测到就后台下载"；
   - `ALLOW_SILENT_SELF_REPLACE = false`：退出时不再自动替换 exe。
   手动「检查更新 / 下载 / 安装」按钮仍然可用，但不会无提示地覆盖定制功能。
3. 官方 v1.8.9 备份为 `VersePC2.exe.官方v1.8.9.bak`。

### 验收（实测）
- 部署后启动：updater-download.log 行数 184 → 184（**没有**再下载更新包），
  exe 的 size/sha 在启动前后完全一致（71,872,512 / f015f412…）。
- 「编辑布局」→ 工具栏出现 `#home-widget-add-btn`（26×26，坐标 763,628，在可视区内，
  深色主题下白色虚线边框清晰可见）。
- 点「+」→ 控件库 19 项，「下载自定义文件」显示为已添加（✓）。
