// @vitest-environment jsdom
// accounts-list.test.js — 账户列表渲染、轮播切换与详情入口单元测试
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { getApiCalls, installTauriMock, mockApiRoute } from '../../test-utils/tauri-mock.js';

const ACCOUNTS = [
  { id: 'a1', username: 'Steve', uuid: '1111-2222-3333-4444', type: 'microsoft' },
  { id: 'a2', username: 'Alex', uuid: '', type: 'offline' },
  { id: 'a3', username: 'Third', uuid: '3333', type: 'thirdparty', serverUrl: 'https://example.com/api' },
];

let S;
let mod;

function buildDom() {
  document.body.innerHTML = `
    <div id="page-accounts" class="page active">
      <div class="accts-stage"></div>
      <div class="page-header"></div>
      <div id="accounts-list"></div>
      <div id="accts-carousel"><div id="accts-track"></div></div>
      <div id="page-account-detail" style="display:none">
        <div id="detail-username"></div>
        <div id="detail-uuid"></div>
        <div id="detail-skin-type"></div>
        <div id="detail-account-type"></div>
      </div>
    </div>
    <div id="home-avatar-box"><div id="home-avatar"></div></div>
    <div id="home-player-name"></div>
    <div id="home-account-type"></div>
    <div id="launch-player-name"></div>
    <div id="launch-avatar"></div>
    <div id="toast-container"></div>
    <div id="add-account-modal" class="modal"></div>
  `;
}

function addSkinViewerContainer() {
  const detail = document.getElementById('page-account-detail');
  const el = document.createElement('div');
  el.id = 'skin-3d-container';
  detail.appendChild(el);
  return el;
}

beforeAll(async () => {
  installTauriMock();
  S = (await import('./accounts-state.js')).accountsState;
  mod = await import('./accounts-list.js');
});

beforeEach(() => {
  installTauriMock();
  buildDom();
  S._carouselCards = [];
  S._carouselCurrentIndex = 0;
  S._currentDetailAccount = null;
  S._skinViewer = null;
  S._skinResizeObserver = null;
  S._skinResizeTimer = null;
  S._currentSkinBg = 'white';
  S._avatarMemCache.clear();

  vi.stubGlobal('fetch', vi.fn(async () => ({
    headers: { get: () => null },
    blob: async () => new Blob(['x']),
  })));
  Object.defineProperty(URL, 'createObjectURL', {
    configurable: true, writable: true, value: vi.fn(() => 'blob:mock'),
  });
  Object.defineProperty(URL, 'revokeObjectURL', {
    configurable: true, writable: true, value: vi.fn(),
  });
  // jsdom 不做布局，固定尺寸让轮播定位逻辑走同步分支
  Object.defineProperty(HTMLElement.prototype, 'clientWidth', {
    configurable: true, get: () => 800,
  });
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', {
    configurable: true, get: () => 180,
  });
});

afterEach(() => {
  if (S?._skinResizeTimer) {
    clearTimeout(S._skinResizeTimer);
    S._skinResizeTimer = null;
  }
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  document.body.innerHTML = '';
  delete HTMLElement.prototype.clientWidth;
  delete HTMLElement.prototype.offsetWidth;
});

function routeAccounts(accounts) {
  mockApiRoute('/api/accounts', accounts);
}

function routeSettings(settings) {
  mockApiRoute('/api/settings', settings);
}

describe('loadAccounts', () => {
  it('无账户时显示空状态并展示添加账号入口', async () => {
    routeAccounts([]);
    routeSettings({});

    await mod.loadAccounts();

    const list = document.getElementById('accounts-list');
    expect(list.textContent).toContain('暂无账户，请添加账户');
    expect(document.getElementById('home-player-name').textContent).toBe('未登录');
    expect(document.getElementById('home-account-type').textContent).toBe('离线模式');
    expect(document.getElementById('launch-player-name').textContent).toBe('Player');
    expect(document.getElementById('launch-avatar').querySelector('img')).toBeTruthy();
    expect(document.getElementById('home-avatar').innerHTML).toContain('添加账号');
    expect(document.getElementById('home-avatar-box').classList.contains('home-avatar-add')).toBe(true);

    const cards = document.querySelectorAll('#accts-track .accts-card');
    expect(cards.length).toBe(1);
    expect(cards[0].dataset.accountId).toBe(S.ADD_CARD_ID);
    expect(cards[0].classList.contains('is-selected')).toBe(true);
    expect(S._carouselCurrentIndex).toBe(0);
  });

  it('渲染多账户列表并标记当前使用账户', async () => {
    routeAccounts(ACCOUNTS);
    routeSettings({ selectedAccount: 'a2' });

    await mod.loadAccounts();

    const items = document.querySelectorAll('#accounts-list .account-item');
    expect(items.length).toBe(3);
    const selected = items[1];
    expect(selected.classList.contains('selected')).toBe(true);
    expect(selected.textContent).toContain('当前使用');
    expect(selected.textContent).toContain('Alex');
    expect(selected.querySelector('.account-item-type').textContent).toBe('离线账户');
    expect(items[0].textContent).toContain('选择');
    expect(items[0].querySelector('.account-item-type').textContent).toBe('微软账户');
    expect(items[2].querySelector('.account-item-type').textContent).toBe('外置登录');
    // 无 uuid 的账户回退到用户名首字母头像
    expect(items[1].querySelector('.account-avatar-text').textContent).toBe('A');

    expect(document.getElementById('home-player-name').textContent).toBe('Alex');
    expect(document.getElementById('home-account-type').textContent).toBe('离线模式');
    expect(localStorage.getItem('cachedPlayerName')).toBe('Alex');

    const cards = document.querySelectorAll('#accts-track .accts-card');
    expect(cards.length).toBe(4);
    expect(cards[0].dataset.accountId).toBe(S.ADD_CARD_ID);
    expect(S._carouselCurrentIndex).toBe(2);
    expect(cards[2].dataset.accountId).toBe('a2');
    expect(cards[2].classList.contains('is-selected')).toBe(true);
    expect(cards[2].querySelector('.accts-card-type').textContent).toBe('离线账户');
    expect(cards[1].querySelector('.accts-card-type')).toBeNull();
  });

  it('选中账户不存在时回退到第一个账户', async () => {
    routeAccounts(ACCOUNTS);
    routeSettings({ selectedAccount: 'missing' });

    await mod.loadAccounts();

    expect(document.getElementById('home-player-name').textContent).toBe('Steve');
    expect(document.getElementById('home-account-type').textContent).toBe('微软账户');
    expect(S._carouselCurrentIndex).toBe(1);
    expect(document.querySelector('#accts-track .accts-card.is-selected').dataset.accountId).toBe('a1');
  });

  it('接口失败时记录错误且不抛出', async () => {
    const errSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
    mockApiRoute('/api/accounts', { status: 500, body: { error: 'boom' } });
    routeSettings({});

    await expect(mod.loadAccounts()).resolves.toBeUndefined();
    expect(errSpy).toHaveBeenCalled();
    expect(document.getElementById('accounts-list').innerHTML).toBe('');
  });
});

describe('账户轮播交互', () => {
  it('点击添加卡片打开添加账户弹窗', async () => {
    routeAccounts(ACCOUNTS);
    routeSettings({ selectedAccount: 'a1' });
    await mod.loadAccounts();

    document.querySelector('#accts-track .accts-add-card').click();

    expect(S._carouselCurrentIndex).toBe(0);
    expect(document.getElementById('add-account-modal').getAttribute('data-state')).toBe('open');
  });

  it('点击账户卡片进入账户详情', async () => {
    routeAccounts(ACCOUNTS);
    routeSettings({ selectedAccount: 'a1' });
    await mod.loadAccounts();

    document.querySelector('#accts-track .accts-card[data-account-id="a1"]').click();

    await vi.waitFor(() => {
      expect(document.getElementById('detail-username').textContent).toBe('Steve');
    });
    expect(document.getElementById('detail-uuid').textContent).toBe('1111-2222-3333-4444');
    expect(document.getElementById('detail-skin-type').textContent).toBe('正版');
    expect(S._currentDetailAccount.id).toBe('a1');
    expect(document.querySelector('#page-accounts .accts-stage').style.display).toBe('none');
  });

  it('向左拖动轮播会切换到下一张账户卡片', async () => {
    routeAccounts(ACCOUNTS);
    routeSettings({ selectedAccount: 'a1' });
    mockApiRoute('POST /api/accounts/select', { success: true });
    await mod.loadAccounts();
    // 从"添加账号"卡片（索引 0）开始
    S._carouselCurrentIndex = 0;

    const carousel = document.getElementById('accts-carousel');
    carousel.dispatchEvent(new MouseEvent('pointerdown', { clientX: 0, clientY: 0, button: 0 }));
    document.dispatchEvent(new MouseEvent('pointermove', { clientX: -220, clientY: 0 }));
    document.dispatchEvent(new MouseEvent('pointerup', { clientX: -220, clientY: 0 }));

    await vi.waitFor(() => {
      const call = getApiCalls().find((c) => c.path === '/api/accounts/select');
      expect(call).toBeTruthy();
      expect(call.args.body.accountId).toBe('a1');
    });
    // 等待切换后的列表刷新完成，避免异步回调落到已销毁的 DOM 上
    await vi.waitFor(() => {
      expect(document.getElementById('toast-container').textContent).toContain('已切换账户');
    });
  });

  it('_carouselResetPosition 按当前索引居中并同步选中态', async () => {
    routeAccounts(ACCOUNTS);
    routeSettings({ selectedAccount: 'a1' });
    await mod.loadAccounts();

    S._carouselCurrentIndex = 3;
    mod._carouselResetPosition(true);

    const track = document.getElementById('accts-track');
    expect(track.style.transform).toContain('translateX(');
    const cards = [...track.querySelectorAll('.accts-card')];
    expect(cards.map((c) => c.classList.contains('is-selected'))).toEqual([false, false, false, true]);
  });
});

describe('selectAccount / deleteAccount', () => {
  it('切换账户成功后刷新列表并提示', async () => {
    routeAccounts(ACCOUNTS);
    routeSettings({ selectedAccount: 'a2' });
    mockApiRoute('POST /api/accounts/select', { success: true });

    await mod.selectAccount('a3');

    const call = getApiCalls().find((c) => c.path === '/api/accounts/select');
    expect(call.args.body.accountId).toBe('a3');
    expect(getApiCalls().filter((c) => c.path === '/api/accounts').length).toBe(1);
    await vi.waitFor(() => {
      expect(document.getElementById('toast-container').textContent).toContain('已切换账户');
    });
  });

  it('切换账户失败时提示错误', async () => {
    routeAccounts([]);
    routeSettings({});
    mockApiRoute('POST /api/accounts/select', { status: 500, body: { error: 'denied' } });

    await mod.selectAccount('a3');

    await vi.waitFor(() => {
      expect(document.getElementById('toast-container').textContent).toContain('切换失败');
    });
  });

  it('确认删除后调用接口并刷新列表', async () => {
    routeAccounts(ACCOUNTS);
    routeSettings({ selectedAccount: 'a1' });
    mockApiRoute('POST /api/accounts/delete', { success: true });

    const pending = mod.deleteAccount('a2');
    await vi.waitFor(() => {
      expect(document.querySelector('.confirm-ok')).toBeTruthy();
    });
    document.querySelector('.confirm-ok').click();

    await pending;
    const call = getApiCalls().find((c) => c.path === '/api/accounts/delete');
    expect(call.args.body.accountId).toBe('a2');
    await vi.waitFor(() => {
      expect(document.getElementById('toast-container').textContent).toContain('账户已删除');
    });
  });

  it('取消删除时不调用接口', async () => {
    routeAccounts(ACCOUNTS);
    routeSettings({ selectedAccount: 'a1' });

    const pending = mod.deleteAccount('a2');
    await vi.waitFor(() => {
      expect(document.querySelector('.confirm-cancel')).toBeTruthy();
    });
    document.querySelector('.confirm-cancel').click();

    await pending;
    expect(getApiCalls().some((c) => c.path === '/api/accounts/delete')).toBe(false);
  });

  it('删除接口失败时提示错误', async () => {
    routeAccounts(ACCOUNTS);
    routeSettings({ selectedAccount: 'a1' });
    mockApiRoute('POST /api/accounts/delete', { status: 500, body: { error: 'denied' } });

    const pending = mod.deleteAccount('a2');
    await vi.waitFor(() => {
      expect(document.querySelector('.confirm-ok')).toBeTruthy();
    });
    document.querySelector('.confirm-ok').click();

    await pending;
    await vi.waitFor(() => {
      expect(document.getElementById('toast-container').textContent).toContain('删除失败');
    });
  });
});

describe('账户详情页生命周期', () => {
  it('showAccountList 恢复列表并销毁皮肤查看器', async () => {
    const dispose = vi.fn();
    const disconnect = vi.fn();
    S._currentDetailAccount = { id: 'a1', username: 'Steve' };
    S._skinViewer = { dispose };
    S._skinResizeObserver = { disconnect };
    const detail = document.getElementById('page-account-detail');
    detail.style.display = '';
    document.querySelector('#page-accounts .accts-stage').style.display = 'none';
    document.querySelector('#page-accounts .page-header').style.display = 'none';
    const page = document.getElementById('page-accounts');
    page.style.height = '123px';
    page.style.overflow = 'hidden';
    const skinContainer = addSkinViewerContainer();
    skinContainer.innerHTML = '<canvas></canvas>';

    mod.showAccountList();

    expect(detail.style.display).toBe('none');
    expect(document.querySelector('#page-accounts .accts-stage').style.display).toBe('');
    expect(document.querySelector('#page-accounts .page-header').style.display).toBe('');
    expect(page.style.height).toBe('');
    expect(page.style.overflow).toBe('');
    expect(dispose).toHaveBeenCalled();
    expect(disconnect).toHaveBeenCalled();
    expect(S._skinViewer).toBeNull();
    expect(S._skinResizeObserver).toBeNull();
    expect(skinContainer.innerHTML).toBe('');
    expect(S._currentDetailAccount).toBeNull();
  });

  it('destroySkinViewer 在无实例时安全返回', () => {
    expect(() => mod.destroySkinViewer()).not.toThrow();
  });
});

describe('initSkinViewer', () => {
  class MockSkinViewer {
    constructor(options) {
      this.options = options;
      this.canvas = document.createElement('canvas');
      this.cameraLight = {};
      this.globalLight = {};
      this.sizeCalls = [];
      this.renderCalls = 0;
      MockSkinViewer.instance = this;
    }
    setSize(width, height) {
      this.size = { width, height };
      this.sizeCalls.push({ width, height });
    }
    render() { this.renderCalls += 1; }
    dispose() { this.disposed = true; }
  }
  class MockIdleAnimation {
    constructor() { this.speed = 1; }
  }

  let roInstance;

  beforeEach(() => {
    MockSkinViewer.instance = null;
    vi.stubGlobal('skinview3d', { SkinViewer: MockSkinViewer, IdleAnimation: MockIdleAnimation });
    vi.stubGlobal('ResizeObserver', class {
      constructor(cb) { this.cb = cb; roInstance = this; }
      observe(el) { this.el = el; }
      disconnect() { this.disconnected = true; }
    });
  });

  it('使用解析出的模型创建查看器并挂载画布，模型按纹理自动识别', async () => {
    const container = addSkinViewerContainer();
    S._currentDetailAccount = { username: 'Steve', skinModel: 'default' };

    await mod.initSkinViewer('data:image/png;base64,abc', 'default');

    const viewer = MockSkinViewer.instance;
    expect(viewer.options.skin).toBe('data:image/png;base64,abc');
    expect(viewer.options.model).toBe('auto-detect');
    expect(viewer.options.width).toBe(800);
    expect(viewer.canvas.parentElement).toBe(container);
    expect(viewer.zoom).toBe(0.85);
    expect(viewer.autoRotate).toBe(false);
    expect(viewer.nameTag).toBe('Steve');
    expect(roInstance.observe).toBeTruthy();
    expect(roInstance.el).toBe(container);
  });

  it('没有皮肤纹理时按模型使用内置兜底皮肤', async () => {
    addSkinViewerContainer();
    S._currentDetailAccount = { username: 'Alex', skinModel: 'slim' };

    await mod.initSkinViewer('', '');

    expect(MockSkinViewer.instance.options.skin).toBe('img/skin_alex.png');
    expect(MockSkinViewer.instance.options.model).toBe('auto-detect');
  });

  it('查看器初始化失败时展示失败提示', async () => {
    const errSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
    vi.stubGlobal('skinview3d', {
      SkinViewer: class { constructor() { throw new Error('boom'); } },
      IdleAnimation: class {},
    });
    const container = addSkinViewerContainer();

    await mod.initSkinViewer('data:image/png;base64,abc', 'default');

    expect(container.textContent).toContain('皮肤加载失败');
    expect(errSpy).toHaveBeenCalled();
  });

  it('容器尺寸连续变化时防抖，只在稳定后重设一次尺寸', async () => {
    vi.useFakeTimers();
    try {
      const container = addSkinViewerContainer();
      let width = 800;
      Object.defineProperty(container, 'clientWidth', {
        configurable: true, get: () => width,
      });
      Object.defineProperty(container, 'clientHeight', {
        configurable: true, get: () => 420,
      });
      S._currentDetailAccount = { username: 'Steve', skinModel: 'default' };

      const initPromise = mod.initSkinViewer('data:image/png;base64,abc', 'default');
      await vi.advanceTimersByTimeAsync(100);
      await initPromise;
      const viewer = MockSkinViewer.instance;
      expect(viewer.sizeCalls).toHaveLength(0);

      // 模拟侧栏宽度过渡：连续多帧变化
      width = 820;
      roInstance.cb();
      width = 840;
      roInstance.cb();
      width = 860;
      roInstance.cb();

      await vi.advanceTimersByTimeAsync(149);
      expect(viewer.sizeCalls).toHaveLength(0);
      await vi.advanceTimersByTimeAsync(1);
      expect(viewer.sizeCalls).toEqual([{ width: 860, height: 420 }]);
      expect(viewer.renderCalls).toBe(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it('销毁查看器时清理待执行的尺寸防抖定时器', () => {
    vi.useFakeTimers();
    try {
      S._skinResizeTimer = setTimeout(() => {}, 150);
      mod.destroySkinViewer();
      expect(S._skinResizeTimer).toBeNull();
      expect(vi.getTimerCount()).toBe(0);
    } finally {
      vi.useRealTimers();
    }
  });
});
