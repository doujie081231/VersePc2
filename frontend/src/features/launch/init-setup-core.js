import { bridge as bridge } from '../../core/tauri-bridge.js';
import { applyNavigationMode as applyNavigationMode, setupWindowControls as setupWindowControls, setupVersionListClicks as setupVersionListClicks } from '../settings/window-theme.js';
import { loadAccounts as loadAccounts } from '../accounts/accounts-list.js';
import { updateGameStatus as updateGameStatus, checkJavaOnStartup as checkJavaOnStartup, triggerJvmPreheat as triggerJvmPreheat } from './launch.js';
import { initAllCustomSelects as initAllCustomSelects } from '../misc/custom-select.js';
import { cacheCommonElements as cacheCommonElements } from '../misc/utils.js';
import { applyCustomThemeColor as applyCustomThemeColor, getSavedCustomThemeLight as getSavedCustomThemeLight, getSavedCustomThemeColor as getSavedCustomThemeColor } from '../settings/custom-theme.js';
import { initWallpaperAutoAdapt as initWallpaperAutoAdapt } from '../settings/personalize.js';
import { loadSettings as loadSettings } from '../settings/settings-load-save.js';
import { initWallpaper as initWallpaper } from '../settings/wallpaper-engine.js';
import { loadVersions as loadVersions } from '../versions/versions-list.js';
import { waitForVueMount as waitForVueMount } from '../../core/vue-ready.js';
import { S } from './init-setup-state.js';
import { backgroundLoadWallpaper, setupWindowMemoryEvents } from './init-setup-window.js';
import { setupNavigation, setupTabs, setupLaunchBar } from './init-setup-nav.js';
import { setupModBrowse, setupAccountButtons, setupSettingsPage } from './init-setup-pages.js';
import { initDownloadBridge } from '../home/download-bridge.js';

async function init() {
  // 禁用 WebView2 浏览器默认右键菜单（刷新/另存为/检查等），输入区域保留复制粘贴
  document.addEventListener('contextmenu', function (e) {
    var t = e.target;
    var editable = t && (
      (t.tagName && /^(INPUT|TEXTAREA)$/i.test(t.tagName)) ||
      t.isContentEditable ||
      (t.closest && t.closest('input, textarea, [contenteditable="true"]'))
    );
    if (!editable) e.preventDefault();
  });

  const splashProgress = document.getElementById('splash-progress');
  const splashOverlay = document.getElementById('splash-overlay');
  const startTime = Date.now();
  const MIN_SPLASH_DURATION = 100;

  // 记录页面总加载耗时（从 WebView 导航开始到 init 执行），细分每个阶段，用于定位加载慢的瓶颈
  try {
    var _t = window.performance.timing;
    if (bridge && bridge.writeStartupTiming && _t) {
      var _d = function (x) { return x ? Math.round(x - _t.navigationStart) + 'ms' : '?'; };
      bridge.writeStartupTiming(
        'page-nav init@' + Math.round(window.performance.now())
        + 'ms | domLoading=' + _d(_t.domLoading)
        + ' domInteractive=' + _d(_t.domInteractive)
        + ' domContentLoaded=' + _d(_t.domContentLoadedEventEnd)
        + ' loadEnd=' + _d(_t.loadEventEnd)
      );
    }
  } catch (e) {}

  // 启动阶段计时（诊断用）：记录各被 await 的初始化阶段耗时，写入日志
  const _tPhase = {};
  const _tOrder = [];
  function _mark(name) {
    if (!(name in _tPhase)) {
      _tPhase[name] = Date.now(); _tOrder.push(name);
      // 每到一个阶段立即写入一次，即使后续卡住也能看到最后到达的阶段
      try {
        if (bridge && bridge.writeStartupTiming) {
          bridge.writeStartupTiming('mark ' + name + ' @' + (Date.now() - startTime) + 'ms');
        }
      } catch (e) {}
    }
  }
  _mark('t0');

  // 最小化/恢复时做内存整理（对齐 electron：最小化会压缩/释放内存）
  setupWindowMemoryEvents();

  try {
    const earlyTheme = await bridge.store.get('versepc_theme');
    if (earlyTheme) {
      const legacyThemes = ['blue', 'purple', 'green', 'orange', 'red', 'pink', 'teal', 'cyan', 'amber'];
      const themeName = legacyThemes.includes(earlyTheme) ? 'light' : earlyTheme;
      const resolvedTheme = themeName === 'auto'
        ? (window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light')
        : themeName;
      document.documentElement.setAttribute('data-theme', resolvedTheme);
      document.documentElement.classList.toggle('dark-theme', resolvedTheme === 'dark' || resolvedTheme === 'custom');
      document.documentElement.classList.toggle('light-theme', resolvedTheme === 'light');
      document.querySelectorAll('.theme-option').forEach(btn => {
        btn.classList.toggle('active', btn.getAttribute('data-theme') === themeName);
      });
      if (themeName === 'custom' && typeof applyCustomThemeColor === 'function' && typeof getSavedCustomThemeColor === 'function') {
        const customColor = await getSavedCustomThemeColor();
        const isLight = typeof getSavedCustomThemeLight === 'function'
          ? await getSavedCustomThemeLight()
          : false;
        await applyCustomThemeColor(customColor, { save: false, isLight });
      }
    } else {
      document.documentElement.setAttribute('data-theme', 'light');
      document.documentElement.classList.add('light-theme');
    }
  } catch (e) {}
  _mark('theme');

  // 导航栏模式尽早应用：默认经典侧栏，开启「简约导航栏」后切回悬浮胶囊。
  // 尽早切换可避免已保存简约模式的用户在启动时看到经典侧栏闪一下。
  try {
    const savedNavMode = await bridge.store.get('versepc_nav_minimal');
    const useMinimalNav = savedNavMode === '1' || savedNavMode === true;
    if (typeof applyNavigationMode === 'function') {
      applyNavigationMode(useMinimalNav);
    }
  } catch (e) {}

  // 尽早恢复自定义导航栏宽度，避免启动时先显示默认宽度再跳变
  try {
    const savedNavWidth = await bridge.store.get('versepc_nav_width');
    const navWidth = parseInt(savedNavWidth, 10);
    if (Number.isFinite(navWidth)) {
      const clamped = Math.max(64, Math.min(160, navWidth));
      document.body.style.setProperty('--classic-sidebar-width', clamped + 'px');
    }
  } catch (e) {}

  function setProgress(val, statusText) {
    if (!splashProgress) return;
    splashProgress.style.width = Math.min(val, 100) + '%';

    const splashStatus = document.getElementById('splash-status');
    if (splashStatus && statusText) {
      splashStatus.textContent = statusText;
    }
  }

  function safeSetup(name, fn) {
    try { fn(); } catch (e) {
      console.error('Setup failed:', name, e);
    }
  }

  try {
    setProgress(5, '正在初始化界面...');
    safeSetup('navigation', setupNavigation);
    safeSetup('launchBar', setupLaunchBar);
    safeSetup('windowControls', setupWindowControls);
    setProgress(15, '正在构建界面...');

    try {
      const cachedName = localStorage.getItem('cachedPlayerName');
      if (cachedName) {
        const homeName = document.getElementById('home-player-name');
        const launchName = document.getElementById('launch-player-name');
        if (homeName) homeName.textContent = cachedName;
        if (launchName) launchName.textContent = cachedName;
      }
      const cachedAvatarData = localStorage.getItem('cachedAvatarData');
      if (cachedAvatarData) {
        const launchAvatar = document.getElementById('launch-avatar');
        // 仅更新启动栏小头像兜底；主页头像由 loadAccounts/_loadAvatarImg 异步加载真实皮肤头像
        if (launchAvatar) {
          launchAvatar.innerHTML = '<img src="' + cachedAvatarData + '" class="account-avatar-img">';
        }
      }
      const cachedAccountType = localStorage.getItem('cachedAccountType');
      if (cachedAccountType) {
        const homeType = document.getElementById('home-account-type');
        if (homeType) homeType.textContent = cachedAccountType;
      }
    } catch(e) {}

    setProgress(40, '正在准备主页...');
    _mark('setup');

  } catch (e) {
    console.error('Init critical error:', e);
  }

  // 启动动画（splash）会一直显示，覆盖整个 Vue 挂载 + 数据加载过程
  // 这样用户看到的是：流畅的启动动画 → 直接可用的界面，中间没有卡顿
  // 必须等所有 Vue 页面挂载完成后再加载数据，否则 loadAccounts 写入的 innerHTML
  // 会被后续 Vue 挂载的模板覆盖，导致账号列表丢失
  // 阶段1：等 Vue 挂载完成（在 splash 覆盖下，用户看不到卡顿）
  setProgress(55, '正在加载界面...');
  await waitForVueMount();
  _mark('vue');
  // Vue 挂载会替换页面 DOM，必须在挂载完成后初始化自定义下拉框，否则实例引用的 DOM 已被替换，点击无响应
  initAllCustomSelects();
  // Vue 挂载完成后再绑定页面内事件（tab 切换、模组搜索等），避免模板覆盖导致事件丢失
  safeSetup('tabs', setupTabs);
  safeSetup('modBrowse', setupModBrowse);
  safeSetup('accountButtons', setupAccountButtons);
  safeSetup('versionListClicks', setupVersionListClicks);
  safeSetup('favSearch', () => {
    import('../mods/favorites.js')
      .then((m) => m.setupFavSearchListeners())
      .catch((e) => console.error('Setup failed: favSearch', e));
  });
  safeSetup('settingsPage', setupSettingsPage);
  safeSetup('feedback', () => {
    import('../misc/feedback.js')
      .then((m) => m.setupFeedback())
      .catch((e) => console.error('Setup failed: feedback', e));
  });
  // Vue 挂载完成后再绑定 Java 页面事件，避免元素不存在
  import('./java-mgr.js')
    .then((m) => m.setupJavaPage())
    .catch((e) => console.error('Setup failed: javaPage', e));

  // 阶段2：加载核心数据（仍在 splash 覆盖下）
  setProgress(70, '正在加载数据...');
  const _subTiming = {};
  try {
    const _t = function (name, fn) {
      const s = Date.now();
      return Promise.resolve(fn()).catch(function () {}).finally(function () {
        _subTiming[name] = Date.now() - s;
        try {
          if (bridge && bridge.writeStartupTiming) {
            bridge.writeStartupTiming('sub ' + name + ' done=' + (Date.now() - s) + 'ms');
          }
        } catch (e) {}
      });
    };
    await Promise.all([
      _t('loadSettings', loadSettings),
      _t('loadVersions', loadVersions),
      _t('loadAccounts', loadAccounts),
      _t('loadFavoritesData', () => import('../mods/favorites.js').then((m) => m.loadFavoritesData()))
    ]);
  } catch (e) { console.error('后台数据加载失败:', e); }
  // 记录阶段2内部各函数耗时
  S.initSubTiming = _subTiming;
  _mark('data');

  // 阶段3：加载模组与资源数据（后台异步加载，不阻塞 splash）
  // 原因：热门模组/模组分类需要联网请求境外 API（Modrinth/CurseForge），国内网络访问慢，
  // 若 await 会拖住启动画面停在"加载模组"。改为后台加载，用户先进界面、数据稍后到位。
  setProgress(82, '正在加载模组...');
  import('../mods/mod-browse.js')
    .then((m) => Promise.allSettled([
      m.loadModFilterOptions(),
      m.loadInstalledMods(),
      m.loadFeaturedMods()
    ]))
    .catch(() => {});

  // 阶段4：初始化壁纸相关界面元素（轻量、同步，不阻塞 splash）
  setProgress(90, '正在初始化壁纸...');
  cacheCommonElements();
  initWallpaperAutoAdapt();
  // 壁纸引擎（three.bundle.js 约 2MB）改为窗口出现后再后台加载，
  // 避免大脚本下载+解析阻塞主线程、拖慢窗口出现。窗口先出来，壁纸稍后淡入。
  _mark('wallpaper.dispatch');

  // 阶段5：Java 检测 + 游戏状态
  // 改为窗口出现后后台执行，不再阻塞 splash：
  // Java 检测会给每个候选 Java 启动子进程验证版本，较耗时；游戏状态也仅是状态显示。
  // 这些都不影响核心功能，窗口先出现，状态稍后刷新。
  setProgress(96, '正在检查运行环境...');
  (async function checkEnvAsync() {
    try { await checkJavaOnStartup(); } catch (e) { console.error('Java check failed:', e); }
    try { await updateGameStatus(); } catch (e) { console.error('Game status failed:', e); }
    _mark('envDone');
  })();
  _mark('javaCheck.dispatch');

  // 阶段6：准备就绪，淡出 splash
  setProgress(100, '准备就绪!');
  const elapsed = Date.now() - startTime;
  const remainingMinTime = Math.max(MIN_SPLASH_DURATION - elapsed, 0);
  await new Promise(r => setTimeout(r, remainingMinTime));

  if (splashOverlay) {
    try {
      await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));
      void document.body.offsetHeight;
      void document.documentElement.offsetHeight;
      await new Promise(r => requestAnimationFrame(r));
    } catch (e) {}

    splashOverlay.style.transition = 'opacity 0.2s cubic-bezier(0.4,0,0.2,1)';
    splashOverlay.style.opacity = '0';
    splashOverlay.style.pointerEvents = 'none';
    await new Promise(r => setTimeout(r, 200));
    try { splashOverlay.remove(); } catch (err) {}
  }

  // 窗口已出现，后台加载壁纸引擎（three.bundle.js 约 2MB），不阻塞任何交互
  if (typeof initWallpaper === 'function') {
    setTimeout(() => { try { backgroundLoadWallpaper(); } catch (e) {} }, 0);
  }

  // 浏览器下载接管：窗口出现后拉一次待处理请求，把它填进下载控件（绝不自动开始）
  setTimeout(() => {
    try { initDownloadBridge(); } catch (e) { console.error('[init] 下载接管初始化失败:', e); }
  }, 0);

  // 写入启动阶段计时（诊断用）：各阶段耗时，用于定位启动慢的瓶颈
  try {
    _mark('done');
    const now = Date.now();
    const parts = (_tOrder.map(function (name, i) {
      const next = (_tOrder[i + 1] ? _tPhase[_tOrder[i + 1]] : now);
      return name + '=' + (next - _tPhase[name]) + 'ms';
    })).join(' ');
    const total = now - startTime;
    const sub = (S.initSubTiming || {});
    const subParts = (Object.keys(sub).map(function (k) { return k + '=' + sub[k] + 'ms'; })).join(' ');
    if (bridge && bridge.writeStartupTiming) {
      bridge.writeStartupTiming('init total=' + total + 'ms | ' + parts + ' | sub: ' + (subParts || 'none'));
    }
  } catch (e) {}

  // splash 淡出后，仅启动 JVM 预热（纯后台优化，不阻塞用户交互，不显示在界面上）
  setTimeout(() => {
    try { triggerJvmPreheat(); } catch (e) {}
  }, 2000);
}

export { init };
