# 打包与发布

写完插件后，把它 zip 打包并更新市场索引，用户即可在插件市场中下载。

## 1. 打包 zip

用任意压缩工具（Windows 自带、压缩软件、或命令行 `Compress-Archive`）把插件目录打成 zip。

```powershell
# 在插件目录的上一级执行
Compress-Archive -Path .\hello-verse -DestinationPath hello-verse.zip
```

校验点：
- zip 内 `plugin.json` 位于根目录，或位于仅含一个顶层插件的子目录。
- 不要打包成 `.rar`/`.7z`，必须是 `.zip`。

## 2. 计算 sha256 与 size

```powershell
Get-FileHash .\hello-verse.zip -Algorithm SHA256
(Get-Item .\hello-verse.zip).Length
```

## 3. 上传插件包

把 zip 上传到插件仓库 `doujie081231/verseplugin`（`master` 分支），例如根目录下的 `my-plugin.zip`。得到直链：

```
https://gitee.com/doujie081231/verseplugin/raw/master/my-plugin.zip
```

## 4. 更新 index.json

在仓库根的 `index.json` 的 `plugins` 数组中增加（或更新）该项，把上一步得到的 `downloadUrl`/`sha256`/`size` 填进去，然后提交（`master`）。

发布新版本时：更新插件包内的 `version`、重新上传 zip、并同步 `index.json` 的 `version`/`sha256`/`size`。索引版本高于本地时，用户会看到“更新”按钮。

## 端到端自检

- raw 直链可访问：`Invoke-WebRequest` 返回 `200`。
- `sha256`/`size` 与 zip 实际一致。
- 在启动器的插件市场中能看到本插件，并能安装成功。