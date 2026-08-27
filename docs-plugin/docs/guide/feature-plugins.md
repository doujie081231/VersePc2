# 功能插件开发（详细教程）

功能插件用于给启动器**新增功能**，分两种形态，先选型再开发。

| | 类型 A：新增页面 | 类型 B：插入卡片 |
| --- | --- | --- |
| 交互入口 | 侧边栏新增一个按钮 | 页面内部追加一块内容 |
| 需要 SVG 图标 | **需要** | **不需要** |
| 前端框架 | Vue 3 | Vue 3 |
| 适用 | 独立复杂功能（如开服） | 轻量嵌入某个页面 |

组件统一用 **Vue 3**（与启动器前端一致），写 `.js` 文件放插件目录，由插件运行时加载并挂载。

---

## 模块 1：类型 A —— 新增页面（侧边栏按钮 + SVG + Vue）

### 1.1 目录结构

```text
my-feature/
├── plugin.json      # 插件清单（含 ui.pages）
├── page.svg          # 侧边栏按钮图标（SVG）
└── page.js           # 页面 Vue 组件
```

### 1.2 plugin.json 的 ui.pages

```json
{
  "id": "my-feature",
  "name": "我的功能",
  "version": "1.0.0",
  "category": "function",
  "author": "你的名字",
  "description": "新增一个独立功能页面。",
  "ui": {
    "pages": [
      { "id": "main", "title": "我的功能", "icon": "page.svg", "component": "page.js" }
    ]
  }
}
```

`pages[].id` 只要在插件内唯一即可；侧边栏按钮与页面容器会由运行时自动生成。

### 1.3 SVG 图标规范

侧边栏按钮会把 SVG 文件内容直接嵌进按钮。请遵循：

**基本要求**
- 是**完整 `<svg>` 片段**，根元素含 `viewBox="0 0 24 24"`。
- 用 `stroke="currentColor"` 让图标跟随启动器按钮配色（也可写固定颜色）。
- 图形放在 24x24 视口内，居中、留边距，避免贴边被裁切。

**推荐**
- 单色、线性（描边）风格，视觉上与现有侧边栏图标一致。
- 复杂图标可缩绘到 24 视口：`viewBox` 用的是设计坐标系，缩放交给 SVG。

```xml
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <rect x="2" y="3" width="20" height="8" rx="2"/>
  <rect x="2" y="13" width="20" height="8" rx="2"/>
  <circle cx="6" cy="7" r="1" fill="currentColor"/>
  <circle cx="6" cy="17" r="1" fill="currentColor"/>
</svg>
```

### 1.4 Vue 组件（page.js）

组件注册到**固定命名**：

```text
window.VersePC["Plugin_<插件id>_<页面id>"]
```

即插件的 `id` + `ui.pages[].id` 合成名字，运行时靠这个名字找到组件并挂载。

```js
(function () {
  window.VersePC = window.VersePC || {};
  window.VersePC["Plugin_my-feature_main"] = {
    data() {
      return {
        serverName: '',
        list: []
      };
    },
    methods: {
      add() {
        if (this.serverName.trim()) {
          this.list.push(this.serverName.trim());
          this.serverName = '';
        }
      }
    },
    template: `
      <div class="page-header">
        <h2>我的功能</h2>
        <p class="page-subtitle">这是一个通过插件注册的独立页面</p>
      </div>
      <div style="padding:0 24px;max-width:560px;">
        <div style="display:flex;gap:8px;margin-bottom:12px;">
          <input v-model="serverName" class="search-input" placeholder="输入服务器名称">
          <button class="btn btn-accent" @click="add">添加</button>
        </div>
        <ul style="list-style:none;padding:0;margin:0;">
          <li v-for="(n,i) in list" :key="i"
              style="padding:8px 12px;margin-bottom:6px;border-radius:8px;background:var(--bg-secondary);font-size:13px;">
            {{ n }}
          </li>
        </ul>
      </div>
    `
  };
})();
```

**组件能力**（Vue 3 都可用）：
- `data` / `computed` / `methods` / `watch`
- `mounted` / `created` 等生命周期（可用于进入页面后加载数据）
- `v-model`、`v-for`、`v-if` 等模板指令

### 1.5 复用启动器内置样式与全局工具

- **样式类**：直接复用 `page-header`、`page-subtitle`、`btn`（`btn-primary`/`btn-accent`/`btn-secondary`/`btn-sm`）、`search-input`、模态框相关类，观感与内置一致。
- **CSS 变量**：`var(--accent)`、`var(--bg-primary)`、`var(--bg-secondary)`、`var(--text-primary)`、`var(--text-muted)`、`var(--border-color)` 等，跟随用户主题。
- **全局对象/函数**：`navigateToPage('xxx')`（跳转内置页）、`window.bridge`（IPC，调后端）、`showToast(msg, type)`（提示）、`window.VersePC`。

例：在页面里跳转到下载页：

```js
methods: {
  goDownload() { navigateToPage('versions'); }
}
```

### 1.6 运行与调试

1. 将插件打成 zip 安装（市场或本地），或在已有插件上更新。
2. 安装后侧边栏即出现该按钮（若未出现，重启启动器触发运行时重扫；运行时会自动处理）。
3. 定位问题看**启动器控制台**（侧边栏"日志"）里的 `[plugin-runtime]` 错误；也可在浏览器开发者工具（若开启了 debug）看报错。
4. 修改 `component` 文件后需重新打包上传/安装再验证；页面组件每次加载时重新执行，可即时刷新验证。

### 1.7 完整可复制源码

在上面的示例基础上，双传真机模块（如多 Tab、调用后端 API）可直接套用 `window.bridge` 事件，见 `guide/quickstart` 与"个性化 main 脚本"。

---

## 模块 2：类型 B —— 插入卡片（无需 SVG）

在某个已有页面里追加一块内容，适合做小工具面板。

### 2.1 plugin.json

```json
{
  "id": "my-card",
  "name": "我的卡片",
  "version": "1.0.0",
  "category": "function",
  "author": "你的名字",
  "description": "向现有页面插入一张卡片。",
  "ui": {
    "cards": [
      { "id": "widget", "anchor": "#page-mods .mod-list", "component": "card.js" }
    ]
  }
}
```

字段：
- `anchor`：**CSS 选择器**，指向要插入的容器。运行时等待该元素出现后把卡片追加进去。
- `component`：卡片 Vue 组件文件路径。
- `id`：可选，用于区分同一插件多张卡片（组合到组件注册名与防重复）。

### 2.2 组件注册名

```text
window.VersePC["Plugin_<插件id>_card_<卡片id或组件文件名>"]
```

### 2.3 卡片组件（card.js）

```js
(function () {
  window.VersePC = window.VersePC || {};
  window.VersePC["Plugin_my-card_card_widget"] = {
    data() { return { clicks: 0 }; },
    methods: { onClick() { this.clicks++; } },
    template: `
      <div style="padding:14px;margin:12px;border:1px solid var(--border-color);border-radius:12px;background:var(--bg-secondary);">
        <div style="font-weight:600;">我的卡片</div>
        <div style="font-size:12px;color:var(--text-muted);">点击：{{ clicks }}</div>
        <button class="btn btn-primary btn-sm" style="margin-top:8px;" @click="onClick">+1</button>
      </div>
    `
  };
})();
```

**提示**
- 卡片自带上内边距/边框/圆角，避免与宿主页面样式冲突。
- 运行时会对同一张卡片去重，多次重扫不会重复插入。
- `anchor` 对应的元素需在进入该页面后才存在；运行时内置轮询等待，无需担心时序。

---

## 模块 3：功能开发（给现有功能扩展能力 / 新增功能）

页面只是界面，真正的"功能"来自与后端交互。插件可**调用启动器能力、读写设置、介入现有页面**来实现"给某个现有功能加功能"，或组合出"一个全新功能"。

### 3.1 调用后端能力（IPC）

插件统一通过 `window.bridge` 与后端通信：

```js
// 读取/写入启动器设置
window.bridge.store.get('someKey').then(v => console.log(v));
window.bridge.store.set('someKey', true);

// 调用任意后端命令
window.bridge.invoke('plugin_list');

// 读取本地文件（返回字节，转文本）
window.bridge.readFileBuffer('C:/path/file.txt')
  .then(buf => new TextDecoder('utf-8').decode(new Uint8Array(buf)));

// 打开外部链接
window.bridge.invoke('open_external', { url: 'https://example.com' });
```

### 3.2 给"某个现有功能"加功能

可用的介入点：

**① 在现有功能页面里追加内容（卡片）**
类型 B 的 `cards`，`anchor` 指向现有功能页里的容器，追加你的功能块（见模块 2）。适合"给某页加一个小工具"。

**② 通过 `main` 脚本介入全局 / 页面**
`ui.main` 与启动器前端同环境，可监听页面切换、DOM 就绪，在现有功能页里增补元素或行为。例：监听快捷键，跳到内置"下载"页：

```js
document.addEventListener('keydown', function (e) {
  if (e.ctrlKey && e.key.toLowerCase() === 'd') navigateToPage('versions');
});
```

**③ 读写启动器设置来与内置功能联动**
写入/读取与内置功能共享的配置，让插件协同工作：

```js
window.bridge.store.set('launcherAutoRepair', true)
  .then(() => window.bridge.store.get('launcherAutoRepair'));
```

#### 展开示例：给"设置"页面加一个自定义按钮

目标：在"设置 → 其他"页面加入一个自定义按钮，点击后读取已装插件数并提示。演示 `main` 脚本如何介入现有页面并连后端。

**方式 A（推荐、精确定位）：用 `ui.main` 往设置页加按钮**

```json
"ui": { "main": "main.js" }
```

`main.js`：

```js
(function () {
  function addButton() {
    var page = document.getElementById('page-settings-other'); // "设置-其他"页面容器
    if (!page || document.getElementById('my-settings-btn')) return; // 防重复
    var row = document.createElement('div');
    row.id = 'my-settings-btn';
    row.style.cssText = 'padding:12px 20px;border-bottom:1px solid var(--border-color);display:flex;align-items:center;gap:10px;';
    row.innerHTML =
      '<span style="font-size:13px;color:var(--text-secondary);">我加的自定义设置项</span>' +
      '<button class="btn btn-accent btn-sm">点我</button>';
    row.querySelector('button').addEventListener('click', function () {
      window.bridge.invoke('plugin_list')                 // 调真实后端能力
        .then(function (res) {
          var n = (res && res.plugins) ? res.plugins.length : 0;
          showToast('已安装插件数：' + n, 'success');
        })
        .catch(function () { showToast('读取失败', 'error'); });
    });
    page.appendChild(row);
  }
  if (document.readyState === 'complete' || document.readyState === 'interactive') addButton();
  else window.addEventListener('DOMContentLoaded', addButton);
})();
```

要点：
- 用页面 id（`#page-settings-other`）定位；设置页在 DOM 里一直存在，只是切 `active`，所以直接 `appendChild` 即可。
- `getElementById('my-settings-btn')` 防重复，插件重扫/重装不会插入两次。
- 按钮复用内置 `btn btn-accent btn-sm` 样式，观感统一。
- 想放到更精细的位置，把 `page` 换成在开发者工具里看到的真实容器（列表容器等）。
- 想调其它能力，把 `window.bridge.invoke('插件_list')` 换成任意后端命令：`window.bridge.invoke('命令名', 参数对象)`。

**方式 B（更简单、整块追加）：用 `ui.cards` 往设置页插入按钮**

```json
"ui": { "cards": [ { "id": "btn", "anchor": "#page-settings-other", "component": "btn.js" } ] }
```

`btn.js`：

```js
window.VersePC = window.VersePC || {};
window.VersePC["Plugin_my-plugin_card_btn"] = {
  template: '<div style="padding:14px 20px;"><button class="btn btn-accent btn-sm" @click="hi">在设置页追加的按钮</button></div>',
  methods: { hi() { showToast('来自插件的按钮', 'success'); } }
};
```

cards 会把该组件追加到 `anchor` 指向容器的末尾；适合"整块追加"。

**选哪个**
- 想"穿插到现有设置项旁/做精确布局" → **方式 A**（main，定位可靠）。
- 只想"设置页末尾追加一块/一个按钮" → **方式 B**（cards，更省事）。

### 3.3 新增一个完整功能

把"入口 + 逻辑 + 渲染"三块组合即为一个全新功能：

1. **入口**：`ui.pages` 新增页面（+ SVG 侧边栏按钮），或 `ui.cards` 往某页插入入口卡片。
2. **逻辑**：在页面/卡片内调用 `window.bridge`（`invoke`/`store`/`system`）处理后端起事。
3. **渲染**：用 Vue 组件呈现结果（列表、进度、状态）。

```js
methods: {
  async doSomething() {
    try {
      const res = await window.bridge.invoke('plugin_list');
      this.items = (res && res.plugins) ? res.plugins : [];
    } catch (e) {
      showToast('操作失败', 'error');
    }
  }
}
```

### 3.4 安全与规范

- 涉及敏感/高风险操作（启动游戏、改文件、联网）前，务必走[官方审核（GPLv3）](publish)上架，保证用户信任。
- 所有异步调用记得 `.catch` 并给出提示，避免静默失败。

---

## 模块 4：打包与提交

1. 用 zip 工具把**插件目录**打成 zip。
2. 提交邮件审核（GPLv3 开源），见 [提交审核](publish)。
3. 打包/哈希/上传市场细节见 [打包与上传](packaging)。

> 想对**整个启动器前端**（顶栏、侧边栏、卡片之外的更多地方）做自定义？见 [个性化插件](personalize)。