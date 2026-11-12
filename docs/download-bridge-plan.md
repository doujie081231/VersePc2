# 浏览器下载接管 — 实施记录

> 目标：像 IDM / NDM 一样，在浏览器点下载 → 召唤 VersePC2 → 地址/文件名/目录填好 → 由你点「开始下载」。
> 红线：**只填表，绝不自动开始下载**。

---

## 一、用户视角：三步走完

1. 打开 VersePC2 → 设置 → 其他设置 → **浏览器下载接管**
2. 点 **一键安装**
3. 浏览器弹出扩展页 → 点「加载解压缩的扩展」（或重启浏览器让它自动启用）

扩展本体**已打包进 exe**，不需要另外下载任何东西。

---

## 二、一键安装实际做了什么

点按钮后，后端按顺序执行四件事：

| 步骤 | 动作 | 位置 |
|---|---|---|
| 1 | 把内嵌的扩展文件释放到磁盘 | `<data>/extension/` |
| 2 | 注册原生消息宿主（写清单 + 8 个注册表项） | `HKCU\Software\{Google\Chrome,Microsoft\Edge,...}\NativeMessagingHosts\` |
| 3 | 写浏览器「外部扩展」注册表 | `HKCU\Software\{Microsoft\Edge,Google\Chrome}\Extensions\<id>` |
| 4 | 尝试带 `--load-extension` 启动浏览器 | — |

**关于第 4 步的诚实说明**：`--load-extension` 有两个现实限制——
Chrome 137+ 正式版已移除该开关；且浏览器已在运行时参数会被现有实例吞掉。
因此它只是**附加尝试**，失败时会：
- 打开浏览器扩展管理页
- 把扩展目录路径复制到剪贴板
- 在设置页显示明确的「需要手动加载」提示

绝不谎报「已安装成功」。

---

## 三、文件清单

### 后端（src-tauri/src）

| 文件 | 行数 | 作用 |
|---|---|---|
| `bridge/mod.rs` | 238 | 启动参数解析、待处理队列、宿主模式判定 |
| `bridge/ipc.rs` | 234 | 单实例命名管道 `\\.\pipe\VersePC2_ipc_<用户名>` |
| `bridge/native_host.rs` | 172 | Native Messaging 宿主（stdio 长度帧） |
| `bridge/register.rs` | 307 | 原生宿主注册/卸载/状态/自愈 |
| `bridge/extension.rs` | 636 | **扩展内嵌 + 释放 + 注册 + 启动浏览器** |
| `api/download_custom.rs` | 600 | 下载接口六件套 |

### 前端（frontend/src）

| 文件 | 作用 |
|---|---|
| `features/home/home-widgets.js` | 控件注册表、布局 v2 持久化（含 v1 迁移） |
| `features/home/download-bridge.js` | 收取下载请求 → 自动挂控件 → 填表 |
| `features/settings/browser-download.js` | 设置页一键安装/卸载/状态 |
| `vue/pages/home-download-widget.js` | 主页下载迷你卡 |
| `vue/pages/page-home.js` | 「+」控件库、可选控件拖动 |
| `features/versions/custom-download.js` | 下载业务（修好了原本 404 的接口调用） |

### 扩展（extension/，全部编进 exe）

| 文件 | 作用 |
|---|---|
| `manifest.json` | MV3 清单，`key` 字段锁定扩展 ID |
| `background.js` | 拦截下载 → 转交宿主 |
| `popup.html/js` | 工具栏弹窗（状态灯、开关、重试） |
| `options.html/js` | 完整设置页 |
| `README-INSTALL.txt` | 手动安装说明（释放到 `<data>/安装说明-浏览器扩展.txt`） |

---

## 四、协议约定

### 命名管道（单实例）

```
请求: {"action":"download","url":"...","filename":"...","folder":"..."}\n
响应: {"ok":true,"accepted":true,"pending":1}\n
```

首实例在 setup 时起监听；抢不到名字说明已有实例，安静退出。
二次启动先尝试转发，成功则本进程直接返回，不开窗口。

### 原生消息宿主

浏览器拉 `VersePC2.exe` 时传的是**扩展来源**（`chrome-extension://<id>/`），
不是 `--native-host`。`is_native_host_mode()` 两种形态都认——
漏掉这条会导致「一点下载就把启动器主界面拉起来」。

宿主支持 action：`ping` / `download`。
管道不通 → 用 `--download/--filename/--folder` 冷启动自己。
**宿主不写 stdout 任何日志**（协议独占），全部走 eprintln。

### 关键常量

| 项 | 值 |
|---|---|
| Host 名 | `cc.versepc.download_bridge` |
| 扩展 ID | `pfmgdfekkaaigjjigfgljcmnebbhlfoa`（由 manifest.key 推导） |
| 清单路径 | `<data>/bridge/cc.versepc.download_bridge.json`（无 BOM） |
| 清单 path | 直接指向 `VersePC2.exe`（不需要 .cmd 转发器） |

> 改扩展 `manifest.json` 的 `key` 会改变扩展 ID，必须同步改
> `bridge/register.rs` 的 `DEFAULT_ALLOWED_ORIGINS` 与 `bridge/extension.rs` 的 `EXTENSION_ID`。
> 单元测试 `manifest_key_derives_the_expected_extension_id` 会拦住这种不一致。

---

## 五、验证清单

| # | 检查项 | 方法 | 预期 |
|---|---|---|---|
| 1 | 后端编译 | `cargo check --manifest-path src-tauri/Cargo.toml` | 无 error |
| 2 | 后端单测 | `cargo test --manifest-path src-tauri/Cargo.toml` | 全绿 |
| 3 | 前端门禁 | `npm ci && npm run verify` | 全绿 |
| 4 | 单实例 | 连点两次 exe | 任务管理器只有一个进程 |
| 5 | 冷启动填表 | 关掉启动器后执行 `VersePC2.exe --download https://example.com/a.zip` | 窗口出现即已填好 |
| 6 | 热启动转发 | 启动器开着时再执行同一条命令 | 秒退，首实例弹出填好 |
| 7 | 不自动下载 | 填表后等 10 秒 | 磁盘上不该出现文件 |
| 8 | 一键安装 | 设置页点「一键安装」 | 扩展目录生成，浏览器被拉起 |
| 9 | 宿主连通 | 扩展弹窗点「重新检测」 | 两个灯变绿 |
| 10 | 主页控件 | 编辑布局 → 点「+」→ 添加「下载」 | 控件出现且可拖动 |
| 11 | 恢复默认布局 | 个性化页点恢复 | 下载控件仍在，仅位置回默认 |

---

## 六、已知限制（都会如实反馈给用户，不隐藏）

| 场景 | 表现 | 处理 |
|---|---|---|
| 浏览器正在运行 | `--load-extension` 被忽略 | 打开扩展页 + 复制目录 + 提示手动加载 |
| Chrome 137+ 正式版 | 该开关已移除 | 同上，走「外部扩展注册表 + 重启」路径 |
| 带 Cookie 的网盘链接 | 启动器拿不到登录态 → 403 | 与 PCL 版同病，属预期 |
| `blob:` / `data:` 下载 | 无法交接 | 自动交回浏览器 |
| 用户挪动安装目录 | 清单 path 失效 | 启动时 `self_heal()` 自动重写 |


---

## 七、验证结果（2026-11-08 实测）

全部在真机跑通，不是静态检查。

### 前端（Node 24.22.1）

| 项 | 命令 | 结果 |
|---|---|---|
| 单元测试 | `npx vitest run` | **1450 passed / 98 files** ✅ |
| ESLint | `npx eslint frontend/src scripts/*.mjs vite.config.mjs` | **0 error / 0 warning** ✅ |
| 构建 | `npx vite build` | **成功，369ms** ✅ |
| 架构护栏 | `node scripts/check-architecture.mjs --strict` | **EXIT=0** ✅ |
| 测试密度 | `node scripts/check-test-density.mjs` | **通过**（features/home 2 文件 / 19 用例）✅ |
| 覆盖率棘轮 | `npm run test:coverage` + `check-coverage.mjs` | **通过**：core 91.41% / features 56% / total 57.61% ✅ |

### 后端（Rust 1.99.1 + MSVC 14.44 + Windows SDK 10.0.26100）

| 项 | 命令 | 结果 |
|---|---|---|
| 编译 | `cargo check` | **0 error 0 warning** ✅ |
| 单元测试 | `cargo test --lib` | **183 passed / 0 failed**（其中 36 个是本次新增）✅ |
| Clippy | `cargo clippy --all-targets -- -D warnings` | **0 warning** ✅ |
| Release 构建 | `cargo build --release` | **成功，5m17s，17.09 MB exe** ✅ |
| 集成测试 | `cargo test --lib ... -- --ignored` | **prepare_roundtrip ok** ✅ |

### 端到端实测

| # | 场景 | 方法 | 结果 |
|---|---|---|---|
| 1 | 原生消息协议 | 手工构造 21 字节长度帧喂给 `--native-host` | 收到 69 字节合法响应，stderr 干净 ✅ |
| 2 | Chromium 调用形态 | 传 `chrome-extension://<id>/` 作唯一参数 | 走宿主模式，不开窗口 ✅ |
| 3 | 冷启动 | `--download <url> --filename <n>` | 窗口正常启动、进程存活 ✅ |
| 4 | 单实例转发 | 启动器在跑时再执行一次 | **172ms 转发后退出，进程数仍为 1** ✅ |
| 5 | 不擅自安装 | 首次启动检查 `data/extension` | 目录不存在（符合设计）✅ |
| 6 | 一键安装 | `prepare_roundtrip` 集成测试 | 释放 9 个文件 + 写 4 个扩展注册表 + 8 个宿主注册表 ✅ |
| 7 | 卸载 | 同上测试尾部 | 文件与 12 个注册表项全部清干净 ✅ |

### 测试过程中修掉的 4 个真实缺陷

| # | 缺陷 | 影响 | 修复 |
|---|---|---|---|
| 1 | 子进程继承 stdio | 从宿主拉起浏览器时浏览器持有 stdout 管道，Chrome 等不到 EOF → 扩展侧「宿主超时」 | 所有 `Command` 加 `detach()`：stdin/stdout/stderr 全部 `Stdio::null()` + `CREATE_NO_WINDOW` |
| 2 | `guess_file_name` 把主机名当文件名 | `https://example.com` → 文件名变成 `example.com` | 先剥掉 `scheme://host` 只留路径，无路径则回落 `download.bin` |
| 3 | `prepare()` 漏写扩展注册表 | 「准备好」状态不完整，`status` 显示 `registryCount: 0` | 补上 `write_extension_registry` |
| 4 | 卸载不清理说明文件 | 残留 `安装说明-浏览器扩展.txt` | 卸载时一并删除 |

### 环境准备说明

本机原先没有 Rust / Node / MSVC，本次验证时安装：

```powershell
winget install OpenJS.NodeJS.LTS                                    # Node 24.22.1
# rustup-init 从 USTC 镜像下载（官方源 12KB/s，镜像秒下）
# cargo 源替换为 USTC sparse 镜像
winget install Microsoft.VisualStudio.2022.BuildTools `
  --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

> 编译时必须先 `call vcvars64.bat`，否则 cargo 找不到 `link.exe`（Rust 不会自动探测）。


## 扩展打包（crx + zip）交付记录
- 旧 manifest.key 没有对应私钥，无法为旧 ID 签 crx → 重新生成 RSA-2048 密钥，
  manifest.key 换成新公钥，扩展 ID 变为 `pfmgdfekkaaigjjigfgljcmnebbhlfoa`。
- 同步替换 ID 的四处代码/文档：`bridge/register.rs`（allowed_origins）、
  `bridge/mod.rs`（单元测试参数）、`bridge/extension.rs`（EXTENSION_ID）、
  `extension/README-INSTALL.txt`、`docs/download-bridge-plan.md`，并重新编译 exe。
- CRX3 由脚本手工封装：`Cr24` + version=3 + header 长度 +
  CrxFileHeader{sha256_with_rsa{pubkey, sig}, signed_header_data{crx_id}} + zip，
  签名对象为 `"CRX3 SignedData\0" + uint32le(len(shd)) + shd + zip`。
  自检：magic/version/zip 字节一致/公钥一致/签名验证通过/crx_id == sha256(pubkey)[0..16]。
- 顺带修复：`submit_download()` 之前无论窗口在不在都先入队，
  导致浏览器点一次下载就在队列里留一条，下次启动重复填表。
  改成「有窗口只推事件、无窗口才入队」，实测 pending 始终 0、界面照常填表。
- 交付验证：以 Chrome 的方式 `VersePC2.exe chrome-extension://<ID>/` 启动宿主，
  stdin 写 4 字节长度 + JSON，收到 `{ok:true,version:"1.9.0"}`；
  download 帧收到 `{accepted:true}`，启动器下载卡自动出现并填好
  地址/文件名/保存目录，状态为「待确认」，不会自动开始。


## 第二轮修复（用户反馈：Edge 里点下载没被拦，浏览器直接下了）
### 定位
读 `%LOCALAPPDATA%\Microsoft\Edge\User Data\Default\Local Extension Settings\<扩展ID>\`
里的 leveldb：**没有 lastUrl / lastName**。而 `handleIntercept()` 第一件事就是写这两个键，
说明 `chrome.downloads.onDeterminingFilename` 在 Edge 157 上**根本没触发**，
不是宿主调用失败。同时确认扩展本体是装了、已启用的（Secure Preferences:
location=4 已解压、permissions 含 downloads/nativeMessaging、SW 已启动）。

### 修法
1. `background.js` 增加 `chrome.downloads.onCreated`（下载一创建就拦，Chromium 必然触发），
   `onDeterminingFilename` 降级为兜底；两钩子按 `downloadId` 去重。
2. 取消下载的时机后移：先判断开关/协议，确定要接管才 `cancel+erase`，
   避免开关关闭时把用户下载干掉。
3. 回落保护：`reDownloadToBrowser` 记录的 URL 20 秒内不再被拦截，杜绝死循环。
4. 弹窗新增「拦截记录」诊断行（`getLastAttempt` 消息 + `chrome.storage.local.lastAttempt`），
   显示 成功/失败/放行 + 原因。
5. `register.rs` 的 `DEFAULT_ALLOWED_ORIGINS` 同时放行新旧两个扩展 ID
   （`pfmgdfekkaaigjjigfgljcmnebbhlfoa` 与旧版 `bcgdkpfgcbfjjhnahaflogmfkkkdiddi`）。
6. 版本升到 1.1.0（manifest / EXTENSION_VERSION 同步）。

### 验证
- 用 node:vm + 假 chrome API 跑了 8 条逻辑用例（全部 PASS）：
  宿主成功→只取消不回落；宿主失败→取消+回落+通知+记录 fail；
  开关关闭→不取消不连接；blob:→放行；两钩子同 id 只处理一次；
  回落下载不被二次拦截；getLastAttempt 可读；extension 自身 URL 跳过。
- 宿主协议实测：`VersePC2.exe chrome-extension://<新ID>/ ` framed ping →
  `{ok:true,version:"1.9.0"}`，download → `{accepted:true}`；
  旧 ID 同样返回 ok（明确 permitted 了两个来源）。
- 端到端：宿主 download 帧 → 启动器主页下载卡出现并填好
  `https://example.com/newid.zip`，状态「待确认」，未自动开始。
- crx 重新签名并复验：magic/version3、body==zip、签名与 crx_id 全部通过。


## 第三轮修复（用户要求：不先打开启动器，点下载就自动拉起软件）
### 现象
宿主 `cold_start()` 用 `Command::spawn` 拉起启动器，实测**进程活不过 1 分钟，
MainWindowHandle 为 0（窗口没出来）**。用户侧表现就是"点了下载，软件好像动了但没界面"。

### 根因
宿主进程是浏览器用 stdio 拉起的，Chromium 会把它放进 Job Object 并设置
kill-on-close。`Command::spawn` 的子进程默认继承该 job，浏览器/宿主一收工，
启动器就被连带清掉；同时子进程继承了宿主的 stdin/stdout 管道句柄，
WebView2 初始化也会受影响。

### 修法
`cold_start()` 改为优先走 `ShellExecuteW(open, exe, "--download \"url\" --filename \"name\"", cwd)`：
由 shell 拉起，等价于用户双击，天然在 job 之外；
外层的 `Stdio::null()` 与 `CREATE_BREAKAWAY_FROM_JOB` 只作为 ShellExecute 失败时的兜底。
扩展侧在收到 `via: "cold-start"` 时弹一条通知「正在启动 VersePC2…」，
并把下载请求的超时从 20s 放宽到 45s（冷启动要等 WebView2 起来）。

### 验证（实测）
- 前置：`taskkill` 后确认 0 个 VersePC2 进程、命名管道 ENOENT。
- 以浏览器方式拉起宿主（`VersePC2.exe chrome-extension://<ID>/`）并发 download 帧
  → 宿主回 `{ok:true, via:"cold-start"}`。
- 5 秒内出现 `pid=5592 hwnd=1509954 title="VersePC2 - Minecraft Launcher"`；
  连续监控 180 秒进程与窗口句柄始终有效。
- 冷启动实例的 CDP DOM 实测：
  `inputs = ["https://example.com/dom-proof.zip", "...\data\downloads", "dom-proof.zip"]`，
  卡片状态「待确认」，提示语「开始下载始终由你确认」——
  自动拉起、自动填表、不自动开始下载，三条同时成立。


## 第四轮修复（用户反馈：下载 exe 却把文件名写成别的，格式丢了）
### 现象
浏览器里下的是 `.exe`，启动器卡片里的文件名却没有对应后缀，落地文件也不是 exe。

### 根因
`handle_start()` 只有一条取名路径：`guess_file_name(url)` —— 取 URL 最后一个路径段。
遇到 `/download?id=123`、`/get`、`/api/file` 这类端点（CDN、网盘、GitHub release 都这么干），
路径段里根本没有扩展名，于是名字就退化成了 `download` / `opaque` / `file`。
而真实文件名往往在 **响应头 Content-Disposition** 或 **302 之后的最终 URL** 里。

### 修法（后端 `api/download_custom.rs`）
取名优先级改为：
1. 请求里显式给的 `fileName`（用户手填的）
2. HEAD（失败则 `Range: bytes=0-0` 的 GET）探测：
   `Content-Disposition` 的 `filename*=UTF-8''...`（RFC 5987，中文不乱码）→ `filename=...`
3. 跟随重定向后的**最终 URL** 最后一个路径段
4. 响应的 `Content-Type` → 扩展名映射（zip/jar/exe/msi/pdf/7z/rar/tar/gz…；`text/html` 不猜）
5. 原始 URL 路径段 → 最后才 `download.bin`

新增函数：`resolve_file_name` / `ensure_extension` / `has_extension` / `url_looks_like_file`
/ `url_last_segment` / `probe_remote_name` / `parse_content_disposition` / `ext_from_content_type`。
探测失败一律静默降级，绝不让取名环节拖垮下载。

### 修法（前端）
- `applyPendingDownload`：文件名用 `withExtension(req.filename, req.url)`，
  扩展只给 URL 时也能从 URL 补出扩展名。
- `startCustomDownload`：提交前补扩展名；**创建任务成功后把后端解析出的真实
  `result.fileName` 回填到输入框**，保证"界面显示的名字 = 磁盘上的名字"。

### 验证（实测，本地 8770 测试服务器）
| 场景 | 结果 |
|---|---|
| 直链 `/files/setup.exe` | `setup.exe` |
| `/download?id=123` + `Content-Disposition: filename*=UTF-8''安装包.exe` | `安装包.exe` |
| `/get` → 302 → `/files/pkg.zip` | `pkg.zip` |
| 无任何线索 + `Content-Type: application/zip` | `opaque.zip` |
| 无任何线索 + `Content-Type: application/x-msdownload` | `opaque-exe.exe` |
| 用户手填「我的安装包」（无后缀） | `我的安装包.exe` |
| 用户手填 `custom-name.exe` | `custom-name.exe`（原样保留） |

UI 端到端：卡片填 `http://127.0.0.1:8770/download?id=uiflow` + 文件名留空
→ 点「开始下载」→ 输入框与磁盘同时变成 `安装包.exe`，状态「已完成」。
Rust 单元测试 20 个全过（新增 7 个覆盖 `has_extension` / `url_last_segment` /
`parse_content_disposition` / `ext_from_content_type`）。


## 第五轮：扩展加「选择启动器位置」（一键扫描 + 手动指定）
### 为什么必须由启动器来扫
浏览器扩展没有任何文件系统权限，Chrome/Edge 的 file picker 也不会把真实路径回传，
所以"在本机找 VersePC2.exe"只能交给原生宿主（就是 exe 自己）来做。

### 新增 `src-tauri/src/bridge/scan.rs`
扫描来源（按可靠性）：
1. 当前进程自身（宿主本身就是启动器 —— 最可靠）
2. 正在运行的 VersePC2 进程（`sysinfo` 0.30，`refresh_processes()` 无参）
3. 注册表：卸载表 DisplayIcon/InstallLocation、App Paths（HKCU + HKLM + WOW6432）
4. 常见目录：LOCALAPPDATA、APPDATA、Program Files(86)、桌面、下载、开始菜单、
   便携版 data 的上一级
5. 便携版浅层递归（深度 3，跳过 node_modules/windows/target 等）

结果按 mtime 倒序，自身路径标 `recommended: true`。
路径统一剥掉 `\\?\` 前缀（canonicalize 会加，写进宿主清单会失效）。

能力检测 `exe_contains(exe, b"cc.versepc.download_bridge")`：
官方原版/旧版没有这段字符串，会被标 `supportsBridge: false`。

### 宿主新 action
- `scanLauncher` → `{ok, count, self, candidates:[{path,size,mtime,recommended,supportsBridge}]}`
- `probeLauncher {path}` → 校验存在性、MZ 头、是否带 bridge；目录也能传（自动找里面的 exe）
- `useLauncher {path}` → 校验通过后调 `register::install_for(exe)` 把宿主清单指向该路径

`register.rs` 抽出 `install_for(exe, extra_origin)`，`install()` 变成它的薄封装。

### 安全闸（实测发现并修掉）
第一版 `useLauncher` 只看 `ok`，结果 `C:\Windows\notepad.exe` 也能被设成宿主
（是合法 PE，但点下载必失败）。现在 `supportsBridge == false` 直接拒绝并回传原因。

### 扩展 UI（v1.2.0）
- options 页新增「启动器位置」卡片：一键扫描 → 候选列表（推荐 / 无接管模块 标签）→ 点行选用；
  手动选择支持"拖 exe 到输入框"自动取路径（Chromium 的 drag-drop 会给 `file.path`），
  也支持直接粘贴；清除按钮。
- popup 同样有「一键扫描 / 手动指定…」（弹窗里用 prompt 输入路径）。
- 新消息：`scanLauncher` / `probeLauncher` / `useLauncher` / `getLauncherPath`。

### 验证
- `scan.rs` 单测 6 个全过（含 `strip_unc`、`probe_flags_non_bridge_exe`）。
- 宿主协议实测：`scanLauncher` 扫到 1 个候选并标推荐/有接管；
  `probeLauncher` 对自身 ok+有接管、对 notepad 正确报"缺少接管模块"、对不存在路径报"文件不存在"。
- `useLauncher` 实测：notepad 被拒绝、自身路径成功改写宿主清单、再改回来正常。
- options 页用无头 Edge + 假 chrome API 跑通：扫描出 2 行、标签正确、
  点选后提示「已设为：D:\VersePC2.exe」、消息序列
  `getLauncherPath → testHost → scanLauncher → useLauncher → getLauncherPath → testHost`。


## 第六轮：卡片显示 download.bin（文件名在点下载前就是错的）
### 现象（用户截图）
抖音下载地址 `https://www.douyin.com/download/pc/obj/<hash>`，
卡片文件名显示 `download.bin`，格式完全不对。

### 三个原因叠在一起
1. **扩展编了个假名字**：`background.js` 里 `baseName(item.filename || 'download.bin')`，
   而 `onCreated` 阶段 `item.filename` 经常是空的 → 于是把 `download.bin` 当真名发出去。
2. **前端把 .bin 当成合法扩展名**：`withExtension()` 的正则 `\.[A-Za-z0-9]{1,8}$`
   会认为 `download.bin` 已经带扩展名，于是原样保留。
3. **名字只在点「开始下载」时才被后端纠正**：后端的 `resolve_file_name` 已经能拿到
   Content-Disposition，但界面在点之前一直是错的。

### 修法
**扩展**：不再编假名。`item.filename` 为空就发空串；`download/file/unknown(.bin)` 一律视为空。
取消下载后补一次 `chrome.downloads.search({id})`，能拿到浏览器已解析的名字就用它。

**后端**：新增 `POST /api/download-custom/resolve-name`（只解析、不建任务），
复用 `resolve_file_name`（Content-Disposition → 302 最终 URL → Content-Type → 兜底）。
解析不出有意义的名字时回空串，避免又把 download.bin 写回界面。

**前端**：`isPlaceholderName()` 把 `download.bin/download/file/unknown` 当"未知"；
`applyPendingDownload` 发现名字未知就异步调 resolve-name 回填，
期间状态栏显示「正在识别文件名…」。

### 验证（抖音式地址，本机 8780 复现）
| 场景 | 点「开始下载」之前卡片显示 | 落地文件 |
|---|---|---|
| 路径无扩展名 + `Content-Disposition: douyin_setup.exe` | `douyin_setup.exe` | `douyin_setup.exe` |
| 路径无扩展名 + 仅 `Content-Type: application/x-msdownload`（扩展仍发 download.bin） | `onlyct.exe` | — |

实测 UI：`inputs = ["http://127.0.0.1:8780/download/pc/obj/abc123def", "...\\dl2", "douyin_setup.exe"]`，
状态「待确认」；点开始下载后状态「已完成」，磁盘出现 `douyin_setup.exe`。


## 第七轮：下载器加「暂停 / 继续」
### 需求
点「开始下载」之后才出现第三个按钮：取消、打开文件夹、暂停（暂停时变「继续」）。

### 实现（后端）
会话结构 `CustomSession` 加三个字段：
- `url` —— 继续时要重新发请求
- `dest` —— 沿用同一个目标路径，**不能重新 uniquify**（否则会出现 `a (1).zip`）
- `task: Option<JoinHandle<()>>` —— 暂停时 `abort()`

新路由：
| 路由 | 行为 |
|---|---|
| `POST /api/download-custom/pause` | 置 cancel 标志 + `task.abort()`，状态 `running → paused` |
| `POST /api/download-custom/resume` | 状态 `paused → running`，换新的 cancel 标志，重新 spawn `run_download` |

关键细节：
1. `run_download` 结束时若会话已是 `paused`，**直接 return 不覆盖状态**
   （abort 会让引擎返回「已取消」，不能让它把 paused 改成 cancelled）
2. 暂停的会话**不进 60 秒清理队列**，否则用户点「继续」时会话已经没了
3. 续传靠引擎原有能力：单流看 `*.downloading` 剩余长度发 `Range: bytes=N-`；
   分块看 `*.c{i}` 每个分块自己的长度

### 实现（前端）
- `api.js`：`pauseCustomDownload / resumeCustomDownload`
- `custom-download.js`：两个业务函数 + 导出
- `home-download-widget.js`：
  - `paused` / `active` 派生状态
  - 一个按钮两态：`{{ paused ? '继续' : '暂停' }}`
  - 未开始时：开始下载 + 打开文件夹
  - 下载中/暂停中：**取消 + 打开文件夹 + 暂停/继续**（三个）
  - 状态文案加「已暂停」

### 实测（64MB / 200KB/s 慢速服务器）
- 点开始 → 按钮变成 `["取消","打开文件夹","暂停"]` ✅
- 点暂停 → 状态「已暂停」，按钮变「继续」✅
- 暂停期间分块文件大小不变（32 个分块停在 671744）✅
- 点继续 → 分块从 671744 继续涨，**没有归零重下**（真断点续传）✅
- 跑完 → 最终文件 67,108,864 字节，完整 ✅
- 按钮恢复成 `["开始下载","打开文件夹"]` ✅


### 取消 = 彻底清空（后续补充）
用户要求：点「取消」要清空已下载的文件 + 输入框里的地址和文件名。

**后端**：`handle_cancel` 除了置 cancel + abort，还调 `purge_partial(dest)` 删除：
- `dest.downloading`（单流半成品）
- `dest.c0 / dest.c1 / ...`（分块半成品，按 `前缀 + 全数字` 精确匹配）
- `dest` 本身（已合并的成品，取消即丢弃）
返回 `{ success, removed }` 便于排查。

**前端**：
- 新增 `clearCustomDlForm()`：清空 `#custom-dl-url`、`#custom-dl-filename` 和
  `customDlState` 的 url/fileName/status/progress/计数（**保存目录保留**，下次沿用）
- `cancelCustomDownload()` 改为 `clearCustomDlForm() + resetCustomDlUI(false)`
- 组件 `onCancel` 里同时清本地 `url`/`fileName` ref 并保持展开

**实测**：
| 场景 | 结果 |
|---|---|
| 下载中点取消 | 32 个分块全删、目录空；地址/文件名变空；按钮回「开始下载」；状态「未开始」；标题「尚未选择文件」 |
| 暂停后点取消 | 同上，分块同样清空 |
| 保存目录 | 保留（不随取消清空） |


## 第八轮：GitHub 下载加速
### 现象
从 GitHub Release 下文件很慢。实测直连 Release 资产：**20 秒连响应头都拿不到**（AbortError）。

### 先测代理（本机，2MB/4MB Range 探测）
| 代理 | 单连接 | 8 并发 |
|---|---|---|
| github.com 直连 | 超时 | — |
| gh-proxy.com | 295KB/s | **1.92MB/s（6.5x）** |
| ghfast.top | 178KB/s | 1.16MB/s（部分 429） |
| ghproxy.net | 超时 | — |
| ghproxy.link / ghps.cc / 其它 8 个 | 基本 0 | — |

结论：**GitHub 单连接被限速，必须靠分块并发叠加带宽**。

### 改法
`mirror.rs`：
- 新增 `GITHUB_PROXIES = ["https://gh-proxy.com/", "https://ghfast.top/"]`（只留实测可用的）
- 新增 `is_github_download(url)`：识别 `/releases/download/`、`/raw/`、`objects.githubusercontent.com`
- `get_mirror_urls`：GitHub 地址把所有代理都排进候选（代理在前、官方直连兜底），
  上层逐个尝试/竞速，某个代理 429 就自动换下一个
- Temurin JDK 仍优先中科大 USTC 镜像（那条更快且不打代理）

`single.rs`：
- GitHub 源强制 `max_chunks >= 32`，保证分块并发够多

### 实测（真实 GitHub Release，71,697,920 字节）
- 5–20s：探测/建连阶段（代理要先回源，慢是正常的）
- 25s：5%，1.03MB/s
- 40s：30%，4.95MB/s
- 50s：52%，**6.88MB/s（峰值）**
- 约 65 秒完成，最终文件 71,697,920 字节与官方一致

对比改之前：直连连响应头都拿不到。


## 第九轮：GitHub 只有几十 KB/s 的真凶（并发调度器逻辑写反）
### 现象
上一轮接了代理，用户仍反馈"只有几十 KB/s"。

### 对照实验（同一 Release 文件，1MB Range）
| 源 | 1 并发 | 4 | 8 | 16 |
|---|---|---|---|---|
| github.com 直连 | 47KB/s | 161KB/s | 309KB/s | 431KB/s |
| gh-proxy 代理 | 312KB/s | 1.89MB/s | 2.60MB/s | **5.40MB/s** |

结论：**速度完全由并发数决定**，代理本身没问题。

### 真凶
`ChunkScheduler::should_add()`：
```rust
let floor = SPEED_FLOOR_BASE.max(speed * 85 / 100);
speed >= floor          // ← 恒为 true
```
`speed * 0.85` 永远小于 `speed`，所以 `speed >= floor` **恒真** → 函数返回"已达标"→
**永远不增加并发**，实际只有初始 4 个分块在跑。这就是几十 KB/s 的根源。

### 修法
```rust
// 目标速度：低于它就继续加并发
const SPEED_TARGET: u64 = 512 * 1024;
speed < SPEED_TARGET      // 慢 → 继续加
```
并把初始并发从 4 提到 **8**（代理单连接 ~300KB/s，8 并发才够）。

加了两个单测防回归：
- `slow_source_triggers_more_chunks`：速度为 0 时必须继续加；速度 10MB/s 时不再加
- `respects_max_chunks`：到上限后不再加

### 实测（真实 Release，71,697,920 字节）
| 指标 | 修复前 | 修复后 |
|---|---|---|
| 起步速度 | 几十 KB/s | **3.99MB/s** |
| 平均 | — | ~1MB/s（后期分块陆续完成，并发自然下降） |
| 总耗时 | 数分钟且常失败 | **约 75 秒** |
| 结果 | — | 71,697,920 字节完整 |

### 遗留（可继续优化，非阻塞）
尾部拖尾：最后 1 个分块差 131KB 时，其余 31 个已完成，
并发掉到 1，速度 560KB/s 爬完最后一点。可选优化方向：
剩余未完成字节数 < 分块大小时，把剩余部分再切细分给多个连接。


## 第十轮：B 站 403 + 全站满速（本轮收尾）
### 需求
1. B 站下载 403 → 修好
2. 不论 GitHub 源码 / 任意网站软件 / 开不开 VPN，都要满速
3. 全部测一遍再交付

### 一、B 站 403 根因与修法
B 站视频流 CDN（`upos-*.bilivideo.com`）强制校验 `Referer`，缺了直接 403。

三层修法：
1. **域名白名单注入 Referer**：`mirror::referer_for(url)` 覆盖 B 站（bilibili/bilivideo/hdslb/biliapi）、
   抖音、微博、知乎、QQ、音乐站，直接带对应主站 Referer
2. **403/451 自动重试**：任何站点返回 403 时，用 `guess_referer(url)`（取 scheme://host/）
   补上 Referer 再试一次 —— 这是 IDM/aria2 的通用做法，能救回绝大部分"只校验同域"的 CDN
3. **分块路径同样处理**：探测（probe）、分块请求、单流请求三个位置都覆盖

日志证据（本地模拟 + 真实站点）：
```
403 retry with referer=https://www.bilibili.com/ ok
probe 403 retry ok referer=...
```

### 二、GitHub 反而变慢的真凶（第二轮）
上一轮修好后，GitHub 有时仍要 90–130 秒。日志显示：

```
chunked url=https://github.com/...          ← 先试直连！
fast-fail switch source: 分块 0 TTFB 超时
chunked url=https://gh-proxy.com/https://...
```

两次耗时全花在"先试直连超时"上。

**根因**：`probe_speed_sort` 用 `Range: 0-0` 测 TTFB，GitHub 直连的 TCP 握手有时比代理还快，
测速把直连排到了第一位 —— 但直连实际传输只有 47KB/s。

**修法**：GitHub 完全不参与测速排序，固定用 `get_mirror_urls` 排好的「代理优先」顺序。

### 三、代理筛选（实测淘汰）
| 代理 | 8 并发成功 | 速度 |
|---|---|---|
| gh-proxy.com | **8/8** | **1.37MB/s** |
| ghfile.geekertao.top | 8/8 | 646KB/s |
| ghfast.top | 4/8（429） | 273KB/s |
| ghproxy.net / gh.llkk.cc / gh-proxy.net / moeyy / ghps.cc | 0/8 | 不可用 |

最终只保留前两个。另外把 429/403/低速也加入"立即换源"列表，不再在同一源上重试 3 次。

### 四、最终验收（9/9 通过）
| 场景 | 大小 | 峰值 | 耗时 |
|---|---|---|---|
| GitHub Release | 68.38MB | **17.12MB/s** | **15.3s** |
| GitHub raw | 1.6KB | — | 1.6s |
| B 站 hdslb | 7.5KB | — | 0.5s |
| Google 大包 | 227.36MB | **27.77MB/s** | **10.1s** |
| 本地 正常 | 32MB | 60.84MB/s | 0.5s |
| 本地 403 重试 | 32MB | 61.78MB/s | 0.5s |
| 本地 不支持 Range | 300KB | — | 0.5s |
| 本地 302 跳转 | 32MB | 60.38MB/s | 0.6s |
| 本地 chunked | 32MB | — | 0.5s |

Rust 单测 202 个全过。

### 五、诊断
新增 `download/diag.rs`，把「最终用的哪个源、分块数、是否 skip 测速、403 重试结果、换源原因」
写进 `<data>/logs/download-diag.log`（超 1MB 自动截断）。以后用户反馈"慢"，看这个文件即可定位。

---

## 第 11 轮：商店版扩展装进浏览器后无法接管（2026-11-12）

**现象**：把打包好的 `VersePC2-DownloadBridge-edge.zip` 拖进 Edge 安装后，点下载时扩展徽标变红，
弹「VersePC2 接管失败，已交回浏览器」，启动器那边毫无反应。

**排查**：读 Edge 的 `Secure Preferences`，装进去的扩展 ID 是 `cjfhdnfkoenifpgjkbapgddbolecgelo`，
path 指向解压出来的目录（location=4，开发者模式「加载解压缩的扩展」）。而启动器写的
`data/bridge/cc.versepc.download_bridge.json` 里只有两条写死的来源：

```
chrome-extension://pfmgdfekkaaigjjigfgljcmnebbhlfoa/   ← extension/manifest.json 里 key 推导
chrome-extension://bcgdkpfgcbfjjhnahaflogmfkkkdiddi/   ← 历史 ID
```

对不上 → 浏览器直接拒绝 `connectNative`（Access to the specified native messaging host is forbidden），
扩展按设计把下载交回浏览器，于是表现为「拦截失败」。

**根因**：扩展 ID 取决于安装来源，编译期写不死。

| 安装方式 | ID 来源 |
|---|---|
| 启动器释放的未打包版（manifest 带 key） | key → 固定 ID |
| 商店上架版（key 被移除） | 商店分配 |
| zip 直接拖进浏览器 | 解压目录绝对路径哈希 |

**修法**（`bridge/register.rs`）：

1. `discover_extension_ids()`：扫 Chrome / Chrome Beta / Edge / Edge Beta / Edge Dev / Edge SxS / Chromium / Brave 的 User Data，
   逐个 profile 找扩展——先看 `Extensions/<id>/<version>/manifest.json`，
   再看 `Preferences` / `Secure Preferences` 里 `extensions.settings.<id>.path` 指向的 manifest.json；
2. 判定按内容不按 ID：`name` 或 `homepage_url` 里含 `VersePC2` 就算我们的扩展；
3. `install_for()` 把这些 ID 全部并入 `allowed_origins`；
4. `self_heal()` 从「只比对 exe 路径」升级为「路径 OR 来源集合有变化就重建」，每次启动跑一遍
   → 商店版发布后拿到新 ID 也能自动认。

**验收**：

- 把清单手动重置成 2 条 → 启动新 exe → 自动补回第 3 条 `cjfhdnfkoenifpgjkbapgddbolecgelo`；
- 模拟浏览器直连宿主（`VersePC2.exe --native-host chrome-extension://<id>/` + 4 字节 LE 帧）：
  `{"action":"ping"}` 0.01s 返回 `ok:true version 1.9.0`，`{"action":"download"}` 0.13s 返回 `accepted:true`；
- `cargo build --release` 通过，exe 72,070,656 字节。

**注意**：扩展包本身没改（版本仍 1.2.2、key 仍移除），修的是启动器端，商店包不用重新上传。
