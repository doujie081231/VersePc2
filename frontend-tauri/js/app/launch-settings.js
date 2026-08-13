/**
 * @file launch-settings.js
 * @description 启动设置函数 - 内存分配、JVM参数、CDS等启动相关配置
 */

let systemMemoryInfo = null;

function toggleMemoryMode() {
  const mode = document.querySelector('input[name="globalMemoryMode"]:checked')?.value;
  const customSettings = document.getElementById('memory-custom-settings');
  const autoInfo = document.getElementById('memory-auto-info');
  if (customSettings) {
    customSettings.style.display = mode === 'custom' ? 'block' : 'none';
  }
  if (autoInfo) {
    autoInfo.style.display = mode === 'auto' ? 'block' : 'none';
  }
  updateMemoryDisplay();
}

function updateMemoryDisplay() {
  const slider = document.getElementById('memory-slider');
  const display = document.getElementById('memory-value-display');
  const fill = document.getElementById('memory-slider-fill');
  const usedMarker = document.getElementById('memory-used-marker');
  const usedLabel = document.getElementById('memory-used-label');
  if (slider && display) {
    const mb = parseInt(slider.value, 10);
    const gb = (mb / 1024).toFixed(1);
    display.textContent = mb >= 1024 ? `${mb} MB (${gb} GB)` : `${mb} MB`;
    if (fill) {
      const max = parseInt(slider.max, 10) || 16384;
      const min = parseInt(slider.min, 10) || 0;
      const pct = Math.max(0, Math.min(100, ((mb - min) / (max - min)) * 100));
      fill.style.width = pct + '%';
    }
    if (usedMarker && usedLabel && systemMemoryInfo) {
      const usedMB = systemMemoryInfo.usedMB;
      const min = parseInt(slider.min, 10) || 0;
      const max = parseInt(slider.max, 10) || systemMemoryInfo.totalMB;
      const usedPct = Math.max(0, Math.min(100, ((usedMB - min) / (max - min)) * 100));
      usedMarker.style.left = usedPct + '%';
      usedLabel.style.left = usedPct + '%';
      usedMarker.style.display = 'block';
      usedLabel.style.display = 'block';
      usedLabel.textContent = `已用 ${(usedMB / 1024).toFixed(1)} GB`;
    }
  }
  updateAllocatedMemoryDisplay();
}

function updateAllocatedMemoryDisplay() {
  const mode = document.querySelector('input[name="globalMemoryMode"]:checked')?.value;
  const allocatedDisplay = document.getElementById('allocated-memory-display');
  const remainingDisplay = document.getElementById('remaining-memory-display');
  if (!systemMemoryInfo) return;
  let allocMB;
  if (mode === 'auto') {
    allocMB = systemMemoryInfo.autoMB;
  } else {
    const slider = document.getElementById('memory-slider');
    allocMB = slider ? parseInt(slider.value, 10) : systemMemoryInfo.autoMB;
  }
  const allocGB = (allocMB / 1024).toFixed(1);
  const remainMB = systemMemoryInfo.totalMB - allocMB;
  const remainGB = Math.max(0, remainMB / 1024).toFixed(1);
  if (allocatedDisplay) allocatedDisplay.textContent = `${allocGB} GB`;
  if (remainingDisplay) remainingDisplay.textContent = `${remainGB} GB`;
}

async function updateSystemMemoryInfo() {
  try {
    const data = await API.getSystemMemory();
    systemMemoryInfo = data;
    const totalDisplay = document.getElementById('sys-total-memory');
    const usedDisplay = document.getElementById('sys-used-memory');
    const freeDisplay = document.getElementById('sys-free-memory');
    const memBar = document.getElementById('sys-memory-bar');
    const autoValue = document.getElementById('memory-auto-value');
    const sliderMax = document.getElementById('memory-slider-max');
    const slider = document.getElementById('memory-slider');
    if (totalDisplay) totalDisplay.textContent = `${data.totalGB} GB`;
    if (usedDisplay) usedDisplay.textContent = `${data.usedGB} GB`;
    if (freeDisplay) freeDisplay.textContent = `${data.freeGB} GB`;
    if (memBar) {
      const usedPct = Math.min(100, Math.round((data.usedMB / data.totalMB) * 100));
      memBar.style.width = `${usedPct}%`;
      if (usedPct > 80) memBar.style.background = '#ff4d4d';
      else if (usedPct > 60) memBar.style.background = '#ff9800';
      else memBar.style.background = 'var(--accent)';
    }
    if (autoValue) autoValue.textContent = `${data.autoGB} GB`;
    if (slider) {
      slider.max = data.totalMB;
      if (parseInt(slider.value, 10) > data.totalMB) {
        slider.value = data.autoMB;
      }
    }
    if (sliderMax) sliderMax.textContent = `${data.totalMB} MB`;
    updateMemoryDisplay();
  } catch (e) {
    console.error('[Settings] Update memory info error:', e);
  }
}

function toggleAdvancedOptions() {
  const content = document.getElementById('advanced-options-content');
  const arrow = document.getElementById('advanced-options-arrow');
  if (content && arrow) {
    const isHidden = content.style.display === 'none';
    content.style.display = isHidden ? 'block' : 'none';
    arrow.style.transform = isHidden ? 'rotate(180deg)' : 'rotate(0)';
  }
}

async function saveLaunchSettings() {
  let windowSize = document.getElementById('window-size')?.value || 'default';
  if (windowSize === 'default') {
    windowSize = '854x480';
  } else if (windowSize === 'custom') {
    const w = document.getElementById('custom-width')?.value;
    const h = document.getElementById('custom-height')?.value;
    if (w && h) {
      windowSize = `${w}x${h}`;
    } else {
      windowSize = '1920x1080';
    }
  }

  const settings = {
    versionIsolation: document.getElementById('launch-version-isolation')?.value,
    windowTitle: document.getElementById('launch-window-title')?.value,
    customInfo: document.getElementById('launch-custom-info')?.value,
    launcherVisibility: document.getElementById('launcher-visibility')?.value,
    processPriority: document.getElementById('process-priority')?.value,
    windowSize: windowSize,
    fullscreen: document.getElementById('launch-fullscreen')?.checked || false,
    gameJava: document.getElementById('game-java-select')?.value,
    memoryMode: document.querySelector('input[name="globalMemoryMode"]:checked')?.value,
    memoryValue: document.getElementById('memory-slider')?.value,
    jvmArgs: document.getElementById('jvm-args')?.value,
    gameArgs: document.getElementById('game-args')?.value,
    preLaunchCommand: document.getElementById('pre-launch-command')?.value,
    memoryManagement: document.getElementById('memory-management')?.value,
    disableJavaWrapper: document.getElementById('disable-java-wrapper')?.checked,
    disableLWJGLAgent: document.getElementById('disable-lwjgl-agent')?.checked,
    useHighPerformanceGPU: document.getElementById('use-high-performance-gpu')?.checked,
    performanceBoost: document.getElementById('performance-boost')?.checked,
    jvmPreheat: document.getElementById('jvm-preheat')?.checked,
    enableCds: document.getElementById('enable-cds')?.checked
  };

  try {
    await window.electronAPI.store.set('versepc_launch_settings', JSON.stringify(settings));
    showToast('启动设置已保存', 'success');

    // 注意：启动分辨率只作用于游戏窗口，不改变启动器窗口大小
  } catch (e) {
    showToast('保存失败: ' + e.message, 'error');
  }
}

async function browseDataDir() {
  try {
    const current = document.getElementById('setting-data-dir').value;
    const opts = { properties: ['openDirectory'] };
    if (current && current !== '加载中...') opts.defaultPath = current;
    const result = await window.electronAPI.showOpenDialog(opts);
    if (!result || !result.filePaths || result.filePaths.length === 0) return;
    const selectedPath = result.filePaths[0];
    const res = await fetch('/api/settings/data-dir', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ dataDir: selectedPath })
    });
    const data = await res.json();
    if (data.ok) {
      document.getElementById('setting-data-dir').value = selectedPath;
      if (data.message) {
        alert(data.message);
      }
    } else if (data.error) {
      alert('保存失败: ' + data.error);
    }
  } catch (e) {
    alert('保存失败: ' + e.message);
  }
}

async function resetDataDir() {
  try {
    const res = await fetch('/api/settings/data-dir', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ reset: true })
    });
    const data = await res.json();
    if (data.ok) {
      document.getElementById('setting-data-dir').value = '';
      if (data.message) {
        alert(data.message);
      }
    }
  } catch (e) {
    alert('重置失败: ' + e.message);
  }
}

async function resetLaunchSettings() {
  const confirmed = await showConfirmDialog('重置设置', '确定要重置启动设置为默认值吗?', '重置', '取消');
  if (!confirmed) return;

  document.getElementById('launch-version-isolation').value = 'all';
  document.getElementById('launch-window-title').value = '';
  document.getElementById('launch-custom-info').value = 'VersePC2';
  document.getElementById('launcher-visibility').value = 'keep';
  document.getElementById('process-priority').value = 'normal';
  document.getElementById('window-size').value = 'default';
  document.getElementById('launch-fullscreen').checked = false;
  document.getElementById('game-java-select').value = 'auto';
  document.querySelector('input[name="globalMemoryMode"][value="auto"]').checked = true;
  document.getElementById('memory-slider').value = 4096;
  document.getElementById('jvm-args').value = '';
  document.getElementById('game-args').value = '';
  document.getElementById('pre-launch-command').value = '';
  document.getElementById('memory-management').value = 'default';
  document.getElementById('disable-java-wrapper').checked = false;
  document.getElementById('disable-lwjgl-agent').checked = false;
  document.getElementById('use-high-performance-gpu').checked = true;
  document.getElementById('performance-boost').checked = true;
  document.getElementById('jvm-preheat').checked = false;
  document.getElementById('enable-cds').checked = true;

  const _minimizeCb = document.getElementById('setting-minimize-on-game-run');
  if (_minimizeCb) _minimizeCb.checked = true;

  toggleMemoryMode();
  updateMemoryDisplay();
  try { await API.saveSettings({ gameDir: '' }); } catch (e) {}
  try { await window.electronAPI.store.set('minimizeOnGameRun', true); } catch (e) {}
  showToast('启动设置已重置', 'success');
}

async function optimizeJvmArgs() {
  const versionId = launchVersionCustomSelect ? launchVersionCustomSelect.getValue() : '';
  if (!versionId) {
    showToast('请先选择一个游戏版本', 'error');
    return;
  }
  try {
    const result = await API.getOptimizedJvmArgs(versionId);
    if (result && result.args) {
      document.getElementById('jvm-args').value = result.args;
      showToast(`已优化 JVM 参数（分配 ${result.ramGB}GB 内存，检测到 ${result.modCount} 个模组）`, 'success');
    }
  } catch (e) {
    showToast('优化失败: ' + e.message, 'error');
  }
}

async function loadLaunchSettings() {
  try {
    const saved = await window.electronAPI.store.get('versepc_launch_settings');
    if (saved) {
      const settings = JSON.parse(saved);
      if (settings.versionIsolation) document.getElementById('launch-version-isolation').value = settings.versionIsolation;
      if (settings.windowTitle) document.getElementById('launch-window-title').value = settings.windowTitle;
      if (settings.customInfo) document.getElementById('launch-custom-info').value = settings.customInfo;
      if (settings.launcherVisibility) document.getElementById('launcher-visibility').value = settings.launcherVisibility;
      if (settings.processPriority) document.getElementById('process-priority').value = settings.processPriority;
      if (settings.windowSize) {
        const wsVal = settings.windowSize;
        const wsSelect = document.getElementById('window-size');
        const customDiv = document.getElementById('custom-window-size');
        
        if (/^\d+x\d+$/.test(wsVal)) {
          const presetOptions = ['854x480','1280x720','1600x900','1920x1080','2560x1440','3840x2160'];
          if (presetOptions.includes(wsVal)) {
            if (wsSelect) wsSelect.value = wsVal;
            if (customDiv) customDiv.style.display = 'none';
          } else {
            if (wsSelect) wsSelect.value = 'custom';
            if (customDiv) customDiv.style.display = 'flex';
            const [w, h] = wsVal.split('x');
            const cw = document.getElementById('custom-width');
            const ch = document.getElementById('custom-height');
            if (cw) cw.value = w;
            if (ch) ch.value = h;
          }
        } else {
          if (wsSelect) wsSelect.value = wsVal;
          if (customDiv) customDiv.style.display = 'none';
        }
      }
      if (settings.fullscreen !== undefined) document.getElementById('launch-fullscreen').checked = !!settings.fullscreen;
      if (settings.gameJava) document.getElementById('game-java-select').value = settings.gameJava;
      if (settings.memoryMode) {
        document.querySelector(`input[name="globalMemoryMode"][value="${settings.memoryMode}"]`).checked = true;
        toggleMemoryMode();
      }
      if (settings.memoryValue) {
        document.getElementById('memory-slider').value = settings.memoryValue;
        updateMemoryDisplay();
      }
      if (settings.jvmArgs) document.getElementById('jvm-args').value = settings.jvmArgs;
      if (settings.gameArgs) document.getElementById('game-args').value = settings.gameArgs;
      if (settings.preLaunchCommand) document.getElementById('pre-launch-command').value = settings.preLaunchCommand;
      if (settings.memoryManagement) document.getElementById('memory-management').value = settings.memoryManagement;
      if (settings.disableJavaWrapper !== undefined) document.getElementById('disable-java-wrapper').checked = settings.disableJavaWrapper;
      if (settings.disableLWJGLAgent !== undefined) document.getElementById('disable-lwjgl-agent').checked = settings.disableLWJGLAgent;
      if (settings.useHighPerformanceGPU !== undefined) document.getElementById('use-high-performance-gpu').checked = settings.useHighPerformanceGPU;
      if (settings.performanceBoost !== undefined) document.getElementById('performance-boost').checked = settings.performanceBoost;
      if (settings.jvmPreheat !== undefined) document.getElementById('jvm-preheat').checked = settings.jvmPreheat;
      if (settings.enableCds !== undefined) document.getElementById('enable-cds').checked = settings.enableCds;
    }

    // 游戏运行低调模式开关（独立 store key，默认开启）
    try {
      const minimizeOnGameRun = await window.electronAPI.store.get('minimizeOnGameRun');
      const cb = document.getElementById('setting-minimize-on-game-run');
      if (cb) {
        cb.checked = (minimizeOnGameRun === undefined || minimizeOnGameRun === null || minimizeOnGameRun === true);
        if (!cb._versepcBound) {
          cb._versepcBound = true;
          cb.addEventListener('change', () => {
            try { window.electronAPI.store.set('minimizeOnGameRun', cb.checked); } catch (e) {}
          });
        }
      }
    } catch (e) {}

    updateSystemMemoryInfo();
    checkCdsStatus();
    if (window._memMonitorTimer) clearInterval(window._memMonitorTimer);
    window._memMonitorTimer = setInterval(() => {
      const el = document.getElementById('sys-total-memory');
      if (el && el.offsetParent !== null) {
        updateSystemMemoryInfo();
      } else {
        clearInterval(window._memMonitorTimer);
        window._memMonitorTimer = null;
      }
    }, 2000);
  } catch (e) {
    console.error('[Settings] Load launch settings error:', e);
  }
}

async function selectTheme(element) {
  document.querySelectorAll('.theme-option').forEach(opt => opt.classList.remove('active'));
  element.classList.add('active');

  const theme = element.dataset.theme;
  const isCustom = theme === 'custom';

  document.documentElement.setAttribute('data-theme', theme);
  document.documentElement.classList.toggle('dark-theme', theme === 'dark' || isCustom);
  document.documentElement.classList.toggle('light-theme', theme === 'light');

  const app = document.getElementById('app');
  if (app && theme === 'light') {
    app.classList.remove('wp-light', 'wp-dark');
  }

  if (typeof toggleCustomThemeColorGroup === 'function') {
    toggleCustomThemeColorGroup(isCustom);
  }

  if (isCustom) {
    // 自定义主题色：根据保存的颜色/深浅模式动态生成整套主题变量
    const savedColor = typeof getSavedCustomThemeColor === 'function'
      ? await getSavedCustomThemeColor()
      : '#4c8dff';
    const isLight = typeof getSavedCustomThemeLight === 'function'
      ? await getSavedCustomThemeLight()
      : false;
    if (typeof applyCustomThemeColor === 'function') {
      await applyCustomThemeColor(savedColor, { save: false, isLight });
    }
    if (typeof syncCustomThemeLightModeUI === 'function') {
      syncCustomThemeLightModeUI(isLight);
    }
  } else {
    if (typeof clearCustomThemeVars === 'function') clearCustomThemeVars();
    document.documentElement.style.setProperty('--accent', theme === 'dark' ? '#ffffff' : '#1a1a1a');
    document.documentElement.style.setProperty('--accent-hover', theme === 'dark' ? '#d0d0d0' : '#333333');
    document.documentElement.style.setProperty('--accent-rgb', theme === 'dark' ? '255, 255, 255' : '26, 26, 26');
  }

  if (typeof updateWallpaperTheme === 'function') {
    updateWallpaperTheme(theme === 'dark' || isCustom);
  }

  const editorIframe = document.getElementById('editor-iframe');
  if (editorIframe && editorIframe.contentWindow) {
    editorIframe.contentWindow.postMessage({ type: 'editor:set-theme', theme: isCustom ? 'dark' : theme }, '*');
  }

  try {
    await window.electronAPI.store.set('versepc_theme', theme);
  } catch (e) {
    console.error('[Settings] Save theme error:', e);
  }
}
