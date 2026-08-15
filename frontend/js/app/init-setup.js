/**
 * @file init-setup.js
 * @description 应用初始化 - 页面加载完成后的启动流程
 */
async function init() {
  const splashProgress = document.getElementById('splash-progress');
  const splashOverlay = document.getElementById('splash-overlay');
  const startTime = Date.now();
  const MIN_SPLASH_DURATION = 100;

  // 启动阶段计时（诊断用）：记录各被 await 的初始化阶段耗时，写入日志
  const _tPhase = {};
  const _tOrder = [];
  function _mark(name) {
    if (!(name in _tPhase)) {
      _tPhase[name] = Date.now(); _tOrder.push(name);
      // 每到一个阶段立即写入一次，即使后续卡住也能看到最后到达的阶段
      try {
        if (window.electronAPI && window.electronAPI.writeStartupTiming) {
          window.electronAPI.writeStartupTiming('mark ' + name + ' @' + (Date.now() - startTime) + 'ms');
        }
      } catch (e) {}
    }
  }
  _mark('t0');

  // 最小化/恢复时做内存整理（对齐 electron：最小化会压缩/释放内存）
  setupWindowMemoryEvents();

  try {
    const earlyTheme = await window.electronAPI.store.get('versepc_theme');
    if (earlyTheme) {
      const legacyThemes = ['blue', 'purple', 'green', 'orange', 'red', 'pink', 'teal', 'cyan', 'amber'];
      const themeName = legacyThemes.includes(earlyTheme) ? 'light' : earlyTheme;
      document.documentElement.setAttribute('data-theme', themeName);
      document.documentElement.classList.toggle('dark-theme', themeName === 'dark' || themeName === 'custom');
      document.documentElement.classList.toggle('light-theme', themeName === 'light');
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

    safeSetup('console', setupConsole);
    setProgress(40, '正在准备主页...');
    _mark('setup');

  } catch (e) {
    console.error('Init critical error:', e);
  }

  // 启动动画（splash）会一直显示，覆盖整个 Vue 挂载 + 数据加载过程
  // 这样用户看到的是：流畅的启动动画 → 直接可用的界面，中间没有卡顿
  // 必须等所有 Vue 页面挂载完成后再加载数据，否则 loadAccounts 写入的 innerHTML
  // 会被后续 Vue 挂载的模板覆盖，导致账号列表丢失
  function _waitForVueMount() {
    if (window._vueMountDone) return Promise.resolve();
    return new Promise(function(resolve) { window._vueMountResolve = resolve; });
  }

  // 阶段1：等 Vue 挂载完成（在 splash 覆盖下，用户看不到卡顿）
  setProgress(55, '正在加载界面...');
  await _waitForVueMount();
  _mark('vue');
  // Vue 挂载会替换页面 DOM，必须在挂载完成后初始化自定义下拉框，否则实例引用的 DOM 已被替换，点击无响应
  initAllCustomSelects();
  // Vue 挂载完成后再绑定页面内事件（tab 切换、模组搜索等），避免模板覆盖导致事件丢失
  safeSetup('tabs', setupTabs);
  safeSetup('modBrowse', setupModBrowse);
  safeSetup('accountButtons', setupAccountButtons);
  safeSetup('versionListClicks', setupVersionListClicks);
  safeSetup('favSearch', setupFavSearchListeners);
  safeSetup('settingsPage', setupSettingsPage);
  // Vue 挂载完成后再绑定 Java 页面事件，避免元素不存在
  try { setupJavaPage(); } catch (e) { console.error('Setup failed: javaPage', e); }

  // 阶段2：加载核心数据（仍在 splash 覆盖下）
  setProgress(70, '正在加载数据...');
  const _subTiming = {};
  try {
    const _t = function (name, fn) {
      const s = Date.now();
      return Promise.resolve(fn()).catch(function () {}).finally(function () {
        _subTiming[name] = Date.now() - s;
        try {
          if (window.electronAPI && window.electronAPI.writeStartupTiming) {
            window.electronAPI.writeStartupTiming('sub ' + name + ' done=' + (Date.now() - s) + 'ms');
          }
        } catch (e) {}
      });
    };
    await Promise.all([
      _t('loadSettings', loadSettings),
      _t('loadVersions', loadVersions),
      _t('loadAccounts', loadAccounts),
      _t('loadFavoritesData', loadFavoritesData)
    ]);
  } catch (e) { console.error('后台数据加载失败:', e); }
  // 记录阶段2内部各函数耗时
  window._initSubTiming = _subTiming;
  _mark('data');

  // 阶段3：加载模组与资源数据（后台异步加载，不阻塞 splash）
  // 原因：热门模组/模组分类需要联网请求境外 API（Modrinth/CurseForge），国内网络访问慢，
  // 若 await 会拖住启动画面停在"加载模组"。改为后台加载，用户先进界面、数据稍后到位。
  setProgress(82, '正在加载模组...');
  Promise.allSettled([
    loadModFilterOptions(),
    loadInstalledMods(),
    loadFeaturedMods()
  ]).catch(() => {});

  // 阶段4：初始化壁纸相关界面元素（轻量、同步，不阻塞 splash）
  setProgress(90, '正在初始化壁纸...');
  cacheCommonElements();
  initWallpaperDropZone();
  initWallpaperAutoAdapt();
  // 壁纸引擎（three.bundle.js 约 2MB）改为窗口出现后再后台加载，
  // 避免大脚本下载+解析阻塞主线程、拖慢窗口出现。窗口先出来，壁纸稍后淡入。
  _mark('wallpaper.dispatch');

  // 阶段5：Java 检测 + 游戏状态（仍在 splash 覆盖下）
  setProgress(96, '正在检查运行环境...');
  try { await checkJavaOnStartup(); } catch (e) { console.error('Java check failed:', e); }
  _mark('javaCheck');
  try { await updateGameStatus(); } catch (e) { console.error('Game status failed:', e); }
  _mark('java');

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

  // 写入启动阶段计时（诊断用）：各阶段耗时，用于定位启动慢的瓶颈
  try {
    _mark('done');
    const now = Date.now();
    const parts = (_tOrder.map(function (name, i) {
      const next = (_tOrder[i + 1] ? _tPhase[_tOrder[i + 1]] : now);
      return name + '=' + (next - _tPhase[name]) + 'ms';
    })).join(' ');
    const total = now - startTime;
    const sub = (window._initSubTiming || {});
    const subParts = (Object.keys(sub).map(function (k) { return k + '=' + sub[k] + 'ms'; })).join(' ');
    if (window.electronAPI && window.electronAPI.writeStartupTiming) {
      window.electronAPI.writeStartupTiming('init total=' + total + 'ms | ' + parts + ' | sub: ' + (subParts || 'none'));
    }
  } catch (e) {}

  // splash 淡出后，仅启动 JVM 预热（纯后台优化，不阻塞用户交互，不显示在界面上）
  setTimeout(() => {
    try { triggerJvmPreheat(); } catch (e) {}
  }, 2000);
}

/**
 * 监听窗口最小化/恢复事件，做内存整理（对齐 electron 最小化后压缩/释放内存的行为）
 * 最小化：暂停壁纸渲染循环 + 暂停视频 + 主动触发 JS 垃圾回收
 * 恢复：恢复壁纸渲染
 */
function setupWindowMemoryEvents() {
  try {
    if (!window.__TAURI__ || !window.__TAURI__.event) return;
    const ta = window.__TAURI__.event;

    // 壁纸引擎挂起/恢复（若已加载）
    function suspendWallpaper() {
      try {
        const w = window.wallpaperEngine;
        if (w && typeof w.suspend === 'function') w.suspend();
      } catch (e) {}
    }
    function resumeWallpaper() {
      try {
        const w = window.wallpaperEngine;
        if (w && typeof w.resume === 'function') w.resume();
      } catch (e) {}
    }

    ta.listen('window-minimized', function () {
      suspendWallpaper();
      // 最小化时：额外释放前端资源
      try {
        // 暂停启动栏封面图的视频/动画（如果有）
        const launchLogoImg = document.querySelector('.launch-cover-image, .launch-cover-video');
        if (launchLogoImg && launchLogoImg.tagName === 'VIDEO') try { launchLogoImg.pause(); } catch (e) {}
        // 清理不可见的图片缓存（隐藏页面中的大图）
        clearHiddenImageCache();
        // 尝试 V8 GC（如果通过启动参数暴露了 gc 函数）
        try { if (window.gc) { window.gc(); window.gc(); } } catch (e) {}
      } catch (e) {}
    });
    ta.listen('window-restored', function () {
      resumeWallpaper();
      // 恢复封面图视频
      const launchLogoImg = document.querySelector('.launch-cover-video');
      if (launchLogoImg && launchLogoImg.tagName === 'VIDEO' && !launchLogoImg.paused) {
        // 不强制自动播放，避免打扰用户
      }
      // 恢复后也尝试一次 GC 和清理
      try { if (window.gc) window.gc(); } catch (e) {}
    });

    // 后端定时触发的内存优化请求（最小化后每 30 秒一次）
    ta.listen('memory-optimize-request', function () {
      try {
        // 1. 清理不可见页面的图片引用（临时把 src 设为 ''，DOM 不删，返回时浏览器会重新加载）
        clearHiddenImageCache();
        // 2. 如果壁纸已经挂起（最小化状态），把 canvas 清空减少显存占用
        const w = window.wallpaperEngine;
        if (w && w._suspended) {
          try {
            const canvas = document.getElementById('wallpaper-canvas');
            const glCanvas = document.getElementById('wallpaper-canvas-gl');
            if (canvas && canvas.width > 0) {
              const ctx = canvas.getContext('2d');
              if (ctx) ctx.clearRect(0, 0, canvas.width, canvas.height);
            }
          } catch (e) {}
        }
        // 3. V8 GC（仅当启动参数暴露时有效）
        try { if (window.gc) { window.gc(); window.gc(); } } catch (e) {}
        // 4. 清理控制台日志输出缓冲区，避免大字符串长期驻留
        try {
          if (typeof window.clearGameLogBuffer === 'function') {
            window.clearGameLogBuffer();
          }
        } catch (e) {}
      } catch (e) {}
    });
  } catch (e) {}
}

/**
 * 清理隐藏页面中的图片缓存（释放解码后的图像内存）
 * 不删除 DOM 节点，只清除 src，切回页面时浏览器会自动重新请求（大多数是本地文件，速度极快）
 */
function clearHiddenImageCache() {
  try {
    // 找到所有非激活页面里的 img / video 元素
    const pages = document.querySelectorAll('.page:not(.active)');
    pages.forEach(function (page) {
      // 图片：只处理非小图标的大图片（避免清掉 SVG 图标）
      const imgs = page.querySelectorAll('img[src]:not([src*="svg"]):not(.nav-icon-svg):not(.account-avatar-img):not(.mod-icon-img)');
      imgs.forEach(function (img) {
        if (img.naturalWidth > 100 || img.naturalHeight > 100) {
          // 保存原地址到 data 属性，切回页面时恢复（通过 MutationObserver 或页面切换钩子）
          if (!img.dataset._cachedSrc) {
            img.dataset._cachedSrc = img.src;
            // 只清理大内存占用的图片（宽高超过 500px 的解码缓存占用才显著）
            if (img.naturalWidth > 500 || img.naturalHeight > 500) {
              img.removeAttribute('src');
            }
          }
        }
      });
      // 视频：暂停并移除 srcObject
      const videos = page.querySelectorAll('video');
      videos.forEach(function (v) {
        try {
          if (v.srcObject) {
            v._cachedSrcObj = v.srcObject;
            v.srcObject = null;
          }
          v.pause();
        } catch (e) {}
      });
    });
  } catch (e) {}
}

function loadWallpaperSettings() {
  Promise.all([
    window.electronAPI.store.get('versepc_custom_image'),
    window.electronAPI.store.get('versepc_custom_video'),
    window.electronAPI.store.get('versepc_wallpaper'),
    window.electronAPI.store.get('versepc_wallpaper_opacity'),
    window.electronAPI.store.get('versepc_wallpaper_blur'),
    window.electronAPI.store.get('versepc_wallpaper_fit'),
    window.electronAPI.store.get('versepc_panorama_theme'),
    window.electronAPI?.store?.get('versepc_panorama_speed'),
    window.electronAPI?.store?.get('versepc_panorama_mouse_follow'),
  ]).then(([
    savedCustomImage,
    savedCustomVideo,
    savedWallpaper,
    savedOpacity,
    savedBlur,
    savedFit,
    savedPanoramaTheme,
    savedPanoramaSpeed,
    savedMouseFollow,
  ]) => {
    if (savedCustomImage && typeof setCustomWallpaperImage === 'function') {
      setCustomWallpaperImage(savedCustomImage);
    }
    if (savedCustomVideo && typeof setCustomWallpaperVideo === 'function') {
      setCustomWallpaperVideo(savedCustomVideo);
    }
    if (savedWallpaper) {
      let wpName = savedWallpaper;
      if (wpName === 'starry') wpName = 'panorama';
      const wpEl = document.querySelector(`.wallpaper-option[data-wallpaper="${wpName}"]`);
      if (wpEl) selectWallpaper(wpEl);
    }
    if (savedOpacity != null) {
      const slider = document.getElementById('wallpaper-opacity-slider');
      if (slider) { slider.value = savedOpacity; onWallpaperOpacityChange(savedOpacity); }
    }
    if (savedBlur != null) {
      const slider = document.getElementById('wallpaper-blur-slider');
      if (slider) { slider.value = savedBlur; onWallpaperBlurChange(savedBlur); }
    }
    if (savedFit) {
      const select = document.getElementById('wallpaper-fit-select');
      if (select) { select.value = savedFit; onWallpaperFitChange(savedFit); }
    }
    if (savedPanoramaTheme) {
      const themeEl = document.querySelector(`.panorama-theme-option[data-theme="${savedPanoramaTheme}"]`);
      if (themeEl) selectPanoramaTheme(themeEl);
    }
    if (savedPanoramaSpeed) {
      const slider = document.getElementById('panoramaSpeedSlider');
      if (slider) slider.value = savedPanoramaSpeed;
      const label = document.getElementById('panoramaSpeedLabel');
      if (label) label.textContent = savedPanoramaSpeed;
      if (typeof setPanoramaRotationSpeed === 'function') setPanoramaRotationSpeed(savedPanoramaSpeed * 0.001);
    }
    if (savedMouseFollow === true) {
      const toggle = document.getElementById('panoramaMouseFollowToggle');
      if (toggle) toggle.checked = true;
      if (typeof setPanoramaMouseFollow === 'function') setPanoramaMouseFollow(true);
    }
    if (savedCustomImage) {
      const nameEl = document.getElementById('custom-wallpaper-file-name');
      if (nameEl) nameEl.textContent = savedCustomImage.split(/[\\/]/).pop();
      _updateCustomImagePreview(savedCustomImage);
    }
    if (savedCustomVideo) {
      const nameEl = document.getElementById('custom-wallpaper-file-name');
      if (nameEl) nameEl.textContent = savedCustomVideo.split(/[\\/]/).pop();
    }
  }).catch(e => console.error('[Init] Load wallpaper settings error:', e));
}

function setupNavigation() {
  document.querySelectorAll('.nav-btn:not(.nav-submenu-toggle)').forEach(btn => {
    btn.addEventListener('click', () => {
      const page = btn.dataset.page;
      if (!page) return;

      if (page === 'versions') {
        loadVersions(false);
      }

      navigateToPage(page);
    });
  });

  document.querySelectorAll('.nav-submenu-group').forEach(group => {
    const toggle = group.querySelector('.nav-submenu-toggle');
    if (!toggle) return;

    toggle.addEventListener('click', (e) => {
      e.stopPropagation();
      document.querySelectorAll('.nav-submenu-group').forEach(g => g.classList.remove('open'));
      group.classList.add('open');

      const firstSubBtn = group.querySelector('.nav-sub-btn[data-page]');
      const firstPage = firstSubBtn?.dataset.page;
      if (firstPage) {
        navigateToPage(firstPage);
      }
    });

    group.querySelectorAll('.nav-sub-btn').forEach(btn => {
      btn.addEventListener('click', () => {
        const page = btn.dataset.page;
        if (!page) return;
        navigateToPage(page);
      });
    });
  });
}

function setupTabs() {
  document.querySelectorAll('.tab-btn').forEach(btn => {
    btn.addEventListener('click', () => {
      const tab = btn.dataset.tab;
      const parent = btn.closest('.tab-group');
      parent.querySelectorAll('.tab-btn').forEach(b => b.classList.remove('active'));
      btn.classList.add('active');

      if (tab === 'release' || tab === 'snapshot' || tab === 'special' || tab === 'old' || tab === 'installed') {
        currentVersionTab = tab;
        renderVersions();
      } else if (tab === 'browse-modpacks') {
        loadResourcePage('modpack');
      }
    });
  });

  document.querySelectorAll('.loader-type-btn').forEach(btn => {
    btn.addEventListener('click', () => {
      document.querySelectorAll('.loader-type-btn').forEach(b => b.classList.remove('active'));
      btn.classList.add('active');
      currentLoaderType = btn.dataset.loader;
      loadModLoaderVersions();
    });
  });
}

function setupLaunchBar() {
  document.getElementById('launch-btn').addEventListener('click', handleLaunch);
  document.getElementById('home-launch-btn').addEventListener('click', handleLaunch);

  if (!launchVersionCustomSelect) {
    launchVersionCustomSelect = new CustomSelect('launch-version-select-wrapper', {
      onChange: (value) => {
        // 同步到 currentLaunchVersionId 状态变量和主页内嵌卡片
        if (value && value !== currentLaunchVersionId) {
          currentLaunchVersionId = value;
          _cachedLastLaunchVersion = value;
          try { window.electronAPI.store.set('versepc_last_launch_version', value); } catch (_) {}
          renderHomeCurrentVersionCard();
        }
      }
    });
  }

  const windowSizeSelect = document.getElementById('window-size');
  const customWindowSizeDiv = document.getElementById('custom-window-size');
  const customWidthInput = document.getElementById('custom-width');
  const customHeightInput = document.getElementById('custom-height');

  if (windowSizeSelect && customWindowSizeDiv) {
    windowSizeSelect.addEventListener('change', () => {
      if (windowSizeSelect.value === 'custom') {
        customWindowSizeDiv.style.display = 'flex';
        if (!customWidthInput.value) customWidthInput.value = '1920';
        if (!customHeightInput.value) customHeightInput.value = '1080';
      } else {
        customWindowSizeDiv.style.display = 'none';
      }
    });
  }

  const refreshBtn = document.getElementById('refresh-versions-btn');
  if (refreshBtn) refreshBtn.addEventListener('click', async () => {
    showToast('正在刷新版本列表...', 'info');
    await loadVersions(true);
    showToast('版本列表已刷新', 'success');
  });
}

function setupModBrowse() {
  const modSearchBtn = document.getElementById('mod-search-btn');
  if (!modSearchBtn) return;
  const modSearchInput = document.getElementById('mod-search-input');
  modSearchBtn.addEventListener('click', () => {
    modSearchQuery = modSearchInput ? modSearchInput.value.trim() : '';
    modSearchOffset = 0;
    loadMods();
  });
  if (modSearchInput) modSearchInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.isComposing) {
      modSearchQuery = e.target.value.trim();
      modSearchOffset = 0;
      loadMods();
    }
  });
  const modPrevBtn = document.getElementById('mod-prev-btn');
  if (modPrevBtn) modPrevBtn.addEventListener('click', () => {
    if (modSearchOffset >= 15) {
      modSearchOffset -= 15;
      loadMods();
    }
  });
  const modNextBtn = document.getElementById('mod-next-btn');
  if (modNextBtn) modNextBtn.addEventListener('click', () => {
    modSearchOffset += 15;
    loadMods();
  });

  const bindFilter = (id) => {
    const el = document.getElementById(id);
    if (el) el.addEventListener('change', () => { modSearchOffset = 0; loadMods(); });
  };
  bindFilter('mod-filter-loader');
  bindFilter('mod-filter-version');
  bindFilter('mod-filter-category');
  bindFilter('mod-filter-sort');
  bindFilter('mod-filter-source');
}

function setupAccountButtons() {
  // 旧顶部按钮（兼容旧模板）
  const addMsBtn = document.getElementById('add-ms-account-btn');
  if (addMsBtn) addMsBtn.addEventListener('click', startMsAuth);
  const addThirdPartyBtn = document.getElementById('add-thirdparty-account-btn');
  if (addThirdPartyBtn) addThirdPartyBtn.addEventListener('click', () => {
    showModal('thirdparty-account-modal');
  });
  const addOfflineBtn = document.getElementById('add-offline-account-btn');
  if (addOfflineBtn) addOfflineBtn.addEventListener('click', () => {
    showModal('offline-account-modal');
  });

  // 新版：左侧虚线添加按钮 → 打开添加账号弹窗
  const addCta = document.getElementById('add-account-cta');
  if (addCta) addCta.addEventListener('click', () => showModal('add-account-modal'));
  const closeAddModal = document.getElementById('close-add-account-modal');
  if (closeAddModal) closeAddModal.addEventListener('click', () => {
    const m = document.getElementById('add-account-modal');
    if (m) m.style.display = 'none';
  });

  // 新版添加账号弹窗里的三卡片
  const addA = document.getElementById('add-acct-ms');
  if (addA) addA.addEventListener('click', () => {
    const m = document.getElementById('add-account-modal');
    if (m) m.style.display = 'none';
    startMsAuth();
  });
  const addB = document.getElementById('add-acct-offline');
  if (addB) addB.addEventListener('click', () => {
    const m = document.getElementById('add-account-modal');
    if (m) m.style.display = 'none';
    showModal('offline-account-modal');
  });
  const addC = document.getElementById('add-acct-thirdparty');
  if (addC) addC.addEventListener('click', () => {
    const m = document.getElementById('add-account-modal');
    if (m) m.style.display = 'none';
    showModal('thirdparty-account-modal');
  });
  const createOfflineBtn = document.getElementById('create-offline-btn');
  if (createOfflineBtn) createOfflineBtn.addEventListener('click', async () => {
    const offlineUsernameInput = document.getElementById('offline-username-input');
    const username = offlineUsernameInput ? offlineUsernameInput.value.trim() : '';
    if (!username) { showToast('请输入玩家 ID', 'error'); return; }
    if (username.length < 3 || username.length > 16) {
      showToast('玩家 ID 长度需为 3 - 16 位', 'error'); return;
    }
    if (!/^[A-Za-z0-9_]+$/.test(username)) {
      if (!confirm(`你输入的玩家 ID「${username}」不符合标准（3 - 16 位，只可以包含英文字母、数字与下划线），可能导致部分版本的游戏无法启动或发生错误。\n\n强烈建议使用规范的玩家 ID！\n如果你坚持，仍然可以继续创建档案。`)) {
        return;
      }
    }
    try {
      const result = await API.addOfflineAccount(username);
      if (result.success) {
        showToast(`离线账户 ${username} 创建成功`, 'success');
        closeOfflineModal();
        await loadAccounts();
      } else {
        showToast(result.error || '创建失败', 'error');
      }
    } catch (e) {
      showToast('创建离线账户失败', 'error');
    }
  });

  const tpPreset = document.getElementById('tp-server-preset');
  const tpUrl = document.getElementById('tp-server-url');
  if (tpPreset) {
    tpPreset.addEventListener('change', () => {
      const val = tpPreset.value;
      if (val && val !== 'custom') {
        tpUrl.value = val;
        verifyThirdPartyServer(val);
      } else {
        tpUrl.value = '';
      }
    });
  }
  if (tpUrl) {
    tpUrl.addEventListener('blur', () => {
      const url = tpUrl.value.trim();
      if (url) verifyThirdPartyServer(url);
    });
  }

  const tpLoginBtn = document.getElementById('tp-login-btn');
  if (tpLoginBtn) tpLoginBtn.addEventListener('click', async () => {
    const tpServerUrl = document.getElementById('tp-server-url');
    const tpUsernameInput = document.getElementById('tp-username-input');
    const tpPasswordInput = document.getElementById('tp-password-input');
    const serverUrl = tpServerUrl ? tpServerUrl.value.trim() : '';
    const username = tpUsernameInput ? tpUsernameInput.value.trim() : '';
    const password = tpPasswordInput ? tpPasswordInput.value : '';
    if (!serverUrl) { showToast('请输入认证服务器地址', 'error'); return; }
    if (!username) { showToast('请输入邮箱或用户名', 'error'); return; }
    if (!password) { showToast('请输入密码', 'error'); return; }

    const btn = document.getElementById('tp-login-btn');
    btn.disabled = true;
    btn.textContent = '登录中...';
    try {
      const result = await API.loginThirdParty(serverUrl, username, password);
      if (result.success) {
        showToast(`欢迎，${result.account.username}！`, 'success');
        closeThirdPartyModal();
        await loadAccounts();
      } else if (result.needSelectProfile) {
        closeThirdPartyModal();
        showProfileSelectModal(result.accessToken, result.clientToken, result.serverUrl, result.availableProfiles);
      } else {
        showToast(result.error || '登录失败', 'error');
      }
    } catch (e) {
      showToast('登录失败: ' + (e.message || e.error || '未知错误'), 'error');
    } finally {
      btn.disabled = false;
      btn.textContent = '登录';
    }
  });
}

function setupSettingsPage() {
  const saveBtn = document.getElementById('save-settings-btn');
  if (!saveBtn) return;
  saveBtn.addEventListener('click', saveCurrentSettings);
  document.getElementById('reset-settings-btn').addEventListener('click', async () => {
    const confirmed = await showConfirmDialog('重置设置', '确定要重置所有设置为默认值吗？此操作不可恢复！', '重置', '取消');
    if (confirmed) {
      try {
        const result = await API.resetSettings();
        if (result.success) {
          document.documentElement.setAttribute('data-theme', 'light');
          document.querySelectorAll('.theme-option').forEach(btn => {
            btn.classList.toggle('active', btn.getAttribute('data-theme') === 'light');
          });
          applyAccentColor('#1a1a1a');
          await loadSettings();
          showToast('设置已重置为默认值', 'success');
        } else {
          showToast('重置失败: ' + (result.error || '未知错误'), 'error');
        }
      } catch (e) {
        showToast('重置失败: ' + e.message, 'error');
      }
    }
  });

  const accentColorInput = getDOMElement('custom-accent-color');
  if (accentColorInput) {
    const accentColorValueEl = getDOMElement('custom-color-value');
    const colorPreviewDot = document.getElementById('color-preview-dot');
    accentColorInput.addEventListener('input', throttle((e) => {
      const color = e.target.value;
      if (accentColorValueEl) accentColorValueEl.textContent = color;
      if (colorPreviewDot) colorPreviewDot.style.background = color;
    }, 50));
  }
}
