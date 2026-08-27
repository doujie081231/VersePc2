# 市场索引 index.json

插件市场的数据来源是一份远程 JSON —— `index.json`。启动器只在本文件里声明了插件，市场才会展示它。

## 地址

默认市场索引地址（`master` 分支）：

```
https://gitee.com/doujie081231/verseplugin/raw/master/index.json
```

## 结构

```jsonc
{
  "plugins": [
    {
      "id": "my-plugin",
      "name": "我的插件",
      "version": "1.0.0",
      "category": "function",
      "author": "你的名字",
      "description": "插件功能说明。",
      "icon": "",
      "downloadUrl": "https://gitee.com/doujie081231/verseplugin/raw/master/my-plugin.zip",
      "sha256": "…文件的 SHA-256…",
      "size": 321
    }
  ]
}
```

## 字段说明

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `id` | ✔ | 与插件包内 `plugin.json` 的 `id` 一致，用于校验与更新判断 |
| `name` / `description` / `author` |  | 市场卡片展示 |
| `category` |  | 同 [plugin.json 分类](category)，决定 Tab 位置 |
| `version` | ✔ | 最新版本号，市场据此叠加 `hasUpdate` |
| `downloadUrl` | ✔ | 插件 zip 的直链（下载时用于获取文件） |
| `sha256` |  | zip 的 SHA-256 校验值（可选，提供则下载后校验） |
| `size` |  | zip 字节大小（可选，提供则下载后校验） |
| `icon` |  | 展示图标 URL（可选） |

## 启动器叠加的信息

启动器拉取该索引后，会按本地已安装情况追加三个只读字段，开发者无需写入：

| 字段 | 含义 |
| --- | --- |
| `isInstalled` | 该插件当前是否已安装 |
| `installedVersion` | 已安装版本号 |
| `hasUpdate` | 已安装且索引版本更新时为 `true`，前端显示“更新” |

## 注意

- 必须提供 `downloadUrl`，否则安装按钮无法下载插件包。
- `id` 校验：安装时 zip 内 `plugin.json` 的 `id` 必须与此处一致，否则拒绝安装。