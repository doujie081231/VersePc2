import { API as API } from '../../core/api.js';
import { escapeHtml as escapeHtml, formatDate as formatDate, formatBytes as formatBytes } from '../../core/utils.js';
import { getCustomSelectValue as getCustomSelectValue } from '../misc/custom-select.js';
import { showToast as showToast } from '../misc/ui-components.js';
import { pickDependencyRootVersion } from './mod-version-utils.js';
import { dlManager as dlManager } from '../versions/custom-download.js';
import { modDetailState as modDetailState } from './detail-state.js';
import { formatModNameWithChinese as formatModNameWithChinese } from './chinese-names.js';
import { S } from './install-state.js';
import { S as modDetailS } from './mod-detail-state.js';
import { showModDownloadModal } from './install-batch.js';
import { matchInstalledVersion, resolveModSavePath } from './install-paths.js';

function showDepVersionSelectModal(projectId, source, gameVersion, loader, savePath) {
  const existing = document.getElementById('dep-version-select-modal');
  if (existing) existing.remove();

  const modal = document.createElement('div');
  modal.id = 'dep-version-select-modal';
  modal.className = 'modal-overlay';
  modal.style.cssText = 'position:fixed;inset:0;z-index:10001;display:flex;align-items:center;justify-content:center;background:rgba(0,0,0,0.5);';

  const card = document.createElement('div');
  card.className = 'ai-version-select-card';
  card.style.cssText = 'max-width:480px;width:90%;box-shadow:0 20px 60px rgba(0,0,0,0.3);';

  card.innerHTML = `
    <div class="ai-version-select-header" style="padding:14px 16px;background:var(--bg-tertiary);border-bottom:1px solid var(--border);">
      <span class="ai-version-select-title">选择前置模组版本</span>
      <span class="ai-version-select-count" id="dep-ver-count">加载中...</span>
    </div>
    <div class="ai-version-select-list" id="dep-ver-list" style="max-height:360px;overflow-y:auto;">
      <div style="padding:24px;text-align:center;color:var(--text-muted);font-size:13px;">正在获取版本列表...</div>
    </div>
    <div style="padding:12px 16px;border-top:1px solid var(--border);display:flex;justify-content:flex-end;">
      <button id="dep-ver-cancel" style="padding:8px 20px;border-radius:8px;border:1px solid var(--border);background:transparent;color:var(--text-primary);cursor:pointer;font-size:13px;">取消</button>
    </div>
  `;

  modal.appendChild(card);
  document.body.appendChild(modal);

  document.getElementById('dep-ver-cancel').onclick = () => modal.remove();
  modal.onclick = (e) => { if (e.target === modal) modal.remove(); };

  loadDepVersions(projectId, source, gameVersion, loader, savePath, modal);
}

async function loadDepVersions(projectId, source, gameVersion, loader, savePath, modal) {
  try {
    const result = await API.getProjectVersions(projectId, source, gameVersion, loader);
    const versions = result.versions || [];
    const listEl = document.getElementById('dep-ver-list');
    const countEl = document.getElementById('dep-ver-count');
    if (!listEl || !countEl) return;

    countEl.textContent = versions.length + ' 个版本';

    if (versions.length === 0) {
      listEl.innerHTML = '<div style="padding:24px;text-align:center;color:var(--text-muted);font-size:13px;">未找到兼容的版本</div>';
      return;
    }

    listEl.innerHTML = versions.map(v => {
      const loaders = (v.loaders || []).map(l => {
        const lc = l.toLowerCase();
        const cls = lc === 'fabric' ? 'fabric' : lc === 'forge' ? 'forge' : lc === 'neoforge' ? 'neoforge' : 'vanilla';
        return `<span class="ai-version-select-loader ${cls}">${escapeHtml(l)}</span>`;
      }).join(' ');
      const gvs = (v.gameVersions || []).slice(0, 3).join(', ') + (v.gameVersions?.length > 3 ? '...' : '');
      const file = v.files?.find(f => f.primary) || v.files?.[0];
      const sizeStr = file?.size ? formatBytes(file.size) : '';
      const dateStr = v.datePublished ? formatDate(v.datePublished) : '';

      return `<div class="ai-version-select-item" data-version-id="${escapeHtml(v.versionId)}" data-file-id="" data-file-name="${escapeHtml(file?.filename || '')}" data-download-url="${escapeHtml(file?.url || '')}">
        <div class="ai-version-select-icon-wrap">
          <span style="font-size:16px;">📦</span>
        </div>
        <div style="flex:1;min-width:0;">
          <span class="ai-version-select-id">${escapeHtml(v.versionNumber)}</span>
          <div style="display:flex;gap:6px;align-items:center;margin-top:2px;flex-wrap:wrap;">
            ${loaders}
            <span style="font-size:11px;color:var(--text-muted);">${escapeHtml(gvs)}</span>
            ${sizeStr ? `<span style="font-size:11px;color:var(--text-muted);">${sizeStr}</span>` : ''}
            ${dateStr ? `<span style="font-size:11px;color:var(--text-muted);">${dateStr}</span>` : ''}
          </div>
        </div>
      </div>`;
    }).join('');

    listEl.querySelectorAll('.ai-version-select-item').forEach(item => {
      item.onclick = () => {
        const selectedVersionId = item.dataset.versionId;
        modal.remove();
        downloadDepWithNestedDeps(projectId, source, selectedVersionId, savePath, gameVersion, loader);
      };
    });
  } catch (e) {
    const listEl = document.getElementById('dep-ver-list');
    if (listEl) {
      listEl.innerHTML = `<div style="padding:24px;text-align:center;color:var(--charts-red);font-size:13px;">加载失败: ${escapeHtml(e.message)}</div>`;
    }
  }
}

async function downloadDepWithNestedDeps(projectId, source, versionId, savePath, gameVersion, loader) {
  const taskId = 'dep-' + Date.now();
  dlManager.add(taskId, '前置模组', 'mod', '', '');
  dlManager.update(taskId, { progress: 0, status: 'downloading', message: '正在解析嵌套依赖...' });

  try {
    const recursiveDeps = await API.getDependenciesRecursive(versionId, source, gameVersion, loader, projectId);
    const allDeps = recursiveDeps.dependencies || [];
    const downloadableDeps = allDeps.filter(d => d.compatibleVersion);

    if (downloadableDeps.length > 0) {
      dlManager.update(taskId, { message: `发现 ${downloadableDeps.length} 个嵌套依赖，准备下载...` });

      const nestedDepsHtml = downloadableDeps.map(d => {
        const indent = '&nbsp;'.repeat(d.depth * 4);
        const icon = d.depth > 1 ? '↳' : '•';
        return `<div style="padding:3px 0;font-size:12px;color:var(--text-secondary);">${indent}${icon} ${escapeHtml(d.title)} <span style="color:var(--text-muted);">v${d.compatibleVersion.versionNumber}</span></div>`;
      }).join('');

      const modalId = 'nested-modal-' + Date.now();
      const confirmed = await new Promise(resolve => {
        const confirmModal = document.createElement('div');
        confirmModal.id = modalId;
        confirmModal.className = 'modal-overlay';
        confirmModal.style.cssText = 'position:fixed;inset:0;z-index:10002;display:flex;align-items:center;justify-content:center;background:rgba(0,0,0,0.5);';
        confirmModal.innerHTML = `<div style="background:var(--bg-primary);border-radius:12px;padding:24px;max-width:420px;width:90%;box-shadow:0 20px 60px rgba(0,0,0,0.3);">
          <h3 style="margin:0 0 8px;font-size:15px;font-weight:700;">发现嵌套依赖</h3>
          <p style="margin:0 0 12px;font-size:13px;color:var(--text-secondary);">该前置模组还有 ${downloadableDeps.length} 个嵌套依赖需要一起下载：</p>
          <div style="max-height:200px;overflow-y:auto;margin-bottom:16px;padding:8px;background:var(--bg-secondary);border-radius:8px;">${nestedDepsHtml}</div>
          <div style="display:flex;gap:10px;justify-content:flex-end;">
            <button id="${modalId}-cancel" style="padding:8px 20px;border-radius:8px;border:1px solid var(--border);background:transparent;color:var(--text-primary);cursor:pointer;font-size:13px;">取消</button>
            <button id="${modalId}-confirm" style="padding:8px 20px;border-radius:8px;border:none;background:var(--accent);color:var(--accent-text);cursor:pointer;font-size:13px;font-weight:600;">确认下载全部</button>
          </div>
        </div>`;
        document.body.appendChild(confirmModal);
        confirmModal.querySelector(`#${modalId}-cancel`).onclick = () => { confirmModal.remove(); resolve(false); };
        confirmModal.querySelector(`#${modalId}-confirm`).onclick = () => { confirmModal.remove(); resolve(true); };
        confirmModal.onclick = (e) => { if (e.target === confirmModal) { confirmModal.remove(); resolve(false); } };
      });

      if (!confirmed) {
        dlManager.update(taskId, { status: 'failed', message: '用户取消' });
        return;
      }
    }

    const mainResult = await API.downloadModVersion(versionId, projectId, source, '', gameVersion, loader, savePath, false);
    if (mainResult.success) {
      const mainDlId = 'dep-main-' + Date.now();
      dlManager.add(mainDlId, mainResult.fileName || '前置模组', 'mod', '', '');
      dlManager.update(mainDlId, { progress: 0, status: 'downloading', message: '下载中...' });
      dlManager.update(taskId, { progress: 10, message: '主模组下载中...' });
      showModDownloadModal(mainResult.fileName, mainResult.sessionId, savePath, modDetailState.data?.icon || '');
    } else {
      dlManager.update(taskId, { status: 'failed', message: mainResult.error || '主模组下载失败' });
      return;
    }

    const total = downloadableDeps.length;
    let downloaded = 0;
    for (const dep of downloadableDeps) {
      downloaded++;
      const depProgress = Math.round(10 + (downloaded / total) * 90);
      dlManager.update(taskId, { progress: depProgress, message: `下载依赖 ${downloaded}/${total}: ${dep.title}` });
      try {
        const depResult = await API.downloadModVersion(
          dep.compatibleVersion.versionId, dep.projectId, source, '',
          gameVersion, loader, savePath, false
        );
        if (depResult.success && depResult.sessionId) {
          const depDlId = 'dep-' + dep.projectId + '-' + Date.now();
          dlManager.add(depDlId, dep.title || depResult.fileName, 'mod', '', dep.icon || '');
          dlManager.update(depDlId, { progress: 0, status: 'downloading', message: '下载中...' });
          showModDownloadModal(depResult.fileName, depResult.sessionId, savePath, dep.icon || '');
        }
      } catch (e) {
        console.warn(`[Deps] 下载依赖 ${dep.title} 失败:`, e.message);
        const depFailId = 'dep-fail-' + Date.now();
        dlManager.add(depFailId, dep.title || '依赖', 'mod', '', dep.icon || '');
        dlManager.update(depFailId, { status: 'failed', message: e.message || '下载失败' });
      }
    }

    modDetailS.mdDepsCache.clear();
    dlManager.update(taskId, { status: 'completed', progress: 100, message: `全部下载完成 (${total} 个依赖)` });
    showToast(`前置模组下载完成: ${total} 个依赖`, 'success');
  } catch (e) {
    dlManager.update(taskId, { status: 'failed', message: e.message || '下载失败' });
    showToast('前置模组下载失败: ' + (e.message || '未知错误'), 'error');
  }
}

function showDependencyDialog(projectId, source, versionId, fileId, savePath, deps, gameVersion, loader) {
  const existing = document.getElementById('mod-dependency-modal');
  if (existing) existing.remove();

  const modal = document.createElement('div');
  modal.id = 'mod-dependency-modal';
  modal.className = 'modal-overlay';
  modal.style.cssText = 'position:fixed;inset:0;z-index:10000;display:flex;align-items:center;justify-content:center;background:rgba(0,0,0,0.5);';

  const depListHtml = deps.map(dep => {
    const ver = dep.compatibleVersion;
    const verInfo = ver ? `v${ver.versionNumber}` : '未找到兼容版本';
    const iconHtml = dep.icon
      ? `<img src="${dep.icon}" style="width:32px;height:32px;border-radius:6px;object-fit:cover;" data-img-fallback="hide" loading="lazy">`
      : `<div style="width:32px;height:32px;border-radius:6px;background:var(--bg-secondary);display:flex;align-items:center;justify-content:center;font-size:14px;">📦</div>`;
    const btnDisabled = !ver ? 'opacity:0.4;pointer-events:none;' : '';
    return `<div style="display:flex;align-items:center;gap:10px;padding:8px 0;border-bottom:1px solid var(--border);">
      ${iconHtml}
      <div style="flex:1;min-width:0;">
        <div style="font-weight:600;font-size:13px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;">${escapeHtml(dep.title)}</div>
        <div style="font-size:11px;color:var(--text-secondary);">${escapeHtml(verInfo)}</div>
      </div>
      <button class="dep-single-download-btn" data-project-id="${escapeHtml(dep.projectId)}" style="padding:4px 10px;border-radius:6px;border:1px solid var(--accent);background:transparent;color: var(--accent-ink);cursor:pointer;font-size:11px;white-space:nowrap;${btnDisabled}" title="${ver ? '选择版本并下载前置模组' : '无兼容版本'}">下载前置</button>
      ${ver ? '<span style="font-size:11px;color:var(--green);">✓</span>' : '<span style="font-size:11px;color:var(--red);">✗</span>'}
    </div>`;
  }).join('');

  const downloadableCount = deps.filter(d => d.compatibleVersion).length;

  modal.innerHTML = `<div style="background:var(--bg-primary);border-radius:12px;padding:24px;max-width:460px;width:90%;box-shadow:0 20px 60px rgba(0,0,0,0.3);">
    <h3 style="margin:0 0 8px;font-size:16px;font-weight:700;">检测到前置依赖</h3>
    <p style="margin:0 0 16px;font-size:13px;color:var(--text-secondary);">该模组需要以下前置模组才能正常运行：</p>
    <div style="max-height:280px;overflow-y:auto;margin-bottom:16px;">${depListHtml}</div>
    <div style="display:flex;gap:10px;justify-content:flex-end;">
      <button id="dep-cancel-btn" style="padding:8px 20px;border-radius:8px;border:1px solid var(--border);background:transparent;color:var(--text-primary);cursor:pointer;font-size:13px;">取消</button>
      <button id="dep-download-btn" style="padding:8px 20px;border-radius:8px;border:none;background:var(--accent);color:var(--accent-text);cursor:pointer;font-size:13px;font-weight:600;${downloadableCount === 0 ? 'opacity:0.5;pointer-events:none;' : ''}">一键下载全部（${downloadableCount} 个前置）</button>
    </div>
  </div>`;

  document.body.appendChild(modal);

  document.getElementById('dep-cancel-btn').onclick = () => modal.remove();
  document.getElementById('dep-download-btn').onclick = () => {
    modal.remove();
    proceedModInstall(projectId, source, versionId, fileId, savePath, false, deps, gameVersion, loader);
  };
  modal.onclick = (e) => { if (e.target === modal) modal.remove(); };

  modal.querySelectorAll('.dep-single-download-btn').forEach(btn => {
    btn.onclick = (e) => {
      e.stopPropagation();
      const depProjectId = btn.dataset.projectId;
      modal.remove();
      showDepVersionSelectModal(depProjectId, source, gameVersion, loader, savePath);
    };
  });
}

async function proceedModInstall(projectId, source, versionId, fileId, savePath, includeDeps, deps, gameVersion, loader) {
  const pendingTaskId = 'mod-' + Date.now();
  const iconUrl = modDetailState.data?.icon || '';
  dlManager.add(pendingTaskId, '准备下载...', 'mod', '', iconUrl);
  dlManager.update(pendingTaskId, { progress: 0, status: 'downloading', message: '正在获取下载信息...' });
  try {
    const currentGameVersion = gameVersion || getCustomSelectValue('mod-filter-version') || '';
    const currentLoader = loader || getCustomSelectValue('mod-filter-loader') || '';
    const result = await API.downloadModVersion(versionId || '', projectId, source, fileId || '', currentGameVersion, currentLoader, savePath, includeDeps);
    if (result.success) {
      modDetailS.mdDepsCache.clear();
      dlManager.remove(pendingTaskId);
      showModDownloadModal(result.fileName, result.sessionId, savePath, iconUrl);
      if (includeDeps) {
        maybeShowPostInstallDeps(projectId, source, versionId, savePath, currentGameVersion, currentLoader, iconUrl);
      }
    } else {
      dlManager.update(pendingTaskId, { status: 'failed', message: result.error || '下载失败' });
      showToast(result.error || '下载失败', 'error');
    }
  } catch (e) {
    console.error('Mod install error:', e);
    dlManager.update(pendingTaskId, { status: 'failed', message: e.message || '请求失败' });
    showToast('下载请求失败: ' + (e.message || '未知错误'), 'error');
  }
}

async function maybeShowPostInstallDeps(projectId, source, versionId, savePath, gameVersion, loader, iconUrl) {
  try {
    if (!versionId && typeof pickDependencyRootVersion === 'function') {
      versionId = pickDependencyRootVersion(gameVersion || '', loader || '');
    }
    if (!versionId) return;

    const targetGame = gameVersion || getCustomSelectValue('mod-filter-version') || S._mdSaveGame || '';
    const targetLoader = loader || getCustomSelectValue('mod-filter-loader') || S._mdSaveLoader || '';
    const versions = (typeof modDetailState.allVersions !== 'undefined' && Array.isArray(modDetailState.allVersions)) ? modDetailState.allVersions : [];
    const versionData = versions.find(v => v && v.id === versionId);
    const optionalRefs = ((versionData && versionData.dependencies) || [])
      .filter(d => d && d.projectId && (d.dependencyType === 'optional' || d.dependencyType === '2'));

    // 没有任何声明依赖时不必弹窗
    if (versionData && (versionData.dependencies || []).length === 0) return;

    // 先立刻弹出“检查中”弹窗，再异步查版本，避免安装完成后干等
    const existing = document.getElementById('post-install-deps-modal');
    if (existing) existing.remove();
    const modal = document.createElement('div');
    modal.id = 'post-install-deps-modal';
    modal.className = 'modal-overlay';
    modal.style.cssText = 'position:fixed;inset:0;z-index:10005;display:flex;align-items:center;justify-content:center;background:rgba(0,0,0,0.5);';
    modal.innerHTML = `<div style="background:var(--bg-primary);border-radius:14px;padding:22px;width:92%;max-width:520px;box-shadow:0 20px 60px rgba(0,0,0,0.35);">
      <h3 style="margin:0 0 6px;font-size:16px;font-weight:700;">前置模组</h3>
      <p style="margin:0 0 14px;font-size:13px;color:var(--text-secondary);">模组安装已完成，正在检查前置...</p>
      <div id="pi-loading" style="display:flex;align-items:center;gap:8px;padding:10px 0 16px;font-size:13px;color:var(--text-muted);">
        <span class="spinner" style="width:16px;height:16px;border-width:2px;"></span>
        正在检查前置...
      </div>
      <div style="display:flex;gap:10px;justify-content:flex-end;">
        <button id="pi-skip" class="btn btn-secondary">跳过</button>
        <button id="pi-download" class="btn btn-primary" disabled>一键下载</button>
      </div>
    </div>`;
    document.body.appendChild(modal);
    modal.querySelector('#pi-skip').onclick = () => modal.remove();
    modal.onclick = (e) => { if (e.target === modal) modal.remove(); };

    const targetVid = matchInstalledVersion(targetGame, targetLoader);
    const installedPromise = targetVid
      ? API.getVersionMods(targetVid).catch(() => [])
      : API.getInstalledMods().catch(() => []);
    const optionalPromise = optionalRefs.length
      ? API.resolveDepVersions(optionalRefs.map(d => d.projectId), targetGame, targetLoader, source).catch(() => ({}))
      : Promise.resolve({});

    const [requiredRes, optionalRes, installedRaw] = await Promise.all([
      API.getDependenciesRecursive(versionId, source, targetGame, targetLoader, projectId)
        .catch(e => { console.warn('[PostInstallDeps] 获取必选前置失败:', e.message); return { dependencies: [] }; }),
      optionalPromise,
      installedPromise
    ]);

    const installedMods = Array.isArray(installedRaw)
      ? installedRaw
      : ((installedRaw && installedRaw.mods) || []);
    const installedFiles = new Set(
      installedMods.map(m => (m.fileName || m.filename || '').toLowerCase()).filter(Boolean)
    );
    const installedIds = new Set(installedMods.map(m => m.id).filter(Boolean));
    const isInstalled = dep => {
      if (installedIds.has(dep.projectId)) return true;
      const fn = (dep.compatibleVersion && dep.compatibleVersion.fileName || '').toLowerCase();
      return !!fn && installedFiles.has(fn);
    };

    const required = (requiredRes.dependencies || [])
      .filter(d => d && d.compatibleVersion && !isInstalled(d));
    const requiredIds = new Set(required.map(d => d.projectId));

    const optionalMap = new Map();
    optionalRefs.forEach(ref => {
      if (requiredIds.has(ref.projectId) || optionalMap.has(ref.projectId)) return;
      const info = optionalRes[ref.projectId] || {};
      const hasVersion = !!(info.hasCompatibleVersion && info.versionId);
      optionalMap.set(ref.projectId, {
        projectId: ref.projectId,
        title: info.title || ref.projectId,
        icon: info.icon || '',
        dependencyType: 'optional',
        depth: 1,
        compatibleVersion: hasVersion ? {
          versionId: info.versionId,
          versionNumber: info.versionNumber || '',
          fileName: info.fileName || '',
          gameVersions: info.gameVersions || [],
          loaders: info.loaders || []
        } : null
      });
    });
    const optional = [...optionalMap.values()].filter(d => !isInstalled(d));

    if (required.length === 0 && optional.length === 0) {
      const loading = modal.querySelector('#pi-loading');
      if (loading) loading.innerHTML = '未检测到需要下载的前置';
      const skip = modal.querySelector('#pi-skip');
      if (skip) skip.textContent = '知道了';
      setTimeout(() => { if (modal.parentNode) modal.remove(); }, 1400);
      return;
    }

    const rowHtml = (dep, index) => {
      const ver = dep.compatibleVersion;
      const verText = ver
        ? (ver.versionNumber || ver.fileName || '')
        : '无兼容版本';
      const iconHtml = dep.icon
        ? `<img src="${escapeHtml(dep.icon)}" alt="" style="width:32px;height:32px;border-radius:7px;object-fit:cover;" data-img-fallback="hide" loading="lazy">`
        : `<div style="width:32px;height:32px;border-radius:7px;background:var(--bg-secondary);display:flex;align-items:center;justify-content:center;font-size:14px;">📦</div>`;
      const indent = Math.min(Math.max((dep.depth || 1) - 1, 0), 3) * 14;
      const title = typeof formatModNameWithChinese === 'function'
        ? formatModNameWithChinese(dep.slug || dep.projectId, dep.title || dep.projectId)
        : (dep.title || dep.projectId);
      const checkHtml = index === -1
        ? `<span style="font-size:11px;padding:2px 7px;border-radius:4px;background:color-mix(in srgb, var(--green) 12%, transparent);color:var(--green);white-space:nowrap;">必选</span>`
        : `<input type="checkbox" class="pi-optional" data-index="${index}" ${ver ? '' : 'disabled'} style="width:15px;height:15px;accent-color: var(--accent);cursor:${ver ? 'pointer' : 'not-allowed'};">`;
      return `<div style="display:flex;align-items:center;gap:10px;padding:8px 0;border-bottom:1px solid var(--border);margin-left:${indent}px;">
        ${iconHtml}
        <div style="flex:1;min-width:0;">
          <div style="font-weight:600;font-size:13px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;">${escapeHtml(title)}</div>
          <div style="font-size:11px;color:${ver ? 'var(--text-secondary)' : 'var(--warning,orange)'};white-space:nowrap;overflow:hidden;text-overflow:ellipsis;">${escapeHtml(verText)}</div>
        </div>
        ${checkHtml}
      </div>`;
    };

    const rows = [
      ...required.map(d => rowHtml(d, -1)),
      ...optional.map((d, i) => rowHtml(d, i))
    ].join('');

    modal.innerHTML = `<div style="background:var(--bg-primary);border-radius:14px;padding:22px;width:92%;max-width:520px;box-shadow:0 20px 60px rgba(0,0,0,0.35);">
      <h3 style="margin:0 0 6px;font-size:16px;font-weight:700;">前置模组</h3>
      <p style="margin:0 0 12px;font-size:13px;color:var(--text-secondary);">模组安装已完成。检测到以下前置，可一键下载；可选前置按需勾选。</p>
      <div id="pi-dep-list" style="max-height:320px;overflow-y:auto;margin-bottom:12px;">${rows}</div>
      <div style="display:flex;align-items:center;justify-content:space-between;gap:10px;flex-wrap:wrap;">
        <label style="display:flex;align-items:center;gap:6px;font-size:12px;color:var(--text-secondary);cursor:pointer;">
          <input type="checkbox" id="pi-select-all" style="width:14px;height:14px;accent-color: var(--accent);"> 全选可选前置
        </label>
        <div style="display:flex;gap:10px;justify-content:flex-end;">
          <button id="pi-skip" class="btn btn-secondary">跳过</button>
          <button id="pi-download" class="btn btn-primary">一键下载</button>
        </div>
      </div>
    </div>`;

    const downloadBtn = modal.querySelector('#pi-download');
    const selectAll = modal.querySelector('#pi-select-all');
    const optionalBoxes = () => [...modal.querySelectorAll('.pi-optional:not(:disabled)')];
    const updateCount = () => {
      const checked = [...modal.querySelectorAll('.pi-optional:checked')].length;
      const total = required.length + checked;
      downloadBtn.textContent = total > 0 ? `一键下载（${total}）` : '一键下载';
      downloadBtn.disabled = total === 0;
      downloadBtn.style.opacity = total === 0 ? '0.5' : '1';
      if (selectAll) {
        const boxes = optionalBoxes();
        selectAll.checked = boxes.length > 0 && boxes.every(b => b.checked);
      }
    };
    optionalBoxes().forEach(b => b.addEventListener('change', updateCount));
    if (selectAll) {
      selectAll.addEventListener('change', () => {
        optionalBoxes().forEach(b => { b.checked = selectAll.checked; });
        updateCount();
      });
    }
    updateCount();

    modal.querySelector('#pi-skip').onclick = () => modal.remove();
    modal.onclick = (e) => { if (e.target === modal) modal.remove(); };

    downloadBtn.onclick = async () => {
      downloadBtn.disabled = true;
      downloadBtn.textContent = '准备下载...';
      const checkedIdx = [...modal.querySelectorAll('.pi-optional:checked')]
        .map(b => parseInt(b.dataset.index, 10))
        .filter(i => Number.isInteger(i));
      const selectedOptional = checkedIdx.map(i => optional[i]).filter(d => d && d.compatibleVersion);
      const list = [...required, ...selectedOptional];
      const seen = new Set(list.map(d => d.projectId));

      // 可选前置自身也可能有必选前置，一并补进下载列表
      for (const opt of selectedOptional) {
        try {
          const nested = await API.getDependenciesRecursive(
            opt.compatibleVersion.versionId, source, targetGame, targetLoader, opt.projectId
          );
          for (const dep of nested.dependencies || []) {
            if (dep.compatibleVersion && !seen.has(dep.projectId)) {
              seen.add(dep.projectId);
              list.push(dep);
            }
          }
        } catch (e) { console.warn('[PostInstallDeps] 解析可选前置的嵌套依赖失败:', e.message); }
      }

      modal.remove();
      let submitted = 0;
      let failed = 0;
      const targetSavePath = savePath || await resolveModSavePath();
      for (const dep of list) {
        try {
          const r = await API.downloadModVersion(
            dep.compatibleVersion.versionId, dep.projectId, source, '',
            targetGame, targetLoader, targetSavePath, false
          );
          if (r.success && r.sessionId) {
            submitted++;
            showModDownloadModal(r.fileName, r.sessionId, targetSavePath, dep.icon || iconUrl);
          } else {
            failed++;
            showToast(`${dep.title || dep.projectId}: ${r.error || '下载失败'}`, 'error');
          }
        } catch (e) {
          failed++;
          showToast(`${dep.title || dep.projectId}: ${e.message || '下载失败'}`, 'error');
        }
      }
      if (failed > 0) {
        showToast(`已提交 ${submitted}/${list.length} 个前置下载，${failed} 个失败`, 'error');
      } else {
        showToast(`已提交 ${submitted} 个前置下载`, 'success');
      }
    };
  } catch (e) {
    console.warn('[PostInstallDeps] 前置弹窗失败:', e);
    const failedModal = document.getElementById('post-install-deps-modal');
    if (failedModal) failedModal.remove();
  }
}

export { showDepVersionSelectModal, loadDepVersions, downloadDepWithNestedDeps, showDependencyDialog, proceedModInstall, maybeShowPostInstallDeps };
