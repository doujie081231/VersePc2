/* page-home.js - 首页 Vue 组件（页面结构层）
 * ============================================================================
 * 前端三层架构（详见 index.html 顶部注释）：
 *   - 本文件只负责 HTML 模板（Vue template 字符串）
 *   - 交互逻辑全部调用 js/app/*.js 里的函数
 *   - 全局状态变量在 js/app.js
 *
 * 改动原则：
 *   1. CSS 一行不动（class 名保留原样）
 *   2. HTML 结构原样搬运（标签、层级、id 全部不变）
 *   3. JS 函数全部复用（navigateToPage 等仍来自 js/app/*.js）
 *   4. 仅 onclick → @click 这类最小改动
 *   5. 新增页面交互逻辑请写到 js/app/ 对应文件，不要堆在本文件
 *
 * 「启动任务 / 运行中游戏」卡片：由 launch.js 的 updateGameInstanceList 写入
 * 共享响应式 store（window.VersePCGameStore），本组件读取并渲染。
 */

// 共享响应式 store（launch.js 写入，主页卡片读取）
if (!window.VersePCGameStore) {
  window.VersePCGameStore = Vue.reactive({ instances: [], running: false, now: Date.now() });
  // 每秒刷新一次，用于运行时长倒计时显示
  setInterval(function () { window.VersePCGameStore.now = Date.now(); }, 1000);
}

const PIN_TABS = [
  { key: 'save', label: '存档' },
  { key: 'version', label: '版本' },
  { key: 'shader', label: '光影' },
  { key: 'mod', label: '模组' }
];
const PIN_TAB_LABEL = { save: '存档', version: '版本', shader: '光影', mod: '模组' };
const PIN_TAB_ICON_LABEL = { save: '钉子', version: '星星', shader: '钉子', mod: '钉子' };

const PageHome = {
  data() {
    return {
      saves: [],
      savesOpen: false,
      savesLoading: false,
      currentVersionId: '',
      pinsOpen: false,
      pinnedActiveTab: 'save',
      _saveIconCache: {},
      _saveIconLoading: {}
    };
  },
  template: `
    <div class="home-page">
      <!-- 标题：放在原本标题栏位置的中间（最顶部居中，不占内容空间） -->
      <div class="home-page-title topbar-title" data-tauri-drag-region @click="goHome">
        Verse<span class="home-page-title-accent">PC2</span>
      </div>

      <!-- 右上角便利贴入口 -->
      <button class="home-stickynote-btn" :class="{ active: pinsOpen }" @click="togglePins" title="收藏置顶">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h8l6-6V5a2 2 0 0 0-2-2z"/><path d="M14 3v5h5"/><path d="M9 13h6"/><path d="M9 17h4"/></svg>
      </button>

      <!-- 头像（居中放大，皮肤头部像素方块） -->
      <div class="home-avatar-box" id="home-avatar-box" @click="goAccounts" title="账户管理">
        <div class="home-avatar-img-wrap" id="home-avatar"><img src="img/icon.png" alt="" class="home-avatar-img"></div>
      </div>

      <!-- 账户信息（头像下方） -->
      <div class="home-account-bar" @click="goAccounts" style="cursor:pointer" title="账户管理">
        <span class="account-name" id="home-player-name">未登录</span>
        <span class="account-type" id="home-account-type">离线模式</span>
      </div>

      <!-- 版本列表卡片（头像下方） -->
      <div class="home-version-card-wrap">
        <div class="home-current-version-card" id="home-current-version-card" title="点击切换版本">
          <!-- 由 JS 渲染：图标 + 版本名 + 加载器标签 + 右侧箭头 -->
        </div>
      </div>

      <!-- 启动按钮（版本列表下方） -->
      <button id="home-launch-btn" class="btn btn-primary btn-lg home-launch-btn">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polygon points="5 3 19 12 5 21 5 3"/></svg>
        启动游戏
      </button>

      <!-- 启动任务 / 运行中游戏卡片（Vue 响应式，有运行实例时显示） -->
      <div class="home-running-card" v-if="gameInstances.length > 0">
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

      <!-- 底部中间小箭头：点击滑出当前版本存档快速启动 -->
      <button class="home-saves-arrow" :class="{ open: savesOpen }" @click="toggleSaves" :title="savesOpen ? '收起存档' : '展开存档'">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 15 12 9 18 15"/></svg>
      </button>

      <!-- 存档容器（内嵌在首页，从底部展开） -->
      <div class="home-saves-wrap" :class="{ open: savesOpen }">
        <div class="home-saves-drawer">
          <div class="home-saves-drawer-header">
            <span>{{ currentVersionName }} · 存档快速启动</span>
          </div>
          <div class="home-saves-body">
            <p v-if="!currentVersionId" class="home-saves-hint">尚未选择版本，请先在首页选择游戏版本</p>
            <p v-else-if="savesLoading" class="home-saves-hint">加载中...</p>
            <p v-else-if="saves.length === 0" class="home-saves-hint">当前版本暂无存档</p>
            <div v-else class="home-saves-row">
              <div class="home-save-card" v-for="s in saves" :key="s.folder" @click="launchSave(s)" :title="'启动并进入 ' + s.name">
                <div class="home-save-card-bg">
                  <img v-if="saveIconUrl(s)" :src="saveIconUrl(s)" alt="">
                  <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6"><circle cx="12" cy="12" r="10"/><path d="M2 12h20"/><path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"/></svg>
                </div>
                <div class="home-save-card-info">
                  <div class="home-save-card-name" :title="s.name">{{ s.name }}</div>
                  <div class="home-save-card-meta">{{ difficultyLabel(s.difficulty) }}</div>
                </div>
                <button class="pin-btn home-save-pin-btn" :class="{ active: isPinned('save', s.folder) }"
                        :title="isPinned('save', s.folder) ? '取消置顶' : '置顶存档'" @click.stop="pinSave(s)">
                  <span v-html="ic('pin')"></span>
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- 收藏置顶面板（右上角便利贴入口展开） -->
      <div class="home-pins-wrap" :class="{ open: pinsOpen }">
        <div class="home-pins-drawer">
          <div class="home-pins-header">
            <span v-html="ic('pin')" style="display:inline-flex;color:var(--accent)"></span>
            <span>我的置顶</span>
            <button class="home-pins-close" @click="closePins" title="收起">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:16px;height:16px"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
            </button>
          </div>
          <div class="home-pins-tabs">
            <button v-for="t in pinTabs" :key="t.key" class="home-pins-tab" :class="{ active: pinnedActiveTab === t.key }" @click="pinnedActiveTab = t.key">
              {{ t.label }}<span class="home-pins-count">({{ pinsByType(t.key).length }})</span>
            </button>
          </div>
          <div class="home-pins-body">
            <p v-if="pinsByType(pinnedActiveTab).length === 0" class="home-pins-empty">
              暂无置顶{{ currentTabLabel }}，在对应卡片上点击{{ currentTabIconLabel }}即可置顶
            </p>
            <div v-else class="home-pins-item" v-for="p in pinsByType(pinnedActiveTab)" :key="p.type + ':' + p.id" @click="activatePin(p)" :title="pinTip(p)">
              <div class="home-pins-item-icon">
                <img v-if="pinIconUrl(p)" :src="pinIconUrl(p)" alt="">
                <img v-else-if="p.type === 'version'" class="version-icon-img" :data-vi-id="p.id" data-vi-type="release"
                  :data-vi-forge="verFlag(p, 'isForge')" :data-vi-fabric="verFlag(p, 'isFabric')"
                  :data-vi-neoforge="verFlag(p, 'isNeoForge')" :data-vi-modpack="verFlag(p, 'isModpack')" alt="">
                <span v-else v-html="ic(pinIconName(p))"></span>
              </div>
              <div class="home-pins-item-info">
                <div class="home-pins-item-name">{{ p.name }}</div>
                <div class="home-pins-item-sub">{{ pinSub(p) }}</div>
              </div>
              <button class="home-pins-item-del" title="取消置顶" @click.stop="removePinItem(p)">
                <span v-html="ic('trash')"></span>
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>
  `,
  computed: {
    // 读取共享响应式 store，运行实例变化时主页卡片自动更新
    gameInstances() {
      return window.VersePCGameStore ? window.VersePCGameStore.instances : [];
    },
    currentVersionName() {
      const vid = this.currentVersionId;
      if (!vid) return '未选择版本';
      const v = (typeof installedVersions !== 'undefined' && installedVersions || []).find(x => x.id === vid);
      return (v && v.customName) || vid;
    },
    pinTabs() {
      return PIN_TABS;
    },
    currentTabLabel() {
      return PIN_TAB_LABEL[this.pinnedActiveTab] || '存档';
    },
    currentTabIconLabel() {
      return PIN_TAB_ICON_LABEL[this.pinnedActiveTab] || '钉子';
    }
  },
  methods: {
    goAccounts() {
      if (typeof navigateToPage === 'function') {
        navigateToPage('accounts');
      }
    },
    goHome() {
      if (typeof navigateToPage === 'function') {
        navigateToPage('home');
      }
    },
    // ── 收藏置顶面板 ──
    togglePins() {
      if (this.pinsOpen) this.closePins();
      else this.openPins();
    },
    openPins() {
      this.pinsOpen = true;
      if (this.savesOpen) this.closeSaves(); // 与存档抽屉互斥
      // 打开时读取当前选中的版本
      this.currentVersionId = (typeof currentLaunchVersionId !== 'undefined' && currentLaunchVersionId) || '';
      // 面板里的版本项图标是异步加载的，展开后触发一次
      setTimeout(() => {
        const body = this.$el && this.$el.querySelector('.home-pins-body');
        if (body && typeof _loadVersionIcons === 'function') _loadVersionIcons(body);
      }, 80);
    },
    closePins() {
      this.pinsOpen = false;
    },
    isPinned(type, id) {
      return typeof window.isPinned === 'function' && window.isPinned(type, id);
    },
    ic(name) {
      return (window.VersePC.PIN_ICONS && window.VersePC.PIN_ICONS[name]) || '';
    },
    pinSave(s) {
      if (typeof window.togglePinSave === 'function') {
        window.togglePinSave(s.folder, s.name, this.currentVersionId, s.icon || '', s.difficulty);
      }
    },
    pinsByType(type) {
      return (window.VersePC.pinnedStore ? window.VersePC.pinnedStore.items : []).filter(p => p.type === type);
    },
    pinIconName(p) {
      return { save: 'folder', version: 'cube', shader: 'sun', mod: 'puzzle' }[p.type] || 'folder';
    },
    pinIconUrl(p) {
      const e = p.extra || {};
      if (p.type === 'save') {
        if (!e.icon) return '';
        return this.saveIconUrl({ folder: e.folder, icon: e.icon });
      }
      if ((p.type === 'shader' || p.type === 'mod') && e.icon) return e.icon;
      return '';
    },
    pinSub(p) {
      const e = p.extra || {};
      if (p.type === 'save') {
        const parts = [];
        if (e.versionId) parts.push(e.versionId);
        const d = this.difficultyLabel(e.difficulty);
        if (d !== '未知') parts.push(d);
        return parts.join(' · ');
      }
      if (p.type === 'version') return e.loaderText || '原版';
      if (p.type === 'shader' || p.type === 'mod') return e.source === 'curseforge' ? 'CurseForge' : 'Modrinth';
      return '';
    },
    pinTip(p) {
      if (p.type === 'save') return '点击启动并进入该存档';
      if (p.type === 'version') return '点击切换并启动该版本';
      if (p.type === 'shader') return '点击打开光影管理';
      if (p.type === 'mod') return '点击打开模组管理';
      return '';
    },
    verFlag(p, key) {
      const v = (typeof installedVersions !== 'undefined' && installedVersions || []).find(x => x.id === p.id);
      return (v && v[key]) ? '1' : '0';
    },
    async activatePin(p) {
      if (p.type === 'save') {
        const versionId = (p.extra && p.extra.versionId) || this.currentVersionId;
        if (!versionId) {
          showToast('该存档缺少版本信息，请重新置顶', 'warning');
          return;
        }
        this.closePins();
        if (typeof selectLaunchVersion === 'function') selectLaunchVersion(versionId);
        if (typeof quickPlayWorld === 'undefined') {
          showToast('启动入口未就绪', 'error');
          return;
        }
        quickPlayWorld = (p.extra && p.extra.folder) || p.id;
        if (typeof handleLaunch === 'function') handleLaunch();
      } else if (p.type === 'version') {
        this.closePins();
        const id = (p.extra && p.extra.versionId) || p.id;
        if (typeof selectLaunchVersion === 'function') selectLaunchVersion(id);
        if (typeof quickPlayWorld !== 'undefined') quickPlayWorld = '';
        if (typeof handleLaunch === 'function') handleLaunch();
      } else if (p.type === 'shader') {
        this.closePins();
        navigateToPage('shaders');
      } else if (p.type === 'mod') {
        this.closePins();
        navigateToPage('mods');
      }
    },
    async removePinItem(p) {
      const ok = await showConfirmDialog('取消置顶', `确定取消置顶「${p.name}」吗？`, '取消置顶', '再想想');
      if (!ok) return;
      if (typeof window.removePinned === 'function') window.removePinned(p.type, p.id);
      showToast('已取消置顶', 'success');
    },
    // ── 存档快速启动抽屉 ──
    toggleSaves() {
      if (this.savesOpen) this.closeSaves();
      else this.openSaves();
    },
    openSaves() {
      this.savesOpen = true;
      if (this.pinsOpen) this.closePins(); // 与收藏置顶面板互斥
      // 打开时读取当前选中的版本（全局变量非响应式，不能靠 computed 自动跟随）
      this.currentVersionId = (typeof currentLaunchVersionId !== 'undefined' && currentLaunchVersionId) || '';
      this.loadSaves();
    },
    closeSaves() {
      this.savesOpen = false;
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
      // 收起抽屉，走与「启动游戏」一致的完整启动流程，并带上世界名直接进存档
      this.closeSaves();
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
    // Vue 已将首页 DOM 挂载到文档，此时才存在 home-avatar 等元素
    // 延迟一帧确保布局尺寸已计算完成，再补一次头像、版本卡片和启动按钮绑定
    // （因为 init-setup.js 的 setupLaunchBar 执行时机早于 Vue 挂载，会绑定不到 home-launch-btn）
    const init = () => {
      if (typeof loadAccounts !== 'function' || typeof loadVersions !== 'function' || typeof handleLaunch !== 'function') {
        setTimeout(init, 80);
        return;
      }
      // 补绑定主页启动按钮（setupLaunchBar 可能因 DOM 未挂载而绑定失败）
      const homeLaunchBtn = document.getElementById('home-launch-btn');
      if (homeLaunchBtn && !homeLaunchBtn._bound) {
        homeLaunchBtn._bound = true;
        homeLaunchBtn.addEventListener('click', handleLaunch);
      }
      // 刷新账户显示（含头像加载）和版本列表（含主页版本卡片渲染）
      loadAccounts().catch(e => console.error('[PageHome] loadAccounts error:', e));
      loadVersions().catch(e => console.error('[PageHome] loadVersions error:', e));
      // 拉取一次运行中游戏状态，让主页启动任务卡片尽快显示
      if (typeof updateGameStatus === 'function') {
        updateGameStatus().catch(function (e) { console.error('[PageHome] updateGameStatus error:', e); });
      }
    };
    requestAnimationFrame(() => setTimeout(init, 50));
  }
};

// 导出供主入口使用
window.VersePC = window.VersePC || {};
window.VersePC.PageHome = PageHome;