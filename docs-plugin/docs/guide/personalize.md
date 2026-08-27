# 个性化插件开发（详细教程）

个性化插件的核心理念：**整个启动器前端都是可自定义的**。它运行在启动器**同一前端环境**里，用 Vue 模板 + `main` 脚本 + 全局 CSS，你可以改造任何界面模块。

| 能力 | 用什么 | 作用 |
| --- | --- | --- |
| 全局样式 / 主题 | `ui.style` | 注入 CSS，风格化任意模块 |
| 主脚本 / 任意代码 | `ui.main` | DOM、全局对象、事件、注入，整个前端 |
| 自定义视图 | Vue 模板（`pages`/`cards`/组件） | 新增页面、卡片 |
| 宠物挂件 | `ui.pet` | 右下角浮层挂件 |

---

## 模块 1：plugin.json 骨架

```json
{
  "id": "my-personalize",
  "name": "我的个性化",
  "version": "1.0.0",
  "category": "personalize",
  "author": "你的名字",
  "description": "自定义启动器外观并添加挂件。",
  "ui": {
    "main": "main.js",
    "style": "theme.css",
    "pages": [],
    "cards": [],
    "pet": { "component": "pet.js" }
  }
}
```

---

## 模块 2：Vue 模板内容

与[功能插件](feature-plugins)一致，用 Vue 3 写组件，注册到 `window.VersePC["Plugin_<插件id>_<key>"]`。个性化里最常用：`pages`（新增页）与 `cards`（卡片）。

组件直接复用启动器内置类与 CSS 变量（`--accent`、`--bg-secondary`、`--text-muted`），保证风格统一。

---

## 模块 3：style —— 全局 CSS 主题与逐模块风格化

`ui.style` 指向的 CSS 会注入全局，可**风格化任意模块**。

### 3.1 改主题色（CSS 变量）

启动器大量使用 CSS 变量，覆盖它们即可换主题：

```css
/* theme.css */
:root {
  --accent: #7c5cff;          /* 主强调色 */
  --bg-primary: #0f0f12;       /* 主背景 */
  --bg-secondary: #191a20;     /* 卡片/容器背景 */
  --text-primary: #f3f3f5;
  --text-muted: #9a9aa5;
  --border-color: rgba(255,255,255,0.1);
}
```

### 3.2 深/浅色适配

用媒体查询分别处理，确保两种主题都正常：

```css
@media (prefers-color-scheme: dark) {
  :root { --bg-primary: #0f0f12; }
}
@media (prefers-color-scheme: light) {
  :root { --bg-primary: #f5f5f7; --text-primary: #1a1a1a; }
}
```

### 3.3 逐模块风格化

要对某个界面模块做样式定制，步骤是：**先在开发者工具里找到它的选择器，再写覆盖规则**（这里给出常用入口与示例）。

**侧边栏导航**

```css
.sidebar .nav-btn { border-radius: 0 12px 12px 0; }   /* 所有导航项 */
.sidebar .nav-icon-svg { width: 18px; height: 18px; }  /* 图标尺寸 */
```

**顶栏 / 窗口控件**

```css
.global-drag-strip { background: rgba(0,0,0,0.15); }     /* 顶部拖拽条 */
.floating-window-bar .win-btn { color: var(--accent); }  /* 最小化/最大化/关闭 */
```

**页面头部与卡片**

```css
.page-header { padding: 18px 24px 6px; }
.plugin-card-slot, .mod-card, .pack-card { border-radius: 14px; }
```

**按钮、输入框、滚动条**

```css
.btn { font-weight: 600; }
.search-input { border-radius: 8px; }
::-webkit-scrollbar { width: 8px; }
::-webkit-scrollbar-thumb { background: var(--accent); border-radius: 4px; }
```

::: tip
- 覆盖规则记得加在功能的内部（比内置样式后加载，天然优先级更高）。
- 若某些选择器与你的版本不完全一致，用启动器的开发者工具（若开启）或按 **F12/右键-检查** 复制真实选择器替换。
:::

---

## 模块 4：main —— 整个前端任意自定义

`ui.main` 指向的 `.js` 在执行时与启动器前端**同一环境**，可以读写 `window.VersePC`、`window.bridge`、`document`、`localStorage` 等，做**任何**前端定制。

### 4.1 机制

- main 在启动器加载完成（Vue 就绪）后被执行。
- 因为是同环境，代码里可直接 `document.querySelector`、`window.bridge.invoke(...)`、`navigateToPage(...)`。

### 4.2 在侧边栏加一个按钮/徽标

```js
// main.js
(function () {
  function run() {
    var footer = document.querySelector('.sidebar-footer');
    if (!footer || document.getElementById('my-tag')) return;
    var tag = document.createElement('div');
    tag.id = 'my-tag';
    tag.textContent = 'RAD ' + new Date().getFullYear();
    tag.style.cssText = 'text-align:center;font-size:10px;color:var(--text-muted);padding:4px;';
    footer.appendChild(tag);
  }
  if (document.readyState === 'complete' || document.readyState === 'interactive') run();
  else window.addEventListener('DOMContentLoaded', run);
})();
```

### 4.3 调用后端能力（IPC）

启动器统一用 `window.bridge` 与后端通信。示例：读取一个文件并展示：

```js
window.bridge.readFileBuffer('C:/some/path.txt')
  .then(function (buf) {
    var s = new TextDecoder('utf-8').decode(new Uint8Array(buf));
    console.log(s);
  });
```

### 4.4 注入快捷键 / 全局事件

```js
document.addEventListener('keydown', function (e) {
  if (e.ctrlKey && e.key.toLowerCase() === 'p') {
    e.preventDefault();
    navigateToPage('plugins');
  }
});
```

### 4.5 生命周期与防重复

启动器可能重扫插件多次，务必用 `id`/flag 防重复注入（如 `4.2` 的 `getElementById('my-tag')` 判断）。监听类的注册可用 `document` 委托，避免重复监听。

::: warning 安全
`main` 拥有完整前端能力，等同于在启动器内执行任意代码。请只在受信任、且经官方审核上架的场景使用。
:::

---

## 模块 5：宠物挂件制作教程

`ui.pet` 让插件渲染一个右下角浮层挂件，挂件是 **Vue 组件**。

```json
"ui": { "pet": { "component": "pet.js" } }
```

组件注册名：`window.VersePC["Plugin_<插件id>_pet"]`。

### 5.1 单图贴纸（最简单）

```js
(function () {
  window.VersePC = window.VersePC || {};
  window.VersePC["Plugin_my-personalize_pet"] = {
    template: `
      <div style="pointer-events:auto;cursor:pointer;text-align:center;">
        <img src="pet.png" width="80" height="80" style="border-radius:14px;" alt="pet">
        <div style="font-size:11px;color:var(--text-muted);">挂件</div>
      </div>
    `
  };
})();
```

把 `pet.png` 放进插件目录（与 `pet.js` 同层）。

### 5.2 逐帧动画（多图）

用 `setInterval` 切换多帧：

```js
data() { return { frame: 0 }; },
mounted() {
  this.timer = setInterval(() => {
    this.frame = (this.frame + 1) % 4;   // 4 帧
  }, 120);
},
beforeUnmount() { clearInterval(this.timer); },
template: `
  <img :src="'frames/f' + frame + '.png'" width="80" height="80" style="pointer-events:auto;border-radius:14px;">
`
```

配合 `frames/f0.png ~ f3.png` 逐帧图；帧间切换时间即动画速度。

### 5.3 GIF

直接放 GIF 即可，无需切帧：

```js
template: `<img src="pet.gif" width="80" height="80" style="pointer-events:auto;border-radius:14px;">`
```

### 5.4 精灵图动画（一行 CSS）

把多帧拼到一张精灵图，用 `steps()` 逐帧显示：

```css
.pet-sprite {
  width: 80px; height: 80px;
  background: url('sprite.png') 0 0 no-repeat;
  animation: pet-step .8s steps(4, end) infinite;
}
@keyframes pet-step { to { background-position: -320px 0; } }
```

（精灵图 4 帧横排，共 `4×80=320px` 宽，`to` 位移 `-320px`。）

### 5.5 交互

浮层外层是 `pointer-events:none`，内容需自行开启 `pointer-events:auto`，随后可用事件：

```js
methods: {
  poke() {
    this.bouncy = true;
    setTimeout(() => (this.bouncy = false), 400);
  }
},
template: `
  <div @click="poke" :style="bouncy ? 'animation:plugin-bounce .4s' : ''"
       style="pointer-events:auto;cursor:pointer;text-align:center;">
    ...
  </div>
`
```

可配合 `mounted` 里 `setInterval` 做"自动眨眼/走动"等循环动作，`@click`/`@mouseover` 做交互。

### 5.6 尺寸与放置

- 建议挂件 60–120px，放在右下角，避免遮挡操作区。
- 若与现有窗口控件冲突，可在 `main` 里调整挂件图层位置。

### 5.7 完整可复制示例

```js
(function () {
  window.VersePC = window.VersePC || {};
  window.VersePC["Plugin_my-personalize_pet"] = {
    data() { return { frame: 0, bouncy: false }; },
    mounted() { this.timer = setInterval(() => { this.frame = (this.frame + 1) % 4; }, 150); },
    beforeUnmount() { clearInterval(this.timer); },
    methods: { poke() { this.bouncy = true; setTimeout(() => (this.bouncy = false), 400); } },
    template: `
      <div @click="poke" :style="bouncy ? 'animation:plugin-bounce .4s' : ''"
           style="pointer-events:auto;cursor:pointer;text-align:center;">
        <img :src="'frames/f' + frame + '.png'" width="80" height="80" style="border-radius:14px;" alt="pet">
        <div style="font-size:11px;color:var(--text-muted);">点我~</div>
      </div>
    `
  };
})();
```

配套 `frames/f0..f3.png`。（若没有逐帧图，也可把 `:src` 换成 `pet.gif`/单图。）

---

## 模块 6：提交与审核

完成后[打包 zip → 邮件提交审核（GPLv3 开源）](publish)。

> 宠物挂件等个性化内容同样以 GPLv3 开源上架，保证可信与可复用。