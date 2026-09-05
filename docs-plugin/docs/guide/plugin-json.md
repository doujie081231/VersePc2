# plugin.json 字段

`plugin.json` 必须是 UTF-8 编码的 JSON，位于插件有效根目录。启动器会读取它来判断插件是否合法、用于市场展示与排序。

## 必填字段

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `id` | string | 插件唯一标识，建议 `小写字母+连字符`（如 `hello-verse`）。安装后作为目录名 `data/plugins/<id>/`，必须非空 |
| `name` | string | 插件显示名称，市场列表中展示，必须非空 |
| `version` | string | 当前版本号（如 `1.0.0`），市场据此判断是否有更新，必须非空 |

::: warning
`id`、`name`、`version` 三者缺一不可，缺任一项都会被判定为不合法插件，无法安装。
:::

## 可选字段

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `category` | string | 分类，取值 `function` / `personalize` / `server`，详见解说见 [分类约定](category) |
| `description` | string | 插件说明，市场卡片与详情中展示 |
| `author` | string | 作者名 |
| `main` | string | 插件主脚本相对路径（可选）。启动器加载插件时执行，可对整个前端做自定义，后文详解 |
| `icon` | string | 图标 URL（可选），市场卡片展示；留空时前端用默认分类图标 |
| `permissions` | object | 声明插件需要的高危权限，详见下文「权限声明」 |

## 权限声明（permissions）

插件运行在启动器的**沙箱**里，默认只能：读写自身 KV 配置、剪贴板、选择文件对话框、打开外部链接、读取**插件自身目录**内的文件。除此之外的高危能力（网络、执行外部进程）**必须先在 `permissions` 里声明**，否则安装时不会弹出授权、运行时会直接被拦截。

| 子字段 | 类型 | 取值 | 说明 |
| --- | --- | --- | --- |
| `network` | boolean | `true` | 放行插件发起外部网络请求（`fetch` / `apiProxy` / XHR / WebSocket 等），用于调用第三方服务 |
| `native` | array | 含 `"exec"` | 放行「执行外部程序」，可启动/停止本地进程（如 `frpc.exe`）。仅`可信`插件建议声明。安装界面会据此弹出确认卡片，用户需点「信任并安装」才能继续 |

```json
{
  "id": "my-plugin",
  "name": "我的插件",
  "version": "1.0.0",
  "category": "function",
  "permissions": {
    "network": true,
    "native": ["exec"]
  },
  "ui": {}
}
```

> 说明：`native.exec` 使用后端 `plugin_process_exec` / `plugin_process_stop` / `plugin_process_status` 三组命令托管进程，进程 `stdout/stderr` 逐行回传 `plugin:<id>:exec-log`，退出回传 `plugin:<id>:exec-exit`。前端沙箱为声明了该权限的插件注入 `window.bridge.exec` / `stopExec` / `execStatus` / `onExecLog` / `onExecExit`。为安全起见，后端还会再读磁盘上的 `plugin.json` 二次校验，防止绕过前端直接调用。

## 完整示例

```json
{
  "id": "my-plugin",
  "name": "我的插件",
  "version": "1.0.0",
  "category": "function",
  "author": "你的名字",
  "description": "我要开发的能力的说明。",
  "main": "index.js"
}
```

## 说明

- 安装到 `data/plugins/<id>/plugin.json` 后，启动器会额外写入 `installedDir` 与 `installedVersion` 两个只读字段用于内部记录，开发者无需关心。
- 同一 `id` 重复安装会覆盖旧版本（即"更新"）。

## ui（前端界面）

`ui` 字段让插件拥有前端能力。启动器安装插件并加载时，会按声明动态注册界面。支持：

| 子字段 | 类型 | 作用 |
| --- | --- | --- |
| `main` | string | 插件主脚本（`.js`），在启动器同一前端环境执行，可对整个前端做任意自定义（推荐个性化插件使用） |
| `style` | string | 注入全局的 CSS 文件（主题/外观覆盖） |
| `pages` | array | 新增整页：`[{ id, title, icon, component }]`，会在侧边栏生成入口按钮 |
| `cards` | array | 向现有页面注入卡片：`[{ anchor, component, id? }]`，`anchor` 为选择器 |
| `pet` | object | 宠物/挂件：`{ component }`，渲染为个性化浮层 |

-   **功能插件** → 详见 [功能插件：新增页面与卡片](feature-plugins)
-   **个性化插件（含整个前端自定义与宠物挂件）** → 详见 [个性化](personalize)