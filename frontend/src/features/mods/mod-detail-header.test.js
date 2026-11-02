// @vitest-environment jsdom
// mod-detail-header.test.js — 模组详情页头部、侧栏、标签切换与打开流程单元测试
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

const {
  toast,
  formatNumberMock,
  formatNameMock,
  installModpackMock,
  installModFileMock,
  quickInstallMock,
  navigateMock,
  goBackMock,
  pushHistoryMock,
  switchVersionTabMock,
  renderVersionTabsMock,
  proxifyMock,
  hydrateMock,
  setLocalImageMock,
  renderGalleryMock,
  loadDepsMock,
} = vi.hoisted(() => ({
  toast: vi.fn(),
  formatNumberMock: vi.fn((n) => 'N' + n),
  formatNameMock: vi.fn((slug, title) => title || slug),
  installModpackMock: vi.fn(),
  installModFileMock: vi.fn(),
  quickInstallMock: vi.fn(),
  navigateMock: vi.fn(),
  goBackMock: vi.fn(),
  pushHistoryMock: vi.fn(),
  switchVersionTabMock: vi.fn(),
  renderVersionTabsMock: vi.fn(async () => {}),
  proxifyMock: vi.fn((html) => 'proxied:' + html),
  hydrateMock: vi.fn(),
  setLocalImageMock: vi.fn(),
  renderGalleryMock: vi.fn(),
  loadDepsMock: vi.fn(),
}));

const lazyLoadMock = vi.hoisted(() => vi.fn(async () => {}));

vi.mock('../misc/ui-components.js', () => ({ showToast: toast, formatNumber: formatNumberMock }));
vi.mock('./chinese-names.js', () => ({ formatModNameWithChinese: formatNameMock }));
vi.mock('./mod-install.js', () => ({
  installModpackVersion: installModpackMock,
  installModFile: installModFileMock,
  quickInstallResourceVersion: quickInstallMock,
}));
vi.mock('../../core/navigation.js', () => ({ navigateToPage: navigateMock }));
vi.mock('../versions/versions-detail-history.js', () => ({
  goBackFromDetail: goBackMock,
  pushModDetailHistory: pushHistoryMock,
}));
vi.mock('./mod-detail-versions.js', () => ({
  switchMdVersionTab: switchVersionTabMock,
  renderMdVersionTabs: renderVersionTabsMock,
}));
vi.mock('./mod-detail-images.js', () => ({
  proxifyBodyHtml: proxifyMock,
  hydrateBodyImages: hydrateMock,
  setLocalImage: setLocalImageMock,
  renderModGallery: renderGalleryMock,
}));
vi.mock('./mod-detail-deps.js', () => ({ loadModDependencies: loadDepsMock }));
vi.mock('./mod-detail-installed.js', () => ({ getInstalledVersionInfo: vi.fn() }));
vi.mock('../../core/utils.js', () => ({
  escapeHtml: (value) => String(value == null ? '' : value)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;'),
  formatDate: (value) => 'DATE:' + value,
  _lazyLoadScript: lazyLoadMock,
}));

let mod;
let S;
let modDetailState;
let appState;
let API;

function buildDom() {
  document.body.innerHTML = `
    <div id="page-mod-detail">
      <div class="moddetail-page-header"><button class="btn-icon" id="md-back"></button></div>
      <div class="md-tab" data-tab="description"></div>
      <div class="md-tab" data-tab="versions"></div>
      <div class="md-tab" data-tab="gallery"></div>
    </div>
    <div id="md-tab-description"></div>
    <div id="md-tab-versions"></div>
    <div id="md-tab-gallery"></div>
    <div id="md-loader-filter">
      <span class="md-loader-chip" data-loader=""></span>
      <span class="md-loader-chip" data-loader="forge"></span>
      <span class="md-loader-chip" data-loader="fabric"></span>
    </div>
    <div id="md-body-content"></div>
    <div id="md-sidebar-loaders"></div>
    <div id="md-sidebar-versions"></div>
    <div id="md-sidebar-categories"></div>
    <div id="md-sidebar-links"></div>
    <div id="md-sidebar-details"></div>
    <div id="md-sidebar-license"></div>
    <div id="md-compat-section"></div>
    <div id="md-name"></div>
    <div id="md-desc"></div>
    <img id="md-icon-img">
    <div id="md-icon-fallback"></div>
    <div id="md-downloads"></div>
    <div id="md-followers"></div>
    <div id="md-updated"></div>
    <div id="md-source-badge"></div>
    <button id="md-fav-btn" class="btn-secondary"></button>
    <div id="md-version-list"></div>
    <div id="md-version-tabs"></div>
    <div id="md-gallery"></div>
    <div id="md-gallery-tab"></div>
  `;
}

function resetState() {
  S._projectDataCache.clear();
  S._versionPreloadCache.clear();
  S.mdGallery = [];
  S.mdTabMode = 'major';
  modDetailState.data = null;
  modDetailState.allVersions = [];
  modDetailState.loaderFilter = '';
  modDetailState.tab = '';
  modDetailState.type = 'mod';
  modDetailState.seq = 0;
  appState.currentModDetailId = null;
  appState.currentModDetailSource = 'modrinth';
  appState._favorites = [];
}

function detail(overrides = {}) {
  return {
    id: 'p1',
    slug: 'demo',
    title: 'Demo Mod',
    description: 'A demo description',
    body: 'Body text',
    icon: 'https://cdn.example.com/icon.png',
    downloads: 100,
    followers: 2,
    loaders: ['forge'],
    gameVersions: ['1.20.1'],
    categories: ['magic'],
    sourceUrl: 'https://src',
    issuesUrl: 'https://issues',
    wikiUrl: 'https://wiki',
    discordUrl: 'https://discord',
    dateCreated: '2020-01-01',
    dateModified: '2021-01-01',
    clientSide: 'required',
    serverSide: 'optional',
    license: 'MIT',
    ...overrides,
  };
}

beforeAll(async () => {
  vi.resetModules();
  ({ S } = await import('./mod-detail-state.js'));
  ({ modDetailState } = await import('./detail-state.js'));
  ({ appState } = await import('../../core/app-state.js'));
  ({ API } = await import('../../core/api.js'));
  mod = await import('./mod-detail-header.js');
});

beforeEach(() => {
  buildDom();
  resetState();
  vi.useFakeTimers();
  vi.spyOn(console, 'error').mockImplementation(() => {});
  toast.mockClear();
  formatNumberMock.mockClear();
  formatNameMock.mockClear();
  installModpackMock.mockClear();
  installModFileMock.mockClear();
  quickInstallMock.mockClear();
  navigateMock.mockClear();
  goBackMock.mockClear();
  pushHistoryMock.mockClear();
  switchVersionTabMock.mockClear();
  renderVersionTabsMock.mockClear();
  proxifyMock.mockClear();
  hydrateMock.mockClear();
  setLocalImageMock.mockClear();
  renderGalleryMock.mockClear();
  loadDepsMock.mockClear();
  lazyLoadMock.mockClear();
  vi.spyOn(API, 'getModVersions').mockResolvedValue({ versions: [] });
  vi.spyOn(API, 'getModDetail').mockResolvedValue(detail());
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  document.body.innerHTML = '';
});

describe('switchMdTab', () => {
  it('切换激活页签与加载器筛选', () => {
    mod.switchMdTab('versions');

    const tabs = [...document.querySelectorAll('#page-mod-detail .md-tab')];
    expect(tabs[1].classList.contains('active')).toBe(true);
    expect(tabs[0].classList.contains('active')).toBe(false);
    expect(document.getElementById('md-tab-versions').classList.contains('active')).toBe(true);
    expect(document.getElementById('md-tab-description').classList.contains('active')).toBe(false);
    expect(document.getElementById('md-loader-filter').classList.contains('open')).toBe(true);
    expect(document.getElementById('md-loader-filter').getAttribute('aria-hidden')).toBe('false');
  });

  it('非版本页签收起筛选', () => {
    document.getElementById('md-loader-filter').classList.add('open');

    mod.switchMdTab('description');

    expect(document.getElementById('md-loader-filter').classList.contains('open')).toBe(false);
    expect(document.getElementById('md-loader-filter').getAttribute('aria-hidden')).toBe('true');
  });

  it('元素缺失时安全返回', () => {
    document.getElementById('md-loader-filter').remove();
    document.getElementById('md-tab-gallery').remove();

    expect(() => mod.switchMdTab('gallery')).not.toThrow();
  });
});

describe('加载器筛选', () => {
  it('更新筛选芯片激活态', () => {
    modDetailState.loaderFilter = 'fabric';

    mod.updateMdLoaderChips();

    const chips = [...document.querySelectorAll('#md-loader-filter .md-loader-chip')];
    expect(chips[1].classList.contains('active')).toBe(false);
    expect(chips[2].classList.contains('active')).toBe(true);
  });

  it('设置筛选后转小写并重新渲染版本', () => {
    modDetailState.tab = '1.20.1';

    mod.setMdLoaderFilter('FORGE');

    expect(modDetailState.loaderFilter).toBe('forge');
    expect(switchVersionTabMock).toHaveBeenCalledWith('1.20.1', 'major');
  });

  it('空筛选回落空串', () => {
    modDetailState.loaderFilter = 'forge';

    mod.setMdLoaderFilter(null);

    expect(modDetailState.loaderFilter).toBe('');
  });
});

describe('renderModDescription', () => {
  it('容器缺失时直接返回', async () => {
    document.getElementById('md-body-content').remove();

    await expect(mod.renderModDescription(detail())).resolves.toBeUndefined();
  });

  it('无描述时展示占位', async () => {
    await mod.renderModDescription(detail({ body: '' }));

    expect(document.getElementById('md-body-content').textContent).toContain('暂无描述');
  });

  it('CurseForge 描述直接代理输出', async () => {
    appState.currentModDetailId = 'p1';
    appState.currentModDetailSource = 'curseforge';

    await mod.renderModDescription(detail());

    expect(proxifyMock).toHaveBeenCalledWith('Body text');
    expect(document.getElementById('md-body-content').innerHTML).toBe('proxied:Body text');
    expect(hydrateMock).toHaveBeenCalled();
  });

  it('CurseForge 描述中的危险 HTML 会被消毒', async () => {
    appState.currentModDetailId = 'p1';
    appState.currentModDetailSource = 'curseforge';

    await mod.renderModDescription(detail({
      body: '<img src=x onerror="window.__xss=1"><script>window.__xss=2</script>',
    }));

    const html = document.getElementById('md-body-content').innerHTML;
    expect(html).not.toContain('onerror');
    expect(html).not.toContain('<script');
    expect(window.__xss).toBeUndefined();
  });

  it('Markdown 不可用时按换行转 br', async () => {
    appState.currentModDetailId = 'p1';

    await mod.renderModDescription(detail({ body: 'a\nb' }));

    expect(lazyLoadMock).toHaveBeenCalledWith('js/marked.min.js');
    expect(document.getElementById('md-body-content').innerHTML).toBe('proxied:a<br>b');
  });

  it('Markdown 可用时解析 HTML', async () => {
    appState.currentModDetailId = 'p1';
    vi.stubGlobal('marked', { parse: vi.fn(() => '<p>parsed</p>') });

    await mod.renderModDescription(detail());

    expect(marked.parse).toHaveBeenCalledWith('Body text');
    expect(document.getElementById('md-body-content').innerHTML).toBe('proxied:<p>parsed</p>');
  });

  it('Markdown 解析异常时回退换行', async () => {
    appState.currentModDetailId = 'p1';
    vi.stubGlobal('marked', { parse: vi.fn(() => { throw new Error('bad'); }) });

    await mod.renderModDescription(detail({ body: 'a\nb' }));

    expect(document.getElementById('md-body-content').innerHTML).toBe('proxied:a<br>b');
  });

  it('加载脚本失败仍继续渲染', async () => {
    appState.currentModDetailId = 'p1';
    lazyLoadMock.mockRejectedValueOnce(new Error('cdn'));

    await mod.renderModDescription(detail({ body: 'x' }));

    expect(document.getElementById('md-body-content').innerHTML).toBe('proxied:x');
  });

  it('异步加载期间切换项目则放弃渲染', async () => {
    appState.currentModDetailId = 'p1';
    let resolveLoad;
    lazyLoadMock.mockReturnValueOnce(new Promise((resolve) => { resolveLoad = resolve; }));
    document.getElementById('md-body-content').innerHTML = 'keep';

    const pending = mod.renderModDescription(detail());
    await Promise.resolve();
    appState.currentModDetailId = 'p2';
    resolveLoad();
    await pending;

    expect(document.getElementById('md-body-content').innerHTML).toBe('keep');
  });
});

describe('renderMdSidebar', () => {
  it('空详情直接返回', async () => {
    document.getElementById('md-compat-section').style.display = '';

    mod.renderMdSidebar(null);

    expect(document.getElementById('md-compat-section').style.display).toBe('');
    expect(document.getElementById('md-sidebar-loaders').innerHTML).toBe('');
  });

  it('渲染标签、链接与详情行', () => {
    modDetailState.type = 'modpack';

    mod.renderMdSidebar(detail());

    expect(document.getElementById('md-sidebar-loaders').innerHTML).toContain('forge');
    expect(document.getElementById('md-sidebar-categories').innerHTML).toContain('magic');
    expect(document.getElementById('md-sidebar-versions').innerHTML).toContain('1.20.1');
    expect(document.getElementById('md-sidebar-links').innerHTML).toContain('https://src');
    expect(document.getElementById('md-sidebar-links').innerHTML).toContain('https://discord');
    expect(document.getElementById('md-sidebar-details').textContent).toContain('整合包');
    expect(document.getElementById('md-sidebar-details').textContent).toContain('N100');
    expect(document.getElementById('md-sidebar-details').textContent).toContain('DATE:2020-01-01');
    expect(document.getElementById('md-sidebar-details').textContent).toContain('required');
    expect(document.getElementById('md-sidebar-license').textContent).toContain('MIT');
  });

  it('空数组展示占位符', () => {
    mod.renderMdSidebar(detail({ loaders: [], categories: [], gameVersions: [], license: '' }));

    expect(document.getElementById('md-sidebar-loaders').textContent).toBe('—');
    expect(document.getElementById('md-sidebar-categories').textContent).toBe('—');
    expect(document.getElementById('md-sidebar-versions').textContent).toBe('—');
    expect(document.getElementById('md-sidebar-license').textContent).toContain('未知');
  });

  it('版本超过 16 个时展示剩余数量', () => {
    mod.renderMdSidebar(detail({ gameVersions: Array.from({ length: 20 }, (_, i) => 'v' + i) }));

    expect(document.getElementById('md-sidebar-versions').textContent).toContain('+4');
  });

  it('无链接时展示占位符', () => {
    mod.renderMdSidebar(detail({ sourceUrl: '', issuesUrl: '', wikiUrl: '', discordUrl: '' }));

    expect(document.getElementById('md-sidebar-links').textContent).toBe('—');
  });

  it('未知类型使用类型值或默认模组', () => {
    modDetailState.type = 'unknown';

    mod.renderMdSidebar(detail());

    expect(document.getElementById('md-sidebar-details').textContent).toContain('unknown');

    modDetailState.type = '';
    mod.renderMdSidebar(detail());
    expect(document.getElementById('md-sidebar-details').textContent).toContain('模组');
  });
});

describe('installCurrentDetailVersion', () => {
  it('无版本时提示', () => {
    modDetailState.allVersions = [];

    mod.installCurrentDetailVersion();

    expect(toast).toHaveBeenCalledWith('暂无可用版本', 'warning');
  });

  it('整合包走整合包安装', () => {
    modDetailState.allVersions = [{ id: 'v1', files: [{ id: 'f1' }] }];
    modDetailState.type = 'modpack';
    appState.currentModDetailId = 'p1';

    mod.installCurrentDetailVersion();

    expect(installModpackMock).toHaveBeenCalledWith('p1', 'v1');
  });

  it('模组走文件安装并带上主文件 id', () => {
    modDetailState.allVersions = [{ id: 'v1', files: [{ id: 'f1' }] }];
    appState.currentModDetailId = 'p1';
    appState.currentModDetailSource = 'curseforge';

    mod.installCurrentDetailVersion();

    expect(installModFileMock).toHaveBeenCalledWith('p1', 'curseforge', 'v1', 'f1');
  });

  it('模组无文件时使用空文件 id', () => {
    modDetailState.allVersions = [{ id: 'v1' }];
    appState.currentModDetailId = 'p1';

    mod.installCurrentDetailVersion();

    expect(installModFileMock).toHaveBeenCalledWith('p1', 'modrinth', 'v1', '');
  });

  it('其他资源类型走快速安装', () => {
    modDetailState.allVersions = [{ id: 'v1' }];
    modDetailState.type = 'shader';
    appState.currentModDetailId = 'p1';

    mod.installCurrentDetailVersion();

    expect(quickInstallMock).toHaveBeenCalledWith('p1', 'shader', 'v1');
  });
});

describe('resetMdDetailView', () => {
  it('清空筛选、描述、画廊与侧栏', () => {
    modDetailState.loaderFilter = 'forge';
    document.getElementById('md-body-content').innerHTML = 'x';
    document.getElementById('md-gallery').innerHTML = 'y';
    document.getElementById('md-sidebar-license').innerHTML = 'z';
    document.getElementById('md-gallery-tab').style.display = '';

    mod.resetMdDetailView();

    expect(modDetailState.loaderFilter).toBe('');
    expect(document.getElementById('md-body-content').innerHTML).toBe('');
    expect(document.getElementById('md-gallery').innerHTML).toBe('');
    expect(document.getElementById('md-sidebar-license').innerHTML).toBe('');
    expect(document.getElementById('md-gallery-tab').style.display).toBe('none');
    expect(document.getElementById('md-tab-description').classList.contains('active')).toBe(true);
  });

  it('缺失元素时安全返回', () => {
    ['md-body-content', 'md-gallery', 'md-gallery-tab'].forEach((id) => {
      const el = document.getElementById(id);
      if (el) el.remove();
    });

    expect(() => mod.resetMdDetailView()).not.toThrow();
  });
});

describe('_renderModDetailHeader', () => {
  it('填充名称、描述与统计', () => {
    appState._favorites = [{ favs: [] }];

    mod._renderModDetailHeader(detail({ description: 'x'.repeat(250) }), 'modrinth', 'p1');

    expect(document.getElementById('md-name').textContent).toBe('Demo Mod');
    expect(document.getElementById('md-desc').textContent).toHaveLength(200);
    expect(document.getElementById('md-downloads').textContent).toBe('⬇ N100');
    expect(document.getElementById('md-followers').textContent).toBe('❤ N2');
    expect(document.getElementById('md-updated').textContent).toBe('🕐 更新于 DATE:2021-01-01');
    expect(modDetailState.data.id).toBe('p1');
    expect(formatNameMock).toHaveBeenCalledWith('demo', 'Demo Mod');
  });

  it('CurseForge 源徽章使用橙色', () => {
    mod._renderModDetailHeader(detail(), 'curseforge', 'p1');

    const badge = document.getElementById('md-source-badge');
    expect(badge.textContent).toBe('CurseForge');
    expect(badge.style.color).toBe('var(--orange)');
  });

  it('Modrinth 源徽章使用紫色', () => {
    mod._renderModDetailHeader(detail(), 'modrinth', 'p1');

    expect(document.getElementById('md-source-badge').textContent).toBe('Modrinth');
    expect(document.getElementById('md-source-badge').style.color).toBe('var(--purple)');
  });

  it('有图标时交给图片模块设置', () => {
    mod._renderModDetailHeader(detail(), 'modrinth', 'p1');

    expect(document.getElementById('md-icon-img').style.display).toBe('');
    expect(document.getElementById('md-icon-fallback').style.display).toBe('none');
    expect(setLocalImageMock).toHaveBeenCalledWith(document.getElementById('md-icon-img'), detail().icon, 'modrinth');
  });

  it('无图标时展示首字母兜底', () => {
    mod._renderModDetailHeader(detail({ icon: '' }), 'modrinth', 'p1');

    expect(document.getElementById('md-icon-img').style.display).toBe('none');
    expect(document.getElementById('md-icon-fallback').textContent).toBe('D');
    expect(setLocalImageMock).not.toHaveBeenCalled();
  });

  it('已收藏时切换主按钮样式', () => {
    appState._favorites = [{ favs: ['p1'] }];

    mod._renderModDetailHeader(detail(), 'modrinth', 'p1');

    const btn = document.getElementById('md-fav-btn');
    expect(btn.classList.contains('btn-primary')).toBe(true);
    expect(btn.classList.contains('btn-secondary')).toBe(false);
    expect(btn.textContent).toContain('已收藏');
  });

  it('未收藏时使用次级按钮', () => {
    mod._renderModDetailHeader(detail(), 'modrinth', 'other');

    const btn = document.getElementById('md-fav-btn');
    expect(btn.classList.contains('btn-secondary')).toBe(true);
    expect(btn.textContent).toContain('收藏');
  });

  it('缺修改日期时不显示更新时间', () => {
    mod._renderModDetailHeader(detail({ dateModified: '' }), 'modrinth', 'p1');

    expect(document.getElementById('md-updated').textContent).toBe('');
  });
});

describe('openModDetail', () => {
  it('缺少版本容器时记录错误并中止', async () => {
    document.getElementById('md-version-list').remove();

    await mod.openModDetail('p1', 'modrinth');

    expect(console.error).toHaveBeenCalledWith('[ModDetail] Required elements not found');
    expect(appState.currentModDetailId).toBe('p1');
    expect(navigateMock).toHaveBeenCalledWith('mod-detail');
    expect(pushHistoryMock).toHaveBeenCalled();
  });

  it('无缓存时先展示加载中并在返回后渲染', async () => {
    await mod.openModDetail('p1', 'modrinth');

    expect(document.getElementById('md-name').textContent).toBe('Demo Mod');
    expect(S._projectDataCache.has('p1')).toBe(true);
    expect(renderGalleryMock).toHaveBeenCalled();
    expect(loadDepsMock).toHaveBeenCalled();
    expect(renderVersionTabsMock).toHaveBeenCalledWith(1);
    expect(modDetailState.allVersions).toEqual([]);
  });

  it('版本数据非数组时归一为空数组', async () => {
    API.getModVersions.mockResolvedValue({ versions: 'bad' });

    await mod.openModDetail('p1', 'modrinth');

    expect(modDetailState.allVersions).toEqual([]);
  });

  it('数据返回前展示加载版本列表', async () => {
    let resolveDetail;
    API.getModDetail.mockReturnValue(new Promise((resolve) => { resolveDetail = resolve; }));

    const pending = mod.openModDetail('p1', 'modrinth');
    await vi.advanceTimersByTimeAsync(400);

    expect(document.getElementById('md-version-list').textContent).toContain('加载版本列表...');

    resolveDetail(detail());
    await pending;

    expect(document.getElementById('md-name').textContent).toBe('Demo Mod');
  });

  it('预加载版本数据存在时直接使用', async () => {
    S._versionPreloadCache.set('p1', { versions: [{ id: 'v1' }] });

    await mod.openModDetail('p1', 'modrinth');

    expect(API.getModVersions).not.toHaveBeenCalled();
    expect(modDetailState.allVersions).toEqual([{ id: 'v1' }]);
    expect(S._versionPreloadCache.has('p1')).toBe(false);
  });

  it('缓存详情存在时复用详情数据', async () => {
    S._projectDataCache.set('p1', detail({ title: 'Cached' }));

    await mod.openModDetail('p1', 'modrinth');

    expect(document.getElementById('md-name').textContent).toBe('Cached');
    expect(renderGalleryMock).toHaveBeenCalled();
  });

  it('详情接口失败时展示加载失败', async () => {
    API.getModDetail.mockRejectedValue(new Error('offline'));

    await mod.openModDetail('p1', 'modrinth');

    expect(document.getElementById('md-name').textContent).toBe('加载失败');
    expect(document.getElementById('md-version-list').textContent).toContain('无法加载模组详情');
  });

  it('渲染版本标签抛错时展示错误详情', async () => {
    renderVersionTabsMock.mockRejectedValueOnce(new Error('render boom'));

    await mod.openModDetail('p1', 'modrinth');

    expect(document.getElementById('md-name').textContent).toBe('加载失败');
    expect(document.getElementById('md-version-list').textContent).toContain('render boom');
  });

  it('请求期间切换项目则丢弃旧结果', async () => {
    let resolveDetail;
    API.getModDetail.mockReturnValue(new Promise((resolve) => { resolveDetail = resolve; }));

    const pending = mod.openModDetail('p1', 'modrinth');
    await Promise.resolve();
    modDetailState.seq += 1;
    resolveDetail(detail({ title: 'Stale' }));
    await pending;

    expect(document.getElementById('md-name').textContent).toBe('加载中...');
  });

  it('返回按钮绑定返回逻辑', async () => {
    await mod.openModDetail('p1', 'modrinth');

    document.getElementById('md-back').click();

    expect(goBackMock).toHaveBeenCalled();
  });

  it('隐藏兼容区块并重置类型', async () => {
    document.getElementById('md-compat-section').style.display = '';
    modDetailState.type = 'modpack';

    await mod.openModDetail('p1', 'modrinth');

    expect(document.getElementById('md-compat-section').style.display).toBe('none');
    expect(modDetailState.type).toBe('mod');
  });

  it('默认来源为 Modrinth', async () => {
    await mod.openModDetail('p1');

    expect(appState.currentModDetailSource).toBe('modrinth');
    expect(API.getModVersions).toHaveBeenCalledWith('p1', undefined);
  });
});
