// @vitest-environment jsdom
// lan-enderlink.test.js — EnderLink 节点、大厅与联机启停单元测试
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { getCommandCalls, installTauriMock, mockApiRoute, mockCommand } from '../../test-utils/tauri-mock.js';

const { toast } = vi.hoisted(() => ({ toast: vi.fn() }));
vi.mock('./ui-components.js', () => ({ showToast: toast }));

let mod;
let S;
let featureBridge;

function buildDom() {
  document.body.innerHTML = `
    <span id="enderlink-status-dot"></span>
    <span id="enderlink-status-text"></span>
    <div class="enderlink-tabs">
      <button class="lan-tab" data-enderlink-tab="host"></button>
      <button class="lan-tab" data-enderlink-tab="join"></button>
    </div>
    <div class="enderlink-tab-content" id="enderlink-tab-host"></div>
    <div class="enderlink-tab-content" id="enderlink-tab-join"></div>
    <div id="enderlink-node-info"></div>
    <div id="enderlink-node-list"></div>
    <button id="enderlink-node-btn"></button>
    <div id="enderlink-hall-list"></div>
    <button id="enderlink-action-btn"></button>
    <input id="enderlink-room-name">
    <div id="enderlink-public" class="active"></div>
    <div id="enderlink-connected-info" style="display:none">
      <span id="enderlink-room-addr"></span>
    </div>
    <pre id="enderlink-room-log"></pre>
    <div id="enderlink-onboarding">
      <div id="enderlink-ob-dl"></div>
      <div id="enderlink-ob-spinner" style="display:none"></div>
      <div id="enderlink-ob-done" style="display:none"></div>
      <div id="enderlink-ob-title"></div>
      <div id="enderlink-ob-desc"></div>
    </div>
    <div id="enderlink-main" style="display:none"></div>
    <div id="enderlink-home"></div>
    <div id="enderlink-host-panel" style="display:none">
      <div id="enderlink-host-step1"></div>
      <div id="enderlink-host-step2"></div>
      <div id="enderlink-host-step3"></div>
      <button id="enderlink-host-next"></button>
    </div>
    <div id="enderlink-join-panel" style="display:none"></div>
    <div id="enderlink-hint" style="display:none"></div>
  `;
}

function resetS() {
  S.enderlinkGamePoll = null;
  S._enderlinkNodes = [];
  S._enderlinkRunning = false;
  S._enderlinkOpening = false;
  S._enderlinkNodeIdx = 0;
  delete featureBridge.enderlinkSelectedNode;
  delete featureBridge.enderlinkNodes;
}

function clearTimers() {
  if (S.enderlinkGamePoll) clearInterval(S.enderlinkGamePoll);
  S.enderlinkGamePoll = null;
}

function resetBridge() {
  mockCommand('enderlink_nodes', () => ({
    ok: true,
    nodes: [{ id: 'n1', name: '节点一', frpIp: '1.2.3.4', frpPort: 7000 }],
  }));
  mockCommand('enderlink_rooms', () => ({ ok: true, rooms: [] }));
  mockCommand('enderlink_start', () => ({ ok: true, address: 'ender.example.com:25565' }));
  mockCommand('enderlink_stop', () => ({ ok: true }));
  mockCommand('enderlink_status', () => ({ running: false }));
  mockCommand('redstone_scan_port', () => ({ ok: true, port: 25578 }));
}

function commandCalls(cmd) {
  return getCommandCalls().filter((call) => call.cmd === cmd);
}

function logText() {
  return document.getElementById('enderlink-room-log').textContent;
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
  mod = await import('./lan-enderlink.js');
});

beforeEach(() => {
  installTauriMock();
  mockApiRoute('/api/current-context', { selectedVersion: '1.20.1' });
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
    json: async () => ({ selectedVersion: '1.20.1' }),
  })));
});

afterEach(() => {
  clearTimers();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  document.body.innerHTML = '';
});

describe('updateEnderlinkStatus / switchTab', () => {
  it('更新文案与状态点', () => {
    mod.updateEnderlinkStatus('联机已开启', 'connected');
    expect(document.getElementById('enderlink-status-text').textContent).toBe('联机已开启');
    expect(document.getElementById('enderlink-status-dot').classList.contains('connected')).toBe(true);

    mod.updateEnderlinkStatus('正在连接...', 'connecting');
    expect(document.getElementById('enderlink-status-dot').className).toBe('lan-status-dot connecting');

    mod.updateEnderlinkStatus('未连接', 'disconnected');
    expect(document.getElementById('enderlink-status-dot').className).toBe('lan-status-dot disconnected');
  });

  it('元素缺失时安全返回', () => {
    document.getElementById('enderlink-status-text').remove();
    document.getElementById('enderlink-status-dot').remove();

    expect(() => mod.updateEnderlinkStatus('x', 'connected')).not.toThrow();
  });

  it('切换标签高亮与内容', () => {
    mod.enderlinkSwitchTab('join');

    const tabs = [...document.querySelectorAll('.enderlink-tabs .lan-tab')];
    expect(tabs[0].classList.contains('active')).toBe(false);
    expect(tabs[1].classList.contains('active')).toBe(true);
    expect(document.getElementById('enderlink-tab-host').style.display).toBe('none');
    expect(document.getElementById('enderlink-tab-join').style.display).toBe('block');
  });
});

describe('addEnderlinkLog', () => {
  it('追加带时间戳的日志', () => {
    mod.addEnderlinkLog('hello');

    expect(logText()).toMatch(/^\[[^\]]+\] hello\n$/);
  });

  it('日志容器缺失时安全返回', () => {
    document.getElementById('enderlink-room-log').remove();

    expect(() => mod.addEnderlinkLog('x')).not.toThrow();
  });
});

describe('enderlinkRefreshNodes', () => {
  it('渲染节点列表与节点按钮', async () => {
    mockCommand('enderlink_nodes', () => ({
      ok: true,
      nodes: [
        { id: 'a', name: '上海', frpIp: '1.1.1.1', frpPort: 7000 },
        { id: 'b', frpIp: '2.2.2.2', frpPort: 7001 },
      ],
    }));

    await mod.enderlinkRefreshNodes();

    expect(document.getElementById('enderlink-node-info').textContent).toBe('共 2 个节点');
    expect(document.getElementById('enderlink-node-list').textContent).toContain('上海');
    expect(document.getElementById('enderlink-node-list').textContent).toContain('1.1.1.1:7000');
    expect(document.getElementById('enderlink-node-list').textContent).toContain('节点2');
    expect(document.getElementById('enderlink-node-btn').textContent).toBe('节点: 上海 (1.1.1.1:7000)');
    expect(S._enderlinkNodes).toHaveLength(2);
  });

  it('空列表展示暂无节点', async () => {
    mockCommand('enderlink_nodes', () => ({ ok: true, nodes: [] }));

    await mod.enderlinkRefreshNodes();

    expect(document.getElementById('enderlink-node-info').textContent).toBe('节点列表为空');
    expect(document.getElementById('enderlink-node-list').textContent).toBe('暂无可用节点');
    expect(S._enderlinkNodes).toEqual([]);
    expect(document.getElementById('enderlink-node-btn').textContent).toBe('节点: 无可用节点');
  });

  it('接口异常展示错误', async () => {
    mockCommand('enderlink_nodes', () => { throw new Error('offline'); });

    await mod.enderlinkRefreshNodes();

    expect(document.getElementById('enderlink-node-info').textContent).toBe('加载失败: offline');
    expect(document.getElementById('enderlink-node-list').textContent).toBe('加载失败: offline');
  });

  it('节点控件缺失时仍加载数据', async () => {
    document.getElementById('enderlink-node-info').remove();
    document.getElementById('enderlink-node-list').remove();
    document.getElementById('enderlink-node-btn').remove();

    await mod.enderlinkRefreshNodes();

    expect(S._enderlinkNodes).toHaveLength(1);
  });
});

describe('enderlinkRefreshRooms', () => {
  it('渲染公开房间并附带复制动作', async () => {
    mockCommand('enderlink_rooms', () => ({
      ok: true,
      rooms: [
        { room_name: '生存服', server_addr: 'frp.example.com', remote_port: 25565 },
        { server_addr: 'frp2.example.com', remote_port: 25566 },
      ],
    }));

    await mod.enderlinkRefreshRooms();

    const list = document.getElementById('enderlink-hall-list');
    expect(list.textContent).toContain('生存服');
    expect(list.textContent).toContain('frp.example.com:25565');
    expect(list.textContent).toContain('房间2');
    const buttons = list.querySelectorAll('[data-action="enderlinkCopyHall"]');
    expect(buttons).toHaveLength(2);
    expect(buttons[0].getAttribute('data-args')).toBe('["frp.example.com:25565"]');
  });

  it('空房间展示占位文案', async () => {
    mockCommand('enderlink_rooms', () => ({ ok: true, rooms: [] }));

    await mod.enderlinkRefreshRooms();

    expect(document.getElementById('enderlink-hall-list').textContent).toBe('暂无公开房间');
  });

  it('接口异常展示失败', async () => {
    mockCommand('enderlink_rooms', () => { throw new Error('down'); });

    await mod.enderlinkRefreshRooms();

    expect(document.getElementById('enderlink-hall-list').textContent).toBe('加载失败: down');
  });

  it('名单容器缺失时安全返回', async () => {
    document.getElementById('enderlink-hall-list').remove();

    await expect(mod.enderlinkRefreshRooms()).resolves.toBeUndefined();
    expect(commandCalls('enderlink_rooms')).toHaveLength(0);
  });
});

describe('enderlinkCopyHall / enderlinkCopyAddr', () => {
  it('复制大厅地址并提示', async () => {
    mod.enderlinkCopyHall('frp.example.com:25565');
    await settle();

    expect(navigator.clipboard.writeText).toHaveBeenCalledWith('frp.example.com:25565');
    expect(toast).toHaveBeenCalledWith('地址已复制', 'success');
    expect(logText()).toContain('复制大厅地址: frp.example.com:25565');
  });

  it('复制大厅地址失败时不提示', async () => {
    navigator.clipboard.writeText.mockRejectedValue(new Error('denied'));

    mod.enderlinkCopyHall('addr');
    await settle();

    expect(toast).not.toHaveBeenCalled();
    expect(logText()).not.toContain('复制大厅地址');
  });

  it('复制房间地址并跳过占位', async () => {
    const addr = document.getElementById('enderlink-room-addr');

    addr.textContent = 'ender.example.com:25565';
    mod.enderlinkCopyAddr();
    await settle();
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith('ender.example.com:25565');
    expect(logText()).toContain('地址已复制: ender.example.com:25565');

    navigator.clipboard.writeText.mockClear();
    addr.textContent = '--';
    mod.enderlinkCopyAddr();
    await settle();
    expect(navigator.clipboard.writeText).not.toHaveBeenCalled();
  });

  it('地址元素缺失时安全返回', () => {
    document.getElementById('enderlink-room-addr').remove();

    expect(() => mod.enderlinkCopyAddr()).not.toThrow();
  });
});

describe('enderlinkUpdateNodeBtn / enderlinkCycleNode', () => {
  it('按索引取模显示节点', () => {
    S._enderlinkNodes = [
      { name: 'A', frpIp: 'a', frpPort: 1 },
      { name: 'B', frpIp: 'b', frpPort: 2 },
    ];
    S._enderlinkNodeIdx = 3;

    mod.enderlinkUpdateNodeBtn();

    expect(document.getElementById('enderlink-node-btn').textContent).toBe('节点: B (b:2)');
  });

  it('无节点或无按钮时安全返回', () => {
    S._enderlinkNodes = [];
    expect(() => mod.enderlinkUpdateNodeBtn()).not.toThrow();

    document.getElementById('enderlink-node-btn').remove();
    S._enderlinkNodes = [{ name: 'A', frpIp: 'a', frpPort: 1 }];
    expect(() => mod.enderlinkUpdateNodeBtn()).not.toThrow();
  });

  it('循环切换节点并记录日志', () => {
    S._enderlinkNodes = [
      { name: 'A', frpIp: 'a', frpPort: 1 },
      { name: 'B', frpIp: 'b', frpPort: 2 },
    ];

    mod.enderlinkCycleNode();

    expect(S._enderlinkNodeIdx).toBe(1);
    expect(document.getElementById('enderlink-node-btn').textContent).toBe('节点: B (b:2)');
    expect(logText()).toContain('切换到节点: B');
  });

  it('无节点时不切换', () => {
    S._enderlinkNodes = [];

    mod.enderlinkCycleNode();

    expect(S._enderlinkNodeIdx).toBe(0);
  });
});

describe('enderlinkStart', () => {
  it('正在开启时阻止重复调用', async () => {
    S._enderlinkOpening = true;
    S._enderlinkNodes = [{ name: 'A', frpIp: 'a', frpPort: 1 }];

    await mod.enderlinkStart();

    expect(commandCalls('enderlink_start')).toHaveLength(0);
  });

  it('节点未加载时提示', async () => {
    S._enderlinkNodes = [];

    await mod.enderlinkStart();

    expect(window.alert).toHaveBeenCalledWith('请先等待节点列表加载');
    expect(commandCalls('enderlink_start')).toHaveLength(0);
  });

  it('使用局域网端口与公开房间参数开启', async () => {
    S._enderlinkNodes = [{ id: 'n1', name: '节点一', frpIp: '1.2.3.4', frpPort: 7000 }];
    mockApiRoute('/api/game/status', { running: true, lanPort: 25566 });
    document.getElementById('enderlink-room-name').value = '  我的房间  ';

    await mod.enderlinkStart();

    const calls = commandCalls('enderlink_start');
    expect(calls).toHaveLength(1);
    expect(calls[0].args.params).toEqual({
      node: { id: 'n1', name: '节点一', frpIp: '1.2.3.4', frpPort: 7000 },
      localPort: 25566,
      roomName: '我的房间',
      gameVersion: '1.20.1',
      isPublic: true,
    });
    expect(S._enderlinkRunning).toBe(true);
    expect(document.getElementById('enderlink-connected-info').style.display).toBe('');
    expect(document.getElementById('enderlink-room-addr').textContent).toBe('ender.example.com:25565');
    expect(document.getElementById('enderlink-status-text').textContent).toBe('联机已开启 | ender.example.com:25565');
    expect(document.getElementById('enderlink-action-btn').textContent).toBe('关闭联机');
    expect(logText()).toContain('房间名: 我的房间');
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith('ender.example.com:25565');
  });

  it('非公开模式与默认版本', async () => {
    S._enderlinkNodes = [{ name: 'A', frpIp: 'a', frpPort: 1 }];
    mockApiRoute('/api/game/status', { running: true, lanPort: 25566 });
    mockApiRoute('/api/current-context', {});
    document.getElementById('enderlink-public').classList.remove('active');

    await mod.enderlinkStart();

    const params = commandCalls('enderlink_start')[0].args.params;
    expect(params.isPublic).toBe(false);
    expect(params.gameVersion).toBe('1.12.2');
    expect(params.roomName).toBe('');
  });

  it('未探测到端口时扫描', async () => {
    S._enderlinkNodes = [{ name: 'A', frpIp: 'a', frpPort: 1 }];
    mockApiRoute('/api/game/status', { running: true });

    await mod.enderlinkStart();

    expect(logText()).toContain('端口扫描成功: 25578');
    expect(commandCalls('enderlink_start')[0].args.params.localPort).toBe(25578);
  });

  it('扫描失败时回落 25565', async () => {
    S._enderlinkNodes = [{ name: 'A', frpIp: 'a', frpPort: 1 }];
    mockApiRoute('/api/game/status', { running: true });
    mockCommand('redstone_scan_port', () => ({ ok: false }));

    await mod.enderlinkStart();

    expect(logText()).toContain('未检测到端口，将使用默认 25565');
    expect(commandCalls('enderlink_start')[0].args.params.localPort).toBe(25565);
  });

  it('游戏状态异常时记录后继续', async () => {
    S._enderlinkNodes = [{ name: 'A', frpIp: 'a', frpPort: 1 }];
    mockApiRoute('/api/game/status', { status: 500, body: { error: 'x' } });

    await mod.enderlinkStart();

    expect(logText()).toContain('检查游戏状态失败');
    expect(commandCalls('enderlink_start')[0].args.params.localPort).toBe(25565);
  });

  it('版本接口异常时使用默认版本', async () => {
    S._enderlinkNodes = [{ name: 'A', frpIp: 'a', frpPort: 1 }];
    mockApiRoute('/api/game/status', { running: true, lanPort: 25566 });
    mockApiRoute('/api/current-context', { status: 500, body: { error: 'bad' } });

    await mod.enderlinkStart();

    expect(commandCalls('enderlink_start')[0].args.params.gameVersion).toBe('1.12.2');
  });

  it('开启接口返回失败时提示', async () => {
    S._enderlinkNodes = [{ name: 'A', frpIp: 'a', frpPort: 1 }];
    mockApiRoute('/api/game/status', { running: true, lanPort: 25566 });
    mockCommand('enderlink_start', () => ({ ok: false, error: 'no relay' }));

    await mod.enderlinkStart();

    expect(S._enderlinkRunning).toBe(false);
    expect(document.getElementById('enderlink-status-text').textContent).toBe('开启失败');
    expect(document.getElementById('enderlink-action-btn').textContent).toBe('开启联机');
    expect(logText()).toContain('开启失败: no relay');
    expect(toast).toHaveBeenCalledWith('开启失败: no relay', 'error');
    expect(S._enderlinkOpening).toBe(false);
  });

  it('开启接口异常时提示', async () => {
    S._enderlinkNodes = [{ name: 'A', frpIp: 'a', frpPort: 1 }];
    mockApiRoute('/api/game/status', { running: true, lanPort: 25566 });
    mockCommand('enderlink_start', () => { throw new Error('boom'); });

    await mod.enderlinkStart();

    expect(toast).toHaveBeenCalledWith('开启失败: boom', 'error');
    expect(S._enderlinkOpening).toBe(false);
  });

  it('剪贴板失败不影响开启结果', async () => {
    S._enderlinkNodes = [{ name: 'A', frpIp: 'a', frpPort: 1 }];
    mockApiRoute('/api/game/status', { running: true, lanPort: 25566 });
    navigator.clipboard.writeText.mockRejectedValue(new Error('denied'));

    await mod.enderlinkStart();

    expect(S._enderlinkRunning).toBe(true);
    expect(logText()).not.toContain('联机地址已复制到剪贴板');
  });
});

describe('enderlinkStop / enderlinkToggle', () => {
  it('关闭联机复位状态', async () => {
    S._enderlinkRunning = true;
    document.getElementById('enderlink-connected-info').style.display = '';

    await mod.enderlinkStop();

    expect(commandCalls('enderlink_stop')).toHaveLength(1);
    expect(S._enderlinkRunning).toBe(false);
    expect(document.getElementById('enderlink-connected-info').style.display).toBe('none');
    expect(document.getElementById('enderlink-action-btn').textContent).toBe('开启联机');
    expect(document.getElementById('enderlink-status-text').textContent).toBe('未连接');
    expect(logText()).toContain('联机已关闭');
  });

  it('关闭接口异常仍复位', async () => {
    mockCommand('enderlink_stop', () => { throw new Error('gone'); });

    await mod.enderlinkStop();

    expect(logText()).toContain('关闭失败: gone');
    expect(S._enderlinkRunning).toBe(false);
  });

  it('运行中切换执行关闭', async () => {
    S._enderlinkRunning = true;

    await mod.enderlinkToggle();

    expect(commandCalls('enderlink_stop')).toHaveLength(1);
  });

  it('未运行切换执行开启', async () => {
    S._enderlinkNodes = [];

    await mod.enderlinkToggle();

    expect(window.alert).toHaveBeenCalledWith('请先等待节点列表加载');
  });
});

describe('onboarding', () => {
  it('首次进入展示引导并加载节点', async () => {
    mod.enderlinkInitPage();
    await settle();

    expect(document.getElementById('enderlink-onboarding').style.display).toBe('flex');
    expect(document.getElementById('enderlink-main').style.display).toBe('none');
    expect(commandCalls('enderlink_nodes')).toHaveLength(1);
  });

  it('已看过引导直接进入主界面', async () => {
    localStorage.setItem('enderlink_onboarding_seen', '1');

    mod.enderlinkInitPage();
    await settle();

    expect(document.getElementById('enderlink-onboarding').style.display).toBe('none');
    expect(document.getElementById('enderlink-main').style.display).toBe('');
    expect(document.getElementById('enderlink-status-text').textContent).toBe('未连接');
  });

  it('已有隧道时恢复地址', async () => {
    localStorage.setItem('enderlink_onboarding_seen', '1');
    mockCommand('enderlink_status', () => ({ running: true, address: '5.6.7.8:25565' }));

    mod.enderlinkInitPage();
    await settle();

    expect(document.getElementById('enderlink-room-addr').textContent).toBe('5.6.7.8:25565');
  });

  it('状态查询异常时复位运行标记', async () => {
    S._enderlinkRunning = true;
    localStorage.setItem('enderlink_onboarding_seen', '1');
    mockCommand('enderlink_status', () => { throw new Error('no'); });

    await mod.enderlinkInitPage();

    expect(S._enderlinkRunning).toBe(false);
  });

  it('引导页重置显示与文案', () => {
    const ob = document.getElementById('enderlink-onboarding');
    ob.classList.add('terracotta-ob--fadeout');

    mod.showEnderlinkOnboarding();

    expect(ob.style.display).toBe('flex');
    expect(ob.classList.contains('terracotta-ob--fadeout')).toBe(false);
    expect(ob.classList.contains('terracotta-ob--enter')).toBe(true);
    expect(document.getElementById('enderlink-ob-title').textContent).toBe('你好！欢迎使用 EnderLink 联机');
    expect(document.getElementById('enderlink-ob-spinner').style.display).toBe('none');
  });

  it('引导容器缺失时安全返回', () => {
    document.getElementById('enderlink-onboarding').remove();

    expect(() => mod.showEnderlinkOnboarding()).not.toThrow();
    expect(() => mod.showEnderlinkMain()).not.toThrow();
  });

  it('显示主界面并回到首页状态', () => {
    document.getElementById('enderlink-onboarding').style.display = 'flex';

    mod.showEnderlinkMain();

    expect(document.getElementById('enderlink-onboarding').style.display).toBe('none');
    expect(document.getElementById('enderlink-main').style.display).toBe('');
    expect(document.getElementById('enderlink-home').style.display).toBe('');
  });

  it('初始化动画结束后写入标记并进入主界面', async () => {
    mod.enderlinkStartOnboarding();

    expect(document.getElementById('enderlink-ob-dl').style.display).toBe('none');
    expect(document.getElementById('enderlink-ob-spinner').style.display).toBe('flex');

    await vi.advanceTimersByTimeAsync(1200);
    expect(document.getElementById('enderlink-ob-done').style.display).toBe('block');

    await vi.advanceTimersByTimeAsync(1500);
    expect(localStorage.getItem('enderlink_onboarding_seen')).toBe('1');

    await vi.advanceTimersByTimeAsync(450);
    expect(document.getElementById('enderlink-main').style.display).toBe('');
  });

  it('初始化控件缺失时安全返回', () => {
    document.getElementById('enderlink-ob-spinner').remove();

    expect(() => mod.enderlinkStartOnboarding()).not.toThrow();
  });
});

describe('host flow', () => {
  it('隐藏与返回首页', () => {
    document.getElementById('enderlink-host-panel').style.display = '';
    document.getElementById('enderlink-hint').style.display = '';

    mod.enderlinkHide();
    expect(document.getElementById('enderlink-host-panel').style.display).toBe('none');
    expect(document.getElementById('enderlink-hint').style.display).toBe('none');

    mod.enderlinkBackToHome();
    expect(document.getElementById('enderlink-home').style.display).toBe('');
    expect(document.getElementById('enderlink-status-text').textContent).toBe('未连接');
  });

  it('第一步按游戏状态启用下一步', async () => {
    mockApiRoute('/api/game/status', { running: false });

    await mod.enderlinkHostStep1();
    await settle();

    expect(document.getElementById('enderlink-home').style.display).toBe('none');
    expect(document.getElementById('enderlink-host-panel').style.display).toBe('');
    expect(document.getElementById('enderlink-host-step1').style.display).toBe('');
    expect(document.getElementById('enderlink-host-next').disabled).toBe(true);
    expect(S.enderlinkGamePoll).not.toBeNull();
  });

  it('轮询检测到游戏运行后启用', async () => {
    mockApiRoute('/api/game/status', { running: false });
    await mod.enderlinkHostStep1();
    await settle();

    mockApiRoute('/api/game/status', { running: true });
    await vi.advanceTimersByTimeAsync(2000);
    await settle();

    expect(document.getElementById('enderlink-host-next').disabled).toBe(false);
    expect(document.getElementById('enderlink-host-next').title).toBe('下一步');
  });

  it('游戏未运行时无法下一步', async () => {
    mockApiRoute('/api/game/status', { running: false });

    await mod.enderlinkHostNext();

    expect(toast).toHaveBeenCalledWith('请先启动游戏，然后在游戏内开放局域网联机', 'error');
    expect(commandCalls('enderlink_start')).toHaveLength(0);
  });

  it('使用共享节点与局域网端口建立房间', async () => {
    featureBridge.enderlinkSelectedNode = { name: '共享', frpIp: '9.9.9.9', frpPort: 7000 };
    mockApiRoute('/api/game/status', { running: true, lanPort: 25566, versionId: '1.20.1' });

    await mod.enderlinkHostNext();

    expect(commandCalls('enderlink_start')[0].args.params).toEqual({
      node: { name: '共享', frpIp: '9.9.9.9', frpPort: 7000 },
      localPort: 25566,
      roomName: '',
      gameVersion: '1.20.1',
      isPublic: false,
    });
    expect(S._enderlinkRunning).toBe(true);
    expect(document.getElementById('enderlink-host-step3').style.display).toBe('');
    expect(document.getElementById('enderlink-room-addr').textContent).toBe('ender.example.com:25565');
    expect(document.getElementById('enderlink-hint').textContent).toBe('联机地址已生成，发送给朋友即可加入');
  });

  it('无共享节点时加载节点列表', async () => {
    mockApiRoute('/api/game/status', { running: true, lanPort: 25566 });

    await mod.enderlinkHostNext();

    expect(commandCalls('enderlink_nodes')).toHaveLength(1);
    expect(commandCalls('enderlink_start')[0].args.params.node.name).toBe('节点一');
  });

  it('无局域网端口时扫描，失败回落 25565', async () => {
    featureBridge.enderlinkSelectedNode = { name: '共享', frpIp: '9.9.9.9', frpPort: 7000 };
    mockApiRoute('/api/game/status', { running: true });
    mockCommand('redstone_scan_port', () => ({ ok: false }));

    await mod.enderlinkHostNext();

    expect(commandCalls('enderlink_start')[0].args.params.localPort).toBe(25565);
    expect(commandCalls('enderlink_start')[0].args.params.gameVersion).toBe('1.12.2');
  });

  it('节点为空时提示失败并回首页', async () => {
    featureBridge.enderlinkNodes = [];
    mockApiRoute('/api/game/status', { running: true, lanPort: 25566 });
    mockCommand('enderlink_nodes', () => ({ ok: true, nodes: [] }));

    await mod.enderlinkHostNext();

    expect(toast).toHaveBeenCalledWith('开启联机失败: 联机节点不可用', 'error');
    expect(document.getElementById('enderlink-home').style.display).toBe('');
  });

  it('开启失败时提示并回首页', async () => {
    featureBridge.enderlinkSelectedNode = { name: '共享', frpIp: '9.9.9.9', frpPort: 7000 };
    mockApiRoute('/api/game/status', { running: true, lanPort: 25566 });
    mockCommand('enderlink_start', () => ({ ok: false, error: 'no relay' }));

    await mod.enderlinkHostNext();

    expect(toast).toHaveBeenCalledWith('开启联机失败: no relay', 'error');
    expect(document.getElementById('enderlink-home').style.display).toBe('');
  });

  it('断开联机并提示', async () => {
    S._enderlinkRunning = true;

    await mod.enderlinkDisconnect();

    expect(commandCalls('enderlink_stop')).toHaveLength(1);
    expect(S._enderlinkRunning).toBe(false);
    expect(document.getElementById('enderlink-home').style.display).toBe('');
    expect(toast).toHaveBeenCalledWith('已断开 EnderLink 联机', 'info');
  });
});
