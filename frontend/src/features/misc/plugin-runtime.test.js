// @vitest-environment jsdom
// plugin-runtime.test.js — 插件 UI 注册、组件挂载与权限兼容层单元测试
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { getCommandCalls, installTauriMock, mockCommand } from '../../test-utils/tauri-mock.js';

let mod;

const PLUGIN_DIR = 'C:\\plugins\\p1';
const b64 = (text) => Buffer.from(text, 'utf8').toString('base64');

const SCRIPTS = {
  [PLUGIN_DIR + '\\main.js']: b64(`
    window.__pluginMainRan = true;
    fetch('https://example.com').catch(function () { window.__pluginNetDenied = true; });
    PluginBridge.invoke('window_close').catch(function () { window.__pluginCmdDenied = true; });
    PluginBridge.readFileBuffer('C:\\\\other\\\\secret.txt').catch(function () { window.__pluginFileDenied = true; });
  `),
  [PLUGIN_DIR + '\\page.js']: b64(`
    window.VersePC = window.VersePC || {};
    window.VersePC['Plugin_p1_home'] = { render: function () { return null; } };
  `),
  [PLUGIN_DIR + '\\card.js']: b64(`
    window.VersePC = window.VersePC || {};
    window.VersePC['Plugin_p1_card_c1'] = { render: function () { return null; } };
  `),
  [PLUGIN_DIR + '\\pet.js']: b64(`
    window.VersePC = window.VersePC || {};
    window.VersePC['Plugin_p1_pet'] = { render: function () { return null; } };
  `),
  [PLUGIN_DIR + '\\icon.svg']: b64('<svg class="icon"><path d="M1 1"/></svg>'),
  [PLUGIN_DIR + '\\style.css']: b64('.plugin-p1 { color: red; }'),
};

const PLUGIN = {
  id: 'p1',
  installedDir: PLUGIN_DIR,
  permissions: {},
  ui: {
    main: 'main.js',
    style: 'style.css',
    pages: [{ id: 'home', title: '插件页', icon: 'icon.svg', component: 'page.js' }],
    cards: [{ id: 'c1', component: 'card.js', anchor: '#card-anchor' }],
    pet: { component: 'pet.js' },
  },
};

function buildDom() {
  document.body.innerHTML = `
    <div class="content-area"></div>
    <button id="nav-plugins-btn"></button>
    <div id="card-anchor"></div>
  `;
}

function installPluginMocks(list) {
  installTauriMock({ commands: { plugin_list: () => ({ plugins: list }), read_file_buffer: ({ path }) => SCRIPTS[path] ?? '' } });
}

beforeAll(async () => {
  installPluginMocks([]);
  const vueReady = await import('../../core/vue-ready.js');
  vueReady.markVueMountDone();
  mod = await import('./plugin-runtime.js');
});

beforeEach(() => {
  installPluginMocks([PLUGIN]);
  buildDom();
  delete window.__pluginMainRan;
  delete window.__pluginNetDenied;
  delete window.__pluginCmdDenied;
  delete window.__pluginFileDenied;
  delete window.__pluginNetOk;
  delete window.VersePC;
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  document.body.innerHTML = '';
});

describe('插件 UI 注册', () => {
  it('注册插件页面、导航按钮与图标', async () => {
    vi.stubGlobal('fetch', vi.fn(() => Promise.resolve({ ok: true })));

    await mod.reloadPluginUI();

    const page = document.getElementById('page-plugin_p1_home');
    expect(page).toBeTruthy();
    expect(page.className).toBe('page');
    expect(page.__plugin_mounted).toBe(true);

    const btn = document.getElementById('nav-btn-plugin_p1_home');
    expect(btn).toBeTruthy();
    expect(btn.dataset.page).toBe('plugin_p1_home');
    expect(btn.getAttribute('title')).toBe('插件页');
    expect(btn.textContent).toContain('插件页');
    expect(document.getElementById('nav-plugins-btn').nextElementSibling).toBe(btn);
    await vi.waitFor(() => {
      expect(btn.querySelector('.plugin-page-icon').innerHTML).toContain('<path');
    });

    expect(document.getElementById('plugin-style-p1').textContent).toContain('.plugin-p1');
    expect(window.__pluginMainRan).toBe(true);
  });

  it('向锚点注入卡片槽并挂载组件', async () => {
    await mod.reloadPluginUI();

    const slot = document.getElementById('plugin-card-p1-c1');
    expect(slot).toBeTruthy();
    expect(slot.className).toBe('plugin-card-slot');
    expect(document.getElementById('card-anchor').contains(slot)).toBe(true);
    expect(slot.__plugin_mounted).toBe(true);
  });

  it('创建宠物挂件容器并挂载组件', async () => {
    await mod.reloadPluginUI();

    const layer = document.getElementById('plugin-pet-layer');
    expect(layer).toBeTruthy();
    expect(layer.style.position).toBe('fixed');
    expect(layer.__plugin_mounted).toBe(true);
  });

  it('导航按钮点击不抛错', async () => {
    await mod.reloadPluginUI();
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
    expect(() => document.getElementById('nav-btn-plugin_p1_home').click()).not.toThrow();
    expect(warnSpy).toHaveBeenCalled();
  });

  it('重复初始化会清理旧 UI 后重建', async () => {
    await mod.reloadPluginUI();
    expect(document.querySelectorAll('.plugin-page-btn').length).toBe(1);

    installPluginMocks([]);
    await mod.reloadPluginUI();

    expect(document.querySelectorAll('.plugin-page-btn').length).toBe(0);
    expect(document.querySelectorAll('[id^="page-plugin_"]').length).toBe(0);
    expect(document.querySelectorAll('[id^="plugin-card-"]').length).toBe(0);
    expect(document.querySelectorAll('[id^="plugin-style-"]').length).toBe(0);
    expect(document.getElementById('plugin-pet-layer')).toBeNull();
  });

  it('插件列表失败时记录错误且不阻断', async () => {
    installTauriMock({ commands: { plugin_list: () => { throw new Error('ipc down'); } } });
    const errSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
    await expect(mod.reloadPluginUI()).resolves.toBeUndefined();
    expect(errSpy).toHaveBeenCalled();
  });

  it('缺少 ui 声明的插件被跳过', async () => {
    installPluginMocks([{ id: 'p2', installedDir: 'C:\\plugins\\p2' }]);
    await mod.reloadPluginUI();
    expect(document.querySelectorAll('.plugin-page-btn').length).toBe(0);
  });
});

describe('插件权限兼容层（非安全边界）', () => {
  it('插件与启动器共享 window/fetch 权限，不做虚假沙箱拦截', async () => {
    const realFetch = vi.fn(() => Promise.resolve({ ok: true, marker: 'real' }));
    vi.stubGlobal('fetch', realFetch);
    await mod.reloadPluginUI();
    // 插件脚本直接调用全局 fetch 时不会被替换/拦截
    await vi.waitFor(() => expect(realFetch).toHaveBeenCalled());
    expect(window.__pluginNetDenied).toBeUndefined();
    expect(window.__pluginMainRan).toBe(true);
  });

  it('未授权的桥命令与越界文件读取被拒绝', async () => {
    await mod.reloadPluginUI();
    await vi.waitFor(() => {
      expect(window.__pluginCmdDenied).toBe(true);
      expect(window.__pluginFileDenied).toBe(true);
    });
  });

  it('声明 network 权限后允许插件访问 fetch', async () => {
    const realFetch = vi.fn(() => Promise.resolve({ ok: true, marker: 'real' }));
    vi.stubGlobal('fetch', realFetch);
    installPluginMocks([{
      id: 'p4',
      installedDir: PLUGIN_DIR,
      permissions: { network: true },
      ui: { main: 'net.js' },
    }]);
    mockCommand('read_file_buffer', ({ path }) => (
      path === PLUGIN_DIR + '\\net.js'
        ? b64(`fetch('https://example.com').then(function (r) { window.__pluginNetOk = r.marker; });`)
        : ''
    ));
    await mod.reloadPluginUI();

    await vi.waitFor(() => expect(window.__pluginNetOk).toBe('real'));
    expect(realFetch).toHaveBeenCalled();
  });
});

describe('插件命令调用', () => {
  it('加载插件时调用 plugin_list', async () => {
    await mod.reloadPluginUI();
    expect(getCommandCalls().some((c) => c.cmd === 'plugin_list')).toBe(true);
  });
});
