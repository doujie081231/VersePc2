/* page-home.js - 首页 Vue 组件（页面结构层）
 * 主页可移动控件系统：
 *   - 每个控件（软件名/头像/版本卡/启动按钮/运行卡片）均为绝对定位。
 *   - 默认不可拖动；点击右下角「编辑布局」进入编辑模式后，按住控件主体拖动
 *     （移动超过 5px 视为拖动，原地点击保持原有交互）。
 *   - 编辑期间「启动游戏」按钮置灰且所有控件点击被拦截；
 *     只有点击「确认」才会保存位置并恢复启动按钮，点击「取消」恢复原布局。
 *   - 头像：编辑模式下拖动移动 + 滚轮缩放（0.5x ~ 2.0x），默认点击进入账户管理。
 *   - 个性化设置页「恢复主页默认布局」会派发 versepc-home-layout-reset 事件，
 *     本组件监听后只重置坐标，保留用户已添加的控件。
 *   - 编辑布局时工具栏左侧有「+」：打开控件库，可把「下载」等可选控件加进主页。
 */

import { gameStore } from '../../core/game-store.js';

import { loadAccounts } from '../../features/accounts/accounts-list.js';
import { updateGameStatus, stopGameInstance } from '../../features/launch/launch.js';
import { loadVersions } from '../../features/versions/versions-list.js';
import { navigateToPage } from '../../core/navigation.js';
import {
  loadHomeLayout,
  saveHomeLayout,
  isWidgetVisible,
  ensureWidgetVisible,
  removeWidget,
  resetWidgetPositions,
  onLayoutChange,
  hydrateHomeLayoutFromBackend,
  OPTIONAL_WIDGETS,
  WIDGETS,
} from '../../features/home/home-widgets.js';
import HomeDownloadWidget from './home-download-widget.js';

const AVATAR_MIN = 0.5, AVATAR_MAX = 2.0, DRAG_THRESHOLD = 5;

function clampPos(v, min, max) {
  return Math.max(min, Math.min(max, v));
}

const PageHome = {
  components: { HomeDownloadWidget },
  data() {
    return {
      layoutReady: false,
      editingLayout: false,
      widgetPickerOpen: false,       // 控件库面板是否展开
      visibleWidgets: {},            // { download: true } —— 由 home-widgets 维护
      _boundNav: {},
      optionalWidgets: OPTIONAL_WIDGETS,
      iconPath: (id) => (WIDGETS[id] && WIDGETS[id].icon) || null,
      _draftChanged: {}
    };
  },
  template: `
    <div class="home-page" :class="{ 'home-layout-pending': !layoutReady, 'home-editing': editingLayout }" ref="homePage">
      <!-- 软件名称（编辑模式下按住拖动，点击回主页） -->
      <div class="home-page-title topbar-title" ref="titleEl" data-widget="title"
           :title="editingLayout ? '拖动调整位置' : '点击回主页'">
        Verse<span class="home-page-title-accent">PC2</span>
      </div>

      <!-- 头像控件（编辑模式下拖动 + 滚轮缩放；默认点击进入账户管理） -->
      <div class="home-avatar-widget" ref="avatarEl" data-widget="avatar"
           :title="editingLayout ? '拖动调整位置 · 滚轮缩放' : '点击进入账户'">
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

      <!-- 版本列表卡片（编辑模式下按住拖动） -->
      <div class="home-version-card-wrap" ref="versionEl" data-widget="version"
           :title="editingLayout ? '拖动调整位置' : '点击切换版本'">
        <div class="home-current-version-card" id="home-current-version-card" title="点击切换版本">
          <!-- 由 JS 渲染：图标 + 版本名 + 加载器标签 + 右侧箭头 -->
        </div>
      </div>

      <!-- 启动按钮（默认点击启动；编辑模式下置灰锁定，确认布局后才能点击） -->
      <button id="home-launch-btn" ref="launchEl" class="btn btn-primary btn-lg home-launch-btn"
              :class="{ 'home-launch-btn-locked': editingLayout }"
              :aria-disabled="editingLayout ? 'true' : 'false'"
              :title="editingLayout ? '编辑中，确认布局后才能启动' : '点击启动'">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polygon points="5 3 19 12 5 21 5 3"/></svg>
        启动游戏
      </button>

      <!-- 启动任务 / 运行中游戏卡片（编辑模式下按住拖动，有运行实例时显示） -->
      <div class="home-running-card" ref="runningEl" v-if="gameInstances.length > 0"
           :title="editingLayout ? '拖动调整位置' : ''">
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
            <button class="home-running-stop" @click.stop="stopInstance(inst.sessionId)">停止</button>
          </div>
        </div>
      </div>

      <!-- 可选控件：下载（特殊交互卡） -->
      <HomeDownloadWidget
        v-if="visibleWidgets.download"
        ref="downloadEl"
        class="home-download-widget"
        data-widget="download"
        :editing="editingLayout"
        :title="editingLayout ? '拖动调整位置' : '下载'"
      />

      <!-- 可选控件：功能入口小卡片（v-for 渲染除下载外的全部可选功能） -->
      <button v-for="w in navWidgets"
              :key="w.id"
              :ref="'navEl_' + w.id"
              v-show="visibleWidgets[w.id]"
              class="home-nav-widget"
              :data-widget="w.id"
              :title="editingLayout ? '拖动调整位置' : w.name"
              @click.stop="onNavWidgetClick(w)">
        <svg class="home-nav-widget-icon"><use :href="'#' + w.icon"/></svg>
        <span class="home-nav-widget-name">{{ w.name }}</span>
      </button>

      <!-- 布局编辑入口：默认只显示「编辑布局」，编辑中显示「+」/提示/取消/确认 -->
      <div class="home-edit-toolbar" :class="{ 'home-edit-toolbar-active': editingLayout }">
        <button v-if="editingLayout" type="button" id="home-widget-add-btn"
                class="home-widget-add-btn" title="添加控件"
                @click.stop="toggleWidgetPicker">+</button>
        <button v-if="!editingLayout" type="button" id="home-edit-layout-btn"
                class="btn btn-secondary btn-sm" @click.stop="beginLayoutEdit">编辑布局</button>
        <template v-else>
          <button type="button" id="home-edit-cancel-btn"
                  class="btn btn-secondary btn-sm" @click.stop="cancelLayoutEdit">取消</button>
          <button type="button" id="home-edit-confirm-btn"
                  class="btn btn-primary btn-sm" @click.stop="confirmLayoutEdit">确认</button>
        </template>
      </div>

      <!-- 控件库：编辑模式下点「+」展开，覆盖软件全部功能 -->
      <div v-if="widgetPickerOpen && editingLayout"
           class="home-widget-picker" @click.stop>
        <div class="home-widget-picker-head">
          <span>添加功能到主页</span>
          <span class="home-widget-picker-count">{{ visibleCount }}/{{ optionalWidgets.length }}</span>
          <button type="button" class="home-widget-picker-close" @click.stop="widgetPickerOpen = false">×</button>
        </div>
        <div class="home-widget-picker-grid">
          <button v-for="w in optionalWidgets" :key="w.id" type="button"
                  class="home-widget-picker-item"
                  :class="{ 'is-added': visibleWidgets[w.id] }"
                  :title="w.desc"
                  @click.stop="toggleWidget(w.id)">
            <svg v-if="w.icon" class="home-widget-picker-icon"><use :href="'#' + w.icon"/></svg>
            <span class="home-widget-picker-name">{{ w.name }}</span>
            <span class="home-widget-picker-state">{{ visibleWidgets[w.id] ? '✓' : '+' }}</span>
          </button>
        </div>
      </div>
    </div>
  `,
  computed: {
    gameInstances() {
      return gameStore ? gameStore.instances : [];
    },
    visibleCount() {
      return Object.keys(this.visibleWidgets).length;
    },
    navWidgets() {
      return OPTIONAL_WIDGETS.filter((w) => w.action !== 'download' && w.icon);
    }
  },
  methods: {
    widgetByKey(key) {
      // 核心控件用 ref 拿（它们不是 v-for 生成的）
      const fixed = {
        title: this.$refs.titleEl,
        avatar: this.$refs.avatarEl,
        version: this.$refs.versionEl,
        launch: this.$refs.launchEl,
        running: this.$refs.runningEl,
        download: this.$refs.downloadEl ? (this.$refs.downloadEl.$el || this.$refs.downloadEl) : null
      }[key];
      if (fixed) return fixed;

      // 可选控件（模组/光影包/设置…）由 v-for 渲染，字符串 ref 在生产版 Vue 里
      // 结构不稳定（可能是数组、也可能为空），所以直接按 data-widget 查 DOM。
      // 早先这里没有这条分支，applyLayout 里 if (!el) continue 会把它们全部跳过，
      // 导致可选控件的坐标永远应用不上，看起来就是"布局没保存/全叠在一起"。
      const scope = this.$refs.homePage || document;
      return scope.querySelector('[data-widget="' + key + '"]');
    },
    // ── 控件库 ──
    /** 展开/收起控件库面板 */
    toggleWidgetPicker() {
      this.widgetPickerOpen = !this.widgetPickerOpen;
    },
    /** 点击导航类控件主体：跳到对应功能页（拖动由 bindDrag 处理） */
    onNavWidgetClick(w) {
      if (typeof navigateToPage === 'function' && w.page) navigateToPage(w.page);
    },
    /** 添加/移除一个可选控件 */
    toggleWidget(id) {
      if (this.visibleWidgets[id]) {
        if (!removeWidget(id)) return;
      } else {
        ensureWidgetVisible(id);
        // 新增控件必须立刻分配一个空闲坐标并落盘。
        // 否则它没有位置数据 → applyLayout 走「回退 CSS 默认」→ 所有导航卡
        // 叠在同一个默认位置，用户看起来就是"布局变回默认/控件重叠"。
        this.assignFreeSlot(id);
      }
      this.syncVisibleWidgets();
      // 新增控件后下一帧重新收敛坐标（元素刚挂载时尺寸还是 0）
      this.$nextTick(() => { this.applyLayout(); this.bindOptionalWidgets(); });
    },
    /**
     * 给控件找一个不与现有控件重叠的坐标并写入布局。
     * 网格布局：从右上角开始，按 COL_W × ROW_H 排布，扫描已占用点。
     */
    assignFreeSlot(id) {
      const layout = loadHomeLayout();
      const page = this.$refs.homePage;
      if (!page) return;
      const pw = page.clientWidth || 1000;
      const ph = page.clientHeight || 700;
      const CARD_W = 110, CARD_H = 64, GAP = 14;
      const cols = Math.max(1, Math.floor((pw - 24) / (CARD_W + GAP)));
      const rows = Math.max(1, Math.floor((ph - 24) / (CARD_H + GAP)));
      // 已占用的格子（核心控件 + 已添加的可选控件，凡是有坐标的都算）
      const taken = new Set();
      const mark = (pos) => {
        if (!pos || !Number.isFinite(pos.x) || !Number.isFinite(pos.y)) return;
        const c = Math.round(pos.x / (CARD_W + GAP));
        const r = Math.round(pos.y / (CARD_H + GAP));
        if (c >= 0 && r >= 0 && c < cols && r < rows) taken.add(c + ',' + r);
      };
      for (const key of ['title', 'avatar', 'version', 'launch', 'running']) mark(layout[key]);
      for (const w of this.optionalWidgets) {
        if (w.id !== id) mark(layout[w.id]);
      }
      // 从左下开始找空位（右上通常被版本卡占着）
      for (let r = rows - 1; r >= 0; r--) {
        for (let c = 0; c < cols; c++) {
          if (taken.has(c + ',' + r)) continue;
          layout[id] = { x: c * (CARD_W + GAP) + 12, y: r * (CARD_H + GAP) + 12 };
          saveHomeLayout(layout);
          return;
        }
      }
      // 全满：退化为右下角堆叠
      layout[id] = { x: pw - CARD_W - 12, y: ph - CARD_H - 12 };
      saveHomeLayout(layout);
    },
    /** 从 home-widgets 同步「已添加」集合到视图 */
    syncVisibleWidgets() {
      const layout = loadHomeLayout();
      const next = {};
      for (const w of OPTIONAL_WIDGETS) {
        if (isWidgetVisible(layout, w.id)) next[w.id] = true;
      }
      this.visibleWidgets = next;
    },
    // ── 布局编辑 ──
    /** 进入编辑：只记录草稿位置，不写 localStorage */
    beginLayoutEdit() {
      if (this.editingLayout) return;
      this.editingLayout = true;
      this.widgetPickerOpen = false;
      this._draftChanged = {};
    },
    /** 取消编辑：丢弃草稿，恢复已保存布局 */
    cancelLayoutEdit() {
      if (!this.editingLayout) return;
      this.editingLayout = false;
      this.widgetPickerOpen = false;
      this._draftChanged = {};
      this.applyLayout();
    },
    /** 确认编辑：把本次拖动/缩放写入 localStorage，再恢复普通模式 */
    confirmLayoutEdit() {
      if (!this.editingLayout) return;
      const layout = loadHomeLayout();
      for (const [key, pos] of Object.entries(this._draftChanged)) {
        layout[key] = layout[key] || {};
        if (Number.isFinite(pos.x) && Number.isFinite(pos.y)) {
          layout[key].x = pos.x;
          layout[key].y = pos.y;
        }
        if (key === 'avatar') layout.avatar.s = pos.s || this._avatarScale || 1;
      }
      saveHomeLayout(layout);
      this.editingLayout = false;
      this.widgetPickerOpen = false;
      this._draftChanged = {};
      this.applyLayout();
    },
    markDraftChanged(key, el) {
      this._draftChanged[key] = {
        x: parseInt(el.style.left, 10) || 0,
        y: parseInt(el.style.top, 10) || 0
      };
      if (key === 'avatar') this._draftChanged.avatar.s = this._avatarScale || 1;
    },
    // 有保存位置 → 应用（clamp 到窗口内）；没有 → 清除内联样式回退 CSS 默认（精确居中/停靠）。
    applyLayout(force) {
      const page = this.$refs.homePage;
      if (!page) return;
      // 页面隐藏时 clientWidth/Height 为 0，按 0 计算会把所有控件压到左上角；
      // 页面重新可见后尺寸变化会由 ResizeObserver 重新应用一次
      if (!page.clientWidth || !page.clientHeight) return;
      const layout = force ? {} : loadHomeLayout();
      const pw = page.clientWidth, ph = page.clientHeight;
      // 默认位置的横向基准取启动器窗口中心，而不是会随经典侧栏收放变宽的 .home-page 中心。
      // 这样收起/固定导航栏时，未拖动过的默认控件不会整体横向漂移。
      const pageRect = page.getBoundingClientRect();
      const viewportWidth = document.documentElement.clientWidth || window.innerWidth || 0;
      const defaultCenterLeft = viewportWidth > 0
        ? (viewportWidth / 2) - pageRect.left
        : pw / 2;
      const layoutKeys = ['title', 'avatar', 'version', 'launch', 'running'];
      // 已添加的可选控件全部参与布局收敛（download 走专属卡片，其余走导航卡）
      for (const w of this.optionalWidgets) {
        if (this.visibleWidgets[w.id]) layoutKeys.push(w.id);
      }
      for (const key of layoutKeys) {
        const el = this.widgetByKey(key);
        if (!el) continue;
        const pos = (!force && layout[key]) ? layout[key] : null;
        if (pos) {
          const w = el.offsetWidth || 10, h = el.offsetHeight || 10;
          el.style.left = clampPos(pos.x, 0, Math.max(0, pw - w)) + 'px';
          el.style.top = clampPos(pos.y, 0, Math.max(0, ph - h)) + 'px';
          el.style.right = 'auto';
          el.style.bottom = 'auto';
          // 已保存位置由 left/top 决定，必须清掉 CSS 默认的 translateX(-50%)，
          // 否则控件会再偏移半个自身宽度；同时也能阻止按钮按下时被通用 scale 规则推走。
          el.style.transform = 'none';
        } else {
          // 回退 CSS 默认：标题/头像/版本卡/启动按钮水平居中，运行卡片左下
          el.style.left = '';
          el.style.top = '';
          el.style.right = '';
          el.style.bottom = '';
          el.style.transform = '';
          // 居中控件以窗口中心为 left（CSS 的 translateX(-50%) 继续负责自身半宽修正）；
          // 运行卡片、下载卡与导航卡保持 CSS 停靠，不参与窗口居中。
          const centered = key !== 'running' && key !== 'download'
            && !(WIDGETS[key] && WIDGETS[key].core === false);
          if (centered) el.style.left = defaultCenterLeft + 'px';
        }
      }
      // 头像缩放（恢复默认时重置为 1）
      const s = (!force && layout.avatar && layout.avatar.s) || 1;
      this._avatarScale = s;
      if (this.$refs.avatarScale) {
        this.$refs.avatarScale.style.transform = 'scale(' + s + ')';
      }
      // 首次成功应用后再显示控件，避免从 CSS 默认位置跳到持久化位置
      this.layoutReady = true;
    },
    /**
     * 编辑模式下按住控件主体拖动：移动超过阈值 = 拖动并记录草稿；
     * 非编辑模式不产生拖动，原地松手仍派发 click，保持控件原有交互。
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
          if (!self.editingLayout) return;
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
          if (moved && self.editingLayout) {
            self.markDraftChanged(key, el);
            // 吞掉拖动结束后派发的 click，防止误触内部交互（启动/进账户/切换版本）
            const swallow = function (ev) {
              ev.stopPropagation();
              ev.preventDefault();
              el.removeEventListener('click', swallow, true);
            };
            el.addEventListener('click', swallow, true);
            setTimeout(function () { el.removeEventListener('click', swallow, true); }, 350);
          } else if (!moved && !self.editingLayout && typeof onTap === 'function') {
            // 原地松手（未拖动）= 点击：控件自身没有 click 绑定（如头像）时由这里触发；
            // 编辑模式下点击被拦截，只允许拖动
            onTap();
          }
        };
        document.addEventListener('pointermove', onMove);
        document.addEventListener('pointerup', onUp);
        document.addEventListener('pointercancel', onUp);
      });
    },
    /** 绑定可选控件的拖动（元素存在与否都要安全） */
    bindOptionalWidgets() {
      const self = this;
      // 下载卡
      const dl = this.widgetByKey('download');
      if (dl && this._boundDownloadEl !== dl) {
        this._boundDownloadEl = dl;
        this.bindDrag(dl, 'download', null, 'button, input, a, .home-download-actions');
      }
      // 导航小卡片：v-show 控制显隐，元素在首次渲染后就存在
      for (const w of this.navWidgets) {
        const refs = this.$refs['navEl_' + w.id];
        const el = refs && refs.length ? refs[0] : refs;
        if (!el || this._boundNav[w.id] === el) continue;
        this._boundNav[w.id] = el;
        this.bindDrag(el, w.id, function () { self.onNavWidgetClick(w); });
      }
    },
    /** 编辑模式下拦截控件点击，只有工具栏按钮可用 */
    setupEditClickGuard() {
      const self = this;
      const page = this.$refs.homePage;
      if (!page) return;
      page.addEventListener('click', function (e) {
        if (!self.editingLayout) return;
        if (!e.target || !e.target.closest) return;
        // 工具栏与控件库面板是编辑期唯一可交互的区域
        if (e.target.closest('.home-edit-toolbar') || e.target.closest('.home-widget-picker')) return;
        e.preventDefault();
        e.stopImmediatePropagation();
      }, true);
    },
    setupWidgets() {
      const self = this;
      // 主页控件内的 <img>（头像、版本图标）默认是浏览器原生可拖拽元素：
      // 按住拖动时浏览器接管拖拽，光标显示「禁止放置」并取消 pointermove/pointerup，
      // 自定义拖动完全失效。头像图片由账户逻辑动态替换，故在页面根节点上统一拦截 dragstart。
      if (this.$refs.homePage) {
        this.$refs.homePage.addEventListener('dragstart', function (e) {
          e.preventDefault();
        });
      }
      // 软件名：编辑模式下拖动 + 点击回主页
      this.bindDrag(this.$refs.titleEl, 'title', function () {
        if (typeof navigateToPage === 'function') navigateToPage('home');
      });
      // 头像：编辑模式下拖动 + 滚轮缩放，默认点击进账户
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
          if (!self.editingLayout) return;
          e.preventDefault();
          self._avatarScale = clampPos(self._avatarScale + (e.deltaY < 0 ? 0.1 : -0.1), AVATAR_MIN, AVATAR_MAX);
          scaleWrap.style.transform = 'scale(' + self._avatarScale + ')';
          self._draftChanged.avatar = self._draftChanged.avatar || {};
          self._draftChanged.avatar.s = self._avatarScale;
        }, { passive: false });
      }
      // 版本卡：编辑模式下拖动（内部点击切版本不受影响）
      this.bindDrag(this.$refs.versionEl, 'version', null);
      // 启动按钮：编辑模式下拖动；默认点击启动由 init-setup-nav 的 click 监听负责
      this.bindDrag(this.$refs.launchEl, 'launch', null);
      // 运行卡片：编辑模式下拖动（排除内部「停止」按钮，避免点击停止被拖动吞掉）
      this.bindDrag(this.$refs.runningEl, 'running', null, '.home-running-stop');
      // 下载控件：只有添加后才存在；排除内部按钮，避免拖动吞掉点击
      this.bindOptionalWidgets();
      this.setupEditClickGuard();
      // 控件库添加/移除后同步视图（可能在别处被触发，比如浏览器请求自动添加）
      this._offLayoutChange = onLayoutChange(function () {
        self.syncVisibleWidgets();
        self.$nextTick(function () { self.applyLayout(); self.bindOptionalWidgets(); });
      });
      // 窗口尺寸变化时把控件收回可视区域；编辑中保留草稿位置，不重读已保存布局
      window.addEventListener('resize', function () { if (!self.editingLayout) self.applyLayout(); });
      // 页面自身尺寸变化同样要重新收敛：导航模式切换的 padding 过渡、侧栏展开/收起、
      // 全屏切换都不会触发 window.resize，只监听窗口事件会漏掉这些情况
      if (typeof ResizeObserver !== 'undefined' && this.$refs.homePage) {
        this._layoutObserver = new ResizeObserver(function () { if (!self.editingLayout) self.applyLayout(); });
        this._layoutObserver.observe(this.$refs.homePage);
      } else {
        // 无 ResizeObserver 时兜底显示，避免布局未应用导致控件一直隐藏
        setTimeout(function () {
          self.applyLayout();
          if (!self.layoutReady) self.layoutReady = true;
        }, 200);
      }
      // 页面从隐藏变为可见后重新应用布局：切页返回时页面尺寸可能刚恢复，
      // 必须等可见尺寸稳定后再按持久化位置收敛控件，避免控件停在左上角。
      window.addEventListener('versepc-page-shown', function (e) {
        if (!e || !e.detail || e.detail.pageId !== 'page-home' || self.editingLayout) return;
        requestAnimationFrame(function () {
          self.applyLayout();
          self.syncRunningCard();
        });
      });
      // 个性化页「恢复主页默认布局」：只重置坐标，保留已添加的控件集合
      window.addEventListener('versepc-home-layout-reset', function () {
        self.editingLayout = false;
        self.widgetPickerOpen = false;
        self._draftChanged = {};
        resetWidgetPositions();
        self.syncVisibleWidgets();
        self.$nextTick(function () { self.applyLayout(true); self.bindOptionalWidgets(); });
      });
    },
    /** 运行卡片由 v-if 延迟挂载：出现后应用保存位置并绑定拖动 */
    syncRunningCard() {
      const rEl = this.$refs.runningEl;
      if (!rEl || this._boundRunningEl === rEl) return;
      this._boundRunningEl = rEl;
      const page = this.$refs.homePage;
      const layout = loadHomeLayout();
      const pos = layout.running;
      if (pos && page) {
        rEl.style.left = clampPos(pos.x, 0, Math.max(0, page.clientWidth - (rEl.offsetWidth || 10))) + 'px';
        rEl.style.top = clampPos(pos.y, 0, Math.max(0, page.clientHeight - (rEl.offsetHeight || 10))) + 'px';
        rEl.style.right = 'auto';
        rEl.style.bottom = 'auto';
      }
      this.bindDrag(rEl, 'running', null, '.home-running-stop');
    },
    // 计算运行时长（每秒由 store.now 触发重新渲染）
    formatElapsed(inst) {
      const elapsed = Math.floor(((gameStore.now || Date.now()) - inst.startTime) / 1000);
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
  beforeUnmount() {
    if (typeof this._offLayoutChange === 'function') { this._offLayoutChange(); this._offLayoutChange = null; }
    if (typeof this._offRequest === 'function') { this._offRequest(); this._offRequest = null; }
    if (this._layoutObserver) {
      this._layoutObserver.disconnect();
      this._layoutObserver = null;
    }
    if (this._runningTimer) {
      clearInterval(this._runningTimer);
      this._runningTimer = null;
    }
  },
  mounted() {
    const self = this;
    const init = function () {
      if (typeof loadAccounts !== 'function' || typeof loadVersions !== 'function') {
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
      // 先对齐后端保存的布局（localStorage 可能因强杀丢数据），
      // 再同步「已添加控件」集合，最后收敛坐标。
      hydrateHomeLayoutFromBackend().then(function () {
        self.syncVisibleWidgets();
        self.$nextTick(function () {
          self.applyLayout();
          self.setupWidgets();
        });
      }).catch(function () {
        self.syncVisibleWidgets();
        self.$nextTick(function () {
          self.applyLayout();
          self.setupWidgets();
        });
      });
      // 运行卡片由 v-if 延迟挂载，轮询检测其出现并应用布局
      self.syncRunningCard();
      self._runningTimer = setInterval(function () { self.syncRunningCard(); }, 1200);
    };
    requestAnimationFrame(function () { setTimeout(init, 50); });
  }
};

// 导出供主入口使用
export default PageHome;
