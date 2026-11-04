// @vitest-environment jsdom
// launch-settings.test.js — 窗口尺寸、内存面板、设置保存/重置与主题应用单元测试
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { getApiCalls, getCommandCalls, installTauriMock, mockApiRoute, mockCommand } from '../../test-utils/tauri-mock.js';

let mod;
let appState;

const MEMORY = {
  totalGB: 16,
  usedGB: 8,
  freeGB: 8,
  totalMB: 16384,
  usedMB: 8192,
  freeMB: 8192,
  autoGB: 4,
  autoMB: 4096,
};

function buildForm() {
  document.body.innerHTML = `
    <div id="memory-custom-settings"></div>
    <div id="memory-auto-info"></div>
    <input type="radio" name="globalMemoryMode" value="auto" checked>
    <input type="radio" name="globalMemoryMode" value="custom">
    <input type="range" id="memory-slider" min="1024" max="16384" value="4096">
    <div id="memory-value-display"></div>
    <div id="memory-slider-fill"></div>
    <div id="memory-used-marker"></div>
    <div id="memory-used-label"></div>
    <div id="allocated-memory-display"></div>
    <div id="remaining-memory-display"></div>
    <div id="sys-total-memory"></div>
    <div id="sys-used-memory"></div>
    <div id="sys-free-memory"></div>
    <div id="sys-memory-bar"></div>
    <div id="memory-auto-value"></div>
    <div id="memory-slider-max"></div>
    <div id="advanced-options-content" style="display:none"></div>
    <div id="advanced-options-arrow"></div>
    <select id="window-size">
      <option value="default">默认</option>
      <option value="854x480">854x480</option>
      <option value="1600x900">1600x900</option>
      <option value="custom">自定义</option>
    </select>
    <div id="custom-window-size" style="display:none">
      <input id="custom-width" value="" />
      <input id="custom-height" value="" />
    </div>
    <select id="launch-version-isolation"><option value="all"></option><option value="none"></option></select>
    <input id="launch-window-title" />
    <input id="launch-custom-info" />
    <select id="launcher-visibility"><option value="keep"></option><option value="hide"></option></select>
    <select id="process-priority"><option value="normal"></option><option value="high"></option></select>
    <input type="checkbox" id="launch-fullscreen" />
    <select id="game-java-select"><option value="auto"></option><option value="/jdk"></option></select>
    <input id="jvm-args" />
    <input id="game-args" />
    <input id="pre-launch-command" />
    <select id="memory-management"><option value="default"></option><option value="aggressive"></option></select>
    <input type="checkbox" id="disable-java-wrapper" />
    <input type="checkbox" id="disable-lwjgl-agent" />
    <input type="checkbox" id="use-high-performance-gpu" />
    <input type="checkbox" id="performance-boost" />
    <input type="checkbox" id="jvm-preheat" />
    <input type="checkbox" id="enable-cds" />
    <input type="checkbox" id="setting-minimize-on-game-run" />
    <input id="setting-data-dir" value="" />
    <div id="app"></div>
    <div class="theme-option" data-theme="dark"></div>
    <div class="theme-option" data-theme="light"></div>
    <div id="toast-container"></div>
  `;
}

beforeAll(async () => {
  installTauriMock();
  vi.resetModules();
  appState = (await import('../../core/app-state.js')).appState;
  mod = await import('./launch-settings.js');
});

beforeEach(() => {
  installTauriMock();
  buildForm();
  appState.launchVersionCustomSelect = { getValue: () => '1.20.1' };
  mockApiRoute('/api/system/memory', MEMORY);
  mockApiRoute('/api/settings', { ok: true });
  mockApiRoute('/api/settings/data-dir', { ok: true, message: '已切换' });
  mockApiRoute('/api/jvm/optimize-args', { args: '-Xmx4G -XX:+UseG1GC', ramGB: 4, modCount: 12 });
  document.documentElement.removeAttribute('data-theme');
  document.documentElement.className = '';
  document.documentElement.style.cssText = '';
});

afterEach(() => {
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  document.body.innerHTML = '';
});

describe('resolveWindowSizeValue', () => {
  it('没有选择控件时回退默认分辨率', () => {
    document.body.innerHTML = '';
    expect(mod.resolveWindowSizeValue()).toBe('854x480');
  });

  it('预设分辨率原样返回', () => {
    const select = document.getElementById('window-size');
    select.value = '1600x900';
    expect(mod.resolveWindowSizeValue()).toBe('1600x900');
  });

  it('自定义尺寸使用宽高输入框', () => {
    document.getElementById('window-size').value = 'custom';
    document.getElementById('custom-width').value = '1600';
    document.getElementById('custom-height').value = '900';
    expect(mod.resolveWindowSizeValue()).toBe('1600x900');
  });

  it('自定义尺寸缺失时回退 1920x1080', () => {
    document.getElementById('window-size').value = 'custom';
    expect(mod.resolveWindowSizeValue()).toBe('1920x1080');
  });

  it('default 使用默认值', () => {
    document.getElementById('window-size').value = 'default';
    expect(mod.resolveWindowSizeValue()).toBe('854x480');
  });
});

describe('toggleMemoryMode / 内存展示', () => {
  it('切换到自定义模式显示滑块区域', () => {
    document.querySelector('input[name="globalMemoryMode"][value="custom"]').checked = true;
    mod.toggleMemoryMode();

    expect(document.getElementById('memory-custom-settings').style.display).toBe('block');
    expect(document.getElementById('memory-auto-info').style.display).toBe('none');
    expect(document.getElementById('memory-value-display').textContent).toBe('4096 MB (4.0 GB)');
  });

  it('自动模式显示自动分配信息并隐藏自定义区域', () => {
    document.querySelector('input[name="globalMemoryMode"][value="auto"]').checked = true;
    mod.toggleMemoryMode();

    expect(document.getElementById('memory-custom-settings').style.display).toBe('none');
    expect(document.getElementById('memory-auto-info').style.display).toBe('block');
  });

  it('滑块值换算显示文本与进度条', () => {
    const slider = document.getElementById('memory-slider');
    slider.value = '8192';
    mod.updateMemoryDisplay();

    expect(document.getElementById('memory-value-display').textContent).toBe('8192 MB (8.0 GB)');
    expect(document.getElementById('memory-slider-fill').style.width).toBe('46.666666666666664%');
  });

  it('小内存值只显示 MB', () => {
    const slider = document.getElementById('memory-slider');
    slider.min = '0';
    slider.value = '512';
    mod.updateMemoryDisplay();
    expect(document.getElementById('memory-value-display').textContent).toBe('512 MB');
  });

  it('分配与剩余内存按模式计算', async () => {
    await mod.updateSystemMemoryInfo();
    mod.updateAllocatedMemoryDisplay();
    expect(document.getElementById('allocated-memory-display').textContent).toBe('4.0 GB');
    expect(document.getElementById('remaining-memory-display').textContent).toBe('12.0 GB');

    document.querySelector('input[name="globalMemoryMode"][value="custom"]').checked = true;
    document.getElementById('memory-slider').value = '8192';
    mod.updateAllocatedMemoryDisplay();
    expect(document.getElementById('allocated-memory-display').textContent).toBe('8.0 GB');
    expect(document.getElementById('remaining-memory-display').textContent).toBe('8.0 GB');
  });
});

describe('updateSystemMemoryInfo', () => {
  it('写入系统内存信息、进度条与滑块上限', async () => {
    await mod.updateSystemMemoryInfo();

    expect(document.getElementById('sys-total-memory').textContent).toBe('16 GB');
    expect(document.getElementById('sys-used-memory').textContent).toBe('8 GB');
    expect(document.getElementById('sys-free-memory').textContent).toBe('8 GB');
    expect(document.getElementById('sys-memory-bar').style.width).toBe('50%');
    expect(document.getElementById('sys-memory-bar').style.background).toBe('var(--accent)');
    expect(document.getElementById('memory-auto-value').textContent).toBe('4 GB');
    expect(document.getElementById('memory-slider').max).toBe('16384');
    expect(document.getElementById('memory-slider-max').textContent).toBe('16384 MB');
    expect(document.getElementById('memory-used-marker').style.display).toBe('block');
    expect(document.getElementById('memory-used-label').textContent).toBe('已用 8.0 GB');
  });

  it('高占用时进度条变红', async () => {
    mockApiRoute('/api/system/memory', { ...MEMORY, usedMB: 15000 });
    await mod.updateSystemMemoryInfo();
    expect(document.getElementById('sys-memory-bar').style.background).toBe('rgb(255, 77, 77)');
  });

  it('中高占用时进度条变橙', async () => {
    mockApiRoute('/api/system/memory', { ...MEMORY, usedMB: 11000 });
    await mod.updateSystemMemoryInfo();
    expect(document.getElementById('sys-memory-bar').style.background).toBe('rgb(255, 152, 0)');
  });

  it('滑块当前值超过总内存时回退自动值', async () => {
    document.getElementById('memory-slider').value = '16384';
    mockApiRoute('/api/system/memory', { ...MEMORY, totalMB: 8192, totalGB: 8, autoMB: 4096, autoGB: 4 });
    await mod.updateSystemMemoryInfo();
    expect(document.getElementById('memory-slider').value).toBe('4096');
  });

  it('接口失败时记录错误', async () => {
    const errSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
    mockApiRoute('/api/system/memory', { status: 500, body: {} });
    await mod.updateSystemMemoryInfo();
    expect(errSpy).toHaveBeenCalled();
  });
});

describe('toggleAdvancedOptions', () => {
  it('展开与收起高级选项并旋转箭头', () => {
    mod.toggleAdvancedOptions();
    expect(document.getElementById('advanced-options-content').style.display).toBe('block');
    expect(document.getElementById('advanced-options-arrow').style.transform).toBe('rotate(180deg)');

    mod.toggleAdvancedOptions();
    expect(document.getElementById('advanced-options-content').style.display).toBe('none');
    expect(document.getElementById('advanced-options-arrow').style.transform).toBe('rotate(0)');
  });
});

describe('saveLaunchSettings', () => {
  it('按表单内容保存默认窗口尺寸', async () => {
    document.getElementById('launch-window-title').value = '我的世界';
    document.getElementById('jvm-args').value = '-Xmx2G';
    document.getElementById('launch-fullscreen').checked = true;

    await mod.saveLaunchSettings();

    const call = getCommandCalls().find((c) => c.cmd === 'store_set' && c.args.key === 'versepc_launch_settings');
    const saved = JSON.parse(call.args.value);
    expect(saved.windowSize).toBe('854x480');
    expect(saved.windowTitle).toBe('我的世界');
    expect(saved.jvmArgs).toBe('-Xmx2G');
    expect(saved.fullscreen).toBe(true);
    expect(document.getElementById('toast-container').textContent).toContain('启动设置已保存');
  });

  it('自定义窗口尺寸写入宽高', async () => {
    document.getElementById('window-size').value = 'custom';
    document.getElementById('custom-width').value = '1366';
    document.getElementById('custom-height').value = '768';

    await mod.saveLaunchSettings();

    const call = getCommandCalls().find((c) => c.cmd === 'store_set' && c.args.key === 'versepc_launch_settings');
    expect(JSON.parse(call.args.value).windowSize).toBe('1366x768');
  });

  it('版本隔离写回后端 settings.json', async () => {
    mockApiRoute('/api/settings/set', { success: true });
    document.getElementById('launch-version-isolation').value = 'none';

    await mod.saveLaunchSettings();

    const call = getApiCalls().find((c) => c.path === '/api/settings/set');
    expect(call).toBeTruthy();
    expect(call.args.body).toEqual({ key: 'versionIsolation', value: false });
  });

  it('保存失败时提示错误', async () => {
    mockCommand('store_set', () => { throw new Error('磁盘只读'); });
    await mod.saveLaunchSettings();
    expect(document.getElementById('toast-container').textContent).toContain('保存失败: 磁盘只读');
  });
});

describe('loadLaunchSettings', () => {
  it('版本隔离以后端 settings.json 为准', async () => {
    mockApiRoute('/api/settings', { versionIsolation: false });
    await mod.loadLaunchSettings();
    expect(document.getElementById('launch-version-isolation').value).toBe('none');
  });
});

describe('saveWindowSizeOnly', () => {
  it('合并已有设置后写入窗口尺寸', async () => {
    mockCommand('store_get', ({ key }) => (key === 'versepc_launch_settings' ? JSON.stringify({ performanceBoost: true }) : null));
    document.getElementById('window-size').value = '1600x900';

    await mod.saveWindowSizeOnly();

    const call = getCommandCalls().find((c) => c.cmd === 'store_set' && c.args.key === 'versepc_launch_settings');
    expect(JSON.parse(call.args.value)).toEqual({ performanceBoost: true, windowSize: '1600x900' });
  });

  it('损坏的 JSON 不写入并记录警告', async () => {
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
    mockCommand('store_get', () => '{bad');
    await mod.saveWindowSizeOnly();
    expect(warnSpy).toHaveBeenCalled();
    expect(getCommandCalls().some((c) => c.cmd === 'store_set')).toBe(false);
  });

  it('窗口尺寸变化事件即时自动保存', async () => {
    const select = document.getElementById('window-size');
    select.value = 'custom';
    select.dispatchEvent(new Event('change', { bubbles: true }));

    await vi.waitFor(() => {
      expect(getCommandCalls().some((c) => c.cmd === 'store_set' && c.args.key === 'versepc_launch_settings')).toBe(true);
    });
    expect(document.getElementById('custom-window-size').style.display).toBe('flex');
  });
});

describe('browseDataDir / resetDataDir', () => {
  it('选择目录后保存并展示提示', async () => {
    const alertSpy = vi.spyOn(window, 'alert').mockImplementation(() => {});
    mockCommand('dialog_open', () => ({ filePaths: ['E:/data'] }));
    document.getElementById('setting-data-dir').value = 'E:/old';

    await mod.browseDataDir();

    const call = getApiCalls().find((c) => c.path === '/api/settings/data-dir');
    expect(call.args.body.dataDir).toBe('E:/data');
    expect(document.getElementById('setting-data-dir').value).toBe('E:/data');
    expect(alertSpy).toHaveBeenCalledWith('已切换');
  });

  it('取消选择时不做修改', async () => {
    mockCommand('dialog_open', () => ({ filePaths: [] }));
    document.getElementById('setting-data-dir').value = 'E:/old';
    await mod.browseDataDir();
    expect(document.getElementById('setting-data-dir').value).toBe('E:/old');
    expect(getApiCalls().some((c) => c.path === '/api/settings/data-dir')).toBe(false);
  });

  it('保存失败时提示后端错误', async () => {
    const alertSpy = vi.spyOn(window, 'alert').mockImplementation(() => {});
    mockCommand('dialog_open', () => ({ filePaths: ['E:/data'] }));
    mockApiRoute('/api/settings/data-dir', { ok: false, error: '无权限' });
    await mod.browseDataDir();
    expect(alertSpy).toHaveBeenCalledWith('保存失败: 无权限');
  });

  it('弹窗异常时提示保存失败', async () => {
    const alertSpy = vi.spyOn(window, 'alert').mockImplementation(() => {});
    mockCommand('dialog_open', () => { throw new Error('弹窗不可用'); });
    await mod.browseDataDir();
    expect(alertSpy).toHaveBeenCalledWith('保存失败: 弹窗不可用');
  });

  it('重置数据目录会清空输入', async () => {
    const alertSpy = vi.spyOn(window, 'alert').mockImplementation(() => {});
    document.getElementById('setting-data-dir').value = 'E:/data';
    mockApiRoute('/api/settings/data-dir', { ok: true, message: '已重置' });

    await mod.resetDataDir();

    expect(document.getElementById('setting-data-dir').value).toBe('');
    expect(alertSpy).toHaveBeenCalledWith('已重置');
  });

  it('重置异常时提示', async () => {
    const alertSpy = vi.spyOn(window, 'alert').mockImplementation(() => {});
    mockApiRoute('/api/settings/data-dir', { status: 500, body: {} });
    await mod.resetDataDir();
    expect(alertSpy).toHaveBeenCalledWith(expect.stringContaining('重置失败'));
  });
});

describe('resetLaunchSettings', () => {
  it('确认后恢复默认值并同步存储', async () => {
    document.getElementById('launch-window-title').value = 'x';
    document.getElementById('jvm-args').value = '-Xmx8G';
    document.querySelector('input[name="globalMemoryMode"][value="custom"]').checked = true;
    document.getElementById('memory-slider').value = '8192';

    const pending = mod.resetLaunchSettings();
    await vi.waitFor(() => {
      expect(document.querySelector('.confirm-ok')).toBeTruthy();
    });
    document.querySelector('.confirm-ok').click();
    await pending;

    expect(document.getElementById('launch-version-isolation').value).toBe('all');
    expect(document.getElementById('launch-window-title').value).toBe('');
    expect(document.getElementById('launch-custom-info').value).toBe('VersePC2');
    expect(document.getElementById('window-size').value).toBe('default');
    expect(document.getElementById('jvm-args').value).toBe('');
    expect(document.getElementById('memory-slider').value).toBe('4096');
    expect(document.getElementById('jvm-preheat').checked).toBe(false);
    expect(document.querySelector('input[name="globalMemoryMode"][value="auto"]').checked).toBe(true);
    expect(getApiCalls().some((c) => c.path === '/api/settings')).toBe(true);
    expect(getCommandCalls().some((c) => c.cmd === 'store_set' && c.args.key === 'minimizeOnGameRun')).toBe(true);
    expect(document.getElementById('toast-container').textContent).toContain('启动设置已重置');
  });

  it('取消时不修改表单', async () => {
    document.getElementById('launch-window-title').value = 'keep';
    const pending = mod.resetLaunchSettings();
    await vi.waitFor(() => {
      expect(document.querySelector('.confirm-cancel')).toBeTruthy();
    });
    document.querySelector('.confirm-cancel').click();
    await pending;

    expect(document.getElementById('launch-window-title').value).toBe('keep');
    expect(getApiCalls().some((c) => c.path === '/api/settings')).toBe(false);
  });
});

describe('optimizeJvmArgs', () => {
  it('未选择版本时提示', async () => {
    appState.launchVersionCustomSelect = { getValue: () => '' };
    await mod.optimizeJvmArgs();
    expect(document.getElementById('toast-container').textContent).toContain('请先选择一个游戏版本');
  });

  it('成功时写入优化参数与提示', async () => {
    await mod.optimizeJvmArgs();
    expect(document.getElementById('jvm-args').value).toBe('-Xmx4G -XX:+UseG1GC');
    expect(document.getElementById('toast-container').textContent).toContain('分配 4GB 内存');
  });

  it('失败时提示优化失败', async () => {
    mockApiRoute('/api/jvm/optimize-args', { status: 500, body: {} });
    await mod.optimizeJvmArgs();
    expect(document.getElementById('toast-container').textContent).toContain('优化失败');
  });
});

describe('loadLaunchSettings', () => {
  it('恢复预设窗口尺寸与各开关', async () => {
    vi.useFakeTimers();
    mockCommand('store_get', ({ key }) => {
      if (key === 'versepc_launch_settings') {
        return JSON.stringify({
          versionIsolation: 'none',
          windowTitle: 'Title',
          customInfo: 'Info',
          windowSize: '1600x900',
          fullscreen: true,
          gameJava: '/jdk',
          memoryMode: 'custom',
          memoryValue: '8192',
          jvmArgs: '-Xmx6G',
          jvmPreheat: true,
          enableCds: false,
        });
      }
      return null;
    });

    await mod.loadLaunchSettings();
    await vi.advanceTimersByTimeAsync(0);

    expect(document.getElementById('launch-version-isolation').value).toBe('none');
    expect(document.getElementById('launch-window-title').value).toBe('Title');
    expect(document.getElementById('launch-custom-info').value).toBe('Info');
    expect(document.getElementById('window-size').value).toBe('1600x900');
    expect(document.getElementById('custom-window-size').style.display).toBe('none');
    expect(document.getElementById('launch-fullscreen').checked).toBe(true);
    expect(document.getElementById('game-java-select').value).toBe('/jdk');
    expect(document.querySelector('input[name="globalMemoryMode"][value="custom"]').checked).toBe(true);
    expect(document.getElementById('memory-slider').value).toBe('8192');
    expect(document.getElementById('jvm-args').value).toBe('-Xmx6G');
    expect(document.getElementById('jvm-preheat').checked).toBe(true);
    expect(document.getElementById('enable-cds').checked).toBe(false);
    expect(document.getElementById('setting-minimize-on-game-run').checked).toBe(true);
  });

  it('自定义分辨率回填宽高输入框', async () => {
    mockCommand('store_get', ({ key }) => (
      key === 'versepc_launch_settings' ? JSON.stringify({ windowSize: '1366x768' }) : null
    ));

    await mod.loadLaunchSettings();

    expect(document.getElementById('window-size').value).toBe('custom');
    expect(document.getElementById('custom-window-size').style.display).toBe('flex');
    expect(document.getElementById('custom-width').value).toBe('1366');
    expect(document.getElementById('custom-height').value).toBe('768');
  });

  it('非分辨率格式直接回填下拉值', async () => {
    mockCommand('store_get', ({ key }) => (
      key === 'versepc_launch_settings' ? JSON.stringify({ windowSize: 'default' }) : null
    ));

    await mod.loadLaunchSettings();

    expect(document.getElementById('window-size').value).toBe('default');
    expect(document.getElementById('custom-window-size').style.display).toBe('none');
  });

  it('低调模式开关变更写回存储', async () => {
    mockCommand('store_get', ({ key }) => (key === 'minimizeOnGameRun' ? false : null));
    await mod.loadLaunchSettings();

    const cb = document.getElementById('setting-minimize-on-game-run');
    expect(cb.checked).toBe(false);
    cb.checked = true;
    cb.dispatchEvent(new Event('change'));

    await vi.waitFor(() => {
      expect(getCommandCalls().some((c) => c.cmd === 'store_set' && c.args.key === 'minimizeOnGameRun' && c.args.value === true)).toBe(true);
    });
  });
});

describe('applyThemeByName / selectTheme', () => {
  it('深色主题写入根属性与强调色变量', async () => {
    await mod.applyThemeByName('dark');

    expect(document.documentElement.getAttribute('data-theme')).toBe('dark');
    expect(document.documentElement.classList.contains('dark-theme')).toBe(true);
    expect(document.documentElement.style.getPropertyValue('--accent')).toBe('#ffffff');
    expect(document.documentElement.style.getPropertyValue('--accent-text')).toBe('#0a0a0a');
    const saved = getCommandCalls().find((c) => c.cmd === 'store_set' && c.args.key === 'versepc_theme');
    expect(saved.args.value).toBe('dark');
  });

  it('浅色主题写入浅色强调色并清理壁纸明暗类', async () => {
    const app = document.getElementById('app');
    app.classList.add('wp-light', 'wp-dark');

    await mod.applyThemeByName('light');

    expect(document.documentElement.getAttribute('data-theme')).toBe('light');
    expect(document.documentElement.classList.contains('light-theme')).toBe(true);
    expect(app.classList.contains('wp-light')).toBe(false);
    expect(app.classList.contains('wp-dark')).toBe(false);
    expect(document.documentElement.style.getPropertyValue('--accent')).toBe('#1a1a1a');
  });

  it('selectTheme 更新选中态并应用主题', async () => {
    const options = document.querySelectorAll('.theme-option');
    options[0].classList.add('active');

    await mod.selectTheme(options[1]);

    expect(options[0].classList.contains('active')).toBe(false);
    expect(options[1].classList.contains('active')).toBe(true);
    expect(document.documentElement.getAttribute('data-theme')).toBe('light');
  });

  it('自定义主题读取已保存颜色', async () => {
    mockCommand('store_get', ({ key }) => {
      if (key === 'versepc_custom_theme_color') return '#123456';
      return null;
    });

    await mod.applyThemeByName('custom');

    expect(document.documentElement.getAttribute('data-theme')).toBe('custom');
    expect(document.documentElement.classList.contains('dark-theme')).toBe(true);
  });

  it('跟随系统：深色系统主题解析为 dark 并保存 auto', async () => {
    const media = {
      matches: true,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    };
    vi.stubGlobal('matchMedia', vi.fn(() => media));

    await mod.applyThemeByName('auto');

    expect(document.documentElement.getAttribute('data-theme')).toBe('dark');
    expect(document.documentElement.classList.contains('dark-theme')).toBe(true);
    expect(media.addEventListener).toHaveBeenCalledWith('change', expect.any(Function));
    const saved = getCommandCalls().find((c) => c.cmd === 'store_set' && c.args.key === 'versepc_theme');
    expect(saved.args.value).toBe('auto');
  });

  it('跟随系统：浅色系统主题解析为 light', async () => {
    vi.stubGlobal('matchMedia', vi.fn(() => ({
      matches: false,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    })));

    await mod.applyThemeByName('auto');

    expect(document.documentElement.getAttribute('data-theme')).toBe('light');
    expect(document.documentElement.classList.contains('light-theme')).toBe(true);
    expect(document.documentElement.style.getPropertyValue('--accent')).toBe('#1a1a1a');
  });

  it('跟随系统：系统主题变化时自动重新应用', async () => {
    let handler = null;
    const media = {
      matches: true,
      addEventListener: (_name, cb) => { handler = cb; },
      removeEventListener: vi.fn(),
    };
    vi.stubGlobal('matchMedia', vi.fn(() => media));
    await mod.applyThemeByName('auto');

    media.matches = false;
    handler();
    await Promise.resolve();
    await Promise.resolve();

    expect(document.documentElement.getAttribute('data-theme')).toBe('light');
  });
});
