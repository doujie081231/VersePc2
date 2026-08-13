/**
 * @file window-theme.js
 * @description 主题切换与窗口控制 - 亮/暗主题、自定义强调色、窗口最小化/关闭、页面导航
 */
function switchTheme(themeName) {
  document.documentElement.setAttribute('data-theme', themeName);
  document.documentElement.classList.toggle('dark-theme', themeName === 'dark');
  document.documentElement.classList.toggle('light-theme', themeName === 'light');

  document.documentElement.style.removeProperty('--accent');
  document.documentElement.style.removeProperty('--accent-hover');
  document.documentElement.style.removeProperty('--accent-rgb');

  const app = document.getElementById('app');
  if (app && themeName === 'light') {
    app.classList.remove('wp-light', 'wp-dark');
  }

  if (typeof updateWallpaperTheme === 'function') {
    updateWallpaperTheme(themeName === 'dark');
  }

  document.querySelectorAll('.theme-option').forEach(btn => {
    btn.classList.toggle('active', btn.getAttribute('data-theme') === themeName);
  });

  const themeDef = getComputedStyle(document.documentElement);
  const accentColor = themeDef.getPropertyValue('--accent').trim();

  const accentColorInput = document.getElementById('custom-accent-color');
  if (accentColorInput) accentColorInput.value = accentColor;
  const accentColorValueEl = document.getElementById('custom-color-value');
  if (accentColorValueEl) accentColorValueEl.textContent = accentColor;
  const colorPreviewDot = document.getElementById('color-preview-dot');
  if (colorPreviewDot) colorPreviewDot.style.background = accentColor;

  API.saveSetting('theme', themeName);
  API.saveSetting('accentColor', accentColor);
  window.electronAPI?.store?.set('versepc_theme', themeName).catch(() => {});

  showToast(`已切换到「${getThemeLabel(themeName)}」主题`, 'success');
}

function applyCustomAccent() {
  const colorInput = document.getElementById('custom-accent-color');
  const color = colorInput?.value;
  if (!color) return;
  const colorValueEl = document.getElementById('custom-color-value');
  if (colorValueEl) colorValueEl.textContent = color;
  applyAccentColor(color);
  API.saveSetting('accentColor', color);
  showToast('强调色已应用', 'success');
}

function getThemeLabel(themeName) {
  const labels = {
    dark: '黑色',
    light: '白色'
  };
  return labels[themeName] || themeName;
}

function browseFolder(type) {
  if (window.electronAPI && window.electronAPI.showOpenDialog) {
    window.electronAPI.showOpenDialog({ properties: ['openDirectory'] }).then(result => {
      if (!result.canceled && result.filePaths.length > 0) {
        if (type === 'target') {
          document.getElementById('setting-target-dir').value = result.filePaths[0];
        }
      }
    }).catch(() => {});
  } else {
    showToast('请手动输入路径', 'info');
  }
}

function updateHomeStats() {
  const el = document.getElementById('stat-installed');
  if (el) el.textContent = installedVersions.length;
}

let isWindowMode = false;
let isWindowMaximized = false;

function setupWindowControls() {
  const windowControls = document.getElementById('window-controls');
  const windowModeCheckbox = document.getElementById('setting-window-mode');
  const exitLauncherBtn = document.getElementById('exit-launcher-btn');

  const isMac = window.electronAPI?.platform === 'darwin';
  if (isMac) {
    document.body.classList.add('is-mac');
  }
  if (windowControls && !isMac) windowControls.style.display = 'flex';

  const winBtnMinimize = document.getElementById('win-btn-minimize');
  if (winBtnMinimize) winBtnMinimize.addEventListener('click', () => {
    window.electronAPI.minimize();
  });

  const winBtnMaximize = document.getElementById('win-btn-maximize');
  if (winBtnMaximize) winBtnMaximize.addEventListener('click', () => {
    window.electronAPI.maximize();
  });

  const winBtnRestore = document.getElementById('win-btn-restore');
  if (winBtnRestore) winBtnRestore.addEventListener('click', () => {
    window.electronAPI.maximize();
  });

  const winBtnClose = document.getElementById('win-btn-close');
  if (winBtnClose) winBtnClose.addEventListener('click', () => {
    window.electronAPI.close();
  });

  if (window.electronAPI.onRequestCloseAnimate) {
    window.electronAPI.onRequestCloseAnimate(() => {
      const app = document.getElementById('app');
      if (app && !app.classList.contains('app-closing')) {
        app.classList.add('app-closing');
      }
    });
  }

  window.electronAPI.onWindowStateChanged((data) => {
    isWindowMaximized = data.maximized;
    isWindowMode = !data.fullscreen;
    if (windowModeCheckbox) {
      windowModeCheckbox.checked = isWindowMode;
    }
    updateWindowButtons();
  });

  window.electronAPI.onWindowModeChanged((data) => {
    isWindowMode = data.windowMode;
    isWindowMaximized = data.maximized;
    if (windowModeCheckbox) {
      windowModeCheckbox.checked = data.windowMode;
    }
    updateWindowButtons();
  });

  if (windowModeCheckbox) {
    windowModeCheckbox.addEventListener('change', () => {
      const enabled = windowModeCheckbox.checked;
      isWindowMode = enabled;
      window.electronAPI.setWindowMode(enabled);
      updateWindowButtons();
    });
  }

  if (exitLauncherBtn) {
    exitLauncherBtn.addEventListener('click', () => {
      window.electronAPI.quitApp();
    });
  }

  window.electronAPI.isFullscreen().then((fullscreen) => {
    isWindowMode = !fullscreen;
    if (windowModeCheckbox) {
      windowModeCheckbox.checked = isWindowMode;
    }
    updateWindowButtons();
  });
}

function setupVersionListClicks() {
  document.addEventListener('click', (e) => {
    // 主页内嵌版本卡片：跳转到独立的"已安装版本"页面
    const homeCard = e.target.closest('.home-current-version-card');
    if (homeCard) {
      navigateToPage('installed-versions');
      return;
    }

    // 齿轮按钮：进入版本设置页（阻止冒泡，不触发卡片选中）
    const settingsBtn = e.target.closest('.version-item-settings-btn');
    if (settingsBtn) {
      e.stopPropagation();
      const versionId = settingsBtn.dataset.versionId;
      const customName = settingsBtn.dataset.customName || '';
      if (versionId) {
        openVersionSettings(versionId, customName || versionId);
      }
      return;
    }

    // 卡片本体（排除其他按钮如"设置""删除"）
    const versionItem = e.target.closest('.version-item-clickable');
    if (versionItem && !e.target.closest('button')) {
      const versionId = versionItem.dataset.versionId;
      const versionUrl = versionItem.dataset.versionUrl || '';
      const versionType = versionItem.dataset.versionType || 'release';
      const isInstalled = versionItem.dataset.installed === 'true';
      const customName = versionItem.dataset.customName || '';

      if (versionId) {
        if (isInstalled) {
          selectLaunchVersion(versionId);
        } else {
          openVersionDetail(versionId, versionUrl, versionType);
        }
      }
    }
  });
}

function updateWindowButtons() {
  const controls = document.getElementById('window-controls');
  const maximizeBtn = document.getElementById('win-btn-maximize');
  const restoreBtn = document.getElementById('win-btn-restore');

  if (!controls || window.electronAPI?.platform === 'darwin') return;

  controls.style.display = 'flex';
  if (isWindowMode) {
    if (isWindowMaximized) {
      maximizeBtn.style.display = 'none';
      restoreBtn.style.display = 'flex';
    } else {
      maximizeBtn.style.display = 'flex';
      restoreBtn.style.display = 'none';
    }
  } else {
    maximizeBtn.style.display = 'flex';
    restoreBtn.style.display = 'none';
  }
}





// ─── 设置子菜单和功能函数 ──────────────────────────────────

function setupSettingsSubmenu() {
}

function switchPage(pageName) {
  const currentPage = document.querySelector('.page.active');
  const target = document.getElementById(`page-${pageName}`);
  if (!target || target === currentPage) return;

  if (currentPage && currentPage.id === 'page-accounts' && _currentDetailAccount) {
    showAccountList();
  }

  if (currentPage) {
    currentPage.style.animation = 'pageOut 0.18s var(--ease-out-expo) forwards';
    setTimeout(() => {
      currentPage.classList.remove('active');
      currentPage.style.animation = '';
      target.classList.add('active');
      target.style.animation = 'pageIn 0.35s var(--ease-out-expo) backwards';
    }, 160);
  } else {
    target.classList.add('active');
    target.style.animation = 'pageIn 0.35s var(--ease-out-expo) backwards';
  }

  previousPage = currentPage?.id?.replace('page-', '') || null;
}
