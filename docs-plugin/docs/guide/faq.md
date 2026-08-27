# 常见问题

## 安装失败，提示“插件包内未找到 plugin.json”

- 确认 zip 内确实有一份 `plugin.json`。
- `plugin.json` 必须在有效根：根目录，或仅含一个顶层插件的子目录。
- 文件必须是 UTF-8 编码的合法 JSON，且 `id`/`name`/`version` 均非空。

## 提示“插件 id 不匹配”

`index.json` 里的 `id` 与 zip 内 `plugin.json` 的 `id` 不一致。两者必须相等。

## 市场里看不到我的插件

- 确认已更新仓库根的 `index.json` 并提交到 `master`。
- 确认 `downloadUrl` 的 raw 直链可访问（返回 200）。
- 确认 `index.json` 的 `plugins` 结构正确（是数组，字段名拼写无误）。

## 市场卡片没有图标

留空 `icon` 时前端会用分类默认图标；如需自定义，给 `icon` 填一个可访问的图片 URL。

## 想更新插件版本

1. 修改包内 `plugin.json` 的 `version`，重新打 zip 并上传。
2. 更新 `index.json` 中的 `version`、`sha256`、`size`（与新的 zip 一致）。
3. 提交后，已安装用户会在市场看到“更新”按钮。

## 插件安装后想去掉

在插件市场中该插件的“已安装”状态点“卸载”，或删除 `data/plugins/<id>/` 目录。

## 去哪里提问

插件仓库：<https://gitee.com/doujie081231/verseplugin>