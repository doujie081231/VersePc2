// @vitest-environment jsdom
// versions-loaders.test.js — 加载器选择、Fabric API 选择与安装编排单元测试
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { getApiCalls, getCommandCalls, installTauriMock, mockApiRoute } from '../../test-utils/tauri-mock.js';

let mod;
let S;
let appState;

function buildDom() {
  document.body.innerHTML = `
    <div id="modloader-modal" class="modal">
      <div class="modal-body">
        <div class="form-group" id="fg-game"></div>
        <div class="form-group" id="fg-loader"></div>
      </div>
      <div class="modal-footer" id="modloader-footer"></div>
      <div id="modloader-game-version-wrapper"></div>
      <div id="modloader-version-wrapper"></div>
      <button id="modloader-install-btn"></button>
      <div id="fabric-api-section" style="display:none"></div>
      <div id="fabric-api-card-text"></div>
      <div id="fabric-api-picker-view" style="display:none">
        <div id="fabric-api-list"></div>
      </div>
    </div>
    <div id="page-version-detail">
      <div id="pvd-body"></div>
      <div id="pvd-main"></div>
      <div id="pvd-footer"></div>
      <div id="fabric-api-card-text-detail"></div>
      <div id="fabric-api-list-detail"></div>
    </div>
    <div id="fabric-api-picker-detail" style="display:none"></div>
    <div id="toast-container"></div>
  `;
}

function makeSelect(initial = '1.20.1') {
  return {
    value: initial,
    options: [],
    getValue() { return this.value; },
    setValue(v) { this.value = v; },
    setOptions(options) { this.options = options; },
  };
}

const FABRIC_VERSIONS = {
  versions: [
    { version: '0.15.0', stable: true },
    { version: '0.16.0-beta', stable: false },
  ],
};

const FABRIC_API = {
  recommended: 'api-2',
  versions: [
    { versionId: 'api-1', versionNumber: '0.92.0', releaseType: 'beta', datePublished: '2024-05-01T10:00:00Z' },
    { versionId: 'api-2', versionNumber: '0.91.0', releaseType: 'release', datePublished: '2024-04-01T10:00:00Z' },
  ],
};

beforeAll(async () => {
  installTauriMock();
  vi.resetModules();
  S = (await import('./versions-state.js')).versionsState;
  appState = (await import('../../core/app-state.js')).appState;
  mod = await import('./versions-loaders.js');
});

beforeEach(() => {
  installTauriMock();
  buildDom();
  mockApiRoute('/api/versions', { versions: [], installed: [] });
  mockApiRoute('/api/fabric/versions', FABRIC_VERSIONS);
  mockApiRoute('/api/forge/versions', { versions: [{ version: '47.2.0', type: 'release' }] });
  mockApiRoute('/api/neoforge/versions', { versions: [{ version: '20.4.1', type: 'beta' }] });
  mockApiRoute('/api/fabric-api/versions', FABRIC_API);

  appState.currentLoaderType = '';
  appState.installedVersions = [{ id: '1.20.1' }, { id: 'fabric-1.20.1', isFabric: true }];
  appState.allVersions = [{ id: '1.20.1', type: 'release' }, { id: '1.19.4', type: 'release' }];
  appState.modloaderGameVersionCustomSelect = makeSelect('1.20.1');
  appState.modloaderVersionCustomSelect = makeSelect('0.15.0');
  appState.launchVersionCustomSelect = makeSelect('');
  appState.currentVersionDetail = null;
  S.currentVersionDetail = null;

  S._fabricApiPickerLoadedFor = '';
  S._fabricApiSelectedId = '';
  S._fabricApiSelectedText = '';
  S._fabricApiSelectedFile = { url: '', filename: '' };
  S._fabricApiRecommendedId = '';
  S._fabricApiVersionsCache = [];
  S._fabricApiDetailLoadedFor = '';
  S._fabricApiDetailVersions = [];
  S._fabricApiDetailId = '';
  S._fabricApiDetailText = '不安装';
  S._fabricApiDetailRecommendedId = '';
});

afterEach(() => {
  vi.restoreAllMocks();
  document.body.innerHTML = '';
});

describe('openModLoaderModal / closeModLoaderModal', () => {
  it('打开弹窗时优先使用已安装的原版版本', () => {
    mod.openModLoaderModal('1.20.1');

    expect(document.getElementById('modloader-modal').getAttribute('data-state')).toBe('open');
    expect(appState.modloaderGameVersionCustomSelect.options).toEqual([{ value: '1.20.1', text: '1.20.1' }]);
    expect(appState.modloaderGameVersionCustomSelect.getValue()).toBe('1.20.1');
  });

  it('没有已安装原版版本时回退到可用正式版', () => {
    appState.installedVersions = [{ id: 'fabric-1.20.1', isFabric: true }];
    mod.openModLoaderModal('');

    expect(appState.modloaderGameVersionCustomSelect.options.map((o) => o.value)).toEqual(['1.20.1', '1.19.4']);
  });

  it('关闭弹窗恢复表单与底部按钮', () => {
    document.getElementById('fabric-api-picker-view').style.display = '';
    document.getElementById('modloader-footer').style.display = 'none';
    document.getElementById('fg-game').style.display = 'none';

    mod.closeModLoaderModal();

    expect(document.getElementById('modloader-modal').getAttribute('data-state')).toBe('closed');
    expect(document.getElementById('fabric-api-picker-view').style.display).toBe('none');
    expect(document.getElementById('modloader-footer').style.display).toBe('');
    expect(document.getElementById('fg-game').style.display).toBe('');
  });
});

describe('loadModLoaderVersions', () => {
  it('Fabric 版本写入选择器并选中稳定版', async () => {
    appState.currentLoaderType = 'fabric';
    await mod.loadModLoaderVersions();

    expect(appState.modloaderVersionCustomSelect.options).toEqual([
      { value: '0.15.0', text: '0.15.0 (稳定)' },
      { value: '0.16.0-beta', text: '0.16.0-beta ' },
    ]);
    expect(appState.modloaderVersionCustomSelect.getValue()).toBe('0.15.0');
    expect(document.getElementById('fabric-api-section').style.display).toBe('');
    expect(document.getElementById('fabric-api-card-text').textContent).toBe('不安装');
  });

  it('Forge 版本带上发布类型', async () => {
    appState.currentLoaderType = 'forge';
    await mod.loadModLoaderVersions();

    expect(appState.modloaderVersionCustomSelect.options).toEqual([
      { value: '47.2.0', text: '47.2.0 (release)' },
    ]);
  });

  it('NeoForge 默认选中第一个版本', async () => {
    appState.currentLoaderType = 'neoforge';
    await mod.loadModLoaderVersions();

    expect(appState.modloaderVersionCustomSelect.options).toEqual([
      { value: '20.4.1', text: '20.4.1 (beta)' },
    ]);
    expect(appState.modloaderVersionCustomSelect.getValue()).toBe('20.4.1');
  });

  it('NeoForge 列表为空时清空上一个游戏版本残留的选择', async () => {
    appState.currentLoaderType = 'neoforge';
    appState.modloaderVersionCustomSelect = makeSelect('0.20.1');
    mockApiRoute('/api/neoforge/versions', { versions: [] });

    await mod.loadModLoaderVersions();

    expect(appState.modloaderVersionCustomSelect.getValue()).toBe('');
    expect(appState.modloaderVersionCustomSelect.options).toEqual([
      { value: '', text: '该游戏版本暂无可用 NeoForge' },
    ]);
  });

  it('Fabric 列表为空时清空旧选择', async () => {
    appState.currentLoaderType = 'fabric';
    appState.modloaderVersionCustomSelect = makeSelect('0.15.0');
    mockApiRoute('/api/fabric/versions', { versions: [] });

    await mod.loadModLoaderVersions();

    expect(appState.modloaderVersionCustomSelect.getValue()).toBe('');
    expect(appState.modloaderVersionCustomSelect.options).toEqual([
      { value: '', text: '该游戏版本暂无可用 Fabric' },
    ]);
  });

  it('非 Fabric 类型隐藏 Fabric API 区域', async () => {
    appState.currentLoaderType = '';
    document.getElementById('fabric-api-section').style.display = '';
    await mod.loadModLoaderVersions();
    expect(document.getElementById('fabric-api-section').style.display).toBe('none');
  });
});

describe('Fabric API 选择器', () => {
  it('未选游戏版本时提示且不打开选择器', () => {
    appState.modloaderGameVersionCustomSelect = makeSelect('');
    mod.openFabricApiPicker();
    expect(document.getElementById('toast-container').textContent).toContain('请先选择游戏版本');
    expect(document.getElementById('fabric-api-picker-view').style.display).toBe('none');
  });

  it('打开选择器后加载列表并默认选中推荐版本', async () => {
    appState.modloaderGameVersionCustomSelect = makeSelect('1.20.1');
    mod.openFabricApiPicker();

    expect(document.getElementById('fabric-api-picker-view').style.display).toBe('');
    expect(document.getElementById('modloader-footer').style.display).toBe('none');
    expect(document.getElementById('fg-game').style.display).toBe('none');

    await vi.waitFor(() => {
      expect(document.querySelectorAll('#fabric-api-list .fabric-api-item').length).toBe(3);
    });
    expect(S._fabricApiPickerLoadedFor).toBe('1.20.1');
    expect(S._fabricApiSelectedId).toBe('api-2');
    expect(document.getElementById('fabric-api-card-text').textContent).toBe('0.91.0 (稳定)');
    expect(document.getElementById('fabric-api-list').textContent).toContain('推荐');
    expect(document.getElementById('fabric-api-list').textContent).toContain('0.92.0');
    expect(document.getElementById('fabric-api-list').textContent).toContain('测试 · 2024-05-01');
    expect(document.getElementById('fabric-api-list').textContent).toContain('稳定 · 2024-04-01');
    expect(document.querySelectorAll('#fabric-api-list .selected').length).toBe(1);
  });

  it('再次打开同一版本不重复请求', async () => {
    appState.modloaderGameVersionCustomSelect = makeSelect('1.20.1');
    mod.openFabricApiPicker();
    await vi.waitFor(() => expect(S._fabricApiPickerLoadedFor).toBe('1.20.1'));
    const firstCount = getApiCalls().filter((c) => c.path === '/api/fabric-api/versions').length;

    mod.backFromFabricApiPicker();
    mod.openFabricApiPicker();
    expect(getApiCalls().filter((c) => c.path === '/api/fabric-api/versions').length).toBe(firstCount);
  });

  it('无可用版本时展示空提示', async () => {
    mockApiRoute('/api/fabric-api/versions', { versions: [], recommended: '' });
    await mod.loadFabricApiVersions('1.20.1');
    expect(document.getElementById('fabric-api-list').textContent).toContain('暂无可用的 Fabric API');
  });

  it('加载失败时展示转义后的错误', async () => {
    mockApiRoute('/api/fabric-api/versions', { status: 500, body: { error: '<boom>' } });
    await mod.loadFabricApiVersions('1.20.1');
    const html = document.getElementById('fabric-api-list').innerHTML;
    expect(html).toContain('加载失败');
    expect(html).toContain('&lt;boom&gt;');
  });

  it('返回主表单时恢复布局与区域显示', () => {
    document.getElementById('fabric-api-picker-view').style.display = '';
    document.getElementById('modloader-footer').style.display = 'none';
    document.getElementById('fg-game').style.display = 'none';
    appState.currentLoaderType = 'fabric';
    document.getElementById('fabric-api-section').style.display = 'none';

    mod.backFromFabricApiPicker();

    expect(document.getElementById('fabric-api-picker-view').style.display).toBe('none');
    expect(document.getElementById('modloader-footer').style.display).toBe('');
    expect(document.getElementById('fg-game').style.display).toBe('');
    expect(document.getElementById('fabric-api-section').style.display).toBe('');
  });

  it('选择具体版本写入文件信息与卡片文案', () => {
    S._fabricApiVersionsCache = FABRIC_API.versions;
    mod.selectFabricApiVersion('api-1');

    expect(S._fabricApiSelectedId).toBe('api-1');
    expect(S._fabricApiSelectedText).toBe('0.92.0 (测试)');
    expect(S._fabricApiSelectedFile).toEqual({ url: '', filename: '' });
    expect(document.getElementById('fabric-api-card-text').textContent).toBe('0.92.0 (测试)');
    expect(document.getElementById('fabric-api-card-text').style.color).toBe('var(--text-primary)');
  });

  it('选择不安装时清空状态', () => {
    S._fabricApiVersionsCache = FABRIC_API.versions;
    S._fabricApiSelectedFile = { url: 'x', filename: 'y.jar' };
    mod.selectFabricApiVersion('');

    expect(S._fabricApiSelectedId).toBe('');
    expect(S._fabricApiSelectedText).toBe('不安装');
    expect(S._fabricApiSelectedFile).toEqual({ url: '', filename: '' });
    expect(document.getElementById('fabric-api-card-text').style.color).toBe('var(--text-muted)');
  });
});

describe('installModLoader', () => {
  it('未选游戏版本时提示', async () => {
    appState.modloaderGameVersionCustomSelect = makeSelect('');
    await mod.installModLoader();
    expect(document.getElementById('toast-container').textContent).toContain('请选择游戏版本');
    expect(getApiCalls().some((c) => c.path.includes('/install'))).toBe(false);
  });

  it('不支持的加载器类型提示', async () => {
    appState.currentLoaderType = 'optifine';
    await mod.installModLoader();
    expect(document.getElementById('toast-container').textContent).toContain('不支持的加载器类型');
  });

  it('Fabric 安装成功后刷新版本并记录最后启动版本', async () => {
    appState.currentLoaderType = 'fabric';
    mockApiRoute('/api/fabric/install', { success: true });

    await mod.installModLoader();

    const call = getApiCalls().find((c) => c.path === '/api/fabric/install');
    expect(call.args.body.gameVersion).toBe('1.20.1');
    expect(call.args.body.loaderVersion).toBe('0.15.0');
    expect(appState.launchVersionCustomSelect.getValue()).toBe('fabric-loader-0.15.0-1.20.1');
    expect(getCommandCalls().some((c) => c.cmd === 'store_set' && c.args.key === 'versepc_last_launch_version')).toBe(true);
    expect(document.getElementById('toast-container').textContent).toContain('Fabric 安装成功！');
    expect(document.getElementById('modloader-modal').getAttribute('data-state')).toBe('closed');
    expect(document.getElementById('modloader-install-btn').disabled).toBe(false);
  });

  it('Fabric 安装后按需安装 Fabric API', async () => {
    appState.currentLoaderType = 'fabric';
    S._fabricApiSelectedId = 'api-1';
    S._fabricApiVersionsCache = [{ ...FABRIC_API.versions[0], url: 'https://x/api.jar', filename: 'api.jar' }];
    mockApiRoute('/api/fabric/install', { success: true, versionId: 'fabric-x' });
    mockApiRoute('/api/fabric-api/install', { success: true });

    await mod.installModLoader();

    const call = getApiCalls().find((c) => c.path === '/api/fabric-api/install');
    expect(call.args.body).toMatchObject({
      gameVersion: '1.20.1',
      versionId: 'api-1',
      versionName: 'fabric-x',
      fileUrl: 'https://x/api.jar',
      filename: 'api.jar',
    });
    expect(document.getElementById('toast-container').textContent).toContain('Fabric API 已一并安装');
  });

  it('Fabric API 安装失败时给出警告文案', async () => {
    appState.currentLoaderType = 'fabric';
    S._fabricApiSelectedId = 'api-1';
    mockApiRoute('/api/fabric/install', { success: true });
    mockApiRoute('/api/fabric-api/install', { success: false, error: '网络中断' });

    await mod.installModLoader();

    expect(document.getElementById('toast-container').textContent).toContain('Fabric API 安装失败：网络中断');
    expect(document.getElementById('toast-container').textContent).toContain('但 Fabric API 安装失败');
  });

  it('Forge 缺少版本号时提示且不请求安装', async () => {
    appState.currentLoaderType = 'forge';
    appState.modloaderVersionCustomSelect = makeSelect('');

    await mod.installModLoader();

    expect(document.getElementById('toast-container').textContent).toContain('请选择Forge版本');
    expect(getApiCalls().some((c) => c.path === '/api/forge/install')).toBe(false);
  });

  it('Forge 安装成功', async () => {
    appState.currentLoaderType = 'forge';
    mockApiRoute('/api/forge/install', { success: true });

    await mod.installModLoader();

    const call = getApiCalls().find((c) => c.path === '/api/forge/install');
    expect(call.args.body.forgeVersion).toBe('0.15.0');
    expect(document.getElementById('toast-container').textContent).toContain('Forge 安装成功');
  });

  it('NeoForge 缺少版本号时提示', async () => {
    appState.currentLoaderType = 'neoforge';
    appState.modloaderVersionCustomSelect = makeSelect('');
    await mod.installModLoader();
    expect(document.getElementById('toast-container').textContent).toContain('请选择NeoForge版本');
  });

  it('安装接口返回失败时展示后端错误', async () => {
    appState.currentLoaderType = 'fabric';
    mockApiRoute('/api/fabric/install', { success: false, error: '镜像不可用' });

    await mod.installModLoader();

    expect(document.getElementById('toast-container').textContent).toContain('镜像不可用');
  });

  it('安装请求异常时提示失败并恢复按钮', async () => {
    appState.currentLoaderType = 'fabric';
    const btn = document.getElementById('modloader-install-btn');
    btn.textContent = '安装';
    mockApiRoute('/api/fabric/install', { status: 500, body: { error: 'server down' } });

    await mod.installModLoader();

    expect(document.getElementById('toast-container').textContent).toContain('安装失败');
    expect(btn.textContent).toBe('安装');
    expect(btn.disabled).toBe(false);
  });
});

describe('版本详情页 Fabric API 选择器', () => {
  it('未打开版本详情时不动作', () => {
    S.currentVersionDetail = null;
    mod.openFabricApiDetailPicker();
    expect(document.getElementById('fabric-api-picker-detail').style.display).toBe('none');
  });

  it('打开选择器迁移节点并默认选中推荐版本', async () => {
    S.currentVersionDetail = { id: '1.20.1' };
    mod.openFabricApiDetailPicker();

    expect(document.getElementById('pvd-body').style.display).toBe('none');
    expect(document.getElementById('pvd-main').style.display).toBe('none');
    expect(document.getElementById('pvd-footer').style.display).toBe('none');
    const picker = document.getElementById('fabric-api-picker-detail');
    expect(picker.style.display).toBe('');
    expect(picker.parentElement).toBe(document.body);

    await vi.waitFor(() => {
      expect(document.querySelectorAll('#fabric-api-list-detail .fabric-api-item').length).toBe(3);
    });
    expect(S._fabricApiDetailId).toBe('api-2');
    expect(S._fabricApiDetailText).toBe('0.91.0 (稳定)');
    expect(document.getElementById('fabric-api-card-text-detail').textContent).toBe('0.91.0 (稳定)');
    expect(document.getElementById('fabric-api-list-detail').textContent).toContain('推荐');
  });

  it('无可用版本时展示空提示', async () => {
    mockApiRoute('/api/fabric-api/versions', { versions: [], recommended: '' });
    await mod.loadFabricApiDetailVersions('1.20.1');
    expect(document.getElementById('fabric-api-list-detail').textContent).toContain('暂无可用的 Fabric API');
  });

  it('加载失败展示错误', async () => {
    mockApiRoute('/api/fabric-api/versions', { status: 500, body: { error: 'x' } });
    await mod.loadFabricApiDetailVersions('1.20.1');
    expect(document.getElementById('fabric-api-list-detail').textContent).toContain('加载失败');
  });

  it('返回时把选择器移回详情页并恢复布局', () => {
    S.currentVersionDetail = { id: '1.20.1' };
    mod.openFabricApiDetailPicker();
    document.getElementById('pvd-body').style.display = 'none';

    mod.backFromFabricApiDetailPicker();

    expect(document.getElementById('fabric-api-picker-detail').style.display).toBe('none');
    expect(document.getElementById('fabric-api-picker-detail').parentElement).toBe(document.getElementById('page-version-detail'));
    expect(document.getElementById('pvd-body').style.display).toBe('');
    expect(document.getElementById('pvd-main').style.display).toBe('');
    expect(document.getElementById('pvd-footer').style.display).toBe('');
  });

  it('选择具体版本更新卡片与状态', () => {
    mod.selectFabricApiDetailVersion('api-1', '0.92.0 (测试)');

    expect(S._fabricApiDetailId).toBe('api-1');
    expect(S._fabricApiDetailText).toBe('0.92.0 (测试)');
    expect(document.getElementById('fabric-api-card-text-detail').textContent).toBe('0.92.0 (测试)');
    expect(document.getElementById('fabric-api-card-text-detail').style.color).toBe('var(--text-primary)');
  });

  it('选择不安装时恢复占位文案', () => {
    mod.selectFabricApiDetailVersion('', '');
    expect(S._fabricApiDetailId).toBe('');
    expect(S._fabricApiDetailText).toBe('不安装');
    expect(document.getElementById('fabric-api-card-text-detail').textContent).toBe('不安装');
    expect(document.getElementById('fabric-api-card-text-detail').style.color).toBe('var(--text-muted)');
  });
});
