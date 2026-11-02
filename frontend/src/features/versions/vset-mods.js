import { API as API } from '../../core/api.js';
import { escapeHtml as escapeHtml, normalizeIconUrl as normalizeIconUrl } from '../../core/utils.js';
import { customSelectInstances as customSelectInstances } from '../misc/custom-select.js';
import { showToast as showToast, showConfirmDialog as showConfirmDialog } from '../misc/ui-components.js';
import { loadMods as loadMods } from '../mods/mod-browse.js';
import { openModDetail as openModDetail } from '../mods/mod-detail.js';
import { navigateToPage as navigateToPage } from '../../core/navigation.js';
import { appState as appState } from '../../core/app-state.js';
import { registerLazyLoader } from '../../core/lazy-loaders.js';
import { formatModNameWithChinese as formatModNameWithChinese } from '../mods/chinese-names.js';
import { actionAttrs as actionAttrs, actionAttrsStop as actionAttrsStop } from '../../core/actions.js';
import { S } from './vset-state.js';

async function checkModUpdatesForVersion() {
  if (!appState.currentSettingsVersionId) {
    showToast('请先选择一个版本', 'error');
    return;
  }
  if (S._checkingModUpdates) {
    showToast('正在检查更新，请稍候...', 'info');
    return;
  }
  S._checkingModUpdates = true;
  showToast('正在检查模组更新...', 'info');
  try {
    const result = await API.checkModUpdates(appState.currentSettingsVersionId);
    if (result.error) {
      showToast('检查更新失败: ' + result.error, 'error');
      return;
    }
    const updates = result.updates || [];
    if (updates.length === 0) {
      showToast(`已检查 ${result.checked || 0} 个模组，暂无更新`, 'success');
      return;
    }
    showModUpdateDialog(updates, result.checked || 0);
  } catch (e) {
    showToast('检查更新失败: ' + (e.message || '未知错误'), 'error');
  } finally {
    S._checkingModUpdates = false;
  }
}

function showModUpdateDialog(updates, checkedCount) {
  const overlay = document.createElement('div');
  overlay.className = 'modal-overlay';
  overlay.style.cssText = 'position:fixed;top:0;left:0;right:0;bottom:0;background:rgba(0,0,0,0.5);z-index:10000;display:flex;align-items:center;justify-content:center;';

  const dialog = document.createElement('div');
  dialog.style.cssText = 'background:var(--bg-primary);border-radius:12px;padding:24px;max-width:560px;width:90%;max-height:70vh;overflow-y:auto;box-shadow:0 8px 32px rgba(0,0,0,0.3);';

  const listHtml = updates.map(u => `
    <div style="display:flex;align-items:center;justify-content:space-between;padding:10px 0;border-bottom:1px solid var(--border);">
      <div style="flex:1;min-width:0;">
        <div style="font-weight:600;color:var(--text-primary);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;">${escapeHtml(u.modName)}</div>
        <div style="font-size:12px;color:var(--text-muted);margin-top:2px;">${escapeHtml(u.fileName)} | 当前版本: ${escapeHtml(u.currentVersion)}</div>
      </div>
      <a href="${u.projectUrl}" target="_blank" style="color: var(--accent-ink);font-size:13px;text-decoration:none;white-space:nowrap;margin-left:12px;">查看更新</a>
    </div>
  `).join('');

  dialog.innerHTML = `
    <h3 style="margin:0 0 4px 0;color:var(--text-primary);">模组更新检查</h3>
    <p style="margin:0 0 16px 0;font-size:13px;color:var(--text-muted);">已检查 ${checkedCount} 个模组，发现 ${updates.length} 个可在 Modrinth 上找到</p>
    <div>${listHtml}</div>
    <div style="margin-top:16px;text-align:right;">
      <button class="btn btn-secondary"${actionAttrs('closeOverlay')}>关闭</button>
    </div>
  `;

  overlay.appendChild(dialog);
  overlay.addEventListener('click', (e) => { if (e.target === overlay) overlay.remove(); });
  document.body.appendChild(overlay);
}

async function loadInstalledModsForSettings() {
  if (!appState.currentSettingsVersionId) return;
  const container = document.getElementById('modmgr-mod-list');
  if (container) {
    container.innerHTML = '<p class="empty-text" style="padding:30px 0;text-align:center;color:var(--text-muted)">加载中...</p>';
  }
  try {
    const mods = await API.getVersionMods(appState.currentSettingsVersionId);
    S._modMgrSettingsLoaded = true;
    renderModMgrList(mods || []);
  } catch (e) {
    console.error('[ModMgr] Load error:', e);
  }
}

function renderModMgrList(mods) {
  const container = document.getElementById('modmgr-mod-list');
  const countAll = document.getElementById('modmgr-count-all');
  const countUpdate = document.getElementById('modmgr-count-update');

  if (!container) return;

  if (!mods || mods.length === 0) {
    container.innerHTML = '<p class="empty-text" style="padding:30px 0;text-align:center;color:var(--text-muted)">暂无已安装的模组</p>';
    if (countAll) countAll.textContent = '0';
    if (countUpdate) countUpdate.textContent = '0';
    return;
  }

  const BATCH_SIZE = 30;
  const total = mods.length;
  container.innerHTML = '';

  function renderBatch(start) {
    const fragment = document.createDocumentFragment();
    const end = Math.min(start + BATCH_SIZE, total);
    for (let i = start; i < end; i++) {
      const m = mods[i];
      const iconUrl = m.icon || '';
      const desc = (m.description || '').substring(0, 60);
      const verStr = m.version || '';
      const author = m.author || '';
      const projectId = m.projectId || m.slug || '';
      const isDisabled = m.disabled || false;
      const fileName = m.fileName || m.name || '';
      const toggleLabel = isDisabled ? '启用' : '禁用';
      const toggleClass = isDisabled ? 'btn-primary' : 'btn-secondary';
      const nameStyle = isDisabled ? 'opacity:0.5;text-decoration:line-through;' : '';
      const isApiIcon = iconUrl.startsWith('/api/');
      const iconHash = isApiIcon
        ? (() => { const mm = iconUrl.match(/hash=([^&]+)/); return mm ? decodeURIComponent(mm[1]) : ''; })()
        : '';
      let iconHtml;
      if (iconUrl && !isApiIcon) {
        iconHtml = `<div class="modmgr-icon"><img src="${normalizeIconUrl(iconUrl)}" alt="" loading="lazy" data-img-fallback="modmgr"></div>`;
      } else {
        iconHtml = `<div class="modmgr-icon modmgr-icon--fallback"${iconHash ? ` data-icon-hash="${escapeHtml(iconHash)}"` : ''}></div>`;
      }
      const wrapper = document.createElement('div');
      wrapper.className = `modmgr-item${isDisabled ? ' mod-disabled' : ''}`;
      wrapper.dataset.name = m.name || '';
      wrapper.dataset.desc = desc;
      wrapper.innerHTML = `${iconHtml}
      <div class="modmgr-info">
        <div class="modmgr-name" style="${nameStyle}">${escapeHtml(formatModNameWithChinese(m.slug || m.id || m.fileName, m.name))}${isDisabled ? ' (已禁用)' : ''}</div>
        <div class="modmgr-meta">${author ? escapeHtml(author) : ''}${verStr ? ' | ' + escapeHtml(verStr) : ''}</div>
        <div class="modmgr-desc">${escapeHtml(desc)}</div>
      </div>
      <div class="modmgr-actions-row">
        <button class="btn ${toggleClass} btn-sm"${actionAttrsStop('toggleModInManager', fileName, !isDisabled)}>${toggleLabel}</button>
      ${projectId ? `<button class="btn btn-secondary btn-sm"${actionAttrsStop('previewMod', projectId)}>预览</button>` : ''}
      <button class="btn btn-secondary btn-sm"${actionAttrsStop('openModTranslateDialog', fileName, m.name || fileName)}>汉化</button>
      <button class="btn btn-danger btn-sm"${actionAttrsStop('removeModFromManager', fileName)}>移除</button>
      </div>`;
      fragment.appendChild(wrapper);
    }
    container.appendChild(fragment);
    container.querySelectorAll('.modmgr-icon[data-icon-hash]').forEach(el => {
      const hash = el.getAttribute('data-icon-hash');
      API.getModIcon(hash).then(r => {
        console.warn('[modmgr-icon] ok', hash, (r && r.dataUrl) ? ('len=' + r.dataUrl.length) : JSON.stringify(r));
        if (r && r.dataUrl) {
          el.classList.remove('modmgr-icon--fallback');
          el.removeAttribute('data-icon-hash');
          el.innerHTML = `<img src="${r.dataUrl}" alt="" style="width:100%;height:100%;object-fit:cover;">`;
        }
      }).catch(e => console.warn('[modmgr-icon] err', hash, String(e && e.message || e)));
    });
    if (end < total) {
      requestAnimationFrame(() => renderBatch(end));
    }
  }

  renderBatch(0);
  if (countAll) countAll.textContent = mods.length;
  if (countUpdate) countUpdate.textContent = '0';
}

function previewMod(projectId) {
  if (!projectId) return;
  openModDetail(projectId, 'modrinth');
}

function filterInstalledMods() {
  const keyword = (document.getElementById('modmgr-search')?.value || '').toLowerCase();
  document.querySelectorAll('.modmgr-item').forEach(item => {
    const name = (item.dataset.name || '').toLowerCase();
    const desc = (item.dataset.desc || '').toLowerCase();
    item.style.display = (name.includes(keyword) || desc.includes(keyword)) ? 'flex' : 'none';
  });
}

function filterModMgrTab(filter) {
  document.querySelectorAll('.modmgr-tab').forEach(t => t.classList.remove('active'));
  document.querySelector(`.modmgr-tab[data-filter="${filter}"]`)?.classList.add('active');
}

function selectAllMods() {
  showToast('已选择所有模组', 'info');
}

function installModFromFile() {
  showToast('请选择要安装的 Mod 文件（.jar）', 'info');
  API.selectModFile().then(result => {
    if (result && result.filePath) {
      installModByFile(result.filePath);
    }
  });
}

function installModByFile(filePath) {
  if (!appState.currentSettingsVersionId) {
    showToast('请先选择一个版本', 'error');
    return;
  }
  API.installModFromFile(appState.currentSettingsVersionId, filePath).then(r => {
    if (r.success) {
      showToast('Mod 安装成功', 'success');
      loadInstalledModsForSettings();
    } else {
      showToast(r.error || '安装失败', 'error');
    }
  }).catch(e => showToast('安装失败: ' + e.message, 'error'));
}

function openBrowseMods() {
  appState._modDownloadVersionId = '';
  navigateToPage('mods');
}

function goDownloadMods() {
  if (!appState.currentSettingsVersionId) {
    appState._modDownloadVersionId = '';
    navigateToPage('mods');
    return;
  }
  appState._modDownloadVersionId = appState.currentSettingsVersionId;
  
  const versionInfo = appState.installedVersions.find(v => v.id === appState.currentSettingsVersionId);
  
  let gameVersion = '';
  if (versionInfo && versionInfo.baseVersion) {
    gameVersion = versionInfo.baseVersion;
  } else if (versionInfo && versionInfo.inheritsFrom) {
    gameVersion = versionInfo.inheritsFrom;
  } else {
    gameVersion = appState.currentSettingsVersionId.split('-')[0];
  }
  
  let loaderType = '';
  if (versionInfo) {
    if (versionInfo.isFabric) loaderType = 'fabric';
    else if (versionInfo.isForge) loaderType = 'forge';
    else if (versionInfo.isNeoForge) loaderType = 'neoforge';
  }

  navigateToPage('mods');
  
  setTimeout(() => {
    if (gameVersion && customSelectInstances['mod-filter-version']) {
      customSelectInstances['mod-filter-version'].setValue(gameVersion);
    }
    
    if (loaderType && customSelectInstances['mod-filter-loader']) {
      customSelectInstances['mod-filter-loader'].setValue(loaderType);
    }
    
    appState.modSearchOffset = 0;
    loadMods();
  }, 100);
}

function toggleModInManager(fileName, disable) {
  if (!appState.currentSettingsVersionId) return;
  API.toggleMod(fileName, !disable, appState.currentSettingsVersionId).then(r => {
    if (r.success) {
      showToast(disable ? '已禁用' : '已启用', 'success');
      loadInstalledModsForSettings();
    } else {
      showToast(r.error || '操作失败', 'error');
    }
  }).catch(e => showToast(e.message || '操作失败', 'error'));
}

async function removeModFromManager(fileName) {
  if (!appState.currentSettingsVersionId) return;
  const confirmed = await showConfirmDialog('删除模组', `确定要删除 ${fileName} 吗？`, '删除', '取消');
  if (!confirmed) return;
  API.removeMod(appState.currentSettingsVersionId, fileName).then(r => {
    if (r.success) {
      showToast('已删除', 'success');
      loadInstalledModsForSettings();
    } else {
      showToast(r.error || '删除失败', 'error');
    }
  });
}

export { checkModUpdatesForVersion, showModUpdateDialog, loadInstalledModsForSettings, renderModMgrList, previewMod, filterInstalledMods, filterModMgrTab, selectAllMods, installModFromFile, installModByFile, openBrowseMods, goDownloadMods, toggleModInManager, removeModFromManager };

// 批量安装完成后刷新版本设置中的已安装模组列表
registerLazyLoader('vset:installed-mods', loadInstalledModsForSettings);
