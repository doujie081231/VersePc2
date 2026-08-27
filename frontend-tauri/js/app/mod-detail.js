/**
 * @file mod-detail.js
 * @description 模组详情页 - 展示模组信息、版本列表、依赖关系，支持版本选择与安装
 */
let currentModDetailData = null;
let mdAllVersions = [];
let mdCurrentTab = '';
let currentModDetailType = 'mod';
let mdCurrentDeps = [];
let mdDepsResolved = {};
let mdDepsVersionInfo = {};
let _modDetailSeq = 0;
class LRUCache {
  constructor(maxSize) {
    this._max = maxSize;
    this._map = new Map();
  }
  has(key) { return this._map.has(key); }
  get(key) {
    if (!this._map.has(key)) return undefined;
    const val = this._map.get(key);
    this._map.delete(key);
    this._map.set(key, val);
    return val;
  }
  set(key, val) {
    if (this._map.has(key)) this._map.delete(key);
    this._map.set(key, val);
    if (this._map.size > this._max) {
      const oldest = this._map.keys().next().value;
      this._map.delete(oldest);
    }
  }
  delete(key) { this._map.delete(key); }
  clear() { this._map.clear(); }
  get size() { return this._map.size; }
}

const _projectDataCache = new LRUCache(200);
const _versionPreloadCache = new LRUCache(100);
let _versionPreloadInFlight = new Set();

function preloadModVersions(projectId, source) {
  if (_versionPreloadCache.has(projectId) || _versionPreloadInFlight.has(projectId)) return;
  _versionPreloadInFlight.add(projectId);
  API.getModVersions(projectId, source || 'modrinth').then(data => {
    _versionPreloadCache.set(projectId, data);
    _versionPreloadInFlight.delete(projectId);
  }).catch(() => { _versionPreloadInFlight.delete(projectId); });
}

const _mdImageCache = new Map();

async function getLocalImageDataUrl(url) {
  if (!url) return '';
  if (_mdImageCache.has(url)) {
    const cached = _mdImageCache.get(url);
    if (typeof cached === 'string') return cached;
    return cached;
  }
  const p = API.getResourceImage(url)
    .then(r => (r && r.dataUrl) || '')
    .catch(() => '')
    .then(d => { if (d) _mdImageCache.set(url, d); return d; });
  _mdImageCache.set(url, p);
  return p;
}

async function setLocalImage(el, url, source) {
  if (!el || !url) return;
  const useProxy = (source || currentModDetailSource || 'modrinth') === 'modrinth' && /^https?:\/\//.test(url);
  if (useProxy) {
    const dataUrl = await getLocalImageDataUrl(url);
    if (dataUrl) { el.src = dataUrl; return; }
  }
  el.src = url;
}

function proxifyBodyHtml(html) {
  const source = currentModDetailSource || 'modrinth';
  if (source !== 'modrinth') return html;
  const PLACEHOLDER = 'data:image/gif;base64,R0lGODlhAQABAAAAACH5BAEKAAEALAAAAAABAAEAAAICTAEAOw==';
  return html.replace(/<img([^>]*?)src="([^"]+cdn\.modrinth\.com[^"]+)"([^>]*)>/gi, (m, pre, url, post) => {
    return `<img${pre}src="${PLACEHOLDER}" data-src="${url}"${post}>`;
  });
}

function hydrateBodyImages(container) {
  if (!container) return;
  const source = currentModDetailSource || 'modrinth';
  container.querySelectorAll('img[data-src]').forEach(img => {
    setLocalImage(img, img.getAttribute('data-src'), source);
  });
}

function switchMdTab(tab) {
  document.querySelectorAll('#page-mod-detail .md-tab').forEach(t => {
    t.classList.toggle('active', t.dataset.tab === tab);
  });
  ['description', 'versions', 'gallery'].forEach(name => {
    const el = document.getElementById('md-tab-' + name);
    if (el) el.classList.toggle('active', name === tab);
  });
}

async function renderModDescription(detail) {
  const container = document.getElementById('md-body-content');
  if (!container) return;
  const pid = currentModDetailId;
  const source = currentModDetailSource || 'modrinth';
  const body = detail && detail.body ? detail.body : '';
  if (!body) {
    container.innerHTML = '<p class="empty-text" style="padding:30px 0;text-align:center;color:var(--text-muted)">暂无描述</p>';
    return;
  }
  let html = body;
  if (source === 'curseforge') {
    html = body;
  } else {
    if (typeof marked === 'undefined' && typeof _lazyLoadScript === 'function') {
      try { await _lazyLoadScript('js/marked.min.js'); } catch (e) {}
    }
    if (pid !== currentModDetailId) return;
    if (typeof marked !== 'undefined' && typeof marked.parse === 'function') {
      try { html = marked.parse(body); } catch (e) { html = body.replace(/\n/g, '<br>'); }
    } else {
      html = body.replace(/\n/g, '<br>');
    }
  }
  container.innerHTML = proxifyBodyHtml(html);
  hydrateBodyImages(container);
}

function renderModGallery(detail) {
  const container = document.getElementById('md-gallery');
  const galleryTab = document.getElementById('md-gallery-tab');
  const raw = (detail && detail.gallery) || [];
  const gallery = Array.isArray(raw) ? raw : [];
  if (galleryTab) galleryTab.style.display = gallery.length > 0 ? '' : 'none';
  if (!container) return;
  window._mdGallery = gallery;
  if (gallery.length === 0) {
    container.innerHTML = '<p class="empty-text" style="padding:30px 0;text-align:center;color:var(--text-muted)">暂无截图</p>';
    return;
  }
  const source = currentModDetailSource || 'modrinth';
  const PLACEHOLDER = 'data:image/gif;base64,R0lGODlhAQABAAAAACH5BAEKAAEALAAAAAABAAEAAAICTAEAOw==';
  container.innerHTML = gallery.map((item, idx) => {
    const url = (item && (item.url || item.rawUrl)) || item || '';
    const title = (item && item.title) || '';
    return `<div class="md-gallery-item" onclick="openGalleryLightbox(${idx})">
      <img class="md-gallery-image" src="${PLACEHOLDER}" data-src="${escapeHtml(url)}" alt="${escapeHtml(title)}" loading="lazy">
      ${title ? `<div class="md-gallery-item-title">${escapeHtml(title)}</div>` : ''}
    </div>`;
  }).join('');
  container.querySelectorAll('img[data-src]').forEach(img => {
    setLocalImage(img, img.getAttribute('data-src'), source);
  });
}

function openGalleryLightbox(idx) {
  const gallery = window._mdGallery || [];
  if (gallery.length === 0) return;
  window._mdGalleryIndex = Math.max(0, Math.min(idx, gallery.length - 1));
  const box = document.getElementById('md-gallery-lightbox');
  const img = document.getElementById('md-gallery-lightbox-img');
  const cap = document.getElementById('md-gallery-lightbox-caption');
  if (!box || !img) return;
  const item = gallery[window._mdGalleryIndex];
  const url = (item && (item.rawUrl || item.url)) || item || '';
  img.src = 'data:image/gif;base64,R0lGODlhAQABAAAAACH5BAEKAAEALAAAAAABAAEAAAICTAEAOw==';
  setLocalImage(img, url);
  if (cap) cap.textContent = (item && item.title) ? item.title : '';
  box.style.display = 'flex';
}

function closeGalleryLightbox() {
  const box = document.getElementById('md-gallery-lightbox');
  if (box) box.style.display = 'none';
}

document.addEventListener('keydown', (e) => {
  const box = document.getElementById('md-gallery-lightbox');
  if (!box || box.style.display === 'none') return;
  if (e.key === 'Escape') { e.preventDefault(); closeGalleryLightbox(); }
  else if (e.key === 'ArrowLeft') { e.preventDefault(); prevGalleryImage(); }
  else if (e.key === 'ArrowRight') { e.preventDefault(); nextGalleryImage(); }
});

function prevGalleryImage() {
  const gallery = window._mdGallery || [];
  if (gallery.length === 0 || window._mdGalleryIndex === undefined) return;
  const nextIdx = (window._mdGalleryIndex - 1 + gallery.length) % gallery.length;
  openGalleryLightbox(nextIdx);
}

function nextGalleryImage() {
  const gallery = window._mdGallery || [];
  if (gallery.length === 0 || window._mdGalleryIndex === undefined) return;
  const nextIdx = (window._mdGalleryIndex + 1) % gallery.length;
  openGalleryLightbox(nextIdx);
}

function renderMdSidebar(detail) {
  const nameMap = { mod: '模组', modpack: '整合包', resourcepack: '材质包', shader: '光影包', datapack: '数据包' };
  const loadersEl = document.getElementById('md-sidebar-loaders');
  const versionsEl = document.getElementById('md-sidebar-versions');
  const catsEl = document.getElementById('md-sidebar-categories');
  const linksEl = document.getElementById('md-sidebar-links');
  const detailsEl = document.getElementById('md-sidebar-details');
  const licEl = document.getElementById('md-sidebar-license');
  if (!detail) return;
  const compatSection = document.getElementById('md-compat-section');
  if (compatSection) compatSection.style.display = 'none';
  const tagHtml = (arr) => (arr && arr.length)
    ? arr.map(v => `<span class="md-sidebar-tag">${escapeHtml(v)}</span>`).join('')
    : '<span class="md-sidebar-empty">—</span>';
  if (loadersEl) loadersEl.innerHTML = tagHtml(detail.loaders);
  const gvs = detail.gameVersions || [];
  const shown = gvs.slice(0, 16);
  if (versionsEl) versionsEl.innerHTML = tagHtml(shown) + (gvs.length > shown.length ? `<span class="md-sidebar-tag md-sidebar-more">+${gvs.length - shown.length}</span>` : '');
  if (catsEl) catsEl.innerHTML = tagHtml(detail.categories);
  if (linksEl) {
    const links = [];
    if (detail.sourceUrl) links.push(['源码', detail.sourceUrl]);
    if (detail.issuesUrl) links.push(['问题', detail.issuesUrl]);
    if (detail.wikiUrl) links.push(['Wiki', detail.wikiUrl]);
    if (detail.discordUrl) links.push(['Discord', detail.discordUrl]);
    linksEl.innerHTML = links.length
      ? links.map(([label, url]) => `<a href="${escapeHtml(url)}" target="_blank" rel="noreferrer" class="md-sidebar-link">${escapeHtml(label)}</a>`).join('')
      : '<span class="md-sidebar-empty">—</span>';
  }
  if (detailsEl) {
    const rows = [
      ['类型', nameMap[currentModDetailType] || currentModDetailType || '模组'],
      ['下载量', formatNumber(detail.downloads || 0)],
      ['关注数', formatNumber(detail.followers || 0)],
    ];
    if (detail.dateCreated) rows.push(['创建', formatDate(detail.dateCreated)]);
    if (detail.dateModified) rows.push(['更新', formatDate(detail.dateModified)]);
    if (detail.clientSide) rows.push(['客户端', detail.clientSide]);
    if (detail.serverSide) rows.push(['服务端', detail.serverSide]);
    detailsEl.innerHTML = rows.map(([k, v]) => `<div class="md-sidebar-detail-row"><span class="md-sidebar-detail-key">${escapeHtml(k)}</span><span class="md-sidebar-detail-value">${escapeHtml(v)}</span></div>`).join('');
  }
  if (licEl) licEl.innerHTML = `<div class="md-sidebar-detail-row"><span class="md-sidebar-detail-key">许可证</span><span class="md-sidebar-detail-value">${escapeHtml(detail.license || '未知')}</span></div>`;
}

function installCurrentDetailVersion() {
  if (!mdAllVersions || mdAllVersions.length === 0) {
    showToast('暂无可用版本', 'warning');
    return;
  }
  const v = mdAllVersions[0];
  const projectId = currentModDetailId;
  const source = currentModDetailSource || 'modrinth';
  const type = currentModDetailType || 'mod';
  const primaryFile = (v.files && v.files[0]) || {};
  if (type === 'modpack') {
    installModpackVersion(projectId, v.id);
  } else if (type === 'mod') {
    installModFile(projectId, source, v.id, primaryFile.id || '');
  } else {
    quickInstallResourceVersion(projectId, type, v.id);
  }
}

function resetMdDetailView() {
  switchMdTab('description');
  const descEl = document.getElementById('md-body-content');
  if (descEl) descEl.innerHTML = '';
  const galEl = document.getElementById('md-gallery');
  if (galEl) galEl.innerHTML = '';
  const galleryTab = document.getElementById('md-gallery-tab');
  if (galleryTab) galleryTab.style.display = 'none';
  ['md-sidebar-loaders','md-sidebar-versions','md-sidebar-categories','md-sidebar-links','md-sidebar-details','md-sidebar-license'].forEach(id => {
    const el = document.getElementById(id);
    if (el) el.innerHTML = '';
  });
}

async function getInstalledVersionInfo() {
  try {
    const settings = await API.getSettings().catch(() => ({}));
    const selectedVersion = settings.selectedVersion || '';
    if (!selectedVersion) return { gameVersion: '', loaderType: '', versionId: '' };

    const versionInfo = installedVersions.find(v => v.id === selectedVersion);
    let gameVersion = '';
    if (versionInfo && versionInfo.baseVersion) {
      gameVersion = versionInfo.baseVersion;
    } else if (versionInfo && versionInfo.inheritsFrom) {
      gameVersion = versionInfo.inheritsFrom;
    } else {
      gameVersion = selectedVersion.split('-')[0];
    }

    let loaderType = '';
    if (versionInfo) {
      if (versionInfo.isFabric) loaderType = 'fabric';
      else if (versionInfo.isForge) loaderType = 'forge';
      else if (versionInfo.isNeoForge) loaderType = 'neoforge';
    }

    return { gameVersion, loaderType, versionId: selectedVersion };
  } catch (e) {
    return { gameVersion: '', loaderType: '', versionId: '' };
  }
}

function _renderModDetailHeader(detail, source, projectId) {
  currentModDetailData = detail;
  const modTitle = formatModNameWithChinese(detail.slug || detail.id, detail.title || '未知模组');
  const mdName = document.getElementById('md-name');
  const mdDesc = document.getElementById('md-desc');
  const mdIconImg = document.getElementById('md-icon-img');
  const mdIconFallback = document.getElementById('md-icon-fallback');
  if (mdName) mdName.textContent = modTitle;
  if (mdDesc) mdDesc.textContent = (detail.description || '').substring(0, 200);
  if (detail.icon && mdIconImg && mdIconFallback) {
    mdIconImg.style.display = ''; mdIconFallback.style.display = 'none';
    setLocalImage(mdIconImg, detail.icon, source);
  } else if (mdIconImg && mdIconFallback) {
    mdIconImg.style.display = 'none'; mdIconFallback.textContent = modTitle.charAt(0).toUpperCase(); mdIconFallback.style.display = '';
  }
  const mdDownloads = document.getElementById('md-downloads');
  const mdFollowers = document.getElementById('md-followers');
  const mdUpdated = document.getElementById('md-updated');
  const srcBadge = document.getElementById('md-source-badge');
  if (mdDownloads) mdDownloads.textContent = `⬇ ${formatNumber(detail.downloads || 0)}`;
  if (mdFollowers) mdFollowers.textContent = `❤ ${formatNumber(detail.followers || 0)}`;
  if (mdUpdated) { const u = detail.dateModified ? formatDate(detail.dateModified) : ''; mdUpdated.textContent = u ? `🕐 更新于 ${u}` : ''; }
  if (srcBadge) {
    if (source === 'curseforge') { srcBadge.textContent = 'CurseForge'; srcBadge.style.color = '#f97316'; srcBadge.style.background = 'rgba(249,115,22,0.12)'; }
    else { srcBadge.textContent = 'Modrinth'; srcBadge.style.color = '#a855f7'; srcBadge.style.background = 'rgba(168,85,247,0.12)'; }
  }
  const mdFavBtn = document.getElementById('md-fav-btn');
  if (mdFavBtn) {
    const isFav = _favorites.some((f) => f.favs.includes(projectId));
    if (isFav) { mdFavBtn.classList.remove('btn-secondary'); mdFavBtn.classList.add('btn-primary'); mdFavBtn.innerHTML = '<svg viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" stroke-width="2" style="width:14px;height:14px"><path d="M20.84 4.61a5.5 5.5 0 00-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 00-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 000-7.78z"/></svg> 已收藏'; }
    else { mdFavBtn.classList.remove('btn-primary'); mdFavBtn.classList.add('btn-secondary'); mdFavBtn.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:14px;height:14px"><path d="M20.84 4.61a5.5 5.5 0 00-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 00-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 000-7.78z"/></svg> 收藏'; }
  }
}

async function openModDetail(projectId, source) {
  const mySeq = ++_modDetailSeq;
  pushModDetailHistory();
  currentModDetailId = projectId;
  currentModDetailSource = source || 'modrinth';
  currentModDetailType = 'mod';

  const compatSection = document.getElementById('md-compat-section');
  if (compatSection) compatSection.style.display = 'none';

  navigateToPage('mod-detail');
  resetMdDetailView();

  const backBtn = document.querySelector('#page-mod-detail .moddetail-page-header .btn-icon');
  if (backBtn) backBtn.setAttribute('onclick', 'goBackFromDetail()');

  const mdVersionList = document.getElementById('md-version-list');
  const mdVersionTabs = document.getElementById('md-version-tabs');

  if (!mdVersionList) { console.error('[ModDetail] Required elements not found'); return; }

  // 立即清空旧版本列表，防止切换时短暂显示上一个模组的版本
  mdVersionList.innerHTML = '';
  if (mdVersionTabs) mdVersionTabs.innerHTML = '';

  const cachedRaw = _projectDataCache.get(projectId);
  const cached = (cachedRaw && typeof cachedRaw.body === 'string') ? cachedRaw : null;
  const hasPreloaded = _versionPreloadCache.has(projectId);
  if (cached) {
    _renderModDetailHeader(cached, source, projectId);
    renderModDescription(cached);
    renderModGallery(cached);
    renderMdSidebar(cached);
  } else {
    const mdName = document.getElementById('md-name');
    if (mdName) mdName.textContent = '加载中...';
  }

  let _loadingTimer = null;
  if (!hasPreloaded) {
    _loadingTimer = setTimeout(() => {
      if (mdVersionList && !mdVersionList.querySelector('.mdv-group')) {
        mdVersionList.innerHTML = '<p class="empty-text" style="padding:30px 0;text-align:center;color:var(--text-muted)">加载版本列表...</p>';
      }
    }, 400);
  }

  try {
    const versionsPromise = hasPreloaded
      ? Promise.resolve(_versionPreloadCache.get(projectId))
      : API.getModVersions(projectId, source).catch(e => { console.error('[ModDetail] getModVersions failed:', e); return null; });
    _versionPreloadCache.delete(projectId);
    const detailPromise = cached ? Promise.resolve(cached) : API.getModDetail(projectId, source).catch(e => { console.error('[ModDetail] getModDetail failed:', e); return null; });

    const [detail, versionsData] = await Promise.all([detailPromise, versionsPromise]);
    if (_loadingTimer) { clearTimeout(_loadingTimer); _loadingTimer = null; }
    if (mySeq !== _modDetailSeq) { return; }
    if (!detail) {
      const mdName = document.getElementById('md-name');
      if (mdName) mdName.textContent = '加载失败';
      mdVersionList.innerHTML = `<p class="empty-text" style="padding:30px 0;text-align:center;color:var(--text-muted)">无法加载模组详情: API请求失败，请检查网络连接</p>`;
      return;
    }
    if (!cached) {
      _projectDataCache.set(projectId, detail);
      _renderModDetailHeader(detail, source, projectId);
      renderModDescription(detail);
      renderModGallery(detail);
      renderMdSidebar(detail);
    }

    mdAllVersions = versionsData ? (versionsData.versions || []) : [];
    if (!Array.isArray(mdAllVersions)) mdAllVersions = [];
    console.error('[ModDetail][debug] projectId=', projectId, 'source=', source, 'versions=', mdAllVersions.length, 'first=', JSON.stringify(mdAllVersions[0]));
    loadModDependencies();
    await renderMdVersionTabs(mySeq);
  } catch (e) {
    if (mySeq !== _modDetailSeq) return;
    console.error('[ModDetail] Error:', e);
    const mdName = document.getElementById('md-name');
    if (mdName) mdName.textContent = '加载失败';
    mdVersionList.innerHTML = `<p class="empty-text" style="padding:30px 0;text-align:center;color:var(--text-muted)">无法加载模组详情: ${escapeHtml(e.message || String(e))}</p>`;
  }
}

// 将版本号转为精确版本（如 1.20.1），快照版返回"快照版"
function _versionToExact(ver) {
  if (!ver) return null;
  const base = ver.split('-')[0];
  if (ver.includes('w') || ver.includes('snapshot')) return '快照版';
  const m = base.match(/^(\d+\.\d+(?:\.\d+)?)/);
  return m ? m[1] : null;
}

// 将版本号转为大版本（如 1.20），用于版本分组
function _versionToMajor(ver) {
  if (!ver) return null;
  if (ver.includes('w') || ver.includes('snapshot')) return '快照版';
  const base = ver.split('-')[0];
  const parts = base.split('.');
  if (parts.length < 2) return null;
  const major = parseInt(parts[0], 10);
  const minor = parseInt(parts[1], 10);
  if (major === 1 && minor >= 14) return '1.' + minor;
  if (major >= 25) return major + '.' + minor;
  return null;
}

function _renderVersionTabs(mode, tabMap, sortedKeys) {
  const tabsContainer = document.getElementById('md-version-tabs');
  let tabsHtml = '<button class="md-vtab active" data-ver="" onclick="switchMdVersionTab(\'\')">全部</button>';
  sortedKeys.forEach(key => {
    tabsHtml += `<button class="md-vtab" data-group="${mode}" data-ver="${escapeHtml(key)}" onclick="switchMdVersionTab('${escapeOnclick(key)}', '${mode}')">${escapeHtml(key)} (${tabMap.get(key)})</button>`;
  });
  if (tabsContainer) tabsContainer.innerHTML = tabsHtml;
}

function renderMdVersionTabs(detailSeq) {
  if (detailSeq !== undefined && detailSeq !== _modDetailSeq) { return; }

  const tabsContainer = document.getElementById('md-version-tabs');

  // 详情页默认展示该模组的全部版本，不被列表页筛选条件自动过滤；
  // 用户可通过版本标签手动筛选，或点击"全部"查看全部版本
  const exactMap = new Map();
  const majorMap = new Map();
  let hasSnapshot = false;
  mdAllVersions.forEach(v => {
    (v.gameVersions || []).forEach(gv => {
      if (gv.includes('w') || gv.includes('snapshot')) { hasSnapshot = true; return; }
      const exact = _versionToExact(gv);
      const major = _versionToMajor(gv);
      if (exact) exactMap.set(exact, (exactMap.get(exact) || 0) + 1);
      if (major) majorMap.set(major, (majorMap.get(major) || 0) + 1);
    });
  });

  const MAX_TABS = 9;
  let mode = 'exact';
  let tabMap = exactMap;
  if (tabMap.size > MAX_TABS) { mode = 'major'; tabMap = majorMap; }
  if (tabMap.size > MAX_TABS) { mode = 'major'; tabMap = majorMap; }

  const sortFn = (a, b) => {
    if (a === '快照版') return -1;
    if (b === '快照版') return 1;
    const pa = a.split('.').map(Number);
    const pb = b.split('.').map(Number);
    if (pa[0] !== pb[0]) return pb[0] - pa[0];
    return (pb[1] || 0) - (pa[1] || 0);
  };
  const sortedKeys = [...tabMap.keys()].sort(sortFn);

  if (tabsContainer) {
    let tabsHtml = '<button class="md-vtab active" data-ver="" onclick="switchMdVersionTab(\'\')">全部</button>';
    sortedKeys.forEach(key => {
      tabsHtml += `<button class="md-vtab" data-group="${mode}" data-ver="${escapeHtml(key)}" onclick="switchMdVersionTab('${escapeOnclick(key)}', '${mode}')">${escapeHtml(key)}</button>`;
    });
    tabsContainer.innerHTML = tabsHtml;
  }
  window._mdTabMode = mode;
  renderMdVersionList(mdAllVersions);
}

async function loadMdVersions(projectId, source, detailSeq) {
  try {
    const data = await API.getModVersions(projectId, source);
    if (detailSeq !== undefined && detailSeq !== _modDetailSeq) { return; }
    mdAllVersions = data.versions || [];
    if (!Array.isArray(mdAllVersions)) mdAllVersions = [];

    loadModDependencies();
    await renderMdVersionTabs(detailSeq);
  } catch (e) {
    console.error('[MDVersions] Error:', e);
    document.getElementById('md-version-list').innerHTML = '<p class="empty-text" style="padding:30px 0;text-align:center;color:var(--text-muted)">加载版本列表失败</p>';
  }
}

function switchMdVersionTab(ver, mode) {
  mdCurrentTab = ver;
  
  document.querySelectorAll('.md-vtab').forEach(tab => {
    tab.classList.toggle('active', tab.dataset.ver === ver);
  });

  let filtered = mdAllVersions;
  if (ver && ver !== '') {
    if (ver === '_filtered') {
      const currentGameVersion = getCustomSelectValue('mod-filter-version');
      const currentLoader = getCustomSelectValue('mod-filter-loader');
      filtered = mdAllVersions.filter(v => {
        const gv = v.gameVersions || [];
        const loaders = (v.loaders || []).map(l => l.toLowerCase());
        let match = true;
        if (currentGameVersion && !gv.includes(currentGameVersion)) match = false;
        if (currentLoader && !loaders.includes(currentLoader.toLowerCase())) match = false;
        return match;
      });
    } else {
      const useMode = mode || window._mdTabMode || 'major';
      filtered = mdAllVersions.filter(v => {
        return (v.gameVersions || []).some(gv => {
          if (useMode === 'exact') return _versionToExact(gv) === ver;
          return _versionToMajor(gv) === ver;
        });
      });
    }
  }

  renderMdVersionList(filtered);
}

const mdDepsCache = new Map();
const MD_DEPS_CACHE_TTL = 5 * 60 * 1000;
const MD_DEPS_CACHE_MAX = 50;

function cleanupMdDepsCache() {
  const now = Date.now();
  for (const [key, entry] of mdDepsCache) {
    if (now - entry.time > MD_DEPS_CACHE_TTL) mdDepsCache.delete(key);
  }
  if (mdDepsCache.size > MD_DEPS_CACHE_MAX) {
    const entries = [...mdDepsCache.entries()].sort((a, b) => a[1].time - b[1].time);
    for (let i = 0; i < entries.length - MD_DEPS_CACHE_MAX; i++) mdDepsCache.delete(entries[i][0]);
  }
}
setInterval(cleanupMdDepsCache, 60000);

async function loadModDependencies() {
  const depsSection = document.getElementById('md-deps-section');
  const depsList = document.getElementById('md-deps-list');
  const depsCount = document.getElementById('md-deps-count');

  if (!depsSection || !depsList) return;

  const allDeps = new Map();
  const mdSource = currentModDetailSource || 'modrinth';
  mdAllVersions.forEach(v => {
    (v.dependencies || []).forEach(d => {
      if (d.projectId && !allDeps.has(d.projectId)) {
        let type = d.dependencyType || '';
        if (mdSource === 'curseforge') {
          if (type === '1' || type === '2') type = 'required';
          else if (type === '5') type = 'incompatible';
          else type = 'optional';
        }
        allDeps.set(d.projectId, Object.assign({}, d, { dependencyType: type }));
      }
    });
  });

  const depArray = Array.from(allDeps.values());
  mdCurrentDeps = depArray;

  if (depArray.length === 0) {
    depsSection.style.display = 'none';
    return;
  }

  const requiredDeps = depArray.filter(d => d.dependencyType === 'required');
  depsSection.style.display = 'block';

  const verInfo = await getInstalledVersionInfo();
  const currentGameVersion = verInfo.gameVersion;
  const currentLoader = verInfo.loaderType;
  const hasVersionFilter = !!(currentGameVersion || currentLoader);

  if (!hasVersionFilter) {
    if (depsCount) depsCount.textContent = `(${requiredDeps.length} 必选, ${depArray.length - requiredDeps.length} 可选) — 请先选择游戏版本`;
  }

  const depIds = depArray.map(d => d.projectId).filter(Boolean);
  if (!depIds.length) {
    depsList.innerHTML = '';
    return;
  }

  const cacheKey = depIds.sort().join(',') + '|' + (currentGameVersion || '') + '|' + (currentLoader || '');
  const cached = mdDepsCache.get(cacheKey);
  if (cached && (Date.now() - cached.time < MD_DEPS_CACHE_TTL)) {
    mdDepsResolved = cached.resolved;
    mdDepsVersionInfo = cached.versionInfo;
    renderDepsList(depArray, cached.resolved, cached.versionInfo, hasVersionFilter, currentGameVersion, currentLoader, cached.installedMods, depsList, depsCount, requiredDeps);
    return;
  }

  depsList.innerHTML = depArray.map(d => {
    const depType = d.dependencyType || 'optional';
    const typeLabel = depType === 'required' ? '必选' : (depType === 'incompatible' ? '冲突' : '可选');
    const badgeClass = depType === 'required' ? 'required' : (depType === 'incompatible' ? 'incompatible' : 'optional');
    return `<div class="md-dep-item" id="md-dep-${escapeOnclick(d.projectId)}" onclick="openModDetail('${escapeOnclick(d.projectId)}', '${escapeOnclick(mdSource)}')">
      <div class="md-dep-icon"><div class="spinner" style="width:20px;height:20px;border-width:2px"></div></div>
      <div class="md-dep-info">
        <div class="md-dep-name" style="color:var(--text-muted)">加载中...</div>
      </div>
      <span class="md-dep-badge ${badgeClass}">${typeLabel}</span>
      <span class="md-dep-status not-installed">...</span>
    </div>`;
  }).join('');

  try {
    let resolved = {};
    let versionInfo = {};
    let installedMods = [];
    const isCF = mdSource === 'curseforge';

    if (!isCF) {
      const [resolveResult, installedModsData] = await Promise.all([
        hasVersionFilter
          ? API.resolveDepVersions(depIds, currentGameVersion, currentLoader, 'modrinth')
          : API.resolveModDeps(depIds.join(',')).then(r => ({ _basic: r })),
        API.getInstalledMods().catch(() => []).then(r => Array.isArray(r) ? r : (r.mods || []))
      ]);
      installedMods = Array.isArray(installedModsData) ? installedModsData : [];

      if (hasVersionFilter) {
        versionInfo = resolveResult;
        mdDepsVersionInfo = versionInfo;
        for (const pid of depIds) {
          const info = versionInfo[pid] || {};
          resolved[pid] = {
            id: info.id || pid,
            title: info.title || pid,
            slug: info.slug || '',
            icon: info.icon || '',
            description: info.description || '',
            downloads: info.downloads || 0
          };
        }
        mdDepsResolved = resolved;

        const failedIds = depIds.filter(pid => {
          const r = resolved[pid];
          return !r || !r.title || r.title === pid;
        });
        if (failedIds.length > 0) {
          try {
            const retryRes = await API.resolveDepVersions(failedIds, '', '', 'modrinth');
            for (const pid of failedIds) {
              const ri = retryRes[pid];
              if (ri && ri.title && ri.title !== pid) {
                resolved[pid] = { ...resolved[pid], ...ri };
              }
            }
            mdDepsResolved = resolved;
          } catch (e) { console.warn('[ModInstall] 依赖检查失败:', e.message); }
        }

        const compatibleCount = requiredDeps.filter(d => versionInfo[d.projectId]?.hasCompatibleVersion).length;
        const incompatibleCount = requiredDeps.filter(d => !versionInfo[d.projectId]?.hasCompatibleVersion).length;
        if (depsCount) {
          let countText = `(${requiredDeps.length} 必选, ${depArray.length - requiredDeps.length} 可选)`;
          countText += ` — ${compatibleCount} 个有对应版本`;
          if (incompatibleCount > 0) {
            countText += `，${incompatibleCount} 个未有对应版本`;
          }
          depsCount.textContent = countText;
        }
      } else {
        resolved = resolveResult._basic;
        mdDepsResolved = resolved;
        mdDepsVersionInfo = {};
        if (depsCount) depsCount.textContent = `(${requiredDeps.length} 必选, ${depArray.length - requiredDeps.length} 可选)`;
      }
    } else {
      try {
        const d = await API.getInstalledMods().catch(() => []);
        installedMods = Array.isArray(d) ? d : ((d && d.mods) || []);
      } catch (e) {}
      resolved = {};
      try {
        const r = await API.resolveModDeps(depIds.join(','), 'curseforge') || {};
        for (const pid of depIds) {
          const ri = r[pid];
          if (ri) {
            resolved[pid] = {
              id: ri.id || pid,
              title: ri.title || ri.name || pid,
              slug: ri.slug || '',
              icon: ri.icon || ri.logo || '',
              description: ri.description || '',
              downloads: ri.downloads || 0
            };
          }
        }
      } catch (e) { console.warn('[ModInstall] CF依赖解析失败:', e.message); }
      mdDepsResolved = resolved;
      mdDepsVersionInfo = {};
      if (depsCount) depsCount.textContent = `(${requiredDeps.length} 必选, ${depArray.length - requiredDeps.length} 可选)`;
    }

    mdDepsCache.set(cacheKey, { resolved, versionInfo, installedMods, time: Date.now() });

    renderDepsList(depArray, resolved, versionInfo, hasVersionFilter, currentGameVersion, currentLoader, installedMods, depsList, depsCount, requiredDeps);
  } catch (e) {
    depsList.innerHTML = depArray.map(d => {
      const depType = d.dependencyType || 'optional';
      const typeLabel = depType === 'required' ? '必选' : (depType === 'incompatible' ? '冲突' : '可选');
      const badgeClass = depType === 'required' ? 'required' : (depType === 'incompatible' ? 'incompatible' : 'optional');
      return `<div class="md-dep-item" onclick="openModDetail('${escapeOnclick(d.projectId)}', '${escapeOnclick(mdSource)}')">
        <div class="md-dep-info">
          <div class="md-dep-name">${escapeHtml(d.projectId)}</div>
        </div>
        <span class="md-dep-badge ${badgeClass}">${typeLabel}</span>
      </div>`;
    }).join('');
  }
}

function renderDepsList(depArray, resolved, versionInfo, hasVersionFilter, currentGameVersion, currentLoader, installedMods, depsList, depsCount, requiredDeps) {
  depsList.innerHTML = depArray.map(d => {
    const info = resolved[d.projectId] || {};
    const title = info.title || d.projectId;
    const icon = info.icon || '';
    const desc = info.description || '';
    const depType = d.dependencyType || 'optional';
    const typeLabel = depType === 'required' ? '必选' : (depType === 'incompatible' ? '冲突' : '可选');
    const badgeClass = depType === 'required' ? 'required' : (depType === 'incompatible' ? 'incompatible' : 'optional');

    const isInstalled = installedMods.some(m => {
      if (m.id === d.projectId) return true;
      if (!m.filename) return false;
      const fn = m.filename.toLowerCase();
      const pid = d.projectId.toLowerCase();
      if (fn.includes(pid)) return true;
      const slug = (info.slug || '').toLowerCase();
      if (slug && fn.includes(slug)) return true;
      return false;
    });

    let statusText = '';
    let statusClass = '';
    if (isInstalled) {
      statusText = '✓ 已安装';
      statusClass = 'installed';
    } else if (hasVersionFilter) {
      const vInfo = versionInfo[d.projectId];
      if (vInfo?.hasCompatibleVersion) {
        statusText = '可安装';
        statusClass = 'compatible';
      } else {
        statusText = '未有对应版本';
        statusClass = 'incompatible-version';
      }
    } else {
      statusText = '请先选择版本';
      statusClass = 'not-installed';
    }

    let versionInfoHtml = '';
    if (hasVersionFilter && !isInstalled) {
      const vInfo = versionInfo[d.projectId];
      if (vInfo?.hasCompatibleVersion) {
        const verNum = vInfo.versionNumber || '';
        const loaders = (vInfo.loaders || []).map(l => {
          const ll = l.toLowerCase();
          let color = '#888', bg = 'rgba(136,136,136,0.15)';
          if (ll === 'fabric') { color = '#dbb07c'; bg = 'rgba(219,176,124,0.15)'; }
          else if (ll === 'forge') { color = '#4a6b8a'; bg = 'rgba(74,107,138,0.15)'; }
          else if (ll === 'neoforge') { color = '#f47733'; bg = 'rgba(244,119,51,0.15)'; }
          else if (ll === 'quilt') { color = '#9b59b6'; bg = 'rgba(155,89,182,0.15)'; }
          return `<span style="font-size:10px;padding:1px 5px;border-radius:3px;background:${bg};color:${color}">${escapeHtml(l)}</span>`;
        }).join('');
        versionInfoHtml = `<div style="font-size:11px;color:var(--text-muted);margin-top:2px;display:flex;align-items:center;gap:4px">${verNum ? escapeHtml(verNum) : ''} ${loaders}</div>`;
      } else {
        versionInfoHtml = `<div style="font-size:11px;color:var(--warning,orange);margin-top:2px">⚠ ${currentGameVersion || '未知版本'}${currentLoader ? ' / ' + currentLoader : ''} 无对应版本</div>`;
      }
    }

    return `<div class="md-dep-item" onclick="openModDetail('${escapeOnclick(d.projectId)}', '${escapeOnclick(currentModDetailSource || 'modrinth')}')">
      ${icon ? `<div class="md-dep-icon"><img src="${icon}" alt="" onerror="this.parentElement.remove()"></div>` : ''}
      <div class="md-dep-info">
        <div class="md-dep-name">${escapeHtml(formatModNameWithChinese(info.slug || d.projectId, title))}</div>
        <div class="md-dep-desc">${escapeHtml(desc)}</div>
        ${versionInfoHtml}
      </div>
      <span class="md-dep-badge ${badgeClass}">${typeLabel}</span>
      <span class="md-dep-status ${statusClass}">${statusText}</span>
    </div>`;
  }).join('');
}

function toggleMdDepsSection() {
  const depsList = document.getElementById('md-deps-list');
  const arrow = document.getElementById('md-deps-arrow');
  if (!depsList) return;
  depsList.classList.toggle('expanded');
  if (arrow) {
    arrow.style.transform = depsList.classList.contains('expanded') ? 'rotate(180deg)' : '';
  }
}

async function downloadAllDeps() {
  if (!currentModDetailData) return;
  const source = currentModDetailData.source || 'modrinth';
  const versionId = currentModDetailData.selectedVersionId || currentModDetailData.versionId || '';
  const gameVersion = currentModDetailData.selectedGameVersion || getCustomSelectValue('mod-filter-version') || '';
  const loader = currentModDetailData.selectedLoader || getCustomSelectValue('mod-filter-loader') || '';

  if (!versionId) {
    showToast('请先选择一个版本', 'error');
    return;
  }

  const btn = document.getElementById('md-deps-download-all-btn');
  if (btn) { btn.disabled = true; btn.innerHTML = '<span>检测中...</span>'; }

  showToast('正在检测前置依赖...', 'info');

  try {
    const [depResult, installedModsData] = await Promise.all([
      API.getDependenciesRecursive(versionId, source, gameVersion, loader),
      API.getInstalledMods().catch(() => [])
    ]);
    const deps = depResult.dependencies || [];
    const installedMods = Array.isArray(installedModsData) ? installedModsData : [];

    if (deps.length === 0) {
      showToast('该模组没有前置依赖', 'info');
      if (btn) { btn.disabled = false; btn.innerHTML = '<span>一键下载</span>'; }
      return;
    }

    showToast('请选择保存文件夹...', 'info');
    const defaultPath = await resolveModSavePath();
    const folderResult = await API.selectSaveFolder(defaultPath);
    if (folderResult.cancelled || !folderResult.path) {
      showToast('已取消', 'info');
      if (btn) { btn.disabled = false; btn.innerHTML = '<span>一键下载</span>'; }
      return;
    }
    const savePath = folderResult.path;

    const toDownload = [];
    const seen = new Set();
    for (const dep of deps) {
      if (!dep.compatibleVersion) continue;
      if (seen.has(dep.projectId)) continue;
      seen.add(dep.projectId);
      const depFileName = (dep.compatibleVersion.fileName || '').toLowerCase();
      const depBaseName = depFileName.replace(/\.jar$/i, '').replace(/[-_](v?\d[\w.\-]*)$/i, '');
      const alreadyInstalled = installedMods.some(m => {
        if (m.id === dep.projectId) return true;
        if (!m.filename) return false;
        const fn = m.filename.toLowerCase();
        if (fn.includes(dep.projectId.toLowerCase())) return true;
        if (depFileName && fn === depFileName) return true;
        if (depBaseName.length >= 3) {
          const mBase = fn.replace(/\.jar\.disabled$/i, '').replace(/\.jar$/i, '').replace(/[-_](v?\d[\w.\-]*)$/i, '');
          if (mBase.length >= 3 && (mBase === depBaseName || mBase.includes(depBaseName) || depBaseName.includes(mBase))) return true;
        }
        return false;
      });
      if (!alreadyInstalled) toDownload.push(dep);
    }

    if (toDownload.length === 0) {
      showToast('所有前置依赖均已安装', 'info');
      if (btn) { btn.disabled = false; btn.innerHTML = '<span>一键下载</span>'; }
      return;
    }

    if (btn) { btn.innerHTML = `<span>下载中 (0/${toDownload.length})...</span>`; }

    let downloaded = 0;
    for (const dep of toDownload) {
      try {
        if (btn) { btn.innerHTML = `<span>下载中 (${downloaded + 1}/${toDownload.length})...</span>`; }
        const result = await API.downloadModVersion(
          dep.compatibleVersion.versionId, dep.projectId, source, '',
          gameVersion, loader, savePath, false
        );
        if (result.success && result.sessionId) {
          showModDownloadModal(result.fileName, result.sessionId, savePath, dep.icon || '');
        } else {
          showToast(`${dep.title}: ${result.error || '下载失败'}`, 'error');
        }
      } catch (e) {
        showToast(`${dep.title}: ${e.message || '下载失败'}`, 'error');
      }
      downloaded++;
    }

    mdDepsCache.clear();
    showToast(`已提交 ${downloaded} 个前置依赖下载`, 'success');
    if (btn) { btn.disabled = false; btn.innerHTML = '<span>一键下载</span>'; }
  } catch (e) {
    showToast('检测前置依赖失败: ' + (e.message || '未知错误'), 'error');
    if (btn) { btn.disabled = false; btn.innerHTML = '<span>一键下载</span>'; }
  }
}


let _mdvRenderedCount = 0;
const MDV_INITIAL_RENDER = 30;

function _buildVersionItemHtml(v, idx) {
  const verNum = v.versionNumber || v.versionName || v.id.substring(0, 12);
  const gvs = (v.gameVersions || []).slice(0, 3).join(', ');
  const releaseType = v.releaseType === 'release' ? '' : (v.releaseType === 'beta' ? '测试版' : '');
  const files = v.files || [];
  const fileCount = files.length;
  
  const loaders = v.loaders || [];
  const loaderBadges = loaders.map(l => {
    const ll = l.toLowerCase();
    let color = '#888', bg = 'rgba(136,136,136,0.15)';
    if (ll === 'fabric') { color = '#dbb07c'; bg = 'rgba(219,176,124,0.15)'; }
    else if (ll === 'forge') { color = '#4a6b8a'; bg = 'rgba(74,107,138,0.15)'; }
    else if (ll === 'neoforge') { color = '#f47733'; bg = 'rgba(244,119,51,0.15)'; }
    else if (ll === 'quilt') { color = '#9b59b6'; bg = 'rgba(155,89,182,0.15)'; }
    return `<span class="loader-badge" style="background:${bg};color:${color}">${escapeHtml(l)}</span>`;
  }).join('');

  const safeVid = btoa(encodeURIComponent(v.id || ''));
  const primaryFile = files[0] || {};
  const safeFid = btoa(encodeURIComponent(primaryFile.id || ''));
  const isMod = currentModDetailType === 'mod';
  const isModpack = currentModDetailType === 'modpack';
  const hasMultipleFiles = fileCount > 1;

  let primaryBtnHtml;
  if (modMultiSelectMode && isMod) {
    const alreadySelected = modSelectedIds.has(currentModDetailId);
    primaryBtnHtml = `<button class="btn ${alreadySelected ? 'btn-secondary' : 'btn-primary'} btn-sm mdv-install-btn mdv-header-btn" data-vid="${safeVid}" data-fid="${safeFid}" onclick="event.stopPropagation();addModFromDetail('${escapeOnclick(currentModDetailId)}', '${escapeOnclick(currentModDetailSource)}', '${safeVid}', '${safeFid}')">${alreadySelected ? '已添加' : '添加'}</button>`;
  } else if (isModpack) {
    primaryBtnHtml = `<button class="btn btn-primary btn-sm mdv-header-btn" onclick="event.stopPropagation();installModpackVersion('${escapeOnclick(currentModDetailId)}', '${escapeOnclick(v.id)}')">下载</button>`;
  } else if (isMod) {
    primaryBtnHtml = `<button class="btn btn-primary btn-sm mdv-header-btn" onclick="event.stopPropagation();installModFile('${escapeOnclick(currentModDetailId)}', '${escapeOnclick(currentModDetailSource)}', '${escapeOnclick(v.id)}', '${escapeOnclick(primaryFile.id || '')}')">安装</button>`;
  } else {
    primaryBtnHtml = `<button class="btn btn-primary btn-sm mdv-header-btn" onclick="event.stopPropagation();quickInstallResourceVersion('${escapeOnclick(currentModDetailId)}', '${escapeOnclick(currentModDetailType)}', '${escapeOnclick(v.id)}')">安装</button>`;
  }

  const sizeText = fileCount > 0 ? formatNumber(Math.round((primaryFile.size || 1024) / 1024)) + ' KB' : '';

  return `<div class="mdv-group${hasMultipleFiles ? '' : ' single-file'}" id="mdvg-${idx}">
    <div class="mdv-group-header"${hasMultipleFiles ? ` onclick="toggleMdvGroup(${idx})"` : ''}>
      <div style="display:flex;align-items:center;gap:8px;flex-wrap:wrap">
        <span class="mdv-group-title">${escapeHtml(verNum)}</span>
        ${loaderBadges}
        <span style="font-size:11px;color:var(--text-muted)">${gvs}</span>
        ${releaseType ? `<span class="lver-badge" style="margin-left:4px">${releaseType}</span>` : ''}
      </div>
      <div style="display:flex;align-items:center;gap:10px">
        ${hasMultipleFiles ? `<span style="font-size:11px;color:var(--text-muted)">${fileCount} 个文件</span>` : (sizeText ? `<span style="font-size:11px;color:var(--text-muted)">${sizeText}</span>` : '')}
        ${primaryBtnHtml}
        ${hasMultipleFiles ? '<svg class="mdv-expand-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M6 9l6 6 6-6"/></svg>' : ''}
      </div>
    </div>
    ${hasMultipleFiles ? `<div class="mdv-files">
      ${files.map(f => {
        const fname = f.filename || f.name || f.id;
        const size = formatNumber(Math.round((f.size || 1024) / 1024)) + ' KB';
        const dateStr = f.datePublished ? formatDate(f.datePublished).split(' ')[0] : '';
        const stableBadge = f.releaseType === 'release' ? '<span class="lver-badge">稳定</span>' : 
                 (f.releaseType === 'beta' ? '<span class="lver-badge">测试版</span>' : '');
        const loaderIcon = getLoaderFileIcon(fname);
        const safeFid2 = btoa(encodeURIComponent(f.id || ''));
        let addBtn, rowOnclick;
        if (modMultiSelectMode && isMod) {
          const alreadySelected = modSelectedIds.has(currentModDetailId);
          addBtn = `<button class="btn ${alreadySelected ? 'btn-secondary' : 'btn-primary'} btn-sm mdv-install-btn" onclick="event.stopPropagation();addModFromDetail('${escapeOnclick(currentModDetailId)}', '${escapeOnclick(currentModDetailSource)}', '${safeVid}', '${safeFid2}')">${alreadySelected ? '已添加' : '添加'}</button>`;
          rowOnclick = `addModFromDetail('${escapeOnclick(currentModDetailId)}', '${escapeOnclick(currentModDetailSource)}', '${safeVid}', '${safeFid2}')`;
        } else {
          addBtn = isModpack
             ? `<button class="btn btn-primary btn-sm mdv-install-btn" onclick="event.stopPropagation();installModpackVersionSafe(this.closest('.mdv-file-item'))">下载</button>`
             : (isMod
               ? `<button class="btn btn-primary btn-sm mdv-install-btn" onclick="event.stopPropagation();installModFileSafe(this.closest('.mdv-file-item'))">安装</button>`
               : `<button class="btn btn-primary btn-sm mdv-install-btn" onclick="event.stopPropagation();installResourceVersionSafe(this.closest('.mdv-file-item'))">安装</button>`);
          rowOnclick = isModpack ? `installModpackVersionSafe(this)` : (isMod ? `installModFileSafe(this)` : `installResourceVersionSafe(this)`);
        }
        return `<div class="mdv-file-item" data-vid="${safeVid}" data-fid="${safeFid2}" onclick="${rowOnclick}">
          <div class="mdv-file-icon">${loaderIcon}</div>
          <div class="mdv-file-info">
            <div class="mdv-file-name">${escapeHtml(fname)}</div>
            <div class="mdv-file-meta">${size}${dateStr ? ' · ' + dateStr : ''}${stableBadge ? ' · ' + stableBadge : ''}</div>
          </div>
          ${addBtn}
        </div>`;
      }).join('')}
    </div>` : ''}
  </div>`;
}

function _sortVersionsByNumber(versions) {
  return [...versions].sort((a, b) => {
    // 提取版本号字符串，去掉前缀 v/V
    const aRaw = (a.versionNumber || a.versionName || '').trim();
    const bRaw = (b.versionNumber || b.versionName || '').trim();
    const aNum = aRaw.replace(/^v/i, '');
    const bNum = bRaw.replace(/^v/i, '');
    const aParts = aNum.split(/[.\-_]/).map(p => parseInt(p, 10) || 0);
    const bParts = bNum.split(/[.\-_]/).map(p => parseInt(p, 10) || 0);
    for (let i = 0; i < Math.max(aParts.length, bParts.length); i++) {
      const aVal = aParts[i] || 0;
      const bVal = bParts[i] || 0;
      if (aVal !== bVal) return bVal - aVal;
    }
    // 版本号完全相同时，按发布日期降序（新版本在前）
    const aDate = new Date(a.datePublished || 0).getTime();
    const bDate = new Date(b.datePublished || 0).getTime();
    return bDate - aDate;
  });
}

function renderMdVersionList(versions) {
  const container = document.getElementById('md-version-list');
  if (versions.length === 0) {
    container.innerHTML = '<p class="empty-text" style="padding:30px 0;text-align:center;color:var(--text-muted)">无匹配版本</p>';
    return;
  }

  const sorted = _sortVersionsByNumber(versions);
  _mdvCurrentVersions = sorted;
  _mdvRenderedCount = 0;
  const initial = sorted.slice(0, MDV_INITIAL_RENDER);
  container.innerHTML = initial.map((v, i) => _buildVersionItemHtml(v, i)).join('');
  _mdvRenderedCount = initial.length;

  if (versions.length > MDV_INITIAL_RENDER) {
    container.insertAdjacentHTML('beforeend', `<div id="mdv-load-more" style="text-align:center;padding:16px 0">
      <button class="btn btn-secondary" onclick="renderMdVersionListMore()">加载更多 (${versions.length - MDV_INITIAL_RENDER} 个版本)</button>
    </div>`);
  }
}

let _mdvCurrentVersions = [];

function renderMdVersionListMore() {
  const container = document.getElementById('md-version-list');
  const loadMoreBtn = document.getElementById('mdv-load-more');
  if (loadMoreBtn) loadMoreBtn.remove();

  const batch = _mdvCurrentVersions.slice(_mdvRenderedCount, _mdvRenderedCount + MDV_INITIAL_RENDER);
  const fragment = document.createDocumentFragment();
  const temp = document.createElement('div');
  temp.innerHTML = batch.map((v, i) => _buildVersionItemHtml(v, _mdvRenderedCount + i)).join('');
  while (temp.firstChild) fragment.appendChild(temp.firstChild);
  container.appendChild(fragment);
  _mdvRenderedCount += batch.length;

  if (_mdvRenderedCount < _mdvCurrentVersions.length) {
    container.insertAdjacentHTML('beforeend', `<div id="mdv-load-more" style="text-align:center;padding:16px 0">
      <button class="btn btn-secondary" onclick="renderMdVersionListMore()">加载更多 (${_mdvCurrentVersions.length - _mdvRenderedCount} 个版本)</button>
    </div>`);
  }
}
