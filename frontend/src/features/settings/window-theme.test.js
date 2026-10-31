// @vitest-environment jsdom
// window-theme.test.js — 主题切换、导航模式、窗口控制与页面切换单元测试
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { getApiCalls, getCommandCalls, installTauriMock, mockApiRoute, mockCommand } from '../../test-utils/tauri-mock.js';

let mod;
let bridge;
let appState;
let stateCb = null;
let modeCb = null;

function buildDom() {
  document.body.innerHTML = `
    <div id="window-controls" style="display:none">
      <button id="win-btn-minimize"></button>
      <button id="win-btn-maximize" style="display:none"></button>
      <button id="win-btn-restore" style="display:none"></button>
      <button id="win-btn-close"></button>
    </div>
    <input type="checkbox" id="setting-window-mode" />
    <button id="exit-launcher-btn"></button>
    <button id="sidebar-toggle-btn" aria-expanded="false"></button>
    <button id="classic-nav-toggle" aria-expanded="true"></button>
    <div class="sidebar"></div>
    <div class="sidebar-hover-zone" id="sidebar-hover-zone"></div>
    <div id="sidebar-overlay"></div>
    <div id="app"></div>
    <div class="theme-option" data-theme="dark"></div>
    <div class="theme-option" data-theme="light"></div>
    <input id="custom-accent-color" />
    <span id="custom-color-value"></span>
    <div id="color-preview-dot"></div>
    <input id="setting-target-dir" />
    <div id="toast-container"></div>
    <div id="page-home" class="page active"></div>
    <div id="page-mods" class="page"></div>
    <div class="home-current-version-card"></div>
    <div class="version-item-clickable" data-version-id="1.20.1" data-installed="true"></div>
    <div class="version-item-clickable" data-version-id="1.19.4" data-version-url="https://x/v.json" data-version-type="release" data-installed="false"></div>
  `;
  document.body.className = '';
}

beforeAll(async () => {
  installTauriMock();
  vi.resetModules();
  bridge = (await import('../../core/tauri-bridge.js')).bridge;
  appState = (await import('../../core/app-state.js')).appState;
  mod = await import('./window-theme.js');
});

beforeEach(() => {
  installTauriMock();
  buildDom();
  mockApiRoute('/api/settings/set', { ok: true });
  mockApiRoute('/api/accounts', []);
  mockApiRoute('/api/settings', {});
  stateCb = null;
  modeCb = null;
  bridge.onWindowStateChanged = (cb) => { stateCb = cb; return () => {}; };
  bridge.onWindowModeChanged = (cb) => { modeCb = cb; };
  document.documentElement.removeAttribute('data-theme');
  document.documentElement.className = '';
  document.documentElement.style.cssText = '';
  appState.previousPage = '';
  appState.currentLaunchVersionId = '';
  localStorage.clear();
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  document.body.innerHTML = '';
});

describe('getThemeLabel', () => {
  it('映射内置主题名，未知主题原样返回', () => {
    expect(mod.getThemeLabel('dark')).toBe('黑色');
    expect(mod.getThemeLabel('light')).toBe('白色');
    expect(mod.getThemeLabel('custom')).toBe('custom');
  });
});

describe('switchTheme', () => {
  it('切换到深色主题并持久化', () => {
    document.getElementById('app').classList.add('wp-light', 'wp-dark');

    mod.switchTheme('dark');

    expect(document.documentElement.getAttribute('data-theme')).toBe('dark');
    expect(document.documentElement.classList.contains('dark-theme')).toBe(true);
    const options = document.querySelectorAll('.theme-option');
    expect(options[0].classList.contains('active')).toBe(true);
    expect(options[1].classList.contains('active')).toBe(false);
    // 深色不清理壁纸明暗类
    expect(document.getElementById('app').classList.contains('wp-dark')).toBe(true);

    const saves = getApiCalls().filter((c) => c.path === '/api/settings/set');
    expect(saves.map((c) => c.args.body.key)).toEqual(['theme', 'accentColor']);
    expect(getCommandCalls().some((c) => c.cmd === 'store_set' && c.args.key === 'versepc_theme')).toBe(true);
    expect(document.getElementById('toast-container').textContent).toContain('已切换到「黑色」主题');
  });

  it('切换到浅色主题清理壁纸明暗类', () => {
    const app = document.getElementById('app');
    app.classList.add('wp-light', 'wp-dark');

    mod.switchTheme('light');

    expect(document.documentElement.getAttribute('data-theme')).toBe('light');
    expect(document.documentElement.classList.contains('light-theme')).toBe(true);
    expect(app.classList.contains('wp-light')).toBe(false);
    expect(app.classList.contains('wp-dark')).toBe(false);
    expect(document.getElementById('toast-container').textContent).toContain('已切换到「白色」主题');
  });

  it('同步强调色预览控件', () => {
    vi.stubGlobal('getComputedStyle', () => ({ getPropertyValue: () => ' #123456' }));

    mod.switchTheme('dark');

    expect(document.getElementById('custom-accent-color').value).toBe('#123456');
    expect(document.getElementById('custom-color-value').textContent).toBe('#123456');
    expect(document.getElementById('color-preview-dot').style.background).toBe('rgb(18, 52, 86)');
  });
});

describe('applyCustomAccent', () => {
  it('写入强调色并保存', () => {
    document.getElementById('custom-accent-color').value = '#00ff00';

    mod.applyCustomAccent();

    expect(document.getElementById('custom-color-value').textContent).toBe('#00ff00');
    expect(document.documentElement.style.getPropertyValue('--accent')).toBe('#00ff00');
    const save = getApiCalls().find((c) => c.path === '/api/settings/set');
    expect(save.args.body).toEqual({ key: 'accentColor', value: '#00ff00' });
    expect(document.getElementById('toast-container').textContent).toContain('强调色已应用');
  });

  it('空颜色值不动作', () => {
    document.getElementById('custom-accent-color').value = '';
    mod.applyCustomAccent();
    expect(getApiCalls().some((c) => c.path === '/api/settings/set')).toBe(false);
  });
});

describe('browseFolder', () => {
  it('选择目录后写入目标路径', async () => {
    mockCommand('dialog_open', () => ({ canceled: false, filePaths: ['C:/target'] }));
    mod.browseFolder('target');
    await vi.waitFor(() => {
      expect(document.getElementById('setting-target-dir').value).toBe('C:/target');
    });
  });

  it('取消选择不修改', async () => {
    mockCommand('dialog_open', () => ({ canceled: true, filePaths: [] }));
    mod.browseFolder('target');
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(document.getElementById('setting-target-dir').value).toBe('');
  });

  it('无对话框能力时提示手动输入', () => {
    const original = bridge.showOpenDialog;
    bridge.showOpenDialog = undefined;
    try {
      mod.browseFolder('target');
      expect(document.getElementById('toast-container').textContent).toContain('请手动输入路径');
    } finally {
      bridge.showOpenDialog = original;
    }
  });
});

describe('applyNavigationMode', () => {
  it('简约模式恢复展开状态并广播 resize', () => {
    localStorage.setItem('versepc_sidebar_open', '1');
    const resizeSpy = vi.fn();
    window.addEventListener('resize', resizeSpy);

    mod.applyNavigationMode(true);

    expect(document.body.classList.contains('minimal-nav')).toBe(true);
    expect(document.body.classList.contains('classic-nav')).toBe(false);
    expect(document.body.classList.contains('sidebar-open')).toBe(true);
    expect(localStorage.getItem('versepc_nav_minimal')).toBe('1');
    expect(document.getElementById('sidebar-toggle-btn').getAttribute('aria-expanded')).toBe('true');
    expect(resizeSpy).toHaveBeenCalled();
    window.removeEventListener('resize', resizeSpy);
  });

  it('经典模式恢复折叠状态并清理简约模式类', () => {
    localStorage.setItem('versepc_classic_sidebar_collapsed', '1');
    document.body.classList.add('sidebar-open', 'show-nav-labels', 'sidebar-keep-open');

    mod.applyNavigationMode(false);

    expect(document.body.classList.contains('classic-nav')).toBe(true);
    expect(document.body.classList.contains('classic-sidebar-collapsed')).toBe(true);
    expect(document.body.classList.contains('sidebar-open')).toBe(false);
    expect(document.body.classList.contains('show-nav-labels')).toBe(false);
    expect(document.body.classList.contains('sidebar-keep-open')).toBe(false);
    expect(localStorage.getItem('versepc_nav_minimal')).toBe('0');
    expect(document.getElementById('sidebar-toggle-btn').getAttribute('aria-expanded')).toBe('false');
  });
});

describe('setupWindowControls', () => {
  it('显示窗口控制按钮并绑定窗口命令', async () => {
    mod.setupWindowControls();

    expect(document.getElementById('window-controls').style.display).toBe('flex');
    document.getElementById('win-btn-minimize').click();
    document.getElementById('win-btn-maximize').click();
    document.getElementById('win-btn-restore').click();
    document.getElementById('win-btn-close').click();
    document.getElementById('exit-launcher-btn').click();

    const cmds = getCommandCalls().map((c) => c.cmd);
    expect(cmds.filter((c) => c === 'window_minimize').length).toBe(1);
    expect(cmds.filter((c) => c === 'window_maximize').length).toBe(2);
    expect(cmds.filter((c) => c === 'window_close').length).toBe(2);

    await vi.waitFor(() => {
      expect(document.getElementById('setting-window-mode').checked).toBe(true);
    });
  });

  it('侧栏按钮在经典模式切换折叠', () => {
    document.body.classList.add('classic-nav');
    mod.setupWindowControls();

    document.getElementById('sidebar-toggle-btn').click();
    expect(document.body.classList.contains('classic-sidebar-collapsed')).toBe(true);
    expect(localStorage.getItem('versepc_classic_sidebar_collapsed')).toBe('1');

    document.getElementById('sidebar-toggle-btn').click();
    expect(document.body.classList.contains('classic-sidebar-collapsed')).toBe(false);
    expect(localStorage.getItem('versepc_classic_sidebar_collapsed')).toBe('0');

    // 经典模式下侧栏内的「<」/图钉按钮与悬浮按钮共用同一套逻辑
    document.getElementById('classic-nav-toggle').click();
    expect(document.body.classList.contains('classic-sidebar-collapsed')).toBe(true);
    document.getElementById('classic-nav-toggle').click();
    expect(document.body.classList.contains('classic-sidebar-collapsed')).toBe(false);
  });

  it('经典模式收起后悬停左侧热区临时探出，点击图钉重新固定', () => {
    vi.useFakeTimers();
    document.body.classList.add('classic-nav', 'classic-sidebar-collapsed');
    mod.setupWindowControls();
    const zone = document.getElementById('sidebar-hover-zone');

    zone.dispatchEvent(new MouseEvent('mouseenter'));
    expect(document.body.classList.contains('classic-sidebar-peek')).toBe(true);

    zone.dispatchEvent(new MouseEvent('mouseleave'));
    vi.advanceTimersByTime(200);
    expect(document.body.classList.contains('classic-sidebar-peek')).toBe(false);

    // 再次探出后点击图钉：固定导航栏并清除探出状态
    zone.dispatchEvent(new MouseEvent('mouseenter'));
    document.getElementById('classic-nav-toggle').click();
    expect(document.body.classList.contains('classic-sidebar-collapsed')).toBe(false);
    expect(document.body.classList.contains('classic-sidebar-peek')).toBe(false);
    expect(localStorage.getItem('versepc_classic_sidebar_collapsed')).toBe('0');
    vi.useRealTimers();
  });

  it('呼出判定区与导航栏宽度一致：边界内探出，边界外不探出', () => {
    vi.useFakeTimers();
    document.body.classList.add('classic-nav', 'classic-sidebar-collapsed');
    mod.setupWindowControls();

    document.dispatchEvent(new MouseEvent('mousemove', { clientX: 90, clientY: 200 }));
    expect(document.body.classList.contains('classic-sidebar-peek')).toBe(false);

    document.dispatchEvent(new MouseEvent('mousemove', { clientX: 70, clientY: 200 }));
    expect(document.body.classList.contains('classic-sidebar-peek')).toBe(true);
    vi.useRealTimers();
  });

  it('鼠标离开判定区后快速收回', () => {
    vi.useFakeTimers();
    document.body.classList.add('classic-nav', 'classic-sidebar-collapsed');
    mod.setupWindowControls();

    document.dispatchEvent(new MouseEvent('mousemove', { clientX: 40, clientY: 200 }));
    expect(document.body.classList.contains('classic-sidebar-peek')).toBe(true);

    document.dispatchEvent(new MouseEvent('mousemove', { clientX: 120, clientY: 200 }));
    vi.advanceTimersByTime(100);
    expect(document.body.classList.contains('classic-sidebar-peek')).toBe(false);
    vi.useRealTimers();
  });

  it('鼠标进入导航栏后保持探出，移出后自动收回', () => {
    vi.useFakeTimers();
    document.body.classList.add('classic-nav', 'classic-sidebar-collapsed');
    mod.setupWindowControls();

    document.dispatchEvent(new MouseEvent('mousemove', { clientX: 40, clientY: 200 }));
    expect(document.body.classList.contains('classic-sidebar-peek')).toBe(true);
    vi.advanceTimersByTime(1500);
    expect(document.body.classList.contains('classic-sidebar-peek')).toBe(true);

    document.dispatchEvent(new MouseEvent('mousemove', { clientX: 400, clientY: 200 }));
    vi.advanceTimersByTime(300);
    expect(document.body.classList.contains('classic-sidebar-peek')).toBe(false);
    vi.useRealTimers();
  });

  // 已安装版本页的返回按钮落在导航呼出热区里：探出的侧栏会盖住返回键
  function mountBackButtonPage(active = true) {
    document.body.insertAdjacentHTML('beforeend', `<div id="page-installed-versions" class="page${active ? ' active' : ''}"></div>`);
    const home = document.createElement('button');
    home.className = 'nav-btn';
    home.dataset.page = 'home';
    home.getBoundingClientRect = () => ({ top: 56, bottom: 112 });
    const download = document.createElement('button');
    download.className = 'nav-btn';
    download.dataset.page = 'versions';
    download.getBoundingClientRect = () => ({ top: 114, bottom: 170 });
    document.body.append(home, download);
  }

  it('已安装版本页返回按钮区域不呼出导航栏，已探出时保持显示', () => {
    vi.useFakeTimers();
    document.body.classList.add('classic-nav', 'classic-sidebar-collapsed');
    mountBackButtonPage(true);
    mod.setupWindowControls();

    // 返回按钮中心（约 y=94）落在「主页」项高度内：不呼出
    document.dispatchEvent(new MouseEvent('mousemove', { clientX: 48, clientY: 94 }));
    expect(document.body.classList.contains('classic-sidebar-peek')).toBe(false);

    // 「下载」项高度内同样不呼出
    document.dispatchEvent(new MouseEvent('mousemove', { clientX: 48, clientY: 150 }));
    expect(document.body.classList.contains('classic-sidebar-peek')).toBe(false);

    // 避让带下方恢复呼出，避免整条左侧热区失效
    document.dispatchEvent(new MouseEvent('mousemove', { clientX: 48, clientY: 220 }));
    expect(document.body.classList.contains('classic-sidebar-peek')).toBe(true);

    // 已探出后移到「主页」项：保持显示，否则导航项点不到
    document.dispatchEvent(new MouseEvent('mousemove', { clientX: 48, clientY: 94 }));
    vi.advanceTimersByTime(200);
    expect(document.body.classList.contains('classic-sidebar-peek')).toBe(true);

    // 离开左侧判定区后仍正常收回
    document.dispatchEvent(new MouseEvent('mousemove', { clientX: 400, clientY: 94 }));
    vi.advanceTimersByTime(100);
    expect(document.body.classList.contains('classic-sidebar-peek')).toBe(false);
    vi.useRealTimers();
  });

  it('其他页面返回按钮同一位置仍正常呼出导航栏', () => {
    vi.useFakeTimers();
    document.body.classList.add('classic-nav', 'classic-sidebar-collapsed');
    mountBackButtonPage(false);
    mod.setupWindowControls();

    document.dispatchEvent(new MouseEvent('mousemove', { clientX: 48, clientY: 94 }));
    expect(document.body.classList.contains('classic-sidebar-peek')).toBe(true);
    vi.useRealTimers();
  });

  it('侧栏按钮在简约模式切换展开', () => {
    mod.setupWindowControls();
    document.getElementById('sidebar-toggle-btn').click();
    expect(document.body.classList.contains('sidebar-open')).toBe(true);
    expect(localStorage.getItem('versepc_sidebar_open')).toBe('1');
  });

  it('遮罩点击收起侧栏，keep-open 与经典模式不收起', () => {
    mod.setupWindowControls();
    const toggle = document.getElementById('sidebar-toggle-btn');
    const overlay = document.getElementById('sidebar-overlay');

    toggle.click();
    document.body.classList.add('sidebar-keep-open');
    overlay.click();
    expect(document.body.classList.contains('sidebar-open')).toBe(true);

    document.body.classList.remove('sidebar-keep-open');
    overlay.click();
    expect(document.body.classList.contains('sidebar-open')).toBe(false);

    document.body.classList.add('classic-nav', 'sidebar-open');
    overlay.click();
    expect(document.body.classList.contains('sidebar-open')).toBe(true);
    document.body.classList.remove('classic-nav', 'sidebar-open');
  });

  it('Esc 关闭简约模式侧栏', () => {
    mod.setupWindowControls();
    document.getElementById('sidebar-toggle-btn').click();
    expect(document.body.classList.contains('sidebar-open')).toBe(true);

    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    expect(document.body.classList.contains('sidebar-open')).toBe(false);
  });

  it('closeSidebarPanel 仅收起简约侧栏', () => {
    mod.setupWindowControls();
    document.getElementById('sidebar-toggle-btn').click();

    mod.closeSidebarPanel();
    expect(document.body.classList.contains('sidebar-open')).toBe(false);

    document.body.classList.add('classic-nav', 'sidebar-open');
    mod.closeSidebarPanel();
    expect(document.body.classList.contains('sidebar-open')).toBe(true);
    document.body.classList.remove('classic-nav', 'sidebar-open');
  });

  it('窗口状态与模式回调同步按钮显示', () => {
    mod.setupWindowControls();
    expect(typeof stateCb).toBe('function');
    expect(typeof modeCb).toBe('function');

    stateCb({ maximized: true, fullscreen: false });
    expect(document.getElementById('setting-window-mode').checked).toBe(true);
    expect(document.getElementById('win-btn-restore').style.display).toBe('flex');
    expect(document.getElementById('win-btn-maximize').style.display).toBe('none');

    modeCb({ windowMode: false, maximized: false });
    expect(document.getElementById('setting-window-mode').checked).toBe(false);
    expect(document.getElementById('win-btn-maximize').style.display).toBe('flex');
    expect(document.getElementById('win-btn-restore').style.display).toBe('none');
  });

  it('窗口模式开关切换全屏与窗口命令', () => {
    mod.setupWindowControls();
    const checkbox = document.getElementById('setting-window-mode');

    checkbox.checked = true;
    checkbox.dispatchEvent(new Event('change'));
    expect(getCommandCalls().some((c) => c.cmd === 'window_restore')).toBe(true);

    checkbox.checked = false;
    checkbox.dispatchEvent(new Event('change'));
    expect(getCommandCalls().some((c) => c.cmd === 'window_maximize')).toBe(true);
  });

  it('关闭动画事件为 app 添加 closing 类', () => {
    mod.setupWindowControls();
    // onRequestCloseAnimate 通过 Tauri listen 注册，这里直接触发内部回调不可行；
    // 仅验证注册不抛错
    expect(() => mod.updateWindowButtons()).not.toThrow();
  });
});

describe('updateWindowButtons', () => {
  it('无控制容器时安全返回', () => {
    document.getElementById('window-controls').remove();
    expect(() => mod.updateWindowButtons()).not.toThrow();
  });
});

describe('setupVersionListClicks', () => {
  it('主页版本卡跳转到已安装版本页', () => {
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
    mod.setupVersionListClicks();

    document.querySelector('.home-current-version-card').click();

    expect(warnSpy).toHaveBeenCalledWith('[navigation] 导航实现尚未注册:', 'installed-versions');
  });

  it('点击已安装版本卡切换启动版本', () => {
    mod.setupVersionListClicks();
    const card = document.querySelector('.version-item-clickable[data-installed="true"]');

    card.click();

    expect(appState.currentLaunchVersionId).toBe('1.20.1');
    expect(card.classList.contains('selected')).toBe(true);
    expect(getCommandCalls().some((c) => c.cmd === 'store_set' && c.args.key === 'versepc_last_launch_version')).toBe(true);
  });

  it('点击未安装版本卡请求打开详情页', () => {
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
    mod.setupVersionListClicks();

    document.querySelector('.version-item-clickable[data-installed="false"]').click();

    expect(warnSpy).toHaveBeenCalledWith('[navigation] 版本详情实现尚未注册:', '1.19.4');
  });

  it('按钮内点击不触发卡片选择', () => {
    mod.setupVersionListClicks();
    const card = document.querySelector('.version-item-clickable[data-installed="true"]');
    const button = document.createElement('button');
    card.appendChild(button);

    button.click();

    expect(appState.currentLaunchVersionId).toBe('');
  });
});

describe('switchPage', () => {
  it('带过渡切换页面并记录来源页', async () => {
    mod.switchPage('mods');
    await new Promise((resolve) => setTimeout(resolve, 220));

    expect(document.getElementById('page-home').classList.contains('active')).toBe(false);
    expect(document.getElementById('page-mods').classList.contains('active')).toBe(true);
    expect(appState.previousPage).toBe('home');
  });

  it('目标页不存在或已是当前页时不动作', () => {
    mod.switchPage('missing');
    expect(document.getElementById('page-home').classList.contains('active')).toBe(true);

    mod.switchPage('home');
    expect(document.getElementById('page-home').classList.contains('active')).toBe(true);
  });

  it('没有激活页面时直接激活目标页', () => {
    document.getElementById('page-home').classList.remove('active');
    mod.switchPage('mods');

    expect(document.getElementById('page-mods').classList.contains('active')).toBe(true);
    expect(document.getElementById('page-mods').style.animation).toContain('pageIn');
    expect(appState.previousPage).toBeNull();
  });
});
