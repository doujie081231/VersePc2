/* page-home.js - 首页 Vue 组件（页面结构层）
 * 主页可移动控件系统（参考 ZalithLauncher2 主页卡片模式）：
 *   - 每个控件（软件名/头像/版本卡/启动按钮/存档坞/运行卡片）均为绝对定位，
 *     按住控件主体直接拖动（移动超过 5px 视为拖动，原地点击保持原有交互），
 *     位置与头像缩放持久化到 localStorage（versepc_home_layout_v1）。
 *   - 头像：拖动移动 + 滚轮缩放（0.5x ~ 2.0x），点击进入账户管理。
 *   - 存档坞：常驻卡片，显示当前版本存档，点击存档直接启动。
 *   - 个性化设置页「恢复主页默认布局」会派发 versepc-home-layout-reset 事件，
 *     本组件监听后清空已保存布局并回到默认位置。
 * 交互逻辑调用 js/app/*.js 的全局函数，全局状态变量在 js/app.js。
 */

// 共享响应式 store（launch.js 写入，主页卡片读取）
if (!window.VersePCGameStore) {
  window.VersePCGameStore = Vue.reactive({ instances: [], running: false, now: Date.now() });
  // 每秒刷新一次，用于运行时长倒计时显示
  setInterval(function () { window.VersePCGameStore.now = Date.now(); }, 1000);
}

const LAYOUT_KEY = 'versepc_home_layout_v1';
const AVATAR_MIN = 0.5, AVATAR_MAX = 2.0, DRAG_THRESHOLD = 5;

function loadHomeLayout() {
  try {
    return JSON.parse(localStorage.getItem(LAYOUT_KEY) || '{}') || {};
  } catch (e) { return {}; }
}
function saveHomeLayout(layout) {
  try { localStorage.setItem(LAYOUT_KEY, JSON.stringify(layout)); } catch (e) {}
}
function clampPos(v, min, max) {
  return Math.max(min, Math.min(max, v));
}

const PageHome = {
  data() {
    return {
      saves: [],
      currentVersionId: '',
      savesLoading: false,
      _saveIconCache: {},
      _saveIconLoading: {}
    };
  },
  template: `
    <div class="home-page" ref="homePage">
      <!-- 软件名称（按住拖动，点击回主页） -->
      <div class="home-page-title topbar-title" ref="titleEl" data-widget="title" title="按住拖动 · 点击回主页">
        Verse<span class="home-page-title-accent">PC2</span>
      </div>

      <!-- 头像控件（按住拖动 + 滚轮缩放；点击进入账户管理） -->
      <div class="home-avatar-widget" ref="avatarEl" data-widget="avatar" title="按住拖动 · 滚轮缩放 · 点击进入账户">
        <div class="home-avatar-scale" ref="avatarScale">
          <div class="home-avatar-box" id="home-avatar-box">
            <div class="home-avatar-img-wrap" id="home-avatar"><img src="img/icon.png" alt="" class="home-avatar-img"></div>
          </div>
          <div class="home-account-bar" id="home-account-bar">
            <span class="account-name" id="home-player-name">未登录</span>
            <span class="account-type" id="home-account-type">离线模式</span>
          </div>
        </div>
      </div>

      <!-- 版本列表卡片（按住拖动） -->
      <div class="home-version-card-wrap" ref="versionEl" data-widget="version" title="按住拖动">
        <div class="home-current-version-card" id="home-current-version-card" title="点击切换版本">
          <!-- 由 JS 渲染：图标 + 版本名 + 加载器标签 + 右侧箭头 -->
        </div>
      </div>

      <!-- 启动按钮（按住拖动，点击启动） -->
      <button id="home-launch-btn" ref="launchEl" class="btn btn-primary btn-lg home-launch-btn" title="按住拖动 · 点击启动">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polygon points="5 3 19 12 5 21 5 3"/></svg>
        启动游戏
      </button>

      <!-- 启动任务 / 运行中游戏卡片（按住拖动，有运行实例时显示） -->
      <div class="home-running-card" ref="runningEl" v-if="gameInstances.length > 0" title="按住拖动">
        <div class="home-running-card-header">
          <span class="home-running-dot"></span>
          <span class="home-running-title">启动任务</span>
          <span class="home-running-count" v-if="gameInstances.length > 1">({{ gameInstances.length }})</span>
        </div>
        <div class="home-running-list">
          <div class="home-running-item" v-for="inst in gameInstances" :key="inst.sessionId">
            <div class="home-running-info">
              <div class="home-running-name">{{ inst.versionId }}</div>
              <div class="home-running-meta">PID: {{ inst.pid }} · {{ formatElapsed(inst) }}</div>
            </div>
            <button class="home-running-stop" @click="stopInstance(inst.sessionId)">停止</button>
          </div>
        </div>
      </div>

      <!-- 存档坞：常驻卡片（按住拖动），显示当前版本存档，点击直接启动 -->
      <div class="home-save-dock" ref="dockEl" data-widget="saves" title="按住拖动">
        <div class="home-save-dock-header">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" style="width:16px;height:16px;color:var(--accent)"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
          <span class="home-save-dock-title">{{ currentVersionName }} · 存档</span>
        </div>
        <div class="home-save-dock-body">
          <p v-if="!currentVersionId" class="home-saves-hint">尚未选择版本，请先在首页选择游戏版本</p>
          <p v-else-if="savesLoading" class="home-saves-hint">加载中...</p>
          <p v-else-if="saves.length === 0" class="home-saves-hint">当前版本暂无存档</p>
          <div v-else class="home-save-dock-list">
            <div class="home-save-card" v-for="s in saves" :key="s.folder" @click="launchSave(s)" :title="'启动并进入 ' + s.name">
              <div class="home-save-card-bg">
                <img v-if="saveIconUrl(s)" :src="saveIconUrl(s)" alt="">
                <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6"><circle cx="12" cy="12" r="10"/><path d="M2 12h20"/><path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"/></svg>
              </div>
              <div class="home-save-card-info">
                <div class="home-save-card-name" :title="s.name">{{ s.name }}</div>
                <div class="home-save-card-meta">{{ difficultyLabel(s.difficulty) }}</div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  `,
  computed: {
    gameInstances() {
      return window.VersePCGameStore ? window.VersePCGameStore.instances : [];
    },
    currentVersionName() {
      const vid = this.currentVersionId;
      if (!vid) return '未选择版本';
      const v = (typeof installedVersions !== 'undefined' && installedVersions || []).find(x => x.id === vid);
      return (v && v.customName) || vid;
    }
  },
  methods: {
    widgetByKey(key) {
      return {
        title: this.$refs.titleEl,
        avatar: this.$refs.avatarEl,
        version: this.$refs.versionEl,
        launch: this.$refs.launchEl,
        saves: this.$refs.dockEl,
        running: this.$refs.runningEl
      }[key];
    },
    // ── 可移动控件系统（参考 ZalithLauncher2 主页卡片：按住拖动 + 位置持久化） ──
    // 有保存位置 → 应用（clamp 到窗口内）；没有 → 清除内联样式回退 CSS 默认（精确居中/停靠）。
    applyLayout(force) {
      const page = this.$refs.homePage;
      if (!page) return;
      const layout = force ? {} : loadHomeLayout();
      const pw = page.clientWidth, ph = page.clientHeight;
      for (const key of ['title', 'avatar', 'version', 'launch', 'saves', 'running']) {
        const el = this.widgetByKey(key);
        if (!el) continue;
        const pos = (!force && layout[key]) ? layout[key] : null;
        if (pos) {
          const w = el.offsetWidth || 10, h = el.offsetHeight || 10;
          el.style.left = clampPos(pos.x, 0, Math.max(0, pw - w)) + 'px';
          el.style.top = clampPos(pos.y, 0, Math.max(0, ph - h)) + 'px';
          el.style.right = 'auto';
          el.style.bottom = 'auto';
        } else {
          // 回退 CSS 默认：标题/头像/版本卡/启动按钮水平居中，存档坞右侧停靠，运行卡片左下
          el.style.left = '';
          el.style.top = '';
          el.style.right = '';
          el.style.bottom = '';
          el.style.transform = '';
        }
      }
      // 头像缩放（恢复默认时重置为 1）
      const s = (!force && layout.avatar && layout.avatar.s) || 1;
      this._avatarScale = s;
      if (this.$refs.avatarScale) {
        this.$refs.avatarScale.style.transform = 'scale(' + s + ')';
      }
    },
    persistPos(key, el) {
      const layout = loadHomeLayout();
      layout[key] = layout[key] || {};
      layout[key].x = parseInt(el.style.left, 10) || 0;
      layout[key].y = parseInt(el.style.top, 10) || 0;
      if (key === 'avatar') {
        layout.avatar.s = this._avatarScale || 1;
      }
      saveHomeLayout(layout);
    },
    /**
     * 按住控件主体直接拖动：移动超过阈值 = 拖动并持久化位置；
     * 原地松手（未拖动）= 正常派发 click，保持控件内部原有交互。
     * 拖动结束后的一次 click 会被捕获拦截，防止误触内部交互。
     */
    bindDrag(el, key, onTap, excludeSelector) {
      if (!el) return;
      const self = this;
      el.addEventListener('pointerdown', function (e) {
        if (e.button !== 0) return;
        if (excludeSelector && e.target.closest && e.target.closest(excludeSelector)) return;
        const page = self.$refs.homePage;
        const rect = el.getBoundingClientRect();
        const pageRect = page.getBoundingClientRect();
        const ox = rect.left - pageRect.left;
        const oy = rect.top - pageRect.top;
        const sx = e.clientX, sy = e.clientY;
        let moved = false;
        const onMove = function (ev) {
          const dx = ev.clientX - sx, dy = ev.clientY - sy;
          if (!moved && Math.abs(dx) < DRAG_THRESHOLD && Math.abs(dy) < DRAG_THRESHOLD) return;
          if (!moved) {
            moved = true;
            el.classList.add('home-dragging');
            // 退出 CSS 默认定位（水平居中 transform / 右侧停靠 right），此后由内联 left/top 全权控制。
            // ox/oy 取自 getBoundingClientRect（真实视觉位置），清 transform 不会产生跳变。
            el.style.transform = 'none';
            el.style.right = 'auto';
            el.style.bottom = 'auto';
          }
          ev.preventDefault();
          const w = el.offsetWidth || 10, h = el.offsetHeight || 10;
          el.style.left = clampPos(ox + dx, 0, Math.max(0, pageRect.width - w)) + 'px';
          el.style.top = clampPos(oy + dy, 0, Math.max(0, pageRect.height - h)) + 'px';
        };
        const onUp = function () {
          document.removeEventListener('pointermove', onMove);
          document.removeEventListener('pointerup', onUp);
          document.removeEventListener('pointercancel', onUp);
          el.classList.remove('home-dragging');
          if (moved) {
            self.persistPos(key, el);
            // 吞掉拖动结束后派发的 click，防止误触内部交互（启动/进账户/启动存档）
            const swallow = function (ev) {
              ev.stopPropagation();
              ev.preventDefault();
              el.removeEventListener('click', swallow, true);
            };
            el.addEventListener('click', swallow, true);
            setTimeout(function () { el.removeEventListener('click', swallow, true); }, 350);
          }
        };
        document.addEventListener('pointermove', onMove);
        document.addEventListener('pointerup', onUp);
        document.addEventListener('pointercancel', onUp);
      });
    },
    setupWidgets() {
      const self = this;
      // 主页控件内的 <img>（头像、版本图标、存档图标）默认是浏览器原生可拖拽元素：
      // 按住拖动时浏览器接管拖拽，光标显示「禁止放置」并取消 pointermove/pointerup，
      // 自定义拖动完全失效。头像图片由账户逻辑动态替换，故在页面根节点上统一拦截 dragstart。
      if (this.$refs.homePage) {
        this.$refs.homePage.addEventListener('dragstart', function (e) {
          e.preventDefault();
        });
      }
      // 软件名：按住拖动 + 点击回主页
      this.bindDrag(this.$refs.titleEl, 'title', function () {
        if (typeof navigateToPage === 'function') navigateToPage('home');
      });
      // 头像：按住拖动 + 点击进账户 + 滚轮缩放
      this.bindDrag(this.$refs.avatarEl, 'avatar', function () {
        if (typeof navigateToPage === 'function') navigateToPage('accounts');
      });
      const avatarEl = this.$refs.avatarEl;
      const scaleWrap = this.$refs.avatarScale;
      if (avatarEl && scaleWrap) {
        const layout = loadHomeLayout();
        this._avatarScale = (layout.avatar && layout.avatar.s) || 1;
        scaleWrap.style.transform = 'scale(' + this._avatarScale + ')';
        avatarEl.addEventListener('wheel', function (e) {
          e.preventDefault();
          self._avatarScale = clampPos(self._avatarScale + (e.deltaY < 0 ? 0.1 : -0.1), AVATAR_MIN, AVATAR_MAX);
          scaleWrap.style.transform = 'scale(' + self._avatarScale + ')';
          self.persistPos('avatar', avatarEl);
        }, { passive: false });
      }
      // 版本卡：按住拖动（内部点击切版本不受影响）
      this.bindDrag(this.$refs.versionEl, 'version', null);
      // 启动按钮：按住拖动 + 点击启动
      this.bindDrag(this.$refs.launchEl, 'launch', function () {
        if (typeof handleLaunch === 'function') handleLaunch();
      });
      // 运行卡片：按住拖动（排除内部「停止」按钮，避免点击停止被拖动吞掉）
      this.bindDrag(this.$refs.runningEl, 'running', null, '.home-running-stop');
      // 存档坞：按住拖动（存档卡点击启动不受影响）
      this.bindDrag(this.$refs.dockEl, 'saves', null);
      // 窗口尺寸变化时把控件收回可视区域
      window.addEventListener('resize', function () { self.applyLayout(); });
      // 个性化页「恢复主页默认布局」
      window.addEventListener('versepc-home-layout-reset', function () { self.applyLayout(true); });
    },
    // ── 存档坞 ──
    async pollVersionChange() {
      const vid = (typeof currentLaunchVersionId !== 'undefined' && currentLaunchVersionId) || '';
      if (vid !== this._lastVersionId) {
        this._lastVersionId = vid;
        this.currentVersionId = vid;
        await this.loadSaves();
      }
      // 运行卡片是 v-if 挂载的：出现后按保存位置应用，无保存则回退 CSS 默认（左下角）
      const rEl = this.$refs.runningEl;
      if (rEl && !rEl.style.left) {
        const layout = loadHomeLayout();
        const page = this.$refs.homePage;
        const pos = layout.running;
        if (pos) {
          rEl.style.left = clampPos(pos.x, 0, Math.max(0, page.clientWidth - (rEl.offsetWidth || 10))) + 'px';
          rEl.style.top = clampPos(pos.y, 0, Math.max(0, page.clientHeight - (rEl.offsetHeight || 10))) + 'px';
          rEl.style.right = 'auto';
          rEl.style.bottom = 'auto';
        }
        this.bindDrag(rEl, 'running', null, '.home-running-stop');
      }
    },
    async loadSaves() {
      const vid = this.currentVersionId;
      if (!vid) {
        this.saves = [];
        return;
      }
      this.savesLoading = true;
      try {
        const res = await window.bridge.invoke('version_list_saves', { versionId: vid });
        this.saves = (res && res.saves) || [];
      } catch (e) {
        console.error('[HomeSaves] Load error:', e);
        this.saves = [];
      } finally {
        this.savesLoading = false;
      }
    },
    saveIconUrl(s) {
      if (!s || !s.icon) return '';
      if (this._saveIconCache[s.folder]) return this._saveIconCache[s.folder];
      if (this._saveIconLoading[s.folder]) return '';
      this._saveIconLoading[s.folder] = true;
      const self = this;
      Promise.resolve(window.bridge && window.bridge.readFileBuffer(s.icon)).then((buffer) => {
        try {
          const u8 = typeof window.decodeFileBuffer === 'function'
            ? window.decodeFileBuffer(buffer)
            : (buffer instanceof Uint8Array ? buffer : new Uint8Array(buffer || []));
          if (!u8 || u8.byteLength === 0) return;
          const blob = new Blob([u8], { type: 'image/png' });
          self._saveIconCache[s.folder] = URL.createObjectURL(blob);
        } catch (e) {}
      }).catch(() => {});
      return '';
    },
    launchSave(s) {
      const vid = this.currentVersionId;
      if (!vid) {
        showToast('请先选择游戏版本', 'warning');
        return;
      }
      if (typeof quickPlayWorld === 'undefined') {
        showToast('启动入口未就绪', 'error');
        return;
      }
      quickPlayWorld = s.folder;
      if (typeof handleLaunch === 'function') {
        handleLaunch();
      } else {
        showToast('启动入口未就绪', 'error');
      }
    },
    difficultyLabel(d) {
      return { 0: '和平', 1: '简单', 2: '普通', 3: '困难' }[d] || '未知';
    },
    // 计算运行时长（每秒由 store.now 触发重新渲染）
    formatElapsed(inst) {
      const elapsed = Math.floor(((window.VersePCGameStore.now || Date.now()) - inst.startTime) / 1000);
      const mins = Math.floor(elapsed / 60);
      const secs = elapsed % 60;
      return mins > 0 ? `${mins}分${secs}秒` : `${secs}秒`;
    },
    stopInstance(sessionId) {
      if (typeof stopGameInstance === 'function') {
        stopGameInstance(sessionId);
      }
    }
  },
  mounted() {
    const self = this;
    const init = function () {
      if (typeof loadAccounts !== 'function' || typeof loadVersions !== 'function' || typeof handleLaunch !== 'function') {
        setTimeout(init, 80);
        return;
      }
      // 刷新账户显示（含头像加载）和版本列表（含主页版本卡片渲染）
      loadAccounts().catch(function (e) { console.error('[PageHome] loadAccounts error:', e); });
      loadVersions().catch(function (e) { console.error('[PageHome] loadVersions error:', e); });
      // 拉取一次运行中游戏状态，让主页启动任务卡片尽快显示
      if (typeof updateGameStatus === 'function') {
        updateGameStatus().catch(function (e) { console.error('[PageHome] updateGameStatus error:', e); });
      }
      // 应用持久化布局 / 默认位置，并绑定拖动与缩放
      self.applyLayout();
      self.setupWidgets();
      // 存档坞：初始加载 + 轮询检测版本切换（全局变量非响应式）
      self.pollVersionChange();
      setInterval(function () { self.pollVersionChange(); }, 1200);
    };
    requestAnimationFrame(function () { setTimeout(init, 50); });
  }
};

// 导出供主入口使用
window.VersePC = window.VersePC || {};
window.VersePC.PageHome = PageHome;
