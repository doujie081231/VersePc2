// @vitest-environment jsdom
// lan-terracotta.test.js — 陶瓦联机流程、状态轮询与引导页单元测试
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { getApiCalls, installTauriMock, mockApiRoute } from '../../test-utils/tauri-mock.js';

const { toast, detectLanPort } = vi.hoisted(() => ({
  toast: vi.fn(),
  detectLanPort: vi.fn(async () => null),
}));

vi.mock('./ui-components.js', () => ({ showToast: toast }));
vi.mock('./lan-port.js', () => ({ detectLanPort }));

let mod;
let S;
let clickedAnchor = null;

function buildDom() {
  document.body.innerHTML = `
    <div id="terracotta-status-title"></div>
    <div id="terracotta-status-desc"></div>
    <span id="terracotta-status-dot"></span>
    <div id="terracotta-home"></div>
    <div id="terracotta-host-panel">
      <div id="terracotta-host-step1"></div>
      <div id="terracotta-host-step2"></div>
      <div id="terracotta-host-step3"></div>
      <button id="terracotta-host-next"></button>
      <div id="terracotta-port-hint"></div>
    </div>
    <div id="terracotta-join-panel">
      <div id="terracotta-join-step1"></div>
      <div id="terracotta-join-step2"></div>
      <div id="terracotta-join-step3"></div>
      <input id="terracotta-join-code">
      <button id="terracotta-join-next"></button>
    </div>
    <div id="terracotta-hint"></div>
    <div id="terracotta-roomcode">--</div>
    <div id="terracotta-connect-row"><div id="terracotta-connect-addr">等待分配...</div></div>
    <div id="terracotta-onboarding">
      <div id="terracotta-ob-dl"></div>
      <div id="terracotta-ob-spinner" style="display:none"></div>
      <div id="terracotta-ob-done" style="display:none"></div>
      <div id="terracotta-ob-title"></div>
      <div id="terracotta-ob-desc"></div>
      <div id="terracotta-ob-progress-text"></div>
    </div>
    <div id="terracotta-main" style="display:none"></div>
  `;
}

function resetS() {
  S.terracottaGamePoll = null;
  S.terracottaPollTimer = null;
  S._terracottaPollRefresher = null;
  S.terracottaState = { mode: null, connected: false };
  S._lastTerracottaStateIndex = -1;
  S._terracottaPollFailCount = 0;
}

function clearTimers() {
  for (const key of ['terracottaGamePoll', 'terracottaPollTimer', '_terracottaPollRefresher']) {
    if (S[key]) clearInterval(S[key]);
    S[key] = null;
  }
}

async function settle() {
  await Promise.resolve();
  await Promise.resolve();
  await vi.advanceTimersByTimeAsync(0);
}

function apiCallsFor(path) {
  return getApiCalls().filter((call) => call.path === path);
}

beforeAll(async () => {
  installTauriMock();
  vi.resetModules();
  ({ S } = await import('./lan-state.js'));
  mod = await import('./lan-terracotta.js');
});

beforeEach(() => {
  installTauriMock();
  buildDom();
  resetS();
  localStorage.clear();
  toast.mockClear();
  detectLanPort.mockReset();
  detectLanPort.mockResolvedValue(null);
  clickedAnchor = null;
  vi.useFakeTimers();
  Object.defineProperty(navigator, 'clipboard', {
    value: { writeText: vi.fn(() => Promise.resolve()) },
    configurable: true,
  });
  vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function () {
    clickedAnchor = this;
  });
});

afterEach(() => {
  clearTimers();
  vi.useRealTimers();
  vi.restoreAllMocks();
  document.body.innerHTML = '';
  delete URL.createObjectURL;
  delete URL.revokeObjectURL;
});

describe('updateTerracottaStatus', () => {
  it('更新标题、描述与已连接状态点', () => {
    mod.updateTerracottaStatus('主机', '房间码: A1', 'connected');

    expect(document.getElementById('terracotta-status-title').textContent).toBe('主机');
    expect(document.getElementById('terracotta-status-desc').textContent).toBe('房间码: A1');
    expect(document.getElementById('terracotta-status-dot').classList.contains('connected')).toBe(true);
  });

  it('连接中与未连接使用对应状态类', () => {
    mod.updateTerracottaStatus('连接中', '', 'connecting');
    expect(document.getElementById('terracotta-status-dot').classList.contains('connecting')).toBe(true);

    mod.updateTerracottaStatus('未连接', '', 'disconnected');
    expect(document.getElementById('terracotta-status-dot').className).toBe('lan-status-dot disconnected');
  });

  it('元素缺失时安全返回', () => {
    document.getElementById('terracotta-status-title').remove();
    document.getElementById('terracotta-status-desc').remove();
    document.getElementById('terracotta-status-dot').remove();

    expect(() => mod.updateTerracottaStatus('x', 'y', 'connected')).not.toThrow();
  });
});

describe('terracottaHostStep1', () => {
  it('切到主机第一步并等待游戏启动', async () => {
    mockApiRoute('/api/game/status', { running: false });

    mod.terracottaHostStep1();
    await settle();

    expect(document.getElementById('terracotta-home').style.display).toBe('none');
    expect(document.getElementById('terracotta-join-panel').style.display).toBe('none');
    expect(document.getElementById('terracotta-host-panel').style.display).toBe('');
    expect(document.getElementById('terracotta-host-step1').style.display).toBe('');
    expect(document.getElementById('terracotta-host-next').disabled).toBe(true);
    expect(document.getElementById('terracotta-host-next').title).toBe('请先启动游戏并开放局域网');
    expect(document.getElementById('terracotta-status-title').textContent).toBe('陶瓦联机 - 创建房间');
    expect(S.terracottaGamePoll).not.toBeNull();
  });

  it('游戏运行后启用下一步并探测端口', async () => {
    mockApiRoute('/api/game/status', { running: true });
    detectLanPort.mockResolvedValue(25566);

    mod.terracottaHostStep1();
    await settle();

    expect(document.getElementById('terracotta-host-next').disabled).toBe(false);
    expect(document.getElementById('terracotta-host-next').title).toBe('下一步');
    expect(document.getElementById('terracotta-port-hint').textContent).toBe('已检测到端口：25566');
    expect(detectLanPort).toHaveBeenCalledTimes(1);
  });

  it('定时轮询捕获游戏启动且只探测一次端口', async () => {
    mockApiRoute('/api/game/status', { running: false });
    mod.terracottaHostStep1();
    await settle();

    mockApiRoute('/api/game/status', { running: true });
    detectLanPort.mockResolvedValue(25570);
    await vi.advanceTimersByTimeAsync(2000);
    await settle();

    expect(document.getElementById('terracotta-host-next').disabled).toBe(false);
    await vi.advanceTimersByTimeAsync(2000);
    await settle();
    expect(detectLanPort).toHaveBeenCalledTimes(1);
  });

  it('重复进入会先清理上一轮游戏轮询', async () => {
    mockApiRoute('/api/game/status', { running: false });
    mod.terracottaHostStep1();
    await settle();
    const first = S.terracottaGamePoll;

    mod.terracottaHostStep1();
    await settle();

    expect(S.terracottaGamePoll).not.toBe(first);
    clearInterval(first);
  });
});

describe('terracottaHostNext', () => {
  it('游戏未启动时提示而不创建隧道', async () => {
    mockApiRoute('/api/game/status', { running: false });

    await mod.terracottaHostNext();

    expect(toast).toHaveBeenCalledWith('请先启动游戏，然后在游戏内开放局域网联机', 'error');
    expect(apiCallsFor('/api/easytier/host')).toHaveLength(0);
  });

  it('使用游戏报告的局域网端口创建房间', async () => {
    localStorage.setItem('terracotta_agreement_v2', '1');
    localStorage.setItem('cachedPlayerName', 'Alex');
    mockApiRoute('/api/game/status', { running: true, lanPort: 25570 });
    mockApiRoute('/api/easytier/host', { success: true });
    mockApiRoute('/api/easytier/status', { running: true, state: { state: 'host-connecting' } });

    await mod.terracottaHostNext();

    const calls = apiCallsFor('/api/easytier/host');
    expect(calls).toHaveLength(1);
    expect(calls[0].args.body).toEqual({ gamePort: 25570, playerName: 'Alex' });
    expect(S.terracottaState).toEqual({ mode: 'host', connected: true });
    expect(document.getElementById('terracotta-hint').textContent).toBe('隧道创建中，请稍候...');
    expect(document.getElementById('terracotta-hint').style.display).toBe('');
    expect(document.getElementById('terracotta-host-step2').style.display).toBe('');
    expect(S.terracottaPollTimer).not.toBeNull();
    clearInterval(S.terracottaGamePoll);
  });

  it('游戏未报告端口时探测，探测失败回落 25565', async () => {
    localStorage.setItem('terracotta_agreement_v2', '1');
    mockApiRoute('/api/game/status', { running: true });
    mockApiRoute('/api/easytier/host', {});
    mockApiRoute('/api/easytier/status', { running: false });
    detectLanPort.mockResolvedValueOnce(25571);

    await mod.terracottaHostNext();
    expect(apiCallsFor('/api/easytier/host')[0].args.body.gamePort).toBe(25571);

    detectLanPort.mockResolvedValueOnce(null);
    await mod.terracottaHostNext();
    expect(apiCallsFor('/api/easytier/host')[1].args.body.gamePort).toBe(25565);
  });

  it('玩家名缺失时使用 Player', async () => {
    localStorage.setItem('terracotta_agreement_v2', '1');
    mockApiRoute('/api/game/status', { running: true, lanPort: 25570 });
    mockApiRoute('/api/easytier/host', {});
    mockApiRoute('/api/easytier/status', { running: false });

    await mod.terracottaHostNext();

    expect(apiCallsFor('/api/easytier/host')[0].args.body.playerName).toBe('Player');
  });

  it('拒绝使用须知后不创建房间', async () => {
    mockApiRoute('/api/game/status', { running: true, lanPort: 25570 });

    const pending = mod.terracottaHostNext();
    await settle();
    const modal = document.body.lastElementChild;
    expect(modal.querySelector('#terracotta-agree-btn')).not.toBeNull();
    modal.click();
    await pending;

    expect(apiCallsFor('/api/easytier/host')).toHaveLength(0);
    expect(localStorage.getItem('terracotta_agreement_v2')).toBeNull();
    expect(document.getElementById('terracotta-host-step1').style.display).toBe('');
  });

  it('创建失败时回退首页并提示', async () => {
    localStorage.setItem('terracotta_agreement_v2', '1');
    mockApiRoute('/api/game/status', { running: true, lanPort: 25570 });
    mockApiRoute('/api/easytier/host', { status: 500, body: { error: 'node busy' } });

    await mod.terracottaHostNext();

    expect(toast).toHaveBeenCalledWith('创建联机失败: node busy', 'error');
    expect(document.getElementById('terracotta-home').style.display).toBe('');
    expect(document.getElementById('terracotta-host-panel').style.display).toBe('none');
    expect(S.terracottaState).toEqual({ mode: null, connected: false });
  });
});

describe('join flow', () => {
  it('加入房间第一步清空输入并禁用下一步', () => {
    document.getElementById('terracotta-join-code').value = 'OLD';

    mod.terracottaJoinStep1();

    expect(document.getElementById('terracotta-home').style.display).toBe('none');
    expect(document.getElementById('terracotta-host-panel').style.display).toBe('none');
    expect(document.getElementById('terracotta-join-panel').style.display).toBe('');
    expect(document.getElementById('terracotta-join-code').value).toBe('');
    expect(document.getElementById('terracotta-join-next').disabled).toBe(true);
    expect(document.getElementById('terracotta-status-title').textContent).toBe('陶瓦联机 - 加入房间');
  });

  it('输入房间码后启用下一步', () => {
    const input = document.getElementById('terracotta-join-code');
    const next = document.getElementById('terracotta-join-next');

    input.value = '   ';
    mod.terracottaJoinCodeInput();
    expect(next.disabled).toBe(true);
    expect(next.title).toBe('请先输入房间码');

    input.value = ' ROOM-9 ';
    mod.terracottaJoinCodeInput();
    expect(next.disabled).toBe(false);
    expect(next.title).toBe('下一步');
  });

  it('房间码为空时直接提示', async () => {
    document.getElementById('terracotta-join-code').value = '  ';

    await mod.terracottaJoinNext();

    expect(toast).toHaveBeenCalledWith('请输入房间码', 'error');
    expect(apiCallsFor('/api/easytier/guest')).toHaveLength(0);
  });

  it('加入成功进入连接并启动轮询', async () => {
    localStorage.setItem('terracotta_agreement_v2', '1');
    localStorage.setItem('cachedPlayerName', 'Steve');
    document.getElementById('terracotta-join-code').value = ' ROOM-9 ';
    mockApiRoute('/api/easytier/guest', { success: true });
    mockApiRoute('/api/easytier/status', { running: true, state: { state: 'guest-connecting' } });

    await mod.terracottaJoinNext();

    const calls = apiCallsFor('/api/easytier/guest');
    expect(calls).toHaveLength(1);
    expect(calls[0].args.body).toEqual({ roomCode: 'ROOM-9', playerName: 'Steve' });
    expect(S.terracottaState).toEqual({ mode: 'guest', connected: true });
    expect(document.getElementById('terracotta-hint').textContent).toBe('正在建立 P2P 连接，请稍候...');
    expect(document.getElementById('terracotta-join-step2').style.display).toBe('');
    expect(S.terracottaPollTimer).not.toBeNull();
  });

  it('加入失败时回退首页并提示', async () => {
    localStorage.setItem('terracotta_agreement_v2', '1');
    document.getElementById('terracotta-join-code').value = 'ROOM-9';
    mockApiRoute('/api/easytier/guest', { status: 500, body: { error: 'expired' } });

    await mod.terracottaJoinNext();

    expect(toast).toHaveBeenCalledWith('加入联机失败: expired', 'error');
    expect(document.getElementById('terracotta-home').style.display).toBe('');
    expect(document.getElementById('terracotta-join-panel').style.display).toBe('none');
  });
});

describe('showTerracottaStep', () => {
  it('只显示目标步骤并触发入场动画', () => {
    mod.showTerracottaStep('host', 2);

    expect(document.getElementById('terracotta-host-step1').style.display).toBe('none');
    expect(document.getElementById('terracotta-host-step2').style.display).toBe('');
    expect(document.getElementById('terracotta-host-step3').style.display).toBe('none');
    expect(document.getElementById('terracotta-host-step2').classList.contains('terracotta-step--enter')).toBe(true);
  });

  it('切换步骤只显示当前步骤并给当前步骤添加入场类', () => {
    mod.showTerracottaStep('join', 1);
    mod.showTerracottaStep('join', 3);

    expect(document.getElementById('terracotta-join-step1').style.display).toBe('none');
    expect(document.getElementById('terracotta-join-step2').style.display).toBe('none');
    expect(document.getElementById('terracotta-join-step3').style.display).toBe('');
    expect(document.getElementById('terracotta-join-step3').classList.contains('terracotta-step--enter')).toBe(true);
  });
});

describe('hide / backToHome / disconnect', () => {
  it('隐藏面板并清理轮询定时器', () => {
    S.terracottaPollTimer = setInterval(() => {}, 1000);
    S._terracottaPollRefresher = setInterval(() => {}, 1000);
    S.terracottaGamePoll = setInterval(() => {}, 1000);

    mod.terracottaHide();

    expect(S.terracottaPollTimer).toBeNull();
    expect(S._terracottaPollRefresher).toBeNull();
    expect(S.terracottaGamePoll).not.toBeNull();
    expect(document.getElementById('terracotta-host-panel').style.display).toBe('none');
    expect(document.getElementById('terracotta-join-panel').style.display).toBe('none');
    expect(document.getElementById('terracotta-hint').style.display).toBe('none');
  });

  it('返回首页清理全部定时器并重置状态', () => {
    S.terracottaPollTimer = setInterval(() => {}, 1000);
    S._terracottaPollRefresher = setInterval(() => {}, 1000);
    S.terracottaGamePoll = setInterval(() => {}, 1000);
    S.terracottaState = { mode: 'host', connected: true };

    mod.terracottaBackToHome();

    expect(S.terracottaPollTimer).toBeNull();
    expect(S._terracottaPollRefresher).toBeNull();
    expect(S.terracottaGamePoll).toBeNull();
    expect(S.terracottaState).toEqual({ mode: null, connected: false });
    expect(document.getElementById('terracotta-home').style.display).toBe('');
    expect(document.getElementById('terracotta-hint').style.display).toBe('none');
    expect(document.getElementById('terracotta-status-title').textContent).toBe('未连接');
  });

  it('断开连接调用停止接口并提示', async () => {
    mockApiRoute('/api/easytier/stop', {});

    await mod.terracottaDisconnect();

    expect(apiCallsFor('/api/easytier/stop')).toHaveLength(1);
    expect(toast).toHaveBeenCalledWith('已断开陶瓦联机', 'info');
    expect(document.getElementById('terracotta-home').style.display).toBe('');
  });

  it('停止接口失败仍回到首页', async () => {
    mockApiRoute('/api/easytier/stop', { status: 500, body: { error: 'gone' } });

    await mod.terracottaDisconnect();

    expect(toast).toHaveBeenCalledWith('已断开陶瓦联机', 'info');
    expect(document.getElementById('terracotta-home').style.display).toBe('');
  });
});

describe('copy helpers', () => {
  it('复制房间码', async () => {
    document.getElementById('terracotta-roomcode').textContent = 'ROOM-1';

    mod.terracottaCopyRoomCode();
    await settle();

    expect(navigator.clipboard.writeText).toHaveBeenCalledWith('ROOM-1');
    expect(toast).toHaveBeenCalledWith('房间码已复制！发送给朋友即可加入', 'success');
  });

  it('占位内容不复制', async () => {
    const el = document.getElementById('terracotta-roomcode');

    el.textContent = '--';
    mod.terracottaCopyRoomCode();
    await settle();
    el.textContent = '等待分配房间码...';
    mod.terracottaCopyRoomCode();
    await settle();

    expect(navigator.clipboard.writeText).not.toHaveBeenCalled();
  });

  it('复制连接地址并跳过占位', async () => {
    const el = document.getElementById('terracotta-connect-addr');

    el.textContent = '127.0.0.1:25565';
    mod.terracottaCopyAddr();
    await settle();
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith('127.0.0.1:25565');
    expect(toast).toHaveBeenCalledWith('连接地址已复制', 'success');

    navigator.clipboard.writeText.mockClear();
    el.textContent = '等待分配...';
    mod.terracottaCopyAddr();
    await settle();
    expect(navigator.clipboard.writeText).not.toHaveBeenCalled();
  });
});

describe('terracottaShowAgreement', () => {
  it('已同意后直接返回 true', async () => {
    localStorage.setItem('terracotta_agreement_v2', '1');

    await expect(mod.terracottaShowAgreement()).resolves.toBe(true);
    expect(document.body.querySelector('#terracotta-agree-btn')).toBeNull();
  });

  it('点击同意写入标记并关闭弹窗', async () => {
    const pending = mod.terracottaShowAgreement();
    const modal = document.body.lastElementChild;
    document.getElementById('terracotta-agree-btn').click();

    await expect(pending).resolves.toBe(true);
    expect(localStorage.getItem('terracotta_agreement_v2')).toBe('1');
    expect(modal.parentElement).toBeNull();
  });

  it('点击遮罩视为拒绝', async () => {
    const pending = mod.terracottaShowAgreement();
    const modal = document.body.lastElementChild;
    modal.click();

    await expect(pending).resolves.toBe(false);
    expect(localStorage.getItem('terracotta_agreement_v2')).toBeNull();
    expect(modal.parentElement).toBeNull();
  });
});

describe('terracottaExportLog', () => {
  it('导出日志并下载', async () => {
    URL.createObjectURL = vi.fn(() => 'blob:terracotta');
    URL.revokeObjectURL = vi.fn();
    mockApiRoute('/api/easytier/log', { log: 'line-1\nline-2' });

    await mod.terracottaExportLog();

    expect(URL.createObjectURL).toHaveBeenCalledTimes(1);
    expect(clickedAnchor).not.toBeNull();
    expect(clickedAnchor.download).toMatch(/^terracotta-log-\d{4}-\d{2}-\d{2}\.txt$/);
    expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:terracotta');
    expect(toast).toHaveBeenCalledWith('日志已导出', 'success');
  });

  it('无日志时提示', async () => {
    mockApiRoute('/api/easytier/log', {});

    await mod.terracottaExportLog();

    expect(toast).toHaveBeenCalledWith('暂无日志', 'info');
  });

  it('接口失败时提示错误', async () => {
    mockApiRoute('/api/easytier/log', { status: 500, body: { error: 'io' } });

    await mod.terracottaExportLog();

    expect(toast).toHaveBeenCalledWith('导出日志失败: io', 'error');
  });
});

describe('onboarding', () => {
  it('核心已安装直接进入主界面', async () => {
    mockApiRoute('/api/easytier/status', { installed: true });

    mod.initTerracottaPage();
    await settle();

    expect(document.getElementById('terracotta-onboarding').style.display).toBe('none');
    expect(document.getElementById('terracotta-main').style.display).toBe('');
  });

  it('核心未安装展示引导', async () => {
    mockApiRoute('/api/easytier/status', { installed: false });

    mod.initTerracottaPage();
    await settle();

    expect(document.getElementById('terracotta-onboarding').style.display).toBe('flex');
    expect(document.getElementById('terracotta-main').style.display).toBe('none');
    expect(document.getElementById('terracotta-ob-title').textContent).toBe('你好！欢迎使用陶瓦联机');
  });

  it('状态查询失败时回退引导页', async () => {
    mockApiRoute('/api/easytier/status', { status: 500, body: { error: 'down' } });

    mod.initTerracottaPage();
    await settle();

    expect(document.getElementById('terracotta-onboarding').style.display).toBe('flex');
  });

  it('引导容器缺失时初始化安全返回', () => {
    document.getElementById('terracotta-onboarding').remove();

    expect(() => mod.initTerracottaPage()).not.toThrow();
  });

  it('重置引导页到初始阶段', () => {
    const ob = document.getElementById('terracotta-onboarding');
    ob.classList.add('terracotta-ob--fadeout');
    document.getElementById('terracotta-ob-dl').style.display = 'none';

    mod.showTerracottaOnboarding();

    expect(ob.style.display).toBe('flex');
    expect(ob.classList.contains('terracotta-ob--fadeout')).toBe(false);
    expect(ob.classList.contains('terracotta-ob--enter')).toBe(true);
    expect(document.getElementById('terracotta-ob-dl').style.display).toBe('');
    expect(document.getElementById('terracotta-ob-spinner').style.display).toBe('none');
    expect(document.getElementById('terracotta-ob-done').style.display).toBe('none');
    expect(document.getElementById('terracotta-ob-desc').textContent).toBe('在首次使用前，请先下载核心');
  });

  it('显示主界面并回到未连接状态', () => {
    document.getElementById('terracotta-onboarding').style.display = 'flex';

    mod.showTerracottaMain();

    expect(document.getElementById('terracotta-onboarding').style.display).toBe('none');
    expect(document.getElementById('terracotta-main').style.display).toBe('');
    expect(document.getElementById('terracotta-status-title').textContent).toBe('未连接');
  });
});

describe('运行中的房间恢复', () => {
  it('后台仍有主机房间时恢复到房间码界面', async () => {
    mockApiRoute('/api/easytier/status', {
      installed: true,
      running: true,
      mode: 'host',
      roomCode: 'A1B2C',
      virtualIP: '10.144.1.2',
      gamePort: 25565,
      state: { state: 'host-ok', room: { code: 'A1B2C' } },
      stateIndex: 7,
    });

    mod.initTerracottaPage();
    await settle();

    expect(document.getElementById('terracotta-main').style.display).toBe('');
    expect(document.getElementById('terracotta-home').style.display).toBe('none');
    expect(document.getElementById('terracotta-host-panel').style.display).toBe('');
    expect(document.getElementById('terracotta-host-step3').style.display).toBe('');
    expect(document.getElementById('terracotta-roomcode').textContent).toBe('A1B2C');
    expect(document.getElementById('terracotta-status-title').textContent).toBe('陶瓦联机 - 主机');
    expect(S.terracottaState).toEqual({ mode: 'host', connected: true });
  });

  it('后台仍有客户端房间时恢复到连接地址界面', async () => {
    mockApiRoute('/api/easytier/status', {
      installed: true,
      running: true,
      mode: 'guest',
      state: { state: 'guest-ok', url: '10.144.1.3:25565' },
      stateIndex: 5,
    });

    mod.initTerracottaPage();
    await settle();

    expect(document.getElementById('terracotta-join-panel').style.display).toBe('');
    expect(document.getElementById('terracotta-join-step3').style.display).toBe('');
    expect(document.getElementById('terracotta-connect-addr').textContent).toBe('127.0.0.1:25565');
    expect(S.terracottaState).toEqual({ mode: 'guest', connected: true });
  });

  it('未运行时仍回落到主页', async () => {
    mockApiRoute('/api/easytier/status', { installed: true, running: false, mode: 'idle' });

    mod.initTerracottaPage();
    await settle();

    expect(document.getElementById('terracotta-home').style.display).toBe('');
    expect(S.terracottaState).toEqual({ mode: null, connected: false });
  });
});

describe('terracottaDownloadCore', () => {
  it('下载完成后打钩并在动画结束后进入主界面', async () => {
    mockApiRoute('/api/easytier/download', { ok: true });
    mockApiRoute('/api/easytier/download-status', { status: 'completed' });

    await mod.terracottaDownloadCore();

    expect(document.getElementById('terracotta-ob-dl').style.display).toBe('none');
    expect(document.getElementById('terracotta-ob-spinner').style.display).toBe('none');
    expect(document.getElementById('terracotta-ob-done').style.display).toBe('block');
    expect(document.getElementById('terracotta-ob-done').classList.contains('terracotta-ob-done--show')).toBe(true);

    await vi.advanceTimersByTimeAsync(1600);
    expect(document.getElementById('terracotta-onboarding').classList.contains('terracotta-ob--fadeout')).toBe(true);

    await vi.advanceTimersByTimeAsync(450);
    expect(document.getElementById('terracotta-main').style.display).toBe('');
    expect(document.getElementById('terracotta-onboarding').style.display).toBe('none');
  });

  it('轮询到下载中后继续等待', async () => {
    let calls = 0;
    mockApiRoute('/api/easytier/download', { ok: true });
    mockApiRoute('/api/easytier/download-status', () => (++calls < 3 ? { status: 'downloading' } : { status: 'completed' }));

    const pending = mod.terracottaDownloadCore();
    await vi.advanceTimersByTimeAsync(1000);
    await pending;

    expect(calls).toBe(3);
    expect(document.getElementById('terracotta-ob-done').style.display).toBe('block');
  });

  it('下载状态为 error 时展示失败态', async () => {
    mockApiRoute('/api/easytier/download', { ok: true });
    mockApiRoute('/api/easytier/download-status', { status: 'error' });

    await mod.terracottaDownloadCore();

    expect(document.getElementById('terracotta-ob-dl').style.display).toBe('');
    expect(document.getElementById('terracotta-ob-spinner').style.display).toBe('none');
    expect(document.getElementById('terracotta-ob-title').textContent).toBe('下载失败，请检查网络后重试');
    expect(document.getElementById('terracotta-ob-desc').textContent).toBe('点击下载图标重新下载核心');
  });

  it('下载接口返回错误时展示失败态', async () => {
    mockApiRoute('/api/easytier/download', { error: 'offline' });

    await mod.terracottaDownloadCore();

    expect(document.getElementById('terracotta-ob-title').textContent).toBe('下载失败，请检查网络后重试');
  });

  it('下载接口异常时展示失败态', async () => {
    mockApiRoute('/api/easytier/download', { status: 500, body: { error: 'boom' } });

    await mod.terracottaDownloadCore();

    expect(document.getElementById('terracotta-ob-dl').style.display).toBe('');
    expect(document.getElementById('terracotta-ob-title').textContent).toBe('下载失败，请检查网络后重试');
  });

  it('下载控件缺失时不做请求', async () => {
    document.getElementById('terracotta-ob-dl').remove();

    await mod.terracottaDownloadCore();

    expect(apiCallsFor('/api/easytier/download')).toHaveLength(0);
  });

  it('完成回调可直接驱动打钩与淡出', async () => {
    mod.completeTerracottaOnboarding();

    expect(document.getElementById('terracotta-ob-spinner').style.display).toBe('none');
    expect(document.getElementById('terracotta-ob-done').style.display).toBe('block');

    await vi.advanceTimersByTimeAsync(2100);
    expect(document.getElementById('terracotta-main').style.display).toBe('');
  });

  it('失败回调恢复下载入口', () => {
    document.getElementById('terracotta-ob-dl').style.display = 'none';
    document.getElementById('terracotta-ob-spinner').style.display = 'flex';

    mod.failTerracottaDownload();

    expect(document.getElementById('terracotta-ob-dl').style.display).toBe('');
    expect(document.getElementById('terracotta-ob-spinner').style.display).toBe('none');
    expect(document.getElementById('terracotta-ob-title').textContent).toBe('下载失败，请检查网络后重试');
  });
});

describe('terracottaStartPolling', () => {
  it('主机状态回填房间码与提示', async () => {
    S.terracottaState = { mode: 'host', connected: true };
    mockApiRoute('/api/easytier/status', {
      running: true,
      stateIndex: 3,
      state: { state: 'host-ok', room: 'ROOM-42' },
      profiles: [{}, {}],
    });

    mod.terracottaStartPolling();
    await settle();

    expect(document.getElementById('terracotta-roomcode').textContent).toBe('ROOM-42');
    expect(document.getElementById('terracotta-hint').textContent).toBe('将房间码发送给朋友即可联机 (2人已连接)');
    expect(document.getElementById('terracotta-hint').style.display).toBe('');
    expect(document.getElementById('terracotta-status-desc').textContent).toBe('房间码: ROOM-42');
    expect(document.getElementById('terracotta-status-dot').classList.contains('connected')).toBe(true);
    expect(document.getElementById('terracotta-host-step3').style.display).toBe('');
  });

  it('房间对象形式也读取 code', async () => {
    S.terracottaState = { mode: 'host', connected: true };
    mockApiRoute('/api/easytier/status', {
      running: true,
      stateIndex: 1,
      state: { state: 'host-ok', room: { code: 'OBJ-7' } },
    });

    mod.terracottaStartPolling();
    await settle();

    expect(document.getElementById('terracotta-roomcode').textContent).toBe('OBJ-7');
    expect(document.getElementById('terracotta-hint').textContent).toBe('将房间码发送给朋友即可联机');
  });

  it('有虚拟 IP 时显示连接地址，没有时隐藏整行', async () => {
    S.terracottaState = { mode: 'host', connected: true };
    mockApiRoute('/api/easytier/status', {
      running: true,
      stateIndex: 4,
      virtualIP: '10.0.0.2',
      gamePort: 25565,
      state: { state: 'host-ok', room: 'ROOM-1' },
    });

    mod.terracottaStartPolling();
    await settle();

    expect(document.getElementById('terracotta-connect-addr').textContent).toBe('10.0.0.2:25565');
    expect(document.getElementById('terracotta-connect-row').style.display).toBe('');

    mod.terracottaBackToHome();
    expect(document.getElementById('terracotta-connect-row').style.display).toBe('none');
  });

  it('主机异常展示错误提示', async () => {
    S.terracottaState = { mode: 'host', connected: true };
    mockApiRoute('/api/easytier/status', {
      running: true,
      stateIndex: 1,
      state: { state: 'exception' },
      errorType: 'node',
      errorMessage: '节点不可用',
    });

    mod.terracottaStartPolling();
    await settle();

    expect(document.getElementById('terracotta-hint').textContent).toBe('节点不可用 (node)');
    expect(document.getElementById('terracotta-hint').style.background).toBe('rgba(239, 68, 68, 0.1)');
    expect(toast).toHaveBeenCalledWith('创建房间失败: 节点不可用', 'error');
  });

  it('客户端连接成功把远端地址映射到本地回环', async () => {
    S.terracottaState = { mode: 'guest', connected: true };
    mockApiRoute('/api/easytier/status', {
      running: true,
      stateIndex: 2,
      state: { state: 'guest-ok', url: '10.0.0.8:25565' },
      profiles: [{}],
    });

    mod.terracottaStartPolling();
    await settle();

    expect(document.getElementById('terracotta-connect-addr').textContent).toBe('127.0.0.1:25565');
    expect(document.getElementById('terracotta-hint').textContent).toBe('在Minecraft多人游戏中添加服务器地址: 127.0.0.1:25565 (1人在线)');
    expect(document.getElementById('terracotta-join-step3').style.display).toBe('');
    expect(document.getElementById('terracotta-status-desc').textContent).toBe('连接地址: 127.0.0.1:25565');
  });

  it('回环地址保持原样且无端口时仅保留主机', async () => {
    S.terracottaState = { mode: 'guest', connected: true };
    mockApiRoute('/api/easytier/status', {
      running: true,
      stateIndex: 1,
      state: { state: 'guest-ok', url: '127.0.0.1:25570' },
    });

    mod.terracottaStartPolling();
    await settle();
    expect(document.getElementById('terracotta-connect-addr').textContent).toBe('127.0.0.1:25570');

    mockApiRoute('/api/easytier/status', {
      running: true,
      stateIndex: 2,
      state: { state: 'guest-ok', url: '10.0.0.9' },
    });
    await vi.advanceTimersByTimeAsync(1500);
    await settle();
    expect(document.getElementById('terracotta-connect-addr').textContent).toBe('127.0.0.1');
  });

  it('客户端异常提示加入失败', async () => {
    S.terracottaState = { mode: 'guest', connected: true };
    mockApiRoute('/api/easytier/status', {
      running: true,
      stateIndex: 1,
      state: { state: 'exception' },
    });

    mod.terracottaStartPolling();
    await settle();

    expect(document.getElementById('terracotta-hint').textContent).toBe('连接异常');
    expect(toast).toHaveBeenCalledWith('加入房间失败: 连接异常', 'error');
  });

  it('未运行状态每轮重置失败计数，当前不会触发断开提示', async () => {
    S.terracottaState = { mode: 'host', connected: true };
    mockApiRoute('/api/easytier/status', { running: false });

    mod.terracottaStartPolling();
    await settle();

    await vi.advanceTimersByTimeAsync(12000);
    await settle();

    expect(toast).not.toHaveBeenCalled();
    expect(S._terracottaPollFailCount).toBe(1);
    expect(S.terracottaPollTimer).not.toBeNull();
    expect(S.terracottaState).toEqual({ mode: 'host', connected: true });
  });

  it('连续轮询异常八次后提示网络异常', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => {});
    S.terracottaState = { mode: 'host', connected: true };
    mockApiRoute('/api/easytier/status', { status: 500, body: { error: 'boom' } });

    mod.terracottaStartPolling();
    await settle();
    await vi.advanceTimersByTimeAsync(21000);
    await settle();

    expect(document.getElementById('terracotta-hint').textContent).toBe('网络连接异常，请检查网络');
    expect(document.getElementById('terracotta-hint').style.display).toBe('');
    expect(S._terracottaPollFailCount).toBeGreaterThanOrEqual(8);
    expect(console.warn).toHaveBeenCalledTimes(8);
  });

  it('运行中但无状态数据时保持界面不变', async () => {
    S.terracottaState = { mode: 'host', connected: true };
    mockApiRoute('/api/easytier/status', { running: true });

    mod.terracottaStartPolling();
    await settle();

    expect(document.getElementById('terracotta-hint').textContent).toBe('');
    expect(S._terracottaPollFailCount).toBe(0);
  });

  it('相同状态索引不重复刷新界面', async () => {
    S.terracottaState = { mode: 'host', connected: true };
    mockApiRoute('/api/easytier/status', {
      running: true,
      stateIndex: 4,
      state: { state: 'host-ok', room: 'SAME' },
    });

    mod.terracottaStartPolling();
    await settle();
    document.getElementById('terracotta-roomcode').textContent = 'MUTATED';

    await vi.advanceTimersByTimeAsync(1500);
    await settle();

    expect(document.getElementById('terracotta-roomcode').textContent).toBe('MUTATED');
    expect(S._lastTerracottaStateIndex).toBe(4);
  });

  it('重新启动轮询会清理旧定时器', async () => {
    mockApiRoute('/api/easytier/status', { running: true, state: { state: 'host-connecting' } });
    S.terracottaState = { mode: 'host', connected: true };

    mod.terracottaStartPolling();
    const firstTimer = S.terracottaPollTimer;
    mod.terracottaStartPolling();
    await settle();

    expect(S.terracottaPollTimer).not.toBe(firstTimer);
    clearInterval(firstTimer);
  });
});
