// versions-list.js — 版本列表、卡片与首页当前版本
import { API as API } from '../../core/api.js';
import { bridge as bridge } from '../../core/tauri-bridge.js';
import { escapeHtml as escapeHtml, formatDate as formatDate } from '../../core/utils.js';
import { showToast as showToast, getVersionTypeLabel as getVersionTypeLabel } from '../misc/ui-components.js';
import { populateModVersionFilter as populateModVersionFilter } from '../mods/mod-browse.js';
import { updateHomeStats as updateHomeStats } from '../settings/window-theme.js';
import { appState as appState } from '../../core/app-state.js';
import { isPinned as isPinned, PIN_ICONS as PIN_ICONS } from './pinned-store.js';
import { actionAttrs as actionAttrs, actionAttrsStop as actionAttrsStop } from '../../core/actions.js';
import { getVersionCardStyle as getVersionCardStyle } from './version-card-utils.js';
import { versionsState as S } from './versions-state.js';
import { getSelectedFolder } from './versions-external.js';
import { renderBedrockVersions } from './versions-bedrock.js';

async function loadVersions(forceRefresh = false) {
  if (!forceRefresh && S._versionsLoadTime > 0 &&
    (Date.now() - S._versionsLoadTime) < S._VERSIONS_CACHE_TTL &&
    (appState.allVersions.length > 0 || appState.installedVersions.length > 0)) {
    return;
  }
  try {
    const data = await API.getVersions(forceRefresh);
    appState.allVersions = data.versions || [];
    appState.installedVersions = data.installed || [];
    if (!Array.isArray(appState.allVersions)) appState.allVersions = [];
    if (!Array.isArray(appState.installedVersions)) appState.installedVersions = [];
    appState.versionIconsTimestamp = Date.now();
    S.versionsLoadFailed = false;
    S._versionsLoadTime = Date.now();

    await updateVersionSelects();
    renderVersions();
    updateHomeStats();
    populateModVersionFilter();
  } catch (e) {
    console.error('[Versions] Load failed:', e.message);
    S.versionsLoadFailed = true;
    
    const container = document.getElementById('versions-list');
    if (container && appState.installedVersions.length > 0) {
      appState.currentVersionTab = 'installed';
      renderVersions();
      const tabs = document.querySelectorAll('.tab-btn[data-tab]');
      tabs.forEach(t => t.classList.remove('active'));
      const installedTab = document.querySelector('.tab-btn[data-tab="installed"]');
      if (installedTab) installedTab.classList.add('active');
    } else if (container) {
      container.innerHTML = `
        <p class="empty-text">加载版本列表失败</p>
        <button class="btn btn-primary btn-sm" style="margin-top:12px"${actionAttrs('retryLoadVersions')}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:14px;height:14px;vertical-align:middle;margin-right:4px">
            <polyline points="23 4 23 10 17 10"/><path d="M20.49 15a9 9 0 11-2.12-9.36L23 10"/>
          </svg> 重试
        </button>`;
    }
    populateModVersionFilter();

    if (!forceRefresh && !S.versionsRetryTimer) {
      S.versionsRetryTimer = setTimeout(() => {
        S.versionsRetryTimer = null;
        if (S.versionsLoadFailed) {
          loadVersions(false);
        }
      }, 30000);
    }
  }
}

function retryLoadVersions() {
  if (S.versionsRetryTimer) clearTimeout(S.versionsRetryTimer);
  S.versionsRetryTimer = null;
  const container = document.getElementById('versions-list');
  if (container) {
    container.innerHTML = `<div class="loading-spinner"><div class="spinner"></div><p>正在重新加载...</p></div>`;
  }
  loadVersions(true);
}

async function updateVersionSelects() {
  let currentVal = appState.currentLaunchVersionId;
  if (!currentVal && appState.launchVersionCustomSelect) {
    currentVal = appState.launchVersionCustomSelect.getValue();
  }

  if (!currentVal) {
    try {
      if (appState._cachedLastLaunchVersion === null) {
        appState._cachedLastLaunchVersion = await bridge.store.get('versepc_last_launch_version') || '';
      }
      currentVal = appState._cachedLastLaunchVersion;
    } catch (_) {}
  }

  const versionOptions = appState.installedVersions.filter(v => !v.error).map(v => {
    const customName = v.customName || '';
    let baseName = v.isExternal ? v.id.replace(/ \[外部\d*\]/, '') : v.id;
    let text = customName || baseName;
    let subtext = customName ? baseName : '';
    const platformTag = v.isBedrock ? '[基岩]' : '[Java]';
    text = `${platformTag} ${text}`;
    if (v.isBedrock) { subtext = (subtext ? subtext + ' · ' : '') + '基岩版'; }
    else if (v.isModpack) { text += ` [${v.modpackLoader || '整合包'}]`; subtext = (subtext ? subtext + ' · ' : '') + (v.modpackLoader || '整合包'); }
    else if (v.isFabric) { text += ' [Fabric]'; subtext = (subtext ? subtext + ' · ' : '') + 'Fabric Loader'; }
    else if (v.isForge) { text += ' [Forge]'; subtext = (subtext ? subtext + ' · ' : '') + 'Forge'; }
    else if (v.isNeoForge) { text += ' [NeoForge]'; subtext = (subtext ? subtext + ' · ' : '') + 'NeoForge'; }
    else { subtext = (subtext ? subtext + ' · ' : '') + 'Vanilla'; }
    if (v.isExternal) { subtext += ' · 外部文件夹'; }
    return { value: v.id, text: text, subtext: subtext };
  });

  // 选中的版本不在列表中（已被删除等）→ 回退到第一个
  if (currentVal && !versionOptions.find(o => o.value === currentVal)) {
    currentVal = versionOptions.length > 0 ? versionOptions[0].value : '';
  } else if (!currentVal && versionOptions.length > 0) {
    currentVal = versionOptions[0].value;
  }

  // 同步 appState.currentLaunchVersionId 状态变量
  appState.currentLaunchVersionId = currentVal;
  appState._cachedLastLaunchVersion = currentVal;
  try {
    if (currentVal) bridge.store.set('versepc_last_launch_version', currentVal);
  } catch (_) {}

  // 同步底部启动栏（隐藏）的选择器
  if (appState.launchVersionCustomSelect) {
    appState.launchVersionCustomSelect.setOptions(versionOptions);
    appState.launchVersionCustomSelect.setValue(currentVal);
  }

  // 渲染主页内嵌版本卡片
  renderHomeCurrentVersionCard();
}

function renderHomeCurrentVersionCard() {
  const card = document.getElementById('home-current-version-card');
  if (!card) return;

  const arrow = `<svg class="home-current-version-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="9 18 15 12 9 6"></polyline></svg>`;

  const placeholder = `<div class="home-current-version-placeholder">
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="home-current-version-placeholder-icon">
      <circle cx="12" cy="12" r="10"></circle>
      <line x1="12" y1="8" x2="12" y2="16"></line>
      <line x1="8" y1="12" x2="16" y2="12"></line>
    </svg>
    <span>点击选择版本</span>
  </div>`;

  if (!appState.currentLaunchVersionId) {
    card.innerHTML = placeholder + arrow;
    return;
  }

  const v = appState.installedVersions.find(x => x.id === appState.currentLaunchVersionId);
  if (!v) {
    // 选中的版本已被删除，重置
    appState.currentLaunchVersionId = '';
    card.innerHTML = placeholder + arrow;
    return;
  }

  const iconDataAttrs = `data-vi-id="${escapeHtml(v.id)}" data-vi-type="release" data-vi-forge="${v.isForge ? '1' : '0'}" data-vi-fabric="${v.isFabric ? '1' : '0'}" data-vi-neoforge="${v.isNeoForge ? '1' : '0'}" data-vi-modpack="${v.isModpack ? '1' : '0'}" data-vi-extdir="${v.externalVersionDir ? escapeHtml(v.externalVersionDir) : ''}"`;

  let badge = '原版', badgeClass = '';
  if (v.isBedrock) { badge = '基岩版'; badgeClass = 'bedrock'; }
  else if (v.isModpack) { badge = v.modpackLoader || '整合包'; badgeClass = 'modpack'; }
  else if (v.isFabric) { badge = 'Fabric'; badgeClass = 'fabric'; }
  else if (v.isForge) { badge = 'Forge'; badgeClass = 'forge'; }
  else if (v.isNeoForge) { badge = 'NeoForge'; badgeClass = 'forge'; }

  const externalBadgeHtml = v.isExternal ? '<span class="v-badge external">外部</span>' : '';
  const displayName = v.isExternal ? (v.customName || v.id.replace(/ \[外部\d*\]/, '')) : (v.customName || v.id);

  // 星星收藏按钮（置顶到便利贴面板）；必须 stopPropagation，避免命中页面切换委托
  const starBtn = `<button class="pin-btn pin-btn-star${isPinned('version', v.id) ? ' active' : ''}"
    title="${isPinned('version', v.id) ? '取消置顶' : '置顶版本'}"${actionAttrsStop('pinVersion', v.id, displayName, badge)}>
    ${PIN_ICONS.star}</button>`;

  card.innerHTML = `
    <div class="version-item-left">
      <div class="version-item-icon"><img src="" alt="" class="version-icon-img" ${iconDataAttrs}></div>
      <div class="version-item-info">
        <span class="version-item-name">${escapeHtml(displayName)}</span>
        <span class="version-item-meta"><span class="v-badge ${badgeClass}">${badge}</span>${externalBadgeHtml}</span>
      </div>
    </div>
    ${starBtn}
    ${arrow}
  `;
  _loadVersionIcons(card);
}

function selectLaunchVersion(versionId) {
  if (!versionId) return;
  appState.currentLaunchVersionId = versionId;
  appState._cachedLastLaunchVersion = versionId;
  try { bridge.store.set('versepc_last_launch_version', versionId); } catch (_) {}
  // 同步底部启动栏（隐藏）的选择器
  if (appState.launchVersionCustomSelect) {
    appState.launchVersionCustomSelect.setValue(versionId);
  }
  // 更新"已安装"tab 中卡片的 selected 类
  document.querySelectorAll('.version-item-clickable[data-installed="true"]').forEach(item => {
    item.classList.toggle('selected', item.dataset.versionId === versionId);
  });
  // 更新主页内嵌版本卡片
  renderHomeCurrentVersionCard();
}

async function quickDeleteErrorVersion(versionId) {
  if (!confirm(`确定要删除版本 "${versionId}" 吗？`)) {
    return;
  }
  try {
    const r = await API.deleteVersion(versionId, false);
    if (r.success) {
      showToast(`版本 ${versionId} 已删除`, 'success');
      await loadVersions(true);
    } else {
      showToast(r.error || '删除失败', 'error');
    }
  } catch (e) {
    console.error('[Delete] API异常:', e);
    showToast('删除失败: ' + (e.message || e), 'error');
  }
}

function toggleErrorVersions(id) {
  const list = document.getElementById(id);
  const header = list?.previousElementSibling;
  if (!list) return;
  list.classList.toggle('show');
  header?.classList.toggle('expanded');
}

function renderInstalledVersionsInto(container) {
  const selectedFolder = getSelectedFolder();
  let versions = appState.installedVersions;
  if (selectedFolder === '__internal') {
    versions = versions.filter(v => !v.isExternal);
  } else {
    versions = versions.filter(v => v.isExternal && v.externalPath === selectedFolder);
  }
  if (!versions || versions.length === 0) {
    container.innerHTML = '<p class="empty-text">该文件夹下暂无版本，前往"下载"页面安装一个吧</p>';
    return;
  }

  // 同步卡片样式开关 UI；卡片模式时懒加载官方版本宣传图
  const cardMode = getVersionCardStyle();
  syncVersionCardSwitches();
  if (cardMode) fetchVersionBanners();

  const errorVersions = versions.filter(v => v.error);
  const normalVersions = versions.filter(v => !v.error);

  const vanillaVersions = normalVersions.filter(v => {
    return !v.isFabric && !v.isForge && !v.isNeoForge && !v.isOptiFine && !v.isLiteLoader && !v.isModpack;
  });
  const moddedVersions = normalVersions.filter(v => {
    return v.isFabric || v.isForge || v.isNeoForge || v.isOptiFine || v.isLiteLoader || v.isModpack;
  });

  const renderVersionItem = (v) => {
    const iconClass = v.type === 'snapshot' ? 'snapshot' : v.type === 'special' ? 'special' : 'installed';
    const iconDataAttrs = `data-vi-id="${escapeHtml(v.id)}" data-vi-type="${v.type || 'release'}" data-vi-forge="${v.isForge ? '1' : '0'}" data-vi-fabric="${v.isFabric ? '1' : '0'}" data-vi-neoforge="${v.isNeoForge ? '1' : '0'}" data-vi-modpack="${v.isModpack ? '1' : '0'}" data-vi-extdir="${v.externalVersionDir ? escapeHtml(v.externalVersionDir) : ''}"`;
    const externalBadgeHtml = v.isExternal ? '<span style="display:inline-block;background:rgba(255,165,0,0.15);color:#ffa500;font-size:10px;padding:1px 6px;border-radius:4px;margin-left:6px">外部文件夹</span>' : '';
    const externalPathHtml = v.isExternal && v.externalPath ? `<span style="color:var(--text-muted);font-size:11px;margin-left:4px" title="${escapeHtml(v.externalPath)}">${escapeHtml(v.externalPath)}</span>` : '';
    const displayName = v.isExternal ? (v.customName || v.id.replace(/ \[外部\d*\]/, '')) : (v.customName || v.id);
    const deleteBtnHtml = `<button class="btn btn-danger btn-sm"${actionAttrsStop('deleteVersion', v.id)}>${v.isExternal ? '移除' : '删除'}</button>`;
    const loaderText = getVersionTypeLabel(v);
    const starBtnHtml = `<button class="pin-btn pin-btn-star${isPinned('version', v.id) ? ' active' : ''}"
      title="${isPinned('version', v.id) ? '取消置顶' : '置顶版本'}"${actionAttrsStop('pinVersion', v.id, displayName, loaderText)}>
      ${PIN_ICONS.star}</button>`;
    const settingsBtnHtml = v.isBedrock ? '' : `<button class="version-item-settings-btn" data-version-id="${escapeHtml(v.id)}" data-custom-name="${escapeHtml(v.customName || '')}" title="版本设置"${actionAttrsStop('openVersionSettings', v.id, displayName)}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="3"></circle><path d="M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 11-2.83 2.83l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 11-4 0v-.09A1.65 1.65 0 009 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 11-2.83-2.83l.06-.06a1.65 1.65 0 00.33-1.82 1.65 1.65 0 00-1.51-1H3a2 2 0 110-4h.09A1.65 1.65 0 004.6 9a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 112.83-2.83l.06.06a1.65 1.65 0 001.82.33H9a1.65 1.65 0 001-1.51V3a2 2 0 114 0v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 112.83 2.83l-.06.06a1.65 1.65 0 00-.33 1.82V9a1.65 1.65 0 001.51 1H21a2 2 0 110 4h-.09a1.65 1.65 0 00-1.51 1z"></path></svg>
    </button>`;
    const selectedClass = appState.currentLaunchVersionId === v.id ? ' selected' : '';
    return `<div class="version-item version-item-clickable${selectedClass}"
      data-version-id="${escapeHtml(v.id)}"
      data-version-url=""
      data-version-type="${v.type || 'release'}"
      data-installed="true"
      data-custom-name="${escapeHtml(v.customName || '')}">
      <div class="version-item-left">
        <div class="version-item-icon ${iconClass}">
          <img src="" alt="" class="version-icon-img" ${iconDataAttrs}>
        </div>
        <div class="version-item-info">
          <span class="version-item-name">${displayName}${externalBadgeHtml}</span>
          <span class="version-item-meta">${getVersionTypeLabel(v)} \u00B7 ${formatDate(v.releaseTime)}${externalPathHtml}</span>
        </div>
      </div>
      <div class="version-item-actions">
        ${starBtnHtml}
        ${settingsBtnHtml}
        ${deleteBtnHtml}
      </div>
    </div>`;
  };

  const renderCardItem = (v) => {
    const iconClass = v.type === 'snapshot' ? 'snapshot' : v.type === 'special' ? 'special' : 'installed';
    const iconDataAttrs = `data-vi-id="${escapeHtml(v.id)}" data-vi-type="${v.type || 'release'}" data-vi-forge="${v.isForge ? '1' : '0'}" data-vi-fabric="${v.isFabric ? '1' : '0'}" data-vi-neoforge="${v.isNeoForge ? '1' : '0'}" data-vi-modpack="${v.isModpack ? '1' : '0'}" data-vi-extdir="${v.externalVersionDir ? escapeHtml(v.externalVersionDir) : ''}"`;
    const displayName = v.isExternal ? (v.customName || v.id.replace(/ \[外部\d*\]/, '')) : (v.customName || v.id);
    const loaderText = getVersionTypeLabel(v);
    const badge = v.isModpack ? (v.modpackLoader || '整合包') : loaderText;
    // 整合包显示封面图（get_version_icon 对 modpack 返回 pack.png），普通版本显示 Mojang 宣传图，无图回退版本图标
    const bannerUrl = v.isModpack ? '' : getVersionBannerUrl(v.id);
    const bgInner = (v.isModpack || !bannerUrl)
      ? `<img src="" alt="" class="version-icon-img ver-dl-card-img" ${iconDataAttrs}>`
      : `<img src="${bannerUrl}" alt="" class="ver-dl-card-banner" loading="lazy">`;
    const selectedClass = appState.currentLaunchVersionId === v.id ? ' selected' : '';
    const starBtnHtml = `<button class="pin-btn pin-btn-star${isPinned('version', v.id) ? ' active' : ''}"
      title="${isPinned('version', v.id) ? '取消置顶' : '置顶版本'}"${actionAttrsStop('pinVersion', v.id, displayName, loaderText)}>
      ${PIN_ICONS.star}</button>`;
    const settingsBtnHtml = v.isBedrock ? '' : `<button class="pin-btn ver-dl-card-settings" data-version-id="${escapeHtml(v.id)}" data-custom-name="${escapeHtml(v.customName || '')}" title="版本设置"${actionAttrsStop('openVersionSettings', v.id, displayName)}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="3"></circle><path d="M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 11-2.83 2.83l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 11-4 0v-.09A1.65 1.65 0 009 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 11-2.83-2.83l.06-.06a1.65 1.65 0 00.33-1.82 1.65 1.65 0 00-1.51-1H3a2 2 0 110-4h.09A1.65 1.65 0 004.6 9a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 112.83-2.83l.06.06a1.65 1.65 0 001.82.33H9a1.65 1.65 0 001-1.51V3a2 2 0 114 0v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 112.83 2.83l-.06.06a1.65 1.65 0 00-.33 1.82V9a1.65 1.65 0 001.51 1H21a2 2 0 110 4h-.09a1.65 1.65 0 00-1.51 1z"></path></svg>
    </button>`;
    const deleteBtnHtml = `<button class="ver-dl-card-del" title="${v.isExternal ? '移除' : '删除'}"${actionAttrsStop('deleteVersion', v.id)}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 01-2 2H7a2 2 0 01-2-2V6m3 0V4a2 2 0 012-2h4a2 2 0 012 2v2"/></svg>
    </button>`;
    return `<div class="ver-dl-card version-item-clickable${selectedClass}"
      data-version-id="${escapeHtml(v.id)}"
      data-version-url=""
      data-version-type="${v.type || 'release'}"
      data-installed="true"
      data-custom-name="${escapeHtml(v.customName || '')}">
      <div class="ver-dl-card-bg">
        ${bgInner}
        <span class="ver-dl-card-badge">${escapeHtml(badge)}</span>
      </div>
      <div class="ver-dl-card-info">
        <div class="ver-dl-card-name" title="${escapeHtml(displayName)}">${escapeHtml(displayName)}</div>
        <div class="ver-dl-card-meta">${escapeHtml(loaderText)} · ${formatDate(v.releaseTime)}</div>
        <div class="ver-dl-card-actions">
          ${starBtnHtml}${settingsBtnHtml}${deleteBtnHtml}
        </div>
      </div>
    </div>`;
  };

  let html = '';

  if (cardMode) {
    if (normalVersions.length > 0) {
      html += '<div class="ver-dl-grid">' + normalVersions.map(renderCardItem).join('') + '</div>';
    }
  } else {
    if (moddedVersions.length > 0) {
      html += moddedVersions.map(renderVersionItem).join('');
    }
    if (vanillaVersions.length > 0) {
      html += `<div style="margin-top:16px;padding-top:12px;border-top:1px solid var(--border)">
        <div style="display:flex;align-items:center;gap:8px;margin-bottom:8px;padding:0 4px">
          <span style="font-size:12px;color:var(--text-muted);font-weight:600">\u26A0 基础版本 (${vanillaVersions.length})</span>
        </div>
        ${vanillaVersions.map(renderVersionItem).join('')}
      </div>`;
    }
  }

  if (errorVersions.length > 0) {
    const errorId = 'error-versions-' + Date.now();
    const tntIcon = `<img src="assets/tnt.png" alt="TNT" width="40" height="40" style="image-rendering:pixelated;image-rendering:crisp-edges;">`;
    html += `<div class="error-versions-section" style="margin-top:16px;padding-top:12px;border-top:1px solid rgba(239,68,68,0.2)">
      <div class="error-versions-header"${actionAttrs('toggleErrorVersions', errorId)}>
        <span>\u26A0 错误的版本 (<span class="error-count">${errorVersions.length}</span>)</span>
        <span class="error-chevron">▼</span>
      </div>
      <div class="error-versions-list" id="${errorId}">
        ${errorVersions.map(v => {
          const displayName = v.customName || v.id.replace(/ \[外部\d*\]/, '');
          return `<div class="error-version-item">
            <div class="error-version-icon">${tntIcon}</div>
            <div class="error-version-info">
              <span class="error-version-name">${escapeHtml(displayName)}</span>
              <span class="error-version-reason">${escapeHtml(v.errorReason || '无法识别')}</span>
            </div>
            <button class="btn btn-danger btn-sm" style="flex-shrink:0;padding:4px 12px;font-size:12px"${actionAttrsStop('quickDeleteErrorVersion', v.id)}>删除</button>
          </div>`;
        }).join('')}
      </div>
    </div>`;
  }

  container.innerHTML = html;
  _loadVersionIcons(container);
}

function fetchVersionBanners() {
  if (S._versionBannerMap || S._versionBannerLoading) return;
  S._versionBannerLoading = true;
  fetch('https://launchercontent.mojang.com/v2/javaPatchNotes.json', { method: 'GET' })
    .then(r => (r.ok ? r.json() : Promise.reject(new Error('HTTP ' + r.status))))
    .then(data => {
      const map = {};
      (data.entries || []).forEach(e => {
        if (e.version && e.image && e.image.url) {
          map[e.version] = e.image.url.indexOf('http') === 0
            ? e.image.url
            : 'https://launchercontent.mojang.com' + e.image.url;
        }
      });
      S._versionBannerMap = map;
      // 已有卡片在渲染：填充宣传图并重新渲染当前列表
      if (getVersionCardStyle() && appState.currentVersionTab !== 'installed') renderVersions();
    })
    .catch(() => {})
    .finally(() => { S._versionBannerLoading = false; });
}

function getVersionBannerUrl(versionId) {
  return S._versionBannerMap ? (S._versionBannerMap[versionId] || '') : '';
}

function syncVersionCardSwitches() {
  const on = getVersionCardStyle();
  document.querySelectorAll('#ver-card-style-switch').forEach(sw => {
    sw.classList.toggle('active', on);
  });
}

function toggleVersionCardStyle() {
  const next = !getVersionCardStyle();
  try { localStorage.setItem('versepc_version_card_style', next ? 'card' : 'list'); } catch (e) {}
  syncVersionCardSwitches();
  // 重新渲染当前可见的版本列表（未安装列表或已安装列表）
  const active = document.querySelector('.page.active');
  if (active && active.id === 'page-installed-versions') {
    const c = document.getElementById('installed-versions-list');
    if (c) renderInstalledVersionsInto(c);
  } else {
    renderVersions();
  }
  if (typeof showToast === 'function') {
    showToast(next ? '已切换为卡片样式' : '已切换为列表样式', 'success');
  }
}

function renderVersions() {
  const container = document.getElementById('versions-list');
  if (!container) return;
  if (appState.currentVersionTab === 'installed') {
    renderInstalledVersionsInto(container);
    return;
  }
  if (appState.currentVersionTab === 'bedrock') {
    renderBedrockVersions(container);
    return;
  }

  let versions;
  if (appState.currentVersionTab === 'old') {
    versions = appState.allVersions.filter(v => v.type === 'old_beta' || v.type === 'old_alpha');
  } else {
    versions = appState.allVersions.filter(v => v.type === appState.currentVersionTab);
  }

  if (versions.length === 0) {
    container.innerHTML = '<p class="empty-text">暂无版本</p>';
    return;
  }

  // 同步卡片样式开关 UI
  const cardMode = getVersionCardStyle();
  syncVersionCardSwitches();

  // 首次渲染时懒加载官方版本宣传图（后续由 fetch 完成回调重新渲染）
  if (cardMode) fetchVersionBanners();

  // 新版卡片样式：与存档界面卡片一致，主体显示 Mojang 版本宣传图（无图时回退版本图标）
  if (cardMode) {
    container.innerHTML = '<div class="ver-dl-grid">' + versions.map(v => {
      const iconClass = v.type === 'snapshot' ? 'snapshot' : v.type === 'special' ? 'special' : (v.type === 'old_beta' || v.type === 'old_alpha') ? 'old' : 'release';
      const iconDataAttrs = `data-vi-id="${escapeHtml(v.id)}" data-vi-type="${v.type || 'release'}" data-vi-forge="0" data-vi-fabric="0" data-vi-neoforge="0" data-vi-modpack="0" data-vi-extdir=""`;
      const badge = getVersionTypeLabel(v);
      const bannerUrl = getVersionBannerUrl(v.id);
      const bgInner = bannerUrl
        ? `<img src="${bannerUrl}" alt="" class="ver-dl-card-banner" loading="lazy">`
        : `<img src="" alt="" class="version-icon-img ver-dl-card-img" ${iconDataAttrs}>`;
      return `<div class="ver-dl-card version-item-clickable ver-icon-${iconClass}"
        data-version-id="${escapeHtml(v.id)}"
        data-version-url="${escapeHtml(v.url || '')}"
        data-version-type="${escapeHtml(v.type || 'release')}">
        <div class="ver-dl-card-bg">
          ${bgInner}
          <span class="ver-dl-card-badge">${escapeHtml(badge)}</span>
        </div>
        <div class="ver-dl-card-info">
          <div class="ver-dl-card-name" title="${escapeHtml(v.id)}">${escapeHtml(v.id)}</div>
          <div class="ver-dl-card-meta">${escapeHtml(badge)} · ${formatDate(v.releaseTime)}</div>
        </div>
      </div>`;
    }).join('') + '</div>';
    _loadVersionIcons(container);
    return;
  }

  container.innerHTML = versions.map(v => {
    const iconClass = v.type === 'snapshot' ? 'snapshot' : v.type === 'special' ? 'special' : (v.type === 'old_beta' || v.type === 'old_alpha') ? 'old' : 'release';
    const iconDataAttrs = `data-vi-id="${escapeHtml(v.id)}" data-vi-type="${v.type || 'release'}" data-vi-forge="0" data-vi-fabric="0" data-vi-neoforge="0" data-vi-modpack="0" data-vi-extdir=""`;
    return `<div class="version-item version-item-clickable" 
      data-version-id="${escapeHtml(v.id)}" 
      data-version-url="${escapeHtml(v.url || '')}" 
      data-version-type="${escapeHtml(v.type || 'release')}">
      <div class="version-item-left">
        <div class="version-item-icon ${iconClass}">
          <img src="" alt="" class="version-icon-img" ${iconDataAttrs}>
        </div>
        <div class="version-item-info">
          <span class="version-item-name">${v.id}</span>
          <span class="version-item-meta">${getVersionTypeLabel(v)} \u00B7 ${formatDate(v.releaseTime)}</span>
        </div>
      </div>
      <div class="version-item-actions">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:20px;height:20px;opacity:0.5"><path d="M9 18l6-6-6-6"/></svg>
      </div>
    </div>`;
  }).join('');
  _loadVersionIcons(container);
}

function _loadVersionIcons(container) {
  if (!container) return;
  // 统一获取 Tauri invoke 函数（兼容 v1 __TAURI__ 和 v2 __TAURI_INTERNALS__）
  function _getCore() {
    if (window.__TAURI__ && window.__TAURI__.core) return window.__TAURI__.core;
    if (window.__TAURI_INTERNALS__) return window.__TAURI_INTERNALS__;
    return null;
  }
  container.querySelectorAll('.version-icon-img[data-vi-id]').forEach(img => {
    const id = img.dataset.viId;
    const versionType = img.dataset.viType || 'release';
    const forge = img.dataset.viForge === '1';
    const fabric = img.dataset.viFabric === '1';
    const neoforge = img.dataset.viNeoforge === '1';
    const modpack = img.dataset.viModpack === '1';
    const extDir = img.dataset.viExtdir || undefined;

    if (bridge.apiProxy) {
      // 通过 tauri-bridge 的 proxy 调用（兼容 v2）
      bridge.apiProxy('GET', '/api/version-icon', {
        versionId: id,
        type: versionType,
        forge: forge ? '1' : '0',
        fabric: fabric ? '1' : '0',
        neoforge: neoforge ? '1' : '0',
        modpack: modpack ? '1' : '0'
      }, null).then(res => {
        if (res.ok) {
          return res.json();
        }
        throw new Error('HTTP ' + res.status);
      }).then(body => {
        if (body && body.success && body.dataUrl) {
          img.src = body.dataUrl;
        }
      }).catch(() => {});
    } else {
      const core = _getCore();
      if (core && core.invoke) {
        core.invoke('get_version_icon', {
          id,
          versionType,
          forge,
          fabric,
          neoforge,
          modpack,
          extDir: extDir || undefined
        }).then(result => {
          if (result && result.success && result.data_url) {
            img.src = result.data_url;
          }
        }).catch(() => {});
      } else {
        // 非 Tauri 环境回退到 HTTP
        const params = new URLSearchParams({ id, type: versionType });
        if (forge) params.set('forge', 'true');
        if (fabric) params.set('fabric', 'true');
        if (neoforge) params.set('neoforge', 'true');
        if (modpack) params.set('modpack', 'true');
        if (extDir) params.set('extDir', extDir);
        img.src = '/api/version-icon?' + params.toString() + '&_t=' + Date.now();
      }
    }
  });
}

export { loadVersions, retryLoadVersions, updateVersionSelects, renderHomeCurrentVersionCard, selectLaunchVersion, quickDeleteErrorVersion, toggleErrorVersions, renderInstalledVersionsInto, fetchVersionBanners, getVersionBannerUrl, syncVersionCardSwitches, toggleVersionCardStyle, renderVersions, _loadVersionIcons };

document.addEventListener('versepc:bedrock-installed', () => { loadVersions(true); });