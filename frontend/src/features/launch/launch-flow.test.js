// @vitest-environment jsdom
// launch-flow.test.js — 启动编排、依赖检查与自动补全流程单元测试
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { getApiCalls, installTauriMock, mockApiRoute } from '../../test-utils/tauri-mock.js';

let mod;
let S;
let appState;

const OK_CHECK = () => ({
  java: { required: 21, ok: true, version: '21.0.1', message: 'Java 21 ✓' },
  mainJar: { ok: true, message: '主 JAR 完整' },
  libraries: { missing: [], total: 10 },
  missingFiles: [],
  assets: { missing: 0 },
});

function buildDom() {
  document.body.innerHTML = `
    <div id="game-launch-overlay"></div>
    <div id="launch-splash-progress"></div>
    <div id="launch-splash-status"></div>
    <div id="launch-error-section"></div>
    <div id="launch-error-msg"></div>
    <div id="launch-repair-guide"></div>
    <div id="launch-log-section" style="display:none"></div>
    <div id="status-indicator"></div>
    <div id="status-text"></div>
    <div id="game-instance-list"></div>
    <button id="launch-btn"><span>启动游戏</span></button>
    <button id="home-launch-btn"></button>
    <select id="window-size">
      <option value="default">默认</option>
      <option value="1280x720">1280x720</option>
      <option value="custom">自定义</option>
    </select>
    <input id="custom-width" value="">
    <input id="custom-height" value="">
    <div id="toast-container"></div>
  `;
}

function makeSelect(initial = '1.20.1') {
  return {
    value: initial,
    getValue() { return this.value; },
    setValue(v) { this.value = v; },
  };
}

/** 启动并推进假定时器，覆盖启动流程内部的固定延时 */
async function runLaunch(ms = 3200) {
  const promise = mod.handleLaunch();
  await vi.advanceTimersByTimeAsync(ms);
  await promise;
}

beforeAll(async () => {
  installTauriMock();
  vi.resetModules();
  S = (await import('./launch-state.js')).S;
  appState = (await import('../../core/app-state.js')).appState;
  mod = await import('./launch-flow.js');
});

beforeEach(() => {
  vi.useFakeTimers();
  installTauriMock();
  buildDom();
  S.launching = false;
  S.launchDlPollInterval = null;
  S._gameWasRunning = false;
  S._gameStatusErrorCount = 0;
  appState.currentLaunchVersionId = '1.20.1';
  appState.launchVersionCustomSelect = makeSelect('1.20.1');
  appState.quickPlayWorld = '';
  appState._launchCounted = false;
  localStorage.setItem('verse_launchCount', '2');

  mockApiRoute('/api/accounts', [{ id: 'a1', username: 'Steve' }]);
  mockApiRoute('/api/versions', { versions: [{ id: '1.20.1' }], installed: [] });
  mockApiRoute('/api/launch/check', OK_CHECK());
  mockApiRoute('/api/launch', { success: true });
  mockApiRoute('/api/game/status', { running: true, instances: [{ sessionId: 's1', versionId: '1.20.1', pid: 100, startTime: Date.now() }] });
});

afterEach(() => {
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.restoreAllMocks();
  document.body.innerHTML = '';
});

describe('handleLaunch 前置校验', () => {
  it('已有启动流程进行中时提示并退出', async () => {
    S.launching = true;
    await runLaunch(10);
    expect(document.getElementById('toast-container').textContent).toContain('正在启动中');
    expect(getApiCalls().length).toBe(0);
  });

  it('未选择版本时提示且不启动', async () => {
    appState.currentLaunchVersionId = '';
    appState.launchVersionCustomSelect = makeSelect('');
    await runLaunch(10);
    expect(document.getElementById('toast-container').textContent).toContain('请选择游戏版本');
    expect(S.launching).toBe(false);
    expect(getApiCalls().some((c) => c.path === '/api/launch')).toBe(false);
  });

  it('没有账户时展示错误并恢复按钮', async () => {
    mockApiRoute('/api/accounts', []);
    await runLaunch(400);

    expect(document.getElementById('launch-error-section').style.display).toBe('flex');
    expect(document.getElementById('launch-error-msg').textContent).toContain('未登录');
    expect(document.getElementById('launch-splash-status').textContent).toContain('未登录');
    expect(document.getElementById('launch-btn').disabled).toBe(false);
    expect(document.getElementById('home-launch-btn').disabled).toBe(false);
    expect(S.launching).toBe(false);
  });
});

describe('Java 检查与自动补全', () => {
  it('缺少 Java 时自动安装并继续启动', async () => {
    let checkCount = 0;
    mockApiRoute('/api/launch/check', () => {
      checkCount++;
      return checkCount === 1
        ? { ...OK_CHECK(), java: { required: 21, ok: false, message: '缺少 Java' } }
        : OK_CHECK();
    });
    mockApiRoute('/api/java/auto-install', { success: true, javaPath: '/jdk21', version: '21.0.1' });

    await runLaunch();

    expect(getApiCalls().some((c) => c.path === '/api/java/auto-install')).toBe(true);
    expect(checkCount).toBe(2);
    expect(getApiCalls().some((c) => c.path === '/api/launch')).toBe(true);
    expect(S.launching).toBe(false);
  });

  it('旧版本 Java 上限随自动安装请求传给后端', async () => {
    mockApiRoute('/api/launch/check', {
      ...OK_CHECK(),
      java: { required: 8, maxVersion: 23, ok: false, message: '缺少 Java 8~23' },
    });
    mockApiRoute('/api/java/auto-install', { success: true, javaPath: '/jdk8', version: '8.0.1' });

    await runLaunch();

    const call = getApiCalls().find((c) => c.path === '/api/java/auto-install');
    expect(call.args.body).toEqual({ requiredVersion: 8, maxVersion: 23 });
  });

  it('Java 自动安装走会话轮询直到完成', async () => {
    let checkCount = 0;
    let statusCount = 0;
    mockApiRoute('/api/launch/check', () => {
      checkCount++;
      return checkCount === 1
        ? { ...OK_CHECK(), java: { required: 21, ok: false } }
        : OK_CHECK();
    });
    mockApiRoute('/api/java/auto-install', { success: true, sessionId: 'j1' });
    mockApiRoute('/api/java/install-status', () => {
      statusCount++;
      return statusCount === 1 ? { status: 'downloading', progress: 40 } : { status: 'completed' };
    });

    await runLaunch();

    expect(statusCount).toBe(2);
    expect(getApiCalls().some((c) => c.path === '/api/launch')).toBe(true);
  });

  it('Java 下载会话失败时中止启动', async () => {
    mockApiRoute('/api/launch/check', { ...OK_CHECK(), java: { required: 21, ok: false } });
    mockApiRoute('/api/java/auto-install', { success: true, sessionId: 'j2' });
    mockApiRoute('/api/java/install-status', { status: 'error' });

    await runLaunch();

    expect(document.getElementById('launch-error-msg').textContent).toContain('Java');
    expect(document.getElementById('launch-error-msg').textContent).toContain('前往 Java 管理页面');
    expect(getApiCalls().some((c) => c.path === '/api/launch')).toBe(false);
    expect(S.launching).toBe(false);
  });

  it('Java 自动安装请求失败时展示原因', async () => {
    mockApiRoute('/api/launch/check', { ...OK_CHECK(), java: { required: 21, ok: false } });
    mockApiRoute('/api/java/auto-install', { success: false, message: '没有可用镜像' });

    await runLaunch();

    expect(document.getElementById('launch-error-msg').textContent).toContain('没有可用镜像');
    expect(S.launching).toBe(false);
  });

  it('自动安装后 Java 仍不可用时提示仍未就绪', async () => {
    mockApiRoute('/api/launch/check', { ...OK_CHECK(), java: { required: 21, ok: false, message: '需要 Java 21' } });
    mockApiRoute('/api/java/auto-install', { success: true, javaPath: '/jdk' });

    await runLaunch();

    expect(document.getElementById('launch-error-msg').textContent).toContain('需要 Java 21');
    expect(getApiCalls().some((c) => c.path === '/api/launch')).toBe(false);
    expect(S.launching).toBe(false);
  });

  it('waitJavaAutoInstall 轮询到完成返回 true', async () => {
    mockApiRoute('/api/java/install-status', { status: 'completed' });
    const promise = mod.waitJavaAutoInstall('j1', 21, '21+');
    await vi.advanceTimersByTimeAsync(600);
    await expect(promise).resolves.toBe(true);
  });

  it('waitJavaAutoInstall 轮询到失败返回 false', async () => {
    mockApiRoute('/api/java/install-status', { status: 'failed' });
    const promise = mod.waitJavaAutoInstall('j1', 21, '21+');
    await vi.advanceTimersByTimeAsync(600);
    await expect(promise).resolves.toBe(false);
  });
});

describe('文件完整性检查与补全', () => {
  it('主 JAR 缺失但可补全时继续自动补全', async () => {
    let checkCount = 0;
    mockApiRoute('/api/launch/check', () => {
      checkCount++;
      if (checkCount === 1) {
        return {
          ...OK_CHECK(),
          mainJar: { ok: false, message: '主 JAR 缺失' },
          missingFiles: [{ kind: 'main_jar', path: '/x.jar', url: 'https://x' }],
        };
      }
      return OK_CHECK();
    });
    mockApiRoute('/api/launch/download-deps', { failed: 0 });

    await runLaunch();

    expect(getApiCalls().some((c) => c.path === '/api/launch/download-deps')).toBe(true);
    expect(getApiCalls().some((c) => c.path === '/api/launch')).toBe(true);
    expect(document.getElementById('launch-error-section').style.display).toBe('none');
  });

  it('主 JAR 缺失且无下载源时中止', async () => {
    mockApiRoute('/api/launch/check', {
      ...OK_CHECK(),
      mainJar: { ok: false, message: '主 JAR 缺失' },
      missingFiles: [],
    });

    await runLaunch();

    expect(document.getElementById('launch-error-msg').textContent).toContain('主 JAR 缺失');
    expect(S.launching).toBe(false);
    expect(getApiCalls().some((c) => c.path === '/api/launch')).toBe(false);
  });

  it('Forge 核心库缺失且可下载时走检查下载', async () => {
    let checkCount = 0;
    mockApiRoute('/api/launch/check', () => {
      checkCount++;
      if (checkCount === 1) {
        return {
          ...OK_CHECK(),
          forgeCore: { ok: false, missing: [{ desc: 'Forge 核心', name: 'net:forge:1', path: '/forge.jar' }] },
          missingFiles: [{ path: '/forge.jar', url: 'https://forge' }],
        };
      }
      return OK_CHECK();
    });
    mockApiRoute('/api/launch/download-deps', { failed: 0 });
    mockApiRoute('/api/launch', ({ body }) => (body.checkOnly ? { needDownload: false } : { success: true }));

    await runLaunch();

    const launchCalls = getApiCalls().filter((c) => c.path === '/api/launch');
    expect(launchCalls[0].args.body.checkOnly).toBe(true);
    expect(launchCalls.length).toBeGreaterThan(1);
    expect(S.launching).toBe(false);
  });

  it('Forge 核心库缺失且无下载源时给出修复指引', async () => {
    mockApiRoute('/api/launch/check', {
      ...OK_CHECK(),
      forgeCore: { ok: false, missing: [{ desc: 'Forge 核心', name: 'net:forge:1', path: '/forge.jar' }] },
      missingFiles: [],
    });

    await runLaunch();

    expect(document.getElementById('launch-error-msg').textContent).toContain('Forge 核心库文件缺失');
    expect(document.getElementById('launch-error-msg').textContent).toContain('文件修复');
    expect(S.launching).toBe(false);
  });

  it('补全失败时报告失败文件', async () => {
    mockApiRoute('/api/launch/check', {
      ...OK_CHECK(),
      missingFiles: [{ kind: 'library', path: '/a.jar', url: 'https://a' }],
    });
    mockApiRoute('/api/launch/download-deps', { failed: 1, failedFiles: ['/lib/a.jar'] });

    await runLaunch();

    expect(document.getElementById('launch-error-msg').textContent).toContain('仍有 1 个文件下载失败');
    expect(document.getElementById('launch-error-msg').textContent).toContain('a.jar');
    expect(S.launching).toBe(false);
  });

  it('补全后仍有缺失时提示再次补全', async () => {
    let checkCount = 0;
    mockApiRoute('/api/launch/check', () => {
      checkCount++;
      if (checkCount === 1) return { ...OK_CHECK(), missingFiles: [{ kind: 'library', path: '/a.jar', url: 'https://a' }] };
      return { ...OK_CHECK(), missingFiles: [{ kind: 'library', path: '/b.jar' }] };
    });
    mockApiRoute('/api/launch/download-deps', { failed: 0 });

    await runLaunch();

    expect(document.getElementById('launch-error-msg').textContent).toContain('仍有 1 个文件缺失');
    expect(S.launching).toBe(false);
  });
});

describe('启动结果处理', () => {
  it('启动成功写入运行状态并关闭遮罩', async () => {
    await runLaunch();

    expect(document.getElementById('launch-log-section').style.display).toBe('');
    expect(document.getElementById('launch-btn').classList.contains('running')).toBe(true);
    expect(document.getElementById('status-text').textContent).toBe('游戏运行中');
    expect(document.getElementById('launch-btn').disabled).toBe(false);
    expect(document.getElementById('home-launch-btn').disabled).toBe(false);
    const call = getApiCalls().find((c) => c.path === '/api/launch');
    expect(call.args.body.resolution).toBe('854x480');
    expect(localStorage.getItem('verse_launchCount')).toBe('3');
    expect(S.launching).toBe(false);
  });

  it('快速进世界目标只传递一次并在读取后清空', async () => {
    appState.quickPlayWorld = 'world-1';
    await runLaunch();

    const call = getApiCalls().find((c) => c.path === '/api/launch');
    expect(call.args.body.quickPlayWorld).toBe('world-1');
    expect(appState.quickPlayWorld).toBe('');
  });

  it('外部版本目录会传给依赖检查', async () => {
    mockApiRoute('/api/versions', { versions: [{ id: '1.20.1', externalDir: 'D:/versions/1.20.1' }], installed: [] });
    await runLaunch();

    const call = getApiCalls().find((c) => c.path === '/api/launch/check');
    expect(call.args.body.externalVersionDir).toBe('D:/versions/1.20.1');
  });

  it('启动返回需要下载时进入下载轮询并释放启动锁', async () => {
    mockApiRoute('/api/launch', { needDownload: true, sessionId: 'dl-1' });
    mockApiRoute('/api/launch/session-status', { status: 'downloading', progress: 10 });

    await runLaunch(1000);

    expect(S.launching).toBe(false);
  });

  it('启动失败时展示错误详情', async () => {
    const errSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
    mockApiRoute('/api/launch', { success: false, error: 'JVM 崩溃', details: { mainClass: 'net.Main' } });

    await runLaunch();

    expect(document.getElementById('launch-error-msg').textContent).toContain('JVM 崩溃');
    expect(document.getElementById('launch-error-msg').title).toContain('主类: net.Main');
    expect(S.launching).toBe(false);
    expect(errSpy).toHaveBeenCalled();
  });

  it('启动请求异常时展示错误并在超时后关闭遮罩', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    mockApiRoute('/api/accounts', { status: 500, body: { error: 'ipc down' } });

    await runLaunch(6000);

    expect(document.getElementById('launch-splash-status').textContent).toContain('ipc down');
    expect(document.getElementById('launch-error-msg').textContent).toContain('ipc down');
    expect(document.getElementById('game-launch-overlay').style.display).toBe('none');
    expect(document.getElementById('launch-btn').disabled).toBe(false);
    expect(S.launching).toBe(false);
  });
});

describe('窗口尺寸与取消', () => {
  it('默认尺寸回退到 854x480', () => {
    document.getElementById('window-size').value = 'default';
    expect(mod.getCurrentWindowSize()).toBe('854x480');
  });

  it('预设尺寸直接返回', () => {
    document.getElementById('window-size').value = '1280x720';
    expect(mod.getCurrentWindowSize()).toBe('1280x720');
  });

  it('自定义尺寸需同时提供宽高，否则回退 1920x1080', () => {
    const size = document.getElementById('window-size');
    size.value = 'custom';
    document.getElementById('custom-width').value = '1000';
    document.getElementById('custom-height').value = '700';
    expect(mod.getCurrentWindowSize()).toBe('1000x700');

    document.getElementById('custom-height').value = '';
    expect(mod.getCurrentWindowSize()).toBe('1920x1080');
  });

  it('取消启动会清理轮询、调用取消接口并恢复按钮', async () => {
    const tick = vi.fn();
    S.launchDlPollInterval = setInterval(tick, 100);
    mockApiRoute('/api/launch/cancel', {});
    document.getElementById('launch-btn').disabled = true;
    document.getElementById('home-launch-btn').disabled = true;
    document.getElementById('game-launch-overlay').style.display = 'flex';
    S.launching = true;

    mod.cancelLaunchFlow();
    await vi.advanceTimersByTimeAsync(300);

    expect(S.launchDlPollInterval).toBeNull();
    expect(tick).not.toHaveBeenCalled();
    expect(getApiCalls().some((c) => c.path === '/api/launch/cancel')).toBe(true);
    expect(document.getElementById('game-launch-overlay').style.display).toBe('none');
    expect(document.getElementById('launch-btn').disabled).toBe(false);
    expect(document.getElementById('home-launch-btn').disabled).toBe(false);
    expect(S.launching).toBe(false);
  });
});
