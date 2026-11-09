// features/webpanel/panel.js — 内置网页面板（社区 / 皮肤站）通用逻辑
// 网页由本地反向代理转发（Rust 侧 web_panel.rs），这里只负责：
//   · 取代理地址、挂 iframe
//   · 工具栏指令（后退 / 前进 / 刷新 / 回首页）通过 postMessage 交给注入脚本执行
//   · 会话状态轮询与登录态变化后自动刷新
//   · 爱发电登录（交给 Rust 用独立窗口完成，回来后会话写进软件的 cookie 罐）

const KIND_LABEL = {
  community: '社区',
  skin: '皮肤站',
};

/** 统一 invoke（Tauri v2 的 __TAURI_INTERNALS__ 没有 .core，invoke 在自身） */
function call(cmd, args) {
  const t = window.__TAURI__;
  const core = (t && t.core) || window.__TAURI_INTERNALS__;
  if (!core || typeof core.invoke !== 'function') {
    return Promise.reject(new Error('Tauri invoke 不可用: ' + cmd));
  }
  return core.invoke(cmd, args || {});
}

/** 监听 Tauri 事件（拿不到就以轮询兜底） */
function listen(event, handler) {
  try {
    const t = window.__TAURI__;
    if (t && t.event && typeof t.event.listen === 'function') {
      t.event.listen(event, (e) => handler(e && e.payload));
    }
  } catch (e) {}
}

export const PANEL_KINDS = KIND_LABEL;

/**
 * 生成一个面板页组件（社区 / 皮肤站共用）
 * @param {'community'|'skin'} kind
 */
export function createPanelPage(kind) {
  const label = KIND_LABEL[kind] || '内置网页';

  return {
    name: 'Page' + kind.charAt(0).toUpperCase() + kind.slice(1),
    data() {
      return {
        kind,
        label,
        base: '',          // 代理根地址，如 http://127.0.0.1:17866/
        frameSrc: '',      // 当前 iframe 地址
        frameKey: 0,       // 变化时重建 iframe，等价于彻底刷新
        currentUrl: '',    // 注入脚本上报的当前页面地址
        keys: [],          // 会话键
        ready: false,
        hostLabel: '',
        busy: false,
        timer: null,
        onMessage: null,
      };
    },
    computed: {
      loggedIn() {
        return !!(this.keys && this.keys.length);
      },
      stateText() {
        if (!this.ready) return '连接中…';
        return this.loggedIn ? '已登录' : '未登录';
      },
    },
    methods: {
      async init() {
        try {
          const eps = await call('web_panel_endpoints');
          if (!eps || !eps[this.kind]) {
            this.hostLabel = '代理未启动';
            return;
          }
          this.base = eps[this.kind];
          this.frameSrc = this.base;
          this.hostLabel = this.base.replace(/^http:\/\//, '').replace(/\/$/, '');
          this.ready = true;
          this.refreshKeys();
        } catch (e) {
          this.hostLabel = '面板初始化失败: ' + (e && e.message ? e.message : e);
        }
      },
      sendCmd(cmd) {
        const f = this.$refs.frame;
        if (!f || !f.contentWindow) return;
        try {
          f.contentWindow.postMessage({ __versePanelCmd: cmd }, '*');
        } catch (e) {}
      },
      reload() {
        // 重建 iframe：跨域下无法直接访问其 history，换 key 最稳
        if (!this.base) return;
        this.frameSrc = this.base;
        this.frameKey += 1;
      },
      home() {
        this.sendCmd('home');
        this.reload();
      },
      openExternal() {
        const url = this.currentUrl || this.base;
        if (url) call('web_panel_open_external', { url }).catch(() => {});
      },
      async login() {
        if (this.busy) return;
        this.busy = true;
        // 登录会临时占用主窗口，记下当前页，回来时自动跳回
        try {
          window.localStorage.setItem('versePanelReturn', this.kind);
        } catch (e) {}
        try {
          await call('web_panel_login_in_app', { kind: this.kind });
        } catch (e) {
          this.hostLabel = '登录窗口打开失败: ' + (e && e.message ? e.message : e);
        } finally {
          setTimeout(() => { this.busy = false; }, 600);
        }
      },
      async refreshKeys() {
        try {
          const keys = await call('web_panel_session_keys', { kind: this.kind });
          const arr = Array.isArray(keys) ? keys : [];
          if (arr.join('|') !== this.keys.join('|')) {
            const had = this.keys.length;
            this.keys = arr;
            // 首次有会话、或会话从无到有，刷新页面让它带上登录态
            if (arr.length && (had === 0 || arr.length > had)) {
              setTimeout(() => this.reload(), 200);
            }
          }
        } catch (e) {}
      },
    },
    mounted() {
      this.init();
      // 注入脚本上报的状态消息 + 登录请求
      this.onMessage = (ev) => {
        const f = this.$refs.frame;
        if (!f || ev.source !== f.contentWindow) return;
        const d = ev.data || {};
        if (typeof d.__versePanelExternal === 'string' && d.__versePanelExternal) {
          call('web_panel_open_external', { url: d.__versePanelExternal }).catch(() => {});
          return;
        }
        if (d.__versePanelLogin === true) {
          this.login();
          return;
        }
        if (d.__versePanel === 'state' && typeof d.url === 'string') {
          this.currentUrl = d.url;
        }
      };
      window.addEventListener('message', this.onMessage);
      listen('webpanel:session-refreshed', (payload) => {
        if (payload === this.kind || payload === undefined) {
          this.refreshKeys();
          setTimeout(() => this.reload(), 300);
        }
      });
      this.timer = setInterval(() => {
        if (!this.ready) return;
        if (document.getElementById('page-' + this.kind)?.classList.contains('active')) {
          this.refreshKeys();
        }
      }, 2500);
    },
    beforeUnmount() {
      if (this.onMessage) window.removeEventListener('message', this.onMessage);
      if (this.timer) clearInterval(this.timer);
    },
    template: `
      <div class="wp-shell">
        <div class="wp-bar">
          <button class="wp-btn wp-icon" title="后退" @click="sendCmd('back')">‹</button>
          <button class="wp-btn wp-icon" title="前进" @click="sendCmd('forward')">›</button>
          <button class="wp-btn wp-icon" title="刷新" @click="reload()">⟳</button>
          <button class="wp-btn" title="回到首页" @click="home()">首页</button>
          <div class="wp-addr" :title="currentUrl">{{ hostLabel }}</div>
          <div class="wp-state" :class="{ on: loggedIn }">
            <span class="wp-dot"></span>
            <span>{{ stateText }}</span>
          </div>
          <div class="wp-spacer"></div>
          <button class="wp-btn" title="在系统浏览器中打开" @click="openExternal()">浏览器打开</button>
          <button
            class="wp-btn wp-accent"
            :disabled="busy"
            :title="kind === 'community' ? '使用爱发电账号登录' : '在软件内登录皮肤站'"
            @click="login()"
          >{{ kind === 'community' ? '爱发电登录' : '登录' }}</button>
        </div>
        <div class="wp-body">
          <iframe
            v-if="frameSrc"
            ref="frame"
            :key="frameKey"
            :src="frameSrc"
            class="wp-frame"
            allow="clipboard-write; fullscreen"
          ></iframe>
          <div v-else class="wp-empty">{{ hostLabel || '正在启动内置网页…' }}</div>
        </div>
      </div>
    `,
  };
}
