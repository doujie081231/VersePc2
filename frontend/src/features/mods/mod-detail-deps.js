import { API as API } from '../../core/api.js';
import { escapeHtml as escapeHtml } from '../../core/utils.js';
import { getCustomSelectValue as getCustomSelectValue } from '../misc/custom-select.js';
import { showToast as showToast, escapeOnclick as escapeOnclick } from '../misc/ui-components.js';
import { matchInstalledVersion, resolveModSavePath } from './install-paths.js';
import { showModDownloadModal as showModDownloadModal } from './install-batch.js';
import { modDetailState as modDetailState } from './detail-state.js';
import { appState as appState } from '../../core/app-state.js';
import { formatModNameWithChinese as formatModNameWithChinese } from './chinese-names.js';
import { actionAttrs as actionAttrs } from '../../core/actions.js';
import { S } from './mod-detail-state.js';
import { getInstalledVersionInfo } from './mod-detail-installed.js';
import { _sortVersionsByNumber, pickDependencyRootVersion } from './mod-version-utils.js';

function cleanupMdDepsCache() {
  const now = Date.now();
  for (const [key, entry] of S.mdDepsCache) {
    if (now - entry.time > S.MD_DEPS_CACHE_TTL) S.mdDepsCache.delete(key);
  }
  if (S.mdDepsCache.size > S.MD_DEPS_CACHE_MAX) {
    const entries = [...S.mdDepsCache.entries()].sort((a, b) => a[1].time - b[1].time);
    for (let i = 0; i < entries.length - S.MD_DEPS_CACHE_MAX; i++) S.mdDepsCache.delete(entries[i][0]);
  }
}

async function loadModDependencies() {
  const depsSection = document.getElementById('md-deps-section');
  const depsList = document.getElementById('md-deps-list');
  const depsCount = document.getElementById('md-deps-count');

  if (!depsSection || !depsList) return;

  // 依赖兼容性优先跟随下载页当前筛选（版本/加载器），
  // 未筛选时才回退到已选中的游戏实例，避免“筛选了 1.21.11 Fabric 却按 Forge 实例检查”
  const verInfo = await getInstalledVersionInfo();
  const filterGameVersion = getCustomSelectValue('mod-filter-version') || '';
  const filterLoader = getCustomSelectValue('mod-filter-loader') || '';
  const currentGameVersion = filterGameVersion || verInfo.gameVersion;
  const currentLoader = filterLoader || verInfo.loaderType;
  const hasVersionFilter = !!(currentGameVersion || currentLoader);

  // 只汇总当前目标游戏版本/加载器下的依赖。
  // 不能把 modDetailState.allVersions 里所有历史版本的依赖取并集，否则旧版本的前置会污染当前版本，
  // 出现“26.3 明明有对应版本，却显示某个旧前置无对应版本”的误报。
  const targetVersions = hasVersionFilter
    ? modDetailState.allVersions.filter(v => {
        const gvOk = !currentGameVersion || (v.gameVersions || []).includes(currentGameVersion);
        const lOk = !currentLoader || (v.loaders || []).some(l => l.toLowerCase() === currentLoader.toLowerCase());
        return gvOk && lOk;
      })
    : modDetailState.allVersions;

  const allDeps = new Map();
  const mdSource = appState.currentModDetailSource || 'modrinth';
  targetVersions.forEach(v => {
    (v.dependencies || []).forEach(d => {
      if (!d.projectId) return;
      let type = d.dependencyType || '';
      if (mdSource === 'curseforge') {
        // 后端已将 CurseForge relationType 统一为 required/optional/incompatible
        type = type === 'required' ? 'required' : (type === 'incompatible' ? 'incompatible' : 'optional');
      }
      const existing = allDeps.get(d.projectId);
      if (existing) {
        // 同一目标下多个文件对同一前置的要求不一致时，required 优先
        if (type === 'required' && existing.dependencyType !== 'required') {
          existing.dependencyType = 'required';
        }
        return;
      }
      allDeps.set(d.projectId, Object.assign({}, d, { dependencyType: type }));
    });
  });

  const depArray = Array.from(allDeps.values());
  modDetailState.deps = depArray;

  if (depArray.length === 0) {
    depsSection.style.display = 'none';
    return;
  }

  const requiredDeps = depArray.filter(d => d.dependencyType === 'required');
  depsSection.style.display = 'block';

  if (!hasVersionFilter) {
    if (depsCount) depsCount.textContent = `(${requiredDeps.length} 必选, ${depArray.length - requiredDeps.length} 可选) — 请先选择游戏版本`;
  }

  const depIds = depArray.map(d => d.projectId).filter(Boolean);
  if (!depIds.length) {
    depsList.innerHTML = '';
    return;
  }

  const cacheKey = depIds.sort().join(',') + '|' + (currentGameVersion || '') + '|' + (currentLoader || '');
  const cached = S.mdDepsCache.get(cacheKey);
  if (cached && (Date.now() - cached.time < S.MD_DEPS_CACHE_TTL)) {
    modDetailState.depsResolved = cached.resolved;
    modDetailState.depsVersionInfo = cached.versionInfo;
    renderDepsList(depArray, cached.resolved, cached.versionInfo, hasVersionFilter, currentGameVersion, currentLoader, cached.installedMods, depsList, depsCount, requiredDeps);
    return;
  }

  depsList.innerHTML = depArray.map(d => {
    const depType = d.dependencyType || 'optional';
    const typeLabel = depType === 'required' ? '必选' : (depType === 'incompatible' ? '冲突' : '可选');
    const badgeClass = depType === 'required' ? 'required' : (depType === 'incompatible' ? 'incompatible' : 'optional');
    return `<div class="md-dep-item" id="md-dep-${escapeOnclick(d.projectId)}"${actionAttrs('openModDetail', d.projectId, mdSource)}>
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
    // 按筛选后的目标实例读取已安装模组，避免“筛选 Fabric 却查了 Forge 实例”
    const targetVersionId = matchInstalledVersion(currentGameVersion, currentLoader);
    const installedModsPromise = targetVersionId
      ? API.getVersionMods(targetVersionId).catch(() => [])
      : API.getInstalledMods().catch(() => []);

    if (hasVersionFilter) {
      // Modrinth / CurseForge 都走同一个“解析 + 匹配版本”接口，
      // 避免 CurseForge 只查元信息、导致有 1.21.11 Fabric 版本却显示“未有对应版本”
      const [resolveResult, installedModsData] = await Promise.all([
        API.resolveDepVersions(depIds, currentGameVersion, currentLoader, mdSource),
        installedModsPromise
      ]);
      installedMods = Array.isArray(installedModsData)
        ? installedModsData
        : ((installedModsData && installedModsData.mods) || []);

      versionInfo = resolveResult || {};
      modDetailState.depsVersionInfo = versionInfo;

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
      modDetailState.depsResolved = resolved;

      const failedIds = depIds.filter(pid => {
        const r = resolved[pid];
        return !r || !r.title || r.title === pid;
      });
      if (failedIds.length > 0) {
        try {
          const retryRes = await API.resolveModDeps(failedIds.join(','), mdSource);
          for (const pid of failedIds) {
            const ri = retryRes[pid];
            if (ri && ri.title && ri.title !== pid) {
              resolved[pid] = { ...resolved[pid], ...ri };
            }
          }
          modDetailState.depsResolved = resolved;
        } catch (e) { console.warn('[ModInstall] 依赖检查失败:', e.message); }
      }

      const compatibleCount = depArray.filter(d => versionInfo[d.projectId]?.hasCompatibleVersion).length;
      const incompatibleCount = depArray.filter(d => !versionInfo[d.projectId]?.hasCompatibleVersion).length;
      if (depsCount) {
        let countText = `(${requiredDeps.length} 必选, ${depArray.length - requiredDeps.length} 可选)`;
        countText += ` — ${compatibleCount} 个有对应版本`;
        if (incompatibleCount > 0) {
          countText += `，${incompatibleCount} 个未有对应版本`;
        }
        depsCount.textContent = countText;
      }
    } else {
      const [basic, installedModsData] = await Promise.all([
        API.resolveModDeps(depIds.join(','), mdSource),
        installedModsPromise
      ]);
      installedMods = Array.isArray(installedModsData)
        ? installedModsData
        : ((installedModsData && installedModsData.mods) || []);
      resolved = basic || {};
      modDetailState.depsResolved = resolved;
      modDetailState.depsVersionInfo = {};
      if (depsCount) depsCount.textContent = `(${requiredDeps.length} 必选, ${depArray.length - requiredDeps.length} 可选)`;
    }

    S.mdDepsCache.set(cacheKey, { resolved, versionInfo, installedMods, time: Date.now() });

    renderDepsList(depArray, resolved, versionInfo, hasVersionFilter, currentGameVersion, currentLoader, installedMods, depsList, depsCount, requiredDeps);
  } catch (e) {
    depsList.innerHTML = depArray.map(d => {
      const depType = d.dependencyType || 'optional';
      const typeLabel = depType === 'required' ? '必选' : (depType === 'incompatible' ? '冲突' : '可选');
      const badgeClass = depType === 'required' ? 'required' : (depType === 'incompatible' ? 'incompatible' : 'optional');
      return `<div class="md-dep-item"${actionAttrs('openModDetail', d.projectId, mdSource)}>
        <div class="md-dep-info">
          <div class="md-dep-name">${escapeHtml(d.projectId)}</div>
        </div>
        <span class="md-dep-badge ${badgeClass}">${typeLabel}</span>
      </div>`;
    }).join('');
  }
}

function renderDepsList(depArray, resolved, versionInfo, hasVersionFilter, currentGameVersion, currentLoader, installedMods, depsList, _depsCount, _requiredDeps) {
  depsList.innerHTML = depArray.map(d => {
    const info = resolved[d.projectId] || {};
    const title = info.title || d.projectId;
    const icon = info.icon || '';
    const desc = info.description || '';
    const depType = d.dependencyType || 'optional';
    const typeLabel = depType === 'required' ? '必选' : (depType === 'incompatible' ? '冲突' : '可选');
    const badgeClass = depType === 'required' ? 'required' : (depType === 'incompatible' ? 'incompatible' : 'optional');
    const vInfo = versionInfo[d.projectId];

    const isInstalled = installedMods.some(m => {
      if (m.id === d.projectId) return true;
      const fn = (m.fileName || m.filename || '').toLowerCase();
      if (!fn) return false;
      const pid = d.projectId.toLowerCase();
      if (fn.includes(pid)) return true;
      // 后端解析出的兼容文件与本地文件完全一致，视为已安装
      const depFileName = (vInfo?.fileName || '').toLowerCase();
      if (depFileName && fn === depFileName) return true;
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

    return `<div class="md-dep-item"${actionAttrs('openModDetail', d.projectId, appState.currentModDetailSource || 'modrinth')}>
      ${icon ? `<div class="md-dep-icon"><img src="${icon}" alt="" data-img-fallback="remove-parent"></div>` : ''}
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
  if (!modDetailState.data) return;
  const source = modDetailState.data.source || 'modrinth';
  const projectId = modDetailState.data.id || appState.currentModDetailId || '';
  const filterGame = getCustomSelectValue('mod-filter-version') || '';
  const filterLoader = getCustomSelectValue('mod-filter-loader') || '';
  const verInfo = await getInstalledVersionInfo();
  const gameVersion = filterGame || verInfo.gameVersion || '';
  const loader = filterLoader || verInfo.loaderType || '';
  // “一键下载”不强制先点某个版本：按当前筛选/实例自动选根版本，
  // 否则 modDetailState.data 里没有 selectedVersionId 时会永远提示“请先选择一个版本”
  const versionId = modDetailState.data.selectedVersionId
    || modDetailState.data.versionId
    || pickDependencyRootVersion(gameVersion, loader);

  if (!versionId) {
    showToast(gameVersion || loader
      ? `当前筛选（${gameVersion || '未知版本'}${loader ? ' / ' + loader : ''}）下未找到可用版本`
      : '请先选择一个版本', 'error');
    return;
  }

  const btn = document.getElementById('md-deps-download-all-btn');
  if (btn) { btn.disabled = true; btn.innerHTML = '<span>检测中...</span>'; }

  showToast('正在检测前置依赖...', 'info');

  try {
    const targetVersionId = matchInstalledVersion(gameVersion, loader);
    const installedModsPromise = targetVersionId
      ? API.getVersionMods(targetVersionId).catch(() => [])
      : API.getInstalledMods().catch(() => []);
    const [depResult, installedModsData] = await Promise.all([
      API.getDependenciesRecursive(versionId, source, gameVersion, loader, projectId),
      installedModsPromise
    ]);
    const deps = depResult.dependencies || [];
    const unresolved = depResult.unresolved || [];
    const installedMods = Array.isArray(installedModsData)
      ? installedModsData
      : ((installedModsData && installedModsData.mods) || []);

    if (deps.length === 0) {
      if (unresolved.length > 0) {
        showToast(`有 ${unresolved.length} 个前置无法解析，请检查网络或游戏版本/加载器`, 'error');
      } else {
        showToast('该模组没有前置依赖', 'info');
      }
      if (btn) { btn.disabled = false; btn.innerHTML = '<span>一键下载</span>'; }
      return;
    }

    // 后端未找到兼容版本的依赖会带 compatibleVersion: null，先明确提示，避免误报“均已安装”
    const noVersionDeps = deps.filter(d => !d.compatibleVersion);
    const problemNames = [...new Set([
      ...unresolved.map(d => d.title || d.projectId),
      ...noVersionDeps.map(d => d.title || d.projectId)
    ])].filter(Boolean);
    if (problemNames.length > 0) {
      showToast(
        `有 ${problemNames.length} 个前置无兼容版本：${problemNames.slice(0, 3).join('、')}${problemNames.length > 3 ? ' 等' : ''}`,
        'error'
      );
    }

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
        const fn = (m.fileName || m.filename || '').toLowerCase();
        if (!fn) return false;
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
      if (problemNames.length > 0) {
        showToast(`没有可下载的前置（${problemNames.length} 个无法解析或无兼容版本）`, 'error');
      } else {
        showToast('所有前置依赖均已安装', 'info');
      }
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

    if (btn) { btn.innerHTML = `<span>下载中 (0/${toDownload.length})...</span>`; }

    let submitted = 0;
    let failed = 0;
    let index = 0;
    for (const dep of toDownload) {
      index++;
      try {
        if (btn) { btn.innerHTML = `<span>下载中 (${index}/${toDownload.length})...</span>`; }
        const result = await API.downloadModVersion(
          dep.compatibleVersion.versionId, dep.projectId, source, '',
          gameVersion, loader, savePath, false
        );
        if (result.success && result.sessionId) {
          submitted++;
          showModDownloadModal(result.fileName, result.sessionId, savePath, dep.icon || '');
        } else {
          failed++;
          showToast(`${dep.title || dep.projectId}: ${result.error || '下载失败'}`, 'error');
        }
      } catch (e) {
        failed++;
        showToast(`${dep.title || dep.projectId}: ${e.message || '下载失败'}`, 'error');
      }
    }

    S.mdDepsCache.clear();
    if (failed > 0) {
      showToast(`已提交 ${submitted}/${toDownload.length} 个前置下载，${failed} 个失败`, 'error');
    } else {
      showToast(`已提交 ${submitted} 个前置依赖下载`, 'success');
    }
    if (btn) { btn.disabled = false; btn.innerHTML = '<span>一键下载</span>'; }
  } catch (e) {
    showToast('检测前置依赖失败: ' + (e.message || '未知错误'), 'error');
    if (btn) { btn.disabled = false; btn.innerHTML = '<span>一键下载</span>'; }
  }
}

export { cleanupMdDepsCache, loadModDependencies, renderDepsList, toggleMdDepsSection, pickDependencyRootVersion, downloadAllDeps };
