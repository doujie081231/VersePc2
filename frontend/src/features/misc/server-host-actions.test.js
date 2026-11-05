// @vitest-environment jsdom
// server-host-actions.test.js — 开服页列表、创建、启停、命令与删除流程单元测试
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  getCommandCalls,
  installTauriMock,
  resetTauriMock,
} from '../../test-utils/tauri-mock.js';

let host;
let customSelect;

const SERVERS = [
  {
    id: 's1',
    name: '测试服',
    versionId: '1.20.1',
    loader: 'fabric',
    loaderVersion: '0.15.0',
    status: 'stopped',
    port: 25565,
    maxMem: 2048,
  },
];

function mountHost() {
  document.body.innerHTML = `
    <div id="server-host-list"></div>
    <div id="server-host-console"></div>
    <input id="server-host-name" value="测试服" />
    <input id="server-host-port" value="25565" />
    <input id="server-host-mem" value="2048" />
    <input id="server-host-online" type="checkbox" checked />
    <input id="server-host-sync-mods" type="checkbox" checked />
    <input id="server-host-cmd" />
    <button id="server-host-create-btn"></button>
    <button id="server-host-cancel-btn"></button>
    <button id="server-host-start-btn"></button>
    <button id="server-host-stop-btn"></button>
    <div id="server-host-status"></div>
    <div id="server-host-active-badge"></div>
    <div id="server-host-active-msg"></div>
    <div id="server-host-progress"></div>
    <div id="server-host-progress-bar"></div>
    <div id="server-host-progress-meta"></div>
    <div id="server-host-progress-text"></div>
    <div id="server-host-progress-pct"></div>
  `;
}

async function loadHost(commands = {}) {
  installTauriMock({
    commands: {
      server_host_list: () => ({ ok: true, servers: SERVERS.map((item) => ({ ...item })) }),
      server_host_create: () => ({ ok: true, server: { id: 's1', path: 'D:/srv' }, modSync: null }),
      server_host_start: () => ({ ok: true, port: 25565, localIps: ['192.168.1.2'] }),
      server_host_stop: () => ({ ok: true, forced: false }),
      server_host_command: () => ({ ok: true }),
      server_host_delete: () => ({ ok: true }),
      server_host_open_dir: () => ({ ok: true }),
      ...commands,
    },
  });
  vi.resetModules();
  customSelect = await import('./custom-select.js');
  customSelect.customSelectInstances['server-host-version'] = {
    getValue: () => '1.20.1',
    setValue: () => {},
    setOptions: () => {},
  };
  return import('./server-host.js');
}

beforeEach(() => {
  mountHost();
});

afterEach(() => {
  resetTauriMock();
  delete window.__TAURI_INTERNALS__;
  delete customSelect?.customSelectInstances?.['server-host-version'];
  vi.restoreAllMocks();
});

describe('版本选择', () => {
  it('刷新版本后主动选中第一个版本，触发器不再停留在加载中', async () => {
    host = await loadHost();
    const appState = (await import('../../core/app-state.js')).appState;
    appState.installedVersions = [
      { id: '1.21.1' },
      { id: '1.20.1' },
    ];
    const inst = customSelect.customSelectInstances['server-host-version'];
    const setOptions = vi.fn();
    const setValue = vi.fn();
    inst.setOptions = setOptions;
    inst.setValue = setValue;

    await host.serverHostRefreshVersions();

    expect(setOptions).toHaveBeenCalled();
    expect(setValue).toHaveBeenCalledWith('1.20.1');
    expect(document.getElementById('server-host-status').textContent).toContain('已加载');
  });
});

describe('列表与控制台', () => {
  it('刷新列表渲染服务器卡片', async () => {
    host = await loadHost();
    await host.serverHostRefreshList();
    const list = document.getElementById('server-host-list');
    expect(list.textContent).toContain('测试服');
    expect(list.textContent).toContain('Fabric 0.15.0');
    expect(list.textContent).toContain('端口 25565');
    expect(list.querySelector('.server-host-card.is-active')).not.toBeNull();
  });

  it('控制台追加日志并按类型着色，清空时恢复占位', async () => {
    host = await loadHost();
    host._shAppendConsole('Done (1.2s)!', 'out');
    host._shAppendConsole('[VersePC] 提示', 'sys');
    host._shAppendConsole('错误', 'err');
    const consoleEl = document.getElementById('server-host-console');
    expect(consoleEl.children.length).toBe(3);
    expect(consoleEl.textContent).toContain('Done (1.2s)!');
    expect(consoleEl.querySelector('.sh-console-line.ok')).not.toBeNull();
    expect(consoleEl.querySelector('.sh-console-line.sys')).not.toBeNull();
    expect(consoleEl.querySelector('.sh-console-line.err')).not.toBeNull();

    host._shClearConsole();
    expect(consoleEl.textContent).toContain('等待服务器输出');
  });
});

describe('创建与启停', () => {
  it('创建服务器发送版本与选项并刷新列表', async () => {
    host = await loadHost();
    await host.serverHostCreate();
    const call = getCommandCalls().find((item) => item.cmd === 'server_host_create');
    expect(call).toBeDefined();
    expect(call.args).toMatchObject({
      versionId: '1.20.1',
      name: '测试服',
      port: 25565,
      options: { maxMem: 2048, onlineMode: true, syncMods: true },
    });
    expect(document.getElementById('server-host-status').textContent).toContain('D:/srv');
  });

  it('启动服务器追加日志并更新状态', async () => {
    host = await loadHost();
    await host.serverHostRefreshList();
    await host.serverHostStart();
    const consoleText = document.getElementById('server-host-console').textContent;
    expect(consoleText).toContain('请求启动服务器 s1');
    expect(document.getElementById('server-host-active-badge').textContent).toContain('启动中');
  });

  it('停止服务器发送 stop 请求', async () => {
    host = await loadHost();
    await host.serverHostRefreshList();
    await host.serverHostStop();
    const call = getCommandCalls().find((item) => item.cmd === 'server_host_stop');
    expect(call.args).toEqual({ serverId: 's1' });
  });

  it('发送命令后清空输入框', async () => {
    host = await loadHost();
    await host.serverHostRefreshList();
    const input = document.getElementById('server-host-cmd');
    input.value = 'say hello';
    await host.serverHostSendCommand();
    const call = getCommandCalls().find((item) => item.cmd === 'server_host_command');
    expect(call.args).toEqual({ serverId: 's1', cmd: 'say hello' });
    expect(input.value).toBe('');
  });
});

describe('删除服务器', () => {
  it('确认后删除并清空控制台', async () => {
    host = await loadHost();
    await host.serverHostRefreshList();
    vi.stubGlobal('confirm', () => true);
    await host.serverHostDelete();
    const call = getCommandCalls().find((item) => item.cmd === 'server_host_delete');
    expect(call.args).toEqual({ serverId: 's1' });
    expect(document.getElementById('server-host-console').textContent).toContain('等待服务器输出');
  });
});
