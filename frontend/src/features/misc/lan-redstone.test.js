// @vitest-environment jsdom
// lan-redstone.test.js — 红石联机节点、隧道启停与主流程单元测试
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { getApiCalls, getCommandCalls, installTauriMock, mockApiRoute, mockCommand } from '../../test-utils/tauri-mock.js';

const { toast } = vi.hoisted(() => ({ toast: vi.fn() }));
vi.mock('./ui-components.js', () => ({ showToast: toast }));

let mod;
let S;
let featureBridge;

function buildDom() {
  document.body.innerHTML = `
    <span id="redstone-status-dot"></span>
    <span id="redstone-status-text"></span>
    <div class="redstone-tabs">
      <button class="lan-tab" data-redstone-tab="host"></button>
      <button class="lan-tab" data-redstone-tab="join"></button>
    </div>
    <div class="redstone-tab-content" id="redstone-tab-host"></div>
    <div class="redstone-tab-content" id="redstone-tab-join"></div>
    <button id="redstone-server-btn"></button>
    <div id="redstone-server-info"></div>
    <button id="redstone-action-btn"></button>
    <input id="redstone-max-players">
    <div id="redstone-current-version"></div>
    <div id="redstone-connected-info" style="display:none">
      <span id="redstone-room-addr"></span>
    </div>
    <pre id="redstone-room-log"></pre>
    <div id="redstone-onboarding">
      <div id="redstone-ob-dl"></div>
      <div id="redstone-ob-spinner" style="display:none"></div>
      <div id="redstone-ob-done" style="display:none"></div>
      <div id="redstone-ob-title"></div>
      <div id="redstone-ob-desc"></div>
    </div>
    <div id="redstone-main" style="display:none"></div>
    <div id="redstone-home"></div>
    <div id="redstone-host-panel" style="display:none">
      <div id="redstone-host-step1"></div>
      <div id="redstone-host-step2"></div>
      <div id="redstone-host-step3"></div>
      <button id="redstone-host-next"></button>
    </div>
    <div id="redstone-join-panel" style="display:none"></div>
    <div id="redstone-hint" style="display:none"></div>
  `;
}

function resetS() {
  S.redstoneGamePoll = null;
  S._redstoneServers = [];
  S._redstoneRunning = false;
  S._redstoneServerIdx = 0;
  delete featureBridge.redstoneSelectedNode;
  delete featureBridge.redstoneNodes;
}

function clearTimers() {
  if (S.redstoneGamePoll) clearInterval(S.redstoneGamePoll);
  S.redstoneGamePoll = null;
}

function resetBridge() {
  mockCommand('redstone_servers', () => ({ ok: true, servers: [{ name: '南京', address: 'nj.hongshi.site' }] }));
  mockCommand('redstone_scan_port', () => ({ ok: true, port: 25577 }));
  mockCommand('redstone_start', () => ({ ok: true, address: 'frp.example.com:25577' }));
  mockCommand('redstone_stop', () => ({ ok: true }));
  mockCommand('redstone_status', () => ({ running: false }));
}

function commandCalls(cmd) {
  return getCommandCalls().filter((call) => call.cmd === cmd);
}

function logText() {
  return document.getElementById('redstone-room-log').textContent;
}

async function settle() {
  await Promise.resolve();
  await Promise.resolve();
  await vi.advanceTimersByTimeAsync(0);
}

beforeAll(async () => {
  installTauriMock();
  vi.resetModules();
  S = (await import('./lan-state.js')).S;
  featureBridge = (await import('../../core/feature-bridge.js')).featureBridge;
  mod = await import('./lan-redstone.js');
});

beforeEach(() => {
  installTauriMock();
  mockApiRoute('/api/current-context', { selectedVersion: '1.20.1', loader: 'forge', loaderVersion: '47.2.0' });
  resetBridge();
  buildDom();
  resetS();
  localStorage.clear();
  toast.mockClear();
  vi.useFakeTimers();
  Object.defineProperty(navigator, 'clipboard', {
    value: { writeText: vi.fn(() => Promise.resolve()) },
    configurable: true,
  });
  vi.stubGlobal('alert', vi.fn());
  vi.stubGlobal('fetch', vi.fn(async () => ({
    ok: true,
    json: async () => ({ selectedVersion: '1.20.1', loader: 'forge', loaderVersion: '47.2.0' }),
  })));
});

afterEach(() => {
  clearTimers();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  document.body.innerHTML = '';
});

describe('updateRedstoneStatus', () => {
  it('更新文案与连接状态点', () => {
    mod.updateRedstoneStatus('隧道已开启', 'connected');

    expect(document.getElementById('redstone-status-text').textContent).toBe('隧道已开启');
    expect(document.getElementById('redstone-status-dot').classList.contains('connected')).toBe(true);
  });

  it('连接中与断开状态类互斥', () => {
    mod.updateRedstoneStatus('正在连接...', 'connecting');
    expect(document.getElementById('redstone-status-dot').className).toBe('lan-status-dot connecting');

    mod.updateRedstoneStatus('未连接', 'disconnected');
    expect(document.getElementById('redstone-status-dot').className).toBe('lan-status-dot disconnected');
  });

  it('元素缺失时安全返回', () => {
    document.getElementById('redstone-status-dot').remove();
    document.getElementById('redstone-status-text').remove();

    expect(() => mod.updateRedstoneStatus('x', 'connected')).not.toThrow();
  });
});

describe('redstoneSwitchTab', () => {
  it('切换高亮与内容显示', () => {
    mod.redstoneSwitchTab('join');

    const tabs = [...document.querySelectorAll('.redstone-tabs .lan-tab')];
    expect(tabs[0].classList.contains('active')).toBe(false);
    expect(tabs[1].classList.contains('active')).toBe(true);
    expect(document.getElementById('redstone-tab-host').style.display).toBe('none');
    expect(document.getElementById('redstone-tab-join').style.display).toBe('block');
  });
});

describe('redstoneRefreshServers', () => {
  it('加载节点列表并选中首项', async () => {
    mockCommand('redstone_servers', () => ({ ok: true, servers: [
      { name: '上海', address: 'sh.example.com' },
      { name: '北京', address: 'bj.example.com' },
    ] }));

    await mod.redstoneRefreshServers();

    expect(document.getElementById('redstone-server-info').textContent).toBe('共 2 个节点');
    expect(document.getElementById('redstone-server-btn').textContent).toBe('服务器: 上海 (sh.example.com)');
    expect(S._redstoneServers).toHaveLength(2);
    expect(S._redstoneServerIdx).toBe(0);
  });

  it('节点列表为空时回退默认节点', async () => {
    mockCommand('redstone_servers', () => ({ ok: true, servers: [] }));

    await mod.redstoneRefreshServers();

    expect(document.getElementById('redstone-server-info').textContent).toBe('节点列表为空（使用默认节点）');
    expect(S._redstoneServers).toEqual([{ name: '南京', address: 'nj.hongshi.site' }]);
    expect(document.getElementById('redstone-server-btn').textContent).toBe('服务器: 南京 (nj.hongshi.site)');
  });

  it('接口失败时展示错误并回退默认节点', async () => {
    mockCommand('redstone_servers', () => { throw new Error('offline'); });

    await mod.redstoneRefreshServers();

    expect(document.getElementById('redstone-server-info').textContent).toBe('加载失败: offline');
    expect(S._redstoneServers[0].name).toBe('南京');
  });

  it('按钮缺失时仍可加载数据', async () => {
    document.getElementById('redstone-server-btn').remove();
    document.getElementById('redstone-server-info').remove();

    await mod.redstoneRefreshServers();

    expect(S._redstoneServers).toHaveLength(1);
  });
});

describe('updateServerBtn / redstoneCycleServer', () => {
  it('索引取模显示服务器', () => {
    S._redstoneServers = [{ name: 'A', address: 'a:1' }, { name: 'B', address: 'b:2' }];
    S._redstoneServerIdx = 3;

    mod.updateServerBtn();

    expect(document.getElementById('redstone-server-btn').textContent).toBe('服务器: B (b:2)');
  });

  it('无按钮或无节点时安全返回', () => {
    S._redstoneServers = [];
    expect(() => mod.updateServerBtn()).not.toThrow();

    document.getElementById('redstone-server-btn').remove();
    S._redstoneServers = [{ name: 'A', address: 'a:1' }];
    expect(() => mod.updateServerBtn()).not.toThrow();
  });

  it('循环切换服务器并记录日志', () => {
    S._redstoneServers = [{ name: 'A', address: 'a:1' }, { name: 'B', address: 'b:2' }];

    mod.redstoneCycleServer();

    expect(S._redstoneServerIdx).toBe(1);
    expect(document.getElementById('redstone-server-btn').textContent).toBe('服务器: B (b:2)');
    expect(logText()).toContain('切换到服务器: B');
  });

  it('无节点时不切换', () => {
    S._redstoneServers = [];

    mod.redstoneCycleServer();

    expect(S._redstoneServerIdx).toBe(0);
  });
});

describe('addRedstoneLog / redstoneCopyAddr', () => {
  it('追加带时间戳的日志', () => {
    mod.addRedstoneLog('hello');

    expect(logText()).toMatch(/^\[[^\]]+\] hello\n$/);
  });

  it('日志容器缺失时不报错', () => {
    document.getElementById('redstone-room-log').remove();

    expect(() => mod.addRedstoneLog('x')).not.toThrow();
  });

  it('复制非占位地址', async () => {
    document.getElementById('redstone-room-addr').textContent = 'frp:25577';

    mod.redstoneCopyAddr();
    await settle();

    expect(navigator.clipboard.writeText).toHaveBeenCalledWith('frp:25577');
    expect(logText()).toContain('地址已复制: frp:25577');
  });

  it('占位地址不复制', async () => {
    const el = document.getElementById('redstone-room-addr');

    el.textContent = '--';
    mod.redstoneCopyAddr();
    await settle();
    el.textContent = '';
    mod.redstoneCopyAddr();
    await settle();

    expect(navigator.clipboard.writeText).not.toHaveBeenCalled();
  });
});

describe('redstoneStart', () => {
  it('节点未加载时阻止启动', async () => {
    S._redstoneServers = [];

    await mod.redstoneStart();

    expect(window.alert).toHaveBeenCalledWith('请先等待节点列表加载');
    expect(commandCalls('redstone_start')).toHaveLength(0);
  });

  it('游戏未运行且端口扫描失败时提示', async () => {
    S._redstoneServers = [{ name: '南京', address: 'nj.hongshi.site' }];
    mockApiRoute('/api/game/status', { running: false });
    mockCommand('redstone_scan_port', () => ({ ok: false }));

    await mod.redstoneStart();

    expect(logText()).toContain('错误: 未找到 Minecraft 游戏进程');
    expect(document.getElementById('redstone-status-text').textContent).toBe('游戏未运行');
    expect(toast).toHaveBeenCalledWith('未找到 Minecraft 游戏进程，请先启动游戏', 'error');
    expect(commandCalls('redstone_start')).toHaveLength(0);
  });

  it('游戏未运行时扫描端口后继续启动', async () => {
    S._redstoneServers = [{ name: '南京', address: 'nj.hongshi.site' }];
    mockApiRoute('/api/game/status', { running: false });

    await mod.redstoneStart();

    expect(logText()).toContain('端口扫描成功: 25577');
    const calls = commandCalls('redstone_start');
    expect(calls).toHaveLength(1);
    expect(calls[0].args.params).toEqual({
      serverAddress: 'nj.hongshi.site',
      gamePort: 25577,
      maxPlayers: '',
    });
  });

  it('游戏运行但未开放局域网时扫描端口', async () => {
    S._redstoneServers = [{ name: '南京', address: 'nj.hongshi.site' }];
    mockApiRoute('/api/game/status', { running: true });
    document.getElementById('redstone-max-players').value = '8';

    await mod.redstoneStart();

    expect(logText()).toContain('未检测到局域网端口，正在扫描...');
    expect(commandCalls('redstone_start')[0].args.params).toEqual({
      serverAddress: 'nj.hongshi.site',
      gamePort: 25577,
      maxPlayers: '8',
    });
  });

  it('游戏运行但未开放局域网且扫描失败时提示', async () => {
    S._redstoneServers = [{ name: '南京', address: 'nj.hongshi.site' }];
    mockApiRoute('/api/game/status', { running: true });
    mockCommand('redstone_scan_port', () => ({ ok: false }));

    await mod.redstoneStart();

    expect(document.getElementById('redstone-status-text').textContent).toBe('未开放局域网');
    expect(toast).toHaveBeenCalledWith('请在游戏内按 Esc → 对局域网开放，然后才能开启隧道', 'error');
  });

  it('优先使用游戏报告的局域网端口并展示版本描述', async () => {
    S._redstoneServers = [{ name: '南京', address: 'nj.hongshi.site' }];
    mockApiRoute('/api/game/status', { running: true, lanPort: 25566 });

    await mod.redstoneStart();

    expect(commandCalls('redstone_start')[0].args.params.gamePort).toBe(25566);
    expect(document.getElementById('redstone-current-version').textContent).toBe('1.20.1 / Forge 47.2.0');
    expect(document.getElementById('redstone-current-version').style.color).toBe('var(--text-primary)');
    expect(logText()).toContain('检测到局域网端口: 25566');
    expect(logText()).toContain('当前版本: 1.20.1 / Forge 47.2.0');
  });

  it('版本信息缺失时显示未选择', async () => {
    S._redstoneServers = [{ name: '南京', address: 'nj.hongshi.site' }];
    mockApiRoute('/api/game/status', { running: true, lanPort: 25566 });
    mockApiRoute('/api/current-context', {});

    await mod.redstoneStart();

    expect(document.getElementById('redstone-current-version').textContent).toBe('未选择版本');
    expect(document.getElementById('redstone-current-version').style.color).toBe('var(--text-muted)');
  });

  it('版本接口异常时记录日志并继续', async () => {
    S._redstoneServers = [{ name: '南京', address: 'nj.hongshi.site' }];
    mockApiRoute('/api/game/status', { running: true, lanPort: 25566 });
    mockApiRoute('/api/current-context', { status: 500, body: { error: 'bad gateway' } });

    await mod.redstoneStart();

    expect(logText()).toContain('读取当前版本失败: bad gateway');
    expect(commandCalls('redstone_start')).toHaveLength(1);
  });

  it('启动成功后展示地址并复制', async () => {
    S._redstoneServers = [{ name: '南京', address: 'nj.hongshi.site' }];
    mockApiRoute('/api/game/status', { running: true, lanPort: 25566 });

    await mod.redstoneStart();

    expect(S._redstoneRunning).toBe(true);
    expect(document.getElementById('redstone-connected-info').style.display).toBe('');
    expect(document.getElementById('redstone-room-addr').textContent).toBe('frp.example.com:25577');
    expect(document.getElementById('redstone-status-text').textContent).toBe('隧道已开启 | frp.example.com:25577');
    expect(document.getElementById('redstone-action-btn').textContent).toBe('关闭隧道');
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith('frp.example.com:25577');
    expect(logText()).toContain('联机地址已复制到剪贴板');
  });

  it('剪贴板失败不影响启动结果', async () => {
    S._redstoneServers = [{ name: '南京', address: 'nj.hongshi.site' }];
    mockApiRoute('/api/game/status', { running: true, lanPort: 25566 });
    navigator.clipboard.writeText.mockRejectedValue(new Error('denied'));

    await mod.redstoneStart();

    expect(S._redstoneRunning).toBe(true);
    expect(logText()).not.toContain('联机地址已复制到剪贴板');
  });

  it('隧道返回失败时复位按钮', async () => {
    S._redstoneServers = [{ name: '南京', address: 'nj.hongshi.site' }];
    mockApiRoute('/api/game/status', { running: true, lanPort: 25566 });
    mockCommand('redstone_start', () => ({ ok: false, error: 'relay down' }));

    await mod.redstoneStart();

    expect(S._redstoneRunning).toBe(false);
    expect(document.getElementById('redstone-status-text').textContent).toBe('开启失败');
    expect(document.getElementById('redstone-action-btn').textContent).toBe('开启隧道');
    expect(logText()).toContain('开启失败: relay down');
  });

  it('隧道调用异常时复位按钮', async () => {
    S._redstoneServers = [{ name: '南京', address: 'nj.hongshi.site' }];
    mockApiRoute('/api/game/status', { running: true, lanPort: 25566 });
    mockCommand('redstone_start', () => { throw new Error('boom'); });

    await mod.redstoneStart();

    expect(S._redstoneRunning).toBe(false);
    expect(logText()).toContain('开启失败: boom');
  });

  it('游戏状态接口异常时中止', async () => {
    S._redstoneServers = [{ name: '南京', address: 'nj.hongshi.site' }];
    mockApiRoute('/api/game/status', { status: 500, body: { error: 'x' } });

    await mod.redstoneStart();

    expect(logText()).toContain('检查游戏状态失败');
    expect(commandCalls('redstone_start')).toHaveLength(0);
  });
});

describe('redstoneStop / redstoneToggle', () => {
  it('关闭隧道复位状态', async () => {
    S._redstoneRunning = true;
    document.getElementById('redstone-connected-info').style.display = '';

    await mod.redstoneStop();

    expect(commandCalls('redstone_stop')).toHaveLength(1);
    expect(S._redstoneRunning).toBe(false);
    expect(document.getElementById('redstone-connected-info').style.display).toBe('none');
    expect(document.getElementById('redstone-action-btn').textContent).toBe('开启隧道');
    expect(document.getElementById('redstone-status-text').textContent).toBe('未连接');
    expect(logText()).toContain('隧道已关闭');
  });

  it('关闭接口异常仍复位并记录', async () => {
    mockCommand('redstone_stop', () => { throw new Error('gone'); });

    await mod.redstoneStop();

    expect(logText()).toContain('关闭失败: gone');
    expect(S._redstoneRunning).toBe(false);
  });

  it('运行中切换会关闭隧道', async () => {
    S._redstoneRunning = true;

    await mod.redstoneToggle();

    expect(commandCalls('redstone_stop')).toHaveLength(1);
    expect(S._redstoneRunning).toBe(false);
  });

  it('未运行切换会尝试启动', async () => {
    S._redstoneServers = [];

    await mod.redstoneToggle();

    expect(window.alert).toHaveBeenCalledWith('请先等待节点列表加载');
  });
});

describe('redstoneRefreshCurrentVersion', () => {
  it('加载器带版本号时展示短版本', async () => {
    await mod.redstoneRefreshCurrentVersion();

    expect(document.getElementById('redstone-current-version').textContent).toBe('1.20.1 / Forge 47.2.0');
    expect(document.getElementById('redstone-current-version').style.color).toBe('var(--text-primary)');
  });

  it('仅有加载器时省略版本号', async () => {
    mockApiRoute('/api/current-context', { selectedVersion: '1.21', loader: 'fabric' });

    await mod.redstoneRefreshCurrentVersion();

    expect(document.getElementById('redstone-current-version').textContent).toBe('1.21 / Fabric');
  });

  it('无加载器时标记原版', async () => {
    mockApiRoute('/api/current-context', { selectedVersion: '1.21' });

    await mod.redstoneRefreshCurrentVersion();

    expect(document.getElementById('redstone-current-version').textContent).toBe('1.21 / 原版');
  });

  it('接口失败展示读取失败', async () => {
    mockApiRoute('/api/current-context', { status: 502, body: { error: 'bad gateway' } });

    await mod.redstoneRefreshCurrentVersion();

    expect(document.getElementById('redstone-current-version').textContent).toBe('读取失败');
    expect(document.getElementById('redstone-current-version').style.color).toBe('var(--text-muted)');
  });

  it('版本容器缺失时不做请求', async () => {
    document.getElementById('redstone-current-version').remove();

    await mod.redstoneRefreshCurrentVersion();

    expect(getApiCalls().some((c) => c.path === '/api/current-context')).toBe(false);
  });
});

describe('onboarding', () => {
  it('首次进入展示引导并后台加载节点', async () => {
    mod.redstoneInitPage();
    await settle();

    expect(document.getElementById('redstone-onboarding').style.display).toBe('flex');
    expect(document.getElementById('redstone-main').style.display).toBe('none');
    expect(commandCalls('redstone_servers')).toHaveLength(1);
  });

  it('已看过引导直接进入主界面', async () => {
    localStorage.setItem('redstone_onboarding_seen', '1');

    mod.redstoneInitPage();
    await settle();

    expect(document.getElementById('redstone-onboarding').style.display).toBe('none');
    expect(document.getElementById('redstone-main').style.display).toBe('');
    expect(document.getElementById('redstone-status-text').textContent).toBe('未连接');
  });

  it('主进程已有隧道时恢复地址', async () => {
    localStorage.setItem('redstone_onboarding_seen', '1');
    mockCommand('redstone_status', () => ({ running: true, address: '1.2.3.4:25565' }));

    mod.redstoneInitPage();
    await settle();

    expect(document.getElementById('redstone-main').style.display).toBe('');
    expect(document.getElementById('redstone-room-addr').textContent).toBe('1.2.3.4:25565');
  });

  it('状态查询异常时安全返回', async () => {
    localStorage.setItem('redstone_onboarding_seen', '1');
    mockCommand('redstone_status', () => { throw new Error('no'); });

    await expect(mod.redstoneInitPage()).resolves.toBeUndefined();
    expect(document.getElementById('redstone-main').style.display).toBe('');
  });

  it('引导页重置显示与文案', () => {
    const ob = document.getElementById('redstone-onboarding');
    ob.classList.add('terracotta-ob--fadeout');
    document.getElementById('redstone-ob-dl').style.display = 'none';

    mod.showRedstoneOnboarding();

    expect(ob.style.display).toBe('flex');
    expect(ob.classList.contains('terracotta-ob--fadeout')).toBe(false);
    expect(ob.classList.contains('terracotta-ob--enter')).toBe(true);
    expect(document.getElementById('redstone-ob-dl').style.display).toBe('');
    expect(document.getElementById('redstone-ob-spinner').style.display).toBe('none');
    expect(document.getElementById('redstone-ob-title').textContent).toBe('你好！欢迎使用红石联机');
  });

  it('引导容器缺失时安全返回', () => {
    document.getElementById('redstone-onboarding').remove();

    expect(() => mod.showRedstoneOnboarding()).not.toThrow();
    expect(() => mod.showRedstoneMain()).not.toThrow();
  });

  it('显示主界面并回到首页状态', () => {
    document.getElementById('redstone-onboarding').style.display = 'flex';

    mod.showRedstoneMain();

    expect(document.getElementById('redstone-onboarding').style.display).toBe('none');
    expect(document.getElementById('redstone-main').style.display).toBe('');
    expect(document.getElementById('redstone-home').style.display).toBe('');
    expect(document.getElementById('redstone-status-text').textContent).toBe('未连接');
  });

  it('初始化动画结束后写入标记并进入主界面', async () => {
    mod.redstoneStartOnboarding();

    expect(document.getElementById('redstone-ob-dl').style.display).toBe('none');
    expect(document.getElementById('redstone-ob-spinner').style.display).toBe('flex');

    await vi.advanceTimersByTimeAsync(1200);
    expect(document.getElementById('redstone-ob-spinner').style.display).toBe('none');
    expect(document.getElementById('redstone-ob-done').style.display).toBe('block');
    expect(document.getElementById('redstone-ob-done').classList.contains('terracotta-ob-done--show')).toBe(true);

    await vi.advanceTimersByTimeAsync(1500);
    expect(localStorage.getItem('redstone_onboarding_seen')).toBe('1');
    expect(document.getElementById('redstone-onboarding').classList.contains('terracotta-ob--fadeout')).toBe(true);

    await vi.advanceTimersByTimeAsync(450);
    expect(document.getElementById('redstone-main').style.display).toBe('');
  });

  it('初始化控件缺失时安全返回', () => {
    document.getElementById('redstone-ob-dl').remove();

    expect(() => mod.redstoneStartOnboarding()).not.toThrow();
  });
});

describe('host flow', () => {
  it('隐藏与返回首页', () => {
    document.getElementById('redstone-host-panel').style.display = '';
    document.getElementById('redstone-hint').style.display = '';

    mod.redstoneHide();
    expect(document.getElementById('redstone-host-panel').style.display).toBe('none');
    expect(document.getElementById('redstone-hint').style.display).toBe('none');

    mod.redstoneBackToHome();
    expect(document.getElementById('redstone-home').style.display).toBe('');
    expect(document.getElementById('redstone-status-text').textContent).toBe('未连接');
  });

  it('第一步展示主机面板并按游戏状态启用下一步', async () => {
    mockApiRoute('/api/game/status', { running: false });

    await mod.redstoneHostStep1();
    await settle();

    expect(document.getElementById('redstone-home').style.display).toBe('none');
    expect(document.getElementById('redstone-host-panel').style.display).toBe('');
    expect(document.getElementById('redstone-host-step1').style.display).toBe('');
    expect(document.getElementById('redstone-host-next').disabled).toBe(true);
    expect(S.redstoneGamePoll).not.toBeNull();
  });

  it('轮询检测到游戏运行后启用下一步', async () => {
    mockApiRoute('/api/game/status', { running: false });
    await mod.redstoneHostStep1();
    await settle();

    mockApiRoute('/api/game/status', { running: true });
    await vi.advanceTimersByTimeAsync(2000);
    await settle();

    expect(document.getElementById('redstone-host-next').disabled).toBe(false);
    expect(document.getElementById('redstone-host-next').title).toBe('下一步');
  });

  it('游戏未运行时无法下一步', async () => {
    mockApiRoute('/api/game/status', { running: false });

    await mod.redstoneHostNext();

    expect(toast).toHaveBeenCalledWith('请先启动游戏，然后在游戏内开放局域网联机', 'error');
    expect(commandCalls('redstone_start')).toHaveLength(0);
  });

  it('使用共享节点与局域网端口开启隧道', async () => {
    featureBridge.redstoneSelectedNode = { name: '共享节点', address: 'share.example.com' };
    mockApiRoute('/api/game/status', { running: true, lanPort: 25566 });

    await mod.redstoneHostNext();

    expect(commandCalls('redstone_start')[0].args.params).toEqual({
      serverAddress: 'share.example.com',
      gamePort: 25566,
      maxPlayers: '',
    });
    expect(document.getElementById('redstone-host-step3').style.display).toBe('');
    expect(document.getElementById('redstone-room-addr').textContent).toBe('frp.example.com:25577');
    expect(document.getElementById('redstone-hint').textContent).toBe('联机地址已生成，发送给朋友即可加入');
    expect(document.getElementById('redstone-hint').style.display).toBe('');
  });

  it('无共享节点时加载默认节点并扫描端口', async () => {
    mockApiRoute('/api/game/status', { running: true });

    await mod.redstoneHostNext();

    expect(commandCalls('redstone_servers')).toHaveLength(1);
    expect(commandCalls('redstone_start')[0].args.params).toEqual({
      serverAddress: 'nj.hongshi.site',
      gamePort: 25577,
      maxPlayers: '',
    });
  });

  it('端口扫描失败时回落 25565', async () => {
    featureBridge.redstoneSelectedNode = { name: '共享节点', address: 'share.example.com' };
    mockApiRoute('/api/game/status', { running: true });
    mockCommand('redstone_scan_port', () => ({ ok: false }));

    await mod.redstoneHostNext();

    expect(commandCalls('redstone_start')[0].args.params.gamePort).toBe(25565);
  });

  it('节点列表为空时提示失败并回首页', async () => {
    featureBridge.redstoneNodes = [];
    mockApiRoute('/api/game/status', { running: true, lanPort: 25566 });
    mockCommand('redstone_servers', () => ({ ok: true, servers: [] }));
    // 空列表会回退默认节点，这里改为让启动接口失败以触达失败分支
    mockCommand('redstone_start', () => ({ ok: false, error: 'relay down' }));

    await mod.redstoneHostNext();

    expect(toast).toHaveBeenCalledWith('开启隧道失败: relay down', 'error');
    expect(document.getElementById('redstone-home').style.display).toBe('');
  });

  it('开启异常时提示并回首页', async () => {
    featureBridge.redstoneSelectedNode = { name: '共享节点', address: 'share.example.com' };
    mockApiRoute('/api/game/status', { running: true, lanPort: 25566 });
    mockCommand('redstone_start', () => { throw new Error('boom'); });

    await mod.redstoneHostNext();

    expect(toast).toHaveBeenCalledWith('开启隧道失败: boom', 'error');
    expect(document.getElementById('redstone-home').style.display).toBe('');
  });

  it('断开连接关闭隧道并提示', async () => {
    await mod.redstoneDisconnect();

    expect(commandCalls('redstone_stop')).toHaveLength(1);
    expect(document.getElementById('redstone-home').style.display).toBe('');
    expect(toast).toHaveBeenCalledWith('已断开红石联机', 'info');
  });
});
