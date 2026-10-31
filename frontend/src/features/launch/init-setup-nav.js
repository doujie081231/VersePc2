import { bridge as bridge } from '../../core/tauri-bridge.js';
import { VERSE_FEATURES as VERSE_FEATURES } from '../misc/feature-flags.js';
import { closeSidebarPanel as closeSidebarPanel } from '../settings/window-theme.js';
import { handleLaunch as handleLaunch } from './launch.js';
import { CustomSelect as CustomSelect } from '../misc/custom-select.js';
import { showToast as showToast } from '../misc/ui-components.js';
import { loadResourcePage as loadResourcePage } from '../mods/resource.js';
import { loadVersions as loadVersions, renderHomeCurrentVersionCard as renderHomeCurrentVersionCard, renderVersions as renderVersions, navigateToPage as navigateToPage, loadModLoaderVersions as loadModLoaderVersions } from '../versions/versions.js';
import { appState as appState } from '../../core/app-state.js';
import { resetBedrockCache } from '../versions/versions-bedrock.js';

function setupNavigation() {
  // 完成导航后收起悬浮胶囊（安卓抽屉式：选中即收起）
  // 经典侧栏常驻，不因导航点击收起；简约模式由「导航栏单点自动收回」控制
  const collapseSidebar = () => {
    if (document.body.classList.contains('classic-nav')) return;
    if (document.body.classList.contains('sidebar-keep-open')) return;
    if (typeof closeSidebarPanel === 'function') closeSidebarPanel();
  };

  // 版本下载功能开关：入口保留，进入后由 navigateToPage 跳转到合规提示页
  const versionDownloadOn = !!(VERSE_FEATURES && VERSE_FEATURES.versionDownload);

  document.querySelectorAll('.nav-btn:not(.nav-submenu-toggle)').forEach(btn => {
    btn.addEventListener('click', () => {
      const page = btn.dataset.page;
      if (!page) return;

      if (page === 'versions') {
        if (!versionDownloadOn) {
          navigateToPage(page);
          collapseSidebar();
          return;
        }
        loadVersions(false);
      }

      navigateToPage(page);
      collapseSidebar();
    });
  });

  document.querySelectorAll('.nav-submenu-group').forEach(group => {
    const toggle = group.querySelector('.nav-submenu-toggle');
    if (!toggle) return;

    toggle.addEventListener('click', (e) => {
      // 父级按钮只负责展开/收起子菜单，不触发跳转（与安卓悬浮导航一致）
      e.stopPropagation();
      group.classList.toggle('open');
    });

    group.querySelectorAll('.nav-sub-btn').forEach(btn => {
      btn.addEventListener('click', () => {
        const page = btn.dataset.page;
        if (!page) return;
        navigateToPage(page);
        collapseSidebar();
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

      if (tab === 'release' || tab === 'snapshot' || tab === 'special' || tab === 'old' || tab === 'installed' || tab === 'bedrock') {
        appState.currentVersionTab = tab;
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
      appState.currentLoaderType = btn.dataset.loader;
      loadModLoaderVersions();
    });
  });
}

function setupLaunchBar() {
  document.getElementById('launch-btn').addEventListener('click', handleLaunch);
  document.getElementById('home-launch-btn').addEventListener('click', handleLaunch);

  if (!appState.launchVersionCustomSelect) {
    appState.launchVersionCustomSelect = new CustomSelect('launch-version-select-wrapper', {
      onChange: (value) => {
        // 同步到 appState.currentLaunchVersionId 状态变量和主页内嵌卡片
        if (value && value !== appState.currentLaunchVersionId) {
          appState.currentLaunchVersionId = value;
          appState._cachedLastLaunchVersion = value;
          try { bridge.store.set('versepc_last_launch_version', value); } catch (_) {}
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
    resetBedrockCache();
    await loadVersions(true);
    showToast('版本列表已刷新', 'success');
  });
}

export { setupNavigation, setupTabs, setupLaunchBar };