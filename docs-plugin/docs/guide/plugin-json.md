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