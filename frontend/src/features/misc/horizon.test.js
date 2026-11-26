// @vitest-environment jsdom
// horizon.test.js — Rust 版（Horizon）页面状态机与事件驱动
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { getCommandCalls, installTauriMock } from '../../test-utils/tauri-mock.js';

const $ = (id) => document.getElementById(id);

let statusResult;
let stepCode;
let statusThrow;
let listenHandlers;
let stepArgs;
let mod;

function buildDom() {
  document.body.innerHTML = `
    <div id="page-rust">
      <div id="rust-detail"></div>
      <textarea id="rust-log"></textarea>
      <div id="rust-bar-fill"></div>
      <button id="rust-primary"></button>
      <button id="rust-prepare"></button>
      <button id="rust-signin"></button>
      <button id="rust-stop"></button>
      <div id="rust-signin-panel" hidden>
        <span id="rust-signin-code">—</span>
        <span id="rust-signin-url"></span>
        <button id="rust-signin-copy"></button>
        <button id="rust-signin-open"></button>
        <button id="rust-signin-cancel"></button>
      </div>
      <button id="rust-site"></button>
    </div>
  `;
}

async function boot(status) {
  statusResult = status;
  stepCode = 0;
  buildDom();
  installTauriMock({
    commands: {
      horizon_status: () => {
        if (statusThrow) throw new Error('status boom');
        return statusResult;
      },
      horizon_step: (args) => {
        stepArgs.push(args);
        return stepCode;
      },
      horizon_install_runtime: () => {
        statusResult = { ...statusResult, runtime: true };
        return statusResult;
      },
      horizon_stop: () => null,
      open_external: (args) => ({ opened: args.url }),
    },
  });
  window.__TAURI_INTERNALS__.listen = async (name, cb) => {
    listenHandlers.set(name, cb);
    return () => {};
  };
  vi.resetModules();
  mod = await import('./horizon.js');
  return mod;
}

/** 触发后端事件；记录的是桥接层包装后的回调，入参与真实事件一致：{ payload } */
async function fire(name, payload) {
  const handler = listenHandlers.get(name);
  if (handler) await handler({ payload });
}

/** 通过 step-done 事件触发一次 refresh + render */
async function renderNow() {
  await fire('horizon-step-done', { step: 'render', code: 0 });
  await vi.waitFor(() => expect($('rust-detail').textContent).not.toBe(''));
}

beforeEach(() => {
  listenHandlers = new Map();
  stepArgs = [];
  statusThrow = false;
  document.body.innerHTML = '';
});

afterEach(() => {
  vi.restoreAllMocks();
  document.body.innerHTML = '';
});

describe('状态渲染', () => {
  it('内置运行时不可用时引导修复', async () => {
    await boot({ runtime: false, account: '', installed: [], prepared: false });
    await renderNow();
    expect($('rust-detail').textContent).toContain('内置运行时不可用');
    expect($('rust-primary').textContent).toBe('修复运行环境');
    expect($('rust-primary').dataset.rustAction).toBe('runtime');
  });

  it('缺少正版账号时引导登录', async () => {
    await boot({ runtime: true, account: '', installed: ['1.0'], prepared: false });
    await renderNow();
    expect($('rust-detail').textContent).toContain('需要登录');
    expect($('rust-primary').textContent).toBe('登录 Microsoft 账号');
    expect($('rust-primary').dataset.rustAction).toBe('signin');
    expect($('rust-prepare').disabled).toBe(true);
  });

  it('没有已安装版本时引导安装', async () => {
    await boot({ runtime: true, account: 'Steve', installed: [], prepared: false });
    await renderNow();
    expect($('rust-detail').textContent).toContain('还没有安装 Horizon 版本');
    expect($('rust-primary').dataset.rustAction).toBe('install');
  });

  it('资源未准备时引导准备', async () => {
    await boot({ runtime: true, account: 'Steve', installed: ['1.0'], prepared: false });
    await renderNow();
    expect($('rust-detail').textContent).toContain('资源未准备');
    expect($('rust-primary').dataset.rustAction).toBe('prepare');
    expect($('rust-prepare').disabled).toBe(false);
  });

  it('已准备时显示启动游戏', async () => {
    await boot({ runtime: true, account: 'Steve', installed: ['1.0'], prepared: true });
    await renderNow();
    expect($('rust-primary').textContent).toBe('启动游戏');
    expect($('rust-primary').dataset.rustAction).toBe('play');
    expect($('rust-detail').textContent).toContain('Steve');
  });

  it('读取状态失败时显示错误', async () => {
    statusThrow = true;
    await boot({ runtime: true, account: '', installed: [], prepared: false });
    await fire('horizon-step-done', { step: 'x', code: 0 });
    await vi.waitFor(() => expect($('rust-detail').textContent).toContain('状态读取失败'));
    expect($('rust-detail').textContent).toContain('status boom');
  });
});

describe('主按钮动作', () => {
  it('安装版本调用 horizon_step(install)', async () => {
    await boot({ runtime: true, account: 'Steve', installed: [], prepared: false });
    await renderNow();
    $('rust-primary').click();
    await vi.waitFor(() => {
      expect(getCommandCalls().some((c) => c.cmd === 'horizon_step' && c.args.step === 'install')).toBe(true);
    });
  });

  it('修复运行环境调用在线下载并填满进度', async () => {
    await boot({ runtime: false, account: '', installed: [], prepared: false });
    await renderNow();
    $('rust-primary').click();
    await vi.waitFor(() => {
      expect(getCommandCalls().some((c) => c.cmd === 'horizon_install_runtime')).toBe(true);
    });
    await vi.waitFor(() => expect($('rust-bar-fill').style.width).toBe('100%'));
  });

  it('准备与启动分别调用对应步骤', async () => {
    await boot({ runtime: true, account: 'Steve', installed: ['1.0'], prepared: false });
    await renderNow();
    $('rust-primary').click();
    await vi.waitFor(() => expect(stepArgs.some((a) => a.step === 'prepare')).toBe(true));

    statusResult = { runtime: true, account: 'Steve', installed: ['1.0'], prepared: true };
    await renderNow();
    $('rust-primary').click();
    await vi.waitFor(() => expect(stepArgs.some((a) => a.step === 'play')).toBe(true));
  });

  it('步骤退出码非零时写入错误日志', async () => {
    await boot({ runtime: true, account: 'Steve', installed: [], prepared: false });
    await renderNow();
    stepCode = 3;
    $('rust-primary').click();
    await vi.waitFor(() => expect($('rust-log').textContent).toContain('步骤 install 退出码 3'));
  });
});

describe('后端事件', () => {
  it('进度事件更新进度条与按钮文案', async () => {
    await boot({ runtime: false, account: '', installed: [], prepared: false });
    await renderNow();
    await fire('horizon-progress', { done: 5 * 1048576, total: 27 * 1048576 });
    expect($('rust-primary').textContent).toBe('5.0 MB / 27.0 MB');
    expect(parseFloat($('rust-bar-fill').style.width)).toBeCloseTo((5 / 27) * 100, 1);
  });

  it('日志事件写入日志并按级别加前缀', async () => {
    await boot({ runtime: true, account: '', installed: [], prepared: false });
    await fire('horizon-log', { line: 'hello', kind: 'info' });
    await fire('horizon-log', { line: 'oops', kind: 'err' });
    const text = $('rust-log').textContent;
    expect(text).toContain('· hello');
    expect(text).toContain('! oops');
  });

  it('日志中的设备码会展开面板并打开浏览器', async () => {
    await boot({ runtime: true, account: '', installed: [], prepared: false });
    await fire('horizon-log', {
      line: 'To sign in, open https://www.microsoft.com/link in a browser and enter the code ABCD-1234',
      kind: 'info',
    });
    expect($('rust-signin-panel').hidden).toBe(false);
    expect($('rust-signin-code').textContent).toBe('ABCD-1234');
    expect($('rust-signin-url').textContent).toBe('www.microsoft.com/link');
    expect(getCommandCalls().some((c) => c.cmd === 'open_external' && c.args.url === 'https://www.microsoft.com/link')).toBe(true);
  });

  it('步骤结束事件写入日志并刷新状态', async () => {
    await boot({ runtime: true, account: 'Steve', installed: ['1.0'], prepared: true });
    await fire('horizon-step-done', { step: 'prepare', code: 0 });
    expect($('rust-log').textContent).toContain('[prepare] 结束，退出码 0');
    await vi.waitFor(() => expect($('rust-primary').dataset.rustAction).toBe('play'));
  });
});

describe('辅助按钮', () => {
  it('停止游戏调用 horizon_stop', async () => {
    await boot({ runtime: true, account: 'Steve', installed: ['1.0'], prepared: true });
    await renderNow();
    // 未运行时停止按钮是 disabled 的，合成点击绕开浏览器对该状态的拦截
    $('rust-stop').dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await vi.waitFor(() => expect(getCommandCalls().some((c) => c.cmd === 'horizon_stop')).toBe(true));
    await vi.waitFor(() => expect($('rust-log').textContent).toContain('已请求停止游戏'));
  });

  it('取消登录会停止等待并收起面板', async () => {
    await boot({ runtime: true, account: '', installed: [], prepared: false });
    await fire('horizon-log', {
      line: 'open https://www.microsoft.com/link in a browser and enter the code WXYZ-9999',
      kind: 'info',
    });
    $('rust-signin-cancel').dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await vi.waitFor(() => expect(getCommandCalls().some((c) => c.cmd === 'horizon_stop')).toBe(true));
    await vi.waitFor(() => expect($('rust-signin-panel').hidden).toBe(true));
    expect($('rust-signin-code').textContent).toBe('—');
  });

  it('复制设备码写入剪贴板', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    await boot({ runtime: true, account: '', installed: [], prepared: false });
    $('rust-signin-code').textContent = 'CODE-1';
    $('rust-signin-copy').click();
    await vi.waitFor(() => expect(writeText).toHaveBeenCalledWith('CODE-1'));
    expect($('rust-log').textContent).toContain('设备码 CODE-1 已复制');
  });

  it('打开登录地址与官网', async () => {
    const openSpy = vi.spyOn(window, 'open').mockImplementation(() => null);
    await boot({ runtime: true, account: '', installed: [], prepared: false });
    $('rust-signin-open').click();
    expect(getCommandCalls().some((c) => c.cmd === 'open_external' && c.args.url === 'https://www.microsoft.com/link')).toBe(true);

    $('rust-site').click();
    expect(openSpy).toHaveBeenCalledWith('https://playhorizon.dev', '_blank');
  });
});
