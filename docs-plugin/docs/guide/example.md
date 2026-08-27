# 示例插件工程

以线上可安装的最小插件为参照，展示一个真实、可复制的字段布局。这里的 `my-plugin` 仅是占位名，请替换为你插件的 `id`。

## 目录结构

```text
my-plugin/
├── plugin.json      # 插件清单
└── main.js          # 插件主脚本（可选，做前端自定义时使用）
```

## plugin.json

```json
{
  "id": "my-plugin",
  "name": "我的插件",
  "version": "1.0.0",
  "category": "function",
  "author": "你的名字",
  "description": "插件的功能说明，会显示在市场卡片里。",
  "main": "main.js"
}
```

- `category` 支持 `function`（功能）/ `personalize`（个性化），决定市场中的分类展示。
- 若要新增独立页面，在 `ui.pages` 里声明，并把页面 Vue 组件放进插件目录，见 [功能插件](feature-plugins)。
- 若要贴合你的具体功能，只需替换 `id`、`name`、版本、分类与描述。

## 把它改造成你的插件

1. 复制 `my-plugin/` 目录并改名。
2. 修改 `plugin.json`：换掉 `id`（插件内唯一）、`name`、`version`、`description`、`category` 与 `author`。
3. 按需加入 `ui` 界面声明（`pages`/`cards`）与前端的 Vue/`main` 脚本。
4. 重新打包并提交审核上架，见 [提交审核](publish) 与 [打包上传](packaging)。

> 想了解如何**给启动器现有的某个功能扩展能力**或**新增功能**（调用后端、触发事件、介入现有界面）？见 [功能插件 → 功能开发](feature-plugins#功能开发给现有功能扩展能力或新增功能)。