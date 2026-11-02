import { API as API } from '../../core/api.js';
import DOMPurify from 'dompurify';
import { escapeHtml as escapeHtml, formatDate as formatDate, _lazyLoadScript as _lazyLoadScript } from '../../core/utils.js';
import { showToast as showToast, formatNumber as formatNumber } from '../misc/ui-components.js';
import { installModpackVersion as installModpackVersion, installModFile as installModFile, quickInstallResourceVersion as quickInstallResourceVersion } from './mod-install.js';
import { navigateToPage as navigateToPage } from '../../core/navigation.js';
import { goBackFromDetail, pushModDetailHistory } from '../versions/versions-detail-history.js';
import { modDetailState as modDetailState } from './detail-state.js';
import { appState as appState } from '../../core/app-state.js';
import { formatModNameWithChinese as formatModNameWithChinese } from './chinese-names.js';
import { S } from './mod-detail-state.js';
import { switchMdVersionTab, renderMdVersionTabs } from './mod-detail-versions.js';
import { proxifyBodyHtml, hydrateBodyImages, setLocalImage, renderModGallery } from './mod-detail-images.js';
import { loadModDependencies } from './mod-detail-deps.js';
import { getInstalledVersionInfo } from './mod-detail-installed.js';

function switchMdTab(tab) {
  document.querySelectorAll('#page-mod-detail .md-tab').forEach(t => {
    t.classList.toggle('active', t.dataset.tab === tab);
  });
  ['description', 'versions', 'gallery'].forEach(name => {
    const el = document.getElementById('md-tab-' + name);
    if (el) el.classList.toggle('active', name === tab);
  });
  // 加载器筛选只在“版本”标签下向右展开
  const loaderFilter = document.getElementById('md-loader-filter');
  if (loaderFilter) {
    const open = tab === 'versions';
    loaderFilter.classList.toggle('open', open);
    loaderFilter.setAttribute('aria-hidden', open ? 'false' : 'true');
  }
}

function updateMdLoaderChips() {
  document.querySelectorAll('#md-loader-filter .md-loader-chip').forEach(chip => {
    chip.classList.toggle('active', (chip.dataset.loader || '') === modDetailState.loaderFilter);
  });
}

function setMdLoaderFilter(loader) {
  modDetailState.loaderFilter = (loader || '').toLowerCase();
  updateMdLoaderChips();
  // 保留当前游戏版本标签，只重新渲染版本列表
  switchMdVersionTab(modDetailState.tab || '', S.mdTabMode);
}

async function renderModDescription(detail) {
  const container = document.getElementById('md-body-content');
  if (!container) return;
  const pid = appState.currentModDetailId;
  const source = appState.currentModDetailSource || 'modrinth';
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
    if (pid !== appState.currentModDetailId) return;
    if (typeof marked !== 'undefined' && typeof marked.parse === 'function') {
      try { html = marked.parse(body); } catch (e) { html = body.replace(/\n/g, '<br>'); }
    } else {
      html = body.replace(/\n/g, '<br>');
    }
  }
  // 模组描述来自 Modrinth / CurseForge，必须消毒后再进 DOM（审计 P0-5 / P1-3）
  container.innerHTML = DOMPurify.sanitize(proxifyBodyHtml(html));
  hydrateBodyImages(container);
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
      ['类型', nameMap[modDetailState.type] || modDetailState.type || '模组'],
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
  if (!modDetailState.allVersions || modDetailState.allVersions.length === 0) {
    showToast('暂无可用版本', 'warning');
    return;
  }
  const v = modDetailState.allVersions[0];
  const projectId = appState.currentModDetailId;
  const source = appState.currentModDetailSource || 'modrinth';
  const type = modDetailState.type || 'mod';
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
  modDetailState.loaderFilter = '';
  updateMdLoaderChips();
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

function _renderModDetailHeader(detail, source, projectId) {
  modDetailState.data = detail;
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
    const tone = source === 'curseforge' ? '--orange' : '--purple';
    srcBadge.textContent = source === 'curseforge' ? 'CurseForge' : 'Modrinth';
    srcBadge.style.color = `var(${tone})`;
    srcBadge.style.background = `color-mix(in srgb, var(${tone}) 14%, transparent)`;
  }
  const mdFavBtn = document.getElementById('md-fav-btn');
  if (mdFavBtn) {
    const isFav = appState._favorites.some((f) => f.favs.includes(projectId));
    if (isFav) { mdFavBtn.classList.remove('btn-secondary'); mdFavBtn.classList.add('btn-primary'); mdFavBtn.innerHTML = '<svg viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" stroke-width="2" style="width:14px;height:14px"><path d="M20.84 4.61a5.5 5.5 0 00-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 00-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 000-7.78z"/></svg> 已收藏'; }
    else { mdFavBtn.classList.remove('btn-primary'); mdFavBtn.classList.add('btn-secondary'); mdFavBtn.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:14px;height:14px"><path d="M20.84 4.61a5.5 5.5 0 00-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 00-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 000-7.78z"/></svg> 收藏'; }
  }
}

async function openModDetail(projectId, source) {
  const mySeq = ++modDetailState.seq;
  pushModDetailHistory();
  appState.currentModDetailId = projectId;
  appState.currentModDetailSource = source || 'modrinth';
  modDetailState.type = 'mod';

  const compatSection = document.getElementById('md-compat-section');
  if (compatSection) compatSection.style.display = 'none';

  navigateToPage('mod-detail');
  resetMdDetailView();

  const backBtn = document.querySelector('#page-mod-detail .moddetail-page-header .btn-icon');
  if (backBtn) backBtn.addEventListener('click', () => goBackFromDetail());

  const mdVersionList = document.getElementById('md-version-list');
  const mdVersionTabs = document.getElementById('md-version-tabs');

  if (!mdVersionList) { console.error('[ModDetail] Required elements not found'); return; }

  // 立即清空旧版本列表，防止切换时短暂显示上一个模组的版本
  mdVersionList.innerHTML = '';
  if (mdVersionTabs) mdVersionTabs.innerHTML = '';

  const cachedRaw = S._projectDataCache.get(projectId);
  const cached = (cachedRaw && typeof cachedRaw.body === 'string') ? cachedRaw : null;
  const hasPreloaded = S._versionPreloadCache.has(projectId);
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
      ? Promise.resolve(S._versionPreloadCache.get(projectId))
      : API.getModVersions(projectId, source).catch(e => { console.error('[ModDetail] getModVersions failed:', e); return null; });
    S._versionPreloadCache.delete(projectId);
    const detailPromise = cached ? Promise.resolve(cached) : API.getModDetail(projectId, source).catch(e => { console.error('[ModDetail] getModDetail failed:', e); return null; });

    const [detail, versionsData] = await Promise.all([detailPromise, versionsPromise]);
    if (_loadingTimer) { clearTimeout(_loadingTimer); _loadingTimer = null; }
    if (mySeq !== modDetailState.seq) { return; }
    if (!detail) {
      const mdName = document.getElementById('md-name');
      if (mdName) mdName.textContent = '加载失败';
      mdVersionList.innerHTML = `<p class="empty-text" style="padding:30px 0;text-align:center;color:var(--text-muted)">无法加载模组详情: API请求失败，请检查网络连接</p>`;
      return;
    }
    if (!cached) {
      S._projectDataCache.set(projectId, detail);
      _renderModDetailHeader(detail, source, projectId);
      renderModDescription(detail);
      renderModGallery(detail);
      renderMdSidebar(detail);
    }

    modDetailState.allVersions = versionsData ? (versionsData.versions || []) : [];
    if (!Array.isArray(modDetailState.allVersions)) modDetailState.allVersions = [];
    loadModDependencies();
    await renderMdVersionTabs(mySeq);
  } catch (e) {
    if (mySeq !== modDetailState.seq) return;
    console.error('[ModDetail] Error:', e);
    const mdName = document.getElementById('md-name');
    if (mdName) mdName.textContent = '加载失败';
    mdVersionList.innerHTML = `<p class="empty-text" style="padding:30px 0;text-align:center;color:var(--text-muted)">无法加载模组详情: ${escapeHtml(e.message || String(e))}</p>`;
  }
}

export { switchMdTab, updateMdLoaderChips, setMdLoaderFilter, renderModDescription, renderMdSidebar, installCurrentDetailVersion, resetMdDetailView, getInstalledVersionInfo, _renderModDetailHeader, openModDetail };
