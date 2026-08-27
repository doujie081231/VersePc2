# 快速开始

用 3 步写出并本地验证一个最小插件。

## 1. 创建目录与 plugin.json

```text
hello-verse/
└── plugin.json
```

[plugin.json](plugin-json) 至少包含 `id`/`name`/`version`：

```json
{
  "id": "hello-verse",
  "name": "Hello Verse",
  "version": "0.1.0",
  "category": "function",
  "author": "开发者",
  "description": "一个最小示例插件。"
}
```

## 2. 打成 zip

在 `hello-verse/` 的**上一级**目录，把整个 `hello-verse` 文件夹压缩为 `hello-verse.zip`。

::: tip
zip 内允许 `plugin.json` 位于根目录，或位于仅含一个顶层插件的子目录中，两种都能被正确识别。
:::

## 3. 安装验证

在启动器的**插件 → 已安装**（或市场已安装状态）中安装该 zip，若出现则说明包格式正确。

之后想让大家都能下载，还需要把它[发布到市场](packaging)。

## 分类

插件市场按分类展示，填写正确的 [`category`](category) 能让插件显示在合适的位置。

## 参考

- [plugin.json 字段说明](plugin-json)
- [市场索引 index.json](index-json)
- [打包与发布](packaging)