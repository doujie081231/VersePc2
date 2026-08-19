/**
 * @file mod-install.js
 * @description 模组安装 - 模组文件安装、整合包安装、资源快速安装、依赖处理、批量下载
 */
// 当前模组的目标游戏版本/加载器（在 installModFile 中按所选版本捕获）
let _mdSaveGame = '';
let _mdSaveLoader = '';

/// 从版本的 gameVersions/loaders 中识别目标游戏版本与加载器
function resolveModTarget(gameVersions, loaders) {
  const gvs = (gameVersions || []).map(String);
  const lds = (loaders || []).map(String).map(l => l.toLowerCase());
  const LD = ['forge', 'fabric', 'neoforge', 'quilt'];
  let loader = lds.find(l => LD.includes(l)) || '';
  if (!loader) {
    loader = gvs.find(g => LD.includes(g.toLowerCase())) || '';
  }
  let game = gvs.find(g => /^\d+(\.\d+)+$/.test(g)) || '';
  return { game, loader };
}

/// 确保已安装版本列表已加载（installedVersions 仅在访问版本页时填充，
/// 直接从模组页进入时为空会导致路径匹配失败，需主动拉取）
async function ensureInstalledVersions() {
  if (Array.isArray(installedVersions) && installedVersions.length > 0) return true;
  try {
    const data = await API.getVersions(false);
    installedVersions = (data && data.installed) || [];
    if (!Array.isArray(installedVersions)) installedVersions = [];
    allVersions = (data && data.versions) || [];
    if (!Array.isArray(allVersions)) allVersions = [];
    return installedVersions.length > 0;
  } catch (e) {
    console.warn('[matchInstalledVersion] 拉取已安装版本失败:', e);
    return false;
  }
}

/// 按(游戏版本,加载器)匹配已安装版本 id
function matchInstalledVersion(game, loader) {
  const gl = String(game || '').toLowerCase();
  const lc = String(loader || '').toLowerCase();
  if (!Array.isArray(installedVersions) || installedVersions.length === 0) return '';
  const isL = v => {
    if (lc === 'forge') return !!v.isForge;
    if (lc === 'neoforge') return !!v.isNeoForge;
    if (lc === 'fabric') return !!v.isFabric;
    if (lc === 'quilt') return !!v.isFabric;
    return true;
  };
  const arr = installedVersions.filter(v => v && isL(v));
  if (!arr.length) return '';
  // 优先精确匹配游戏版本；游戏版本未知时退化为仅按加载器匹配
  const glArr = gl ? arr.filter(v => (
    (v.baseVersion || '').toLowerCase() === gl ||
    (v.id || '').toLowerCase().includes(gl)
  )) : arr;
  const pool = glArr.length ? glArr : arr;
  const baseMatch = gl ? pool.filter(v => (v.baseVersion || '').toLowerCase() === gl) : [];
  const usePool = baseMatch.length ? baseMatch : pool;
  const pref = usePool.filter(v => (v.id || '').toLowerCase().startsWith(gl || (v.baseVersion || '').toLowerCase()));
  return (pref[0] || usePool[0]).id || '';
}

function installModFileSafe(el) {
  if (!el) return;
  const vid = decodeURIComponent(atob(el.dataset.vid || ''));
  const fid = decodeURIComponent(atob(el.dataset.fid || ''));
  installModFile(currentModDetailId, currentModDetailSource, vid, fid);
}

function addModFromDetail(projectId, source, safeVid, safeFid) {
  const vid = decodeURIComponent(atob(safeVid || ''));
  const fid = decodeURIComponent(atob(safeFid || ''));

  if (modSelectedIds.has(projectId)) {
    const existing = modSelectedVersions.get(projectId);
    if (existing && existing.versionId === vid && existing.fileId === fid) {
      modSelectedIds.delete(projectId);
      modSelectedVersions.delete(projectId);
      showToast('已从选择中移除', 'info');
    } else {
      modSelectedVersions.set(projectId, {
        versionId: vid,
        fileId: fid,
        source: source
      });
      showToast('已更新选择的版本', 'success');
    }
  } else {
    modSelectedIds.add(projectId);
    modSelectedVersions.set(projectId, {
      versionId: vid,
      fileId: fid,
      source: source
    });
    showToast('已添加到下载列表', 'success');
  }
  updateModSelectUI();

  const container = document.getElementById('md-version-list');
  if (container) {
    container.querySelectorAll('.mdv-file-item').forEach(item => {
      const btn = item.querySelector('.mdv-install-btn');
      if (!btn) return;
      const itemVid = decodeURIComponent(atob(item.dataset.vid || ''));
      const itemFid = decodeURIComponent(atob(item.dataset.fid || ''));
      const isSelected = modSelectedIds.has(projectId);
      const isCurrentVersion = isSelected && modSelectedVersions.get(projectId)?.versionId === itemVid;
      btn.textContent = isCurrentVersion ? '已添加' : '添加';
      btn.classList.toggle('btn-secondary', isCurrentVersion);
      btn.classList.toggle('btn-primary', !isCurrentVersion);
    });
    container.querySelectorAll('.mdv-header-btn.mdv-install-btn').forEach(btn => {
      const itemVid = decodeURIComponent(atob(btn.dataset.vid || ''));
      const isSelected = modSelectedIds.has(projectId);
      const isCurrentVersion = isSelected && modSelectedVersions.get(projectId)?.versionId === itemVid;
      btn.textContent = isCurrentVersion ? '已添加' : '添加';
      btn.classList.toggle('btn-secondary', isCurrentVersion);
      btn.classList.toggle('btn-primary', !isCurrentVersion);
    });
  }
}

function installModpackVersionSafe(el) {
  if (!el) return;
  const vid = decodeURIComponent(atob(el.dataset.vid || ''));
  installModpackVersion(currentModDetailId, vid);
}

function installResourceVersionSafe(el) {
  if (!el) return;
  const vid = decodeURIComponent(atob(el.dataset.vid || ''));
  quickInstallResourceVersion(currentModDetailId, currentModDetailType, vid);
}

async function quickInstallResourceVersion(projectId, type, versionId) {
  const typeNames = { resourcepack: '材质包', shader: '光影包', datapack: '数据包' };
  const typeName = typeNames[type] || '资源';
  showToast('请选择保存文件夹...', 'info');
  try {
    const defaultPath = await resolveResourceSavePath(type);
    const folderResult = await API.selectSaveFolder(defaultPath);
    if (folderResult.cancelled) {
      if (folderResult.error) {
        showToast('文件夹选择失败: ' + folderResult.error, 'error');
      }
      return;
    }
    const savePath = folderResult.path;
    if (!savePath) {
      showToast('未选择文件夹', 'error');
      return;
    }
    localStorage.setItem('lastResourceSavePath_' + type, savePath);
    showToast(`正在安装${typeName}...`, 'info');
    const result = await API.downloadResource(versionId, projectId, type, '', savePath, '', currentModDetailSource);
    if (result.success) {
      showModDownloadModal(result.fileName, result.sessionId);
    } else {
      showToast(result.error || '安装失败', 'error');
    }
  } catch (e) {
    showToast('安装失败', 'error');
  }
}

function toggleMdvGroup(idx) {
  const group = document.getElementById(`mdvg-${idx}`);
  group.classList.toggle('expanded');
}

function getLoaderFileIcon(filename) {
  const lower = filename.toLowerCase();
  if (lower.includes('fabric')) return '<img src="img/Fabric.png" alt="" style="width:20px;height:20px;image-rendering:pixelated">';
  if (lower.includes('neoforge')) return '<img src="img/NeoForge.png" alt="" style="width:20px;height:20px;image-rendering:pixelated">';
  if (lower.includes('forge')) return '<img src="img/Anvil.png" alt="" style="width:20px;height:20px;image-rendering:pixelated">';
  if (lower.includes('optifine')) return '<img src="img/OptiFabric.png" alt="" style="width:20px;height:20px;image-rendering:pixelated">';
  return '<img src="img/Grass.png" alt="" style="width:20px;height:20px;image-rendering:pixelated">';
}

function installModFile(projectId, source, versionId, fileId) {
  // 捕获当前模组目标游戏版本/加载器，用于保存路径定位（Forge/Fabric 同版本号不混淆）
  try {
    const versions = (typeof mdAllVersions !== 'undefined') ? mdAllVersions : [];
    const v = versions.find(x => x && x.id === versionId);
    if (v) {
      const t = resolveModTarget(v.gameVersions, v.loaders);
      _mdSaveGame = t.game || _mdSaveGame;
      _mdSaveLoader = t.loader || _mdSaveLoader;
    }
  } catch (e) {}
  showModInstallConfirm(projectId, source, versionId, fileId);
}

async function installModpackVersion(projectId, versionId) {
  const modpackTitle = currentModDetailData?.title || currentModDetailData?.name || projectId;
  showModpackNameModal(modpackTitle, projectId, versionId);
}

function showModpackNameModal(defaultName, projectId, versionId) {
  const modal = document.createElement('div');
  modal.className = 'modal-overlay';
  modal.style.cssText = 'position:fixed;inset:0;z-index:10001;display:flex;align-items:center;justify-content:center;background:rgba(0,0,0,0.5);';

  const card = document.createElement('div');
  card.style.cssText = 'max-width:420px;width:90%;background:var(--bg-secondary);border-radius:12px;box-shadow:0 20px 60px rgba(0,0,0,0.3);overflow:hidden;';

  card.innerHTML = `
    <div style="padding:16px 20px;border-bottom:1px solid var(--border);display:flex;align-items:center;justify-content:space-between;">
      <span style="font-size:15px;font-weight:600;color:var(--text-primary);">设置整合包名称</span>
      <button id="mpnm-close" style="background:none;border:none;color:var(--text-muted);cursor:pointer;font-size:18px;padding:4px;">✕</button>
    </div>
    <div style="padding:20px;">
      <div style="margin-bottom:12px;">
        <label style="display:block;font-size:13px;color:var(--text-secondary);margin-bottom:6px;">版本名称</label>
        <input id="mpnm-input" type="text" value="${defaultName.replace(/"/g, '&quot;')}" 
          style="width:100%;padding:10px 12px;border:1px solid var(--border);border-radius:8px;background:var(--bg-primary);color:var(--text-primary);font-size:14px;outline:none;box-sizing:border-box;" />
        <div id="mpnm-hint" style="margin-top:6px;font-size:12px;color:var(--text-muted);"></div>
      </div>
      <div id="mpnm-warn" style="display:none;padding:10px 12px;border-radius:8px;background:rgba(255,193,7,0.1);border:1px solid rgba(255,193,7,0.3);margin-bottom:12px;">
        <span style="font-size:13px;color:#e6a817;">⚠ 已有相同名称的版本</span>
      </div>
    </div>
    <div style="padding:12px 20px;border-top:1px solid var(--border);display:flex;justify-content:flex-end;gap:10px;">
      <button id="mpnm-cancel" style="padding:8px 20px;border-radius:8px;border:1px solid var(--border);background:transparent;color:var(--text-primary);cursor:pointer;font-size:13px;">取消</button>
      <button id="mpnm-confirm" style="padding:8px 20px;border-radius:8px;border:none;background:var(--accent);color:#fff;cursor:pointer;font-size:13px;font-weight:500;">确认安装</button>
    </div>
  `;

  modal.appendChild(card);
  document.body.appendChild(modal);

  const input = document.getElementById('mpnm-input');
  const hint = document.getElementById('mpnm-hint');
  const warn = document.getElementById('mpnm-warn');
  const confirmBtn = document.getElementById('mpnm-confirm');

  async function checkName() {
    const name = input.value.trim();
    if (!name) {
      hint.textContent = '';
      confirmBtn.disabled = true;
      confirmBtn.style.opacity = '0.5';
      return;
    }
    hint.textContent = '✓';
    confirmBtn.disabled = false;
    confirmBtn.style.opacity = '1';
    confirmBtn.style.cursor = 'pointer';
  }

  input.addEventListener('input', checkName);
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !confirmBtn.disabled) confirmBtn.click();
  });

  document.getElementById('mpnm-close').onclick = () => modal.remove();
  document.getElementById('mpnm-cancel').onclick = () => modal.remove();
  modal.onclick = (e) => { if (e.target === modal) modal.remove(); };

  confirmBtn.onclick = async () => {
    const name = input.value.trim();
    if (!name || confirmBtn.disabled) return;
    modal.remove();
    _doInstallModpack(projectId, versionId, name);
  };

  checkName();
  input.focus();
  input.select();
}

async function _doInstallModpack(projectId, versionId, customName) {
  showToast('正在下载整合包，将创建新版本...', 'info');
  try {
    const result = await API.downloadResource(versionId, projectId, 'modpack', '', '', customName, currentModDetailSource);
    if (result.success) {
      showModpackInstallModal(result.fileName, result.sessionId);
    } else {
      console.error('[Modpack] downloadResource failed:', JSON.stringify(result));
      showToast(`整合包安装失败: ${result.error || '未知错误'}`, 'error');
    }
  } catch (e) {
    console.error('[Modpack] downloadResource error:', e);
    showToast(`整合包安装失败: ${e.message || e}`, 'error');
  }
}

async function quickInstallModpack(projectId, versionId) {
  const modpackTitle = currentModDetailData?.title || currentModDetailData?.name || projectId;
  showModpackNameModal(modpackTitle, projectId, versionId);
}

function showModpackInstallModal(fileName, sessionId) {
  currentInstallSessionId = sessionId;
  const taskId = 'modpack-' + sessionId;
  const iconUrl = currentModDetailData?.icon || '';
  dlManager.add(taskId, fileName || '整合包安装', 'modpack', sessionId, iconUrl);

  // 监听后端的 import-progress 事件，实时累积各阶段进度（stageHistory）与模组文件进度（files）
  let unlisten = null;
  let importActive = false; // 是否已从事件拿到实时阶段进度
  let currentProgress = 0; // 当前总进度（事件与轮询共用，避免被覆盖清零）
  const stageHistory = []; // 累积的各阶段进度 [{stage, progress, message}]

  const upsertStage = (stage, progress, message) => {
    if (!stage) return;
    const existing = stageHistory.find(s => s.stage === stage);
    if (existing) {
      existing.progress = Math.max(existing.progress || 0, progress || 0);
      if (message) existing.message = message;
    } else {
      stageHistory.push({ stage, progress: progress || 0, message: message || stage });
    }
  };

  const normalizeFiles = (list) => (list || []).map(f => ({
    name: f.name || f.filename || f.path || '',
    status: f.status || 'pending',
    progress: f.progress || 0,
    size: f.size ? formatSize(f.size) : ''
  }));

  const registerImportListener = () => {
    const tauriEvent = (window.__TAURI__ && window.__TAURI__.event)
      ? window.__TAURI__.event
      : window.__TAURI_INTERNALS__;
    if (!tauriEvent || !tauriEvent.listen) return;
    tauriEvent.listen('import-progress', function (event) {
      const p = (event && event.payload) || {};
      if (!p || !p.message) return;
      importActive = true;
      const update = { message: p.message, currentFile: p.currentFile || '' };
      if (typeof p.progress === 'number') {
        currentProgress = Math.max(currentProgress, p.progress);
        update.progress = currentProgress;
      }
      if (p.stage) upsertStage(p.stage, p.progress, p.message);
      if (Array.isArray(p.files) && p.files.length) update.files = normalizeFiles(p.files);
      if (stageHistory.length) update.stageHistory = stageHistory.slice();
      dlManager.update(taskId, update);
    }).then(fn => { unlisten = fn; }).catch(() => {});
  };
  registerImportListener();

  let unknownRetries = 0;
  const cleanup = () => {
    if (unlisten) { try { unlisten(); } catch (e) {} unlisten = null; }
    modDownloadPollTimers.forEach(t => clearTimeout(t));
    modDownloadPollTimers = [];
  };
  const poll = async () => {
    try {
      const data = await API.getModDownloadStatus(sessionId);
      const displayStatus = data.status === 'completed' ? 'completed' : data.status === 'failed' ? 'failed' : data.status === 'cancelled' ? 'failed' : 'downloading';
      // 导入阶段优先用 import-progress 事件推送的实时进度，避免被固定的"正在解析整合包"覆盖
      const update = { status: displayStatus, progress: currentProgress };
      if (!importActive) {
        // 整合包文件下载阶段：用轮询的 session 进度与单文件进度
        currentProgress = data.progress || 0;
        update.progress = currentProgress;
        update.message = data.phase === 'importing' ? '正在安装整合包...' : getDownloadStageText(data);
        update.files = normalizeFiles(data.files);
      } else {
        // 导入阶段：文件列表与进度都来自 import-progress 事件，轮询不覆盖
        update.progress = currentProgress;
      }
      // 仅当后端确实返回了阶段历史时才覆盖本地累积，避免用空数组清掉已累积的进度
      if (data.stageHistory && Array.isArray(data.stageHistory) && data.stageHistory.length) {
        update.stageHistory = data.stageHistory;
      }
      dlManager.update(taskId, update);
      if (data.status === 'completed') {
        cleanup();
        showToast('整合包安装完成', 'success');
        loadVersions(true);
        return;
      }
      if (data.status === 'failed') {
        cleanup();
        showToast(`安装失败: ${data.message}`, 'error');
        return;
      }
      if (data.status === 'cancelled') {
        cleanup();
        dlManager.update(taskId, { status: 'failed', message: '已取消' });
        return;
      }
      if (data.phase === 'importing') {
        const timer = setTimeout(poll, 500);
        modDownloadPollTimers.push(timer);
        return;
      }
      if (data.status === 'unknown' || !data.status) {
        unknownRetries++;
        if (unknownRetries <= 1) {
          const timer = setTimeout(poll, 3000);
          modDownloadPollTimers.push(timer);
          return;
        }
        cleanup();
        dlManager.update(taskId, { status: 'failed', message: '会话已失效' });
        return;
      }
      const timer = setTimeout(poll, 500);
      modDownloadPollTimers.push(timer);
    } catch (e) {
      const timer = setTimeout(poll, 800);
      modDownloadPollTimers.push(timer);
    }
  };
  setTimeout(poll, 300);
}

function getDownloadStageText(data) {
  if (!data) return '准备中...';
  if (data.status === 'completed') return '安装完成';
  if (data.status === 'failed') return data.message || '安装失败';
  if (data.status === 'cancelled') return '已取消';
  if (data.message && data.phase !== 'importing') return data.message;
  const phaseMap = {
    'download':        '下载整合包文件...',
    'read':            '正在读取整合包...',
    'base':            '正在准备基础版本...',
    'loader-install':  '正在安装模组加载器...',
    'loader-upgrade':  '正在升级模组加载器...',
    'version-config':  '正在创建版本配置...',
    'loader':          '模组加载器就绪',
    'download-mods':   '下载整合包模组...',
    'overrides':       '解压整合包配置...',
    'verify':          '正在验证整合包完整性...',
    'install':         '安装整合包内容...',
    'importing':       '正在安装整合包...',
  };
  if (data.phase && phaseMap[data.phase]) return phaseMap[data.phase];
  if (data.phase === 'install') return '安装整合包内容...';
  return data.message || '处理中...';
}

async function resolveModSavePath(versionId) {
  try {
    const filterVersion = getCustomSelectValue('mod-filter-version');
    const filterLoader = getCustomSelectValue('mod-filter-loader');
    const targetGame = _mdSaveGame || filterVersion || '';
    const targetLoader = _mdSaveLoader || filterLoader || '';
    await ensureInstalledVersions();
    const matched = matchInstalledVersion(targetGame, targetLoader);
    const vid = matched || versionId || _modDownloadVersionId || filterVersion || '';
    console.warn('[resolveModSavePath] targetGame=', targetGame, 'targetLoader=', targetLoader,
      'installedCount=', (installedVersions || []).length, 'matched=', matched, 'vid=', vid);
    // 版本感知解析：优先调用支持 versionId 的后端命令，目录以传入版本为准，
    // 避免 HTTP 路由回退到 selectedVersion 导致 Forge 模组落到其它目录
    let path = '';
    if (vid && window.electronAPI && typeof window.electronAPI.getDefaultModPath === 'function') {
      const r = await window.electronAPI.getDefaultModPath(vid).catch(() => null);
      if (typeof r === 'string' && r) path = r;
    }
    if (!path) {
      const res = await API.getDefaultModPath(vid).catch(e => { console.warn('[resolveModSavePath] getDefaultModPath error:', e); return null; });
      if (typeof res === 'string') {
        path = res;
      } else if (res && typeof res === 'object') {
        path = res.path || res.data || '';
      }
    }
    console.warn('[resolveModSavePath] path=', path);
    if (path) return path;
  } catch (e) {
    console.warn('[resolveModSavePath] error:', e);
  }
  return localStorage.getItem('lastModSavePath') || '';
}

const resourceFolderMap = { resourcepack: 'resourcepacks', shader: 'shaderpacks', datapack: 'datapacks' };

async function resolveResourceSavePath(type) {
  const folderName = resourceFolderMap[type];
  if (!folderName) return '';
  const storageKey = 'lastResourceSavePath_' + type;
  try {
    const res = await API.getDefaultResourcePath(type).catch(() => null);
    let p = '';
    if (typeof res === 'string') {
      p = res;
    } else if (res && typeof res === 'object') {
      p = res.path || res.data || '';
    }
    if (p) return p;
  } catch (e) {}
  return localStorage.getItem(storageKey) || '';
}

async function showModInstallConfirm(projectId, source, versionId, fileId) {
  showToast('请选择保存文件夹...', 'info');
  try {
    const defaultPath = await resolveModSavePath();
    console.warn('[mod-install] dialog defaultPath=', JSON.stringify(defaultPath));
    const folderResult = await API.selectSaveFolder(defaultPath);
    if (folderResult.cancelled) {
      if (folderResult.error) {
        showToast('文件夹选择失败: ' + folderResult.error, 'error');
      } else {
        showToast('已取消选择', 'info');
      }
      return;
    }
    const savePath = folderResult.path;
    if (!savePath) {
      showToast('未选择文件夹', 'error');
      return;
    }
    localStorage.setItem('lastModSavePath', savePath);

    proceedModInstall(projectId, source, versionId, fileId, savePath, true);
  } catch (e) {
    console.error('Mod install confirm error:', e);
    showToast('操作失败', 'error');
  }
}

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
    const recursiveDeps = await API.getDependenciesRecursive(versionId, source, gameVersion, loader);
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
            <button id="${modalId}-confirm" style="padding:8px 20px;border-radius:8px;border:none;background:var(--primary);color:#fff;cursor:pointer;font-size:13px;font-weight:600;">确认下载全部</button>
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
      showModDownloadModal(mainResult.fileName, mainResult.sessionId, savePath, currentModDetailData?.icon || '');
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

    mdDepsCache.clear();
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
      ? `<img src="${dep.icon}" style="width:32px;height:32px;border-radius:6px;object-fit:cover;" onerror="this.style.display='none'" loading="lazy">`
      : `<div style="width:32px;height:32px;border-radius:6px;background:var(--bg-secondary);display:flex;align-items:center;justify-content:center;font-size:14px;">📦</div>`;
    const btnDisabled = !ver ? 'opacity:0.4;pointer-events:none;' : '';
    return `<div style="display:flex;align-items:center;gap:10px;padding:8px 0;border-bottom:1px solid var(--border);">
      ${iconHtml}
      <div style="flex:1;min-width:0;">
        <div style="font-weight:600;font-size:13px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;">${escapeHtml(dep.title)}</div>
        <div style="font-size:11px;color:var(--text-secondary);">${escapeHtml(verInfo)}</div>
      </div>
      <button class="dep-single-download-btn" data-project-id="${escapeHtml(dep.projectId)}" style="padding:4px 10px;border-radius:6px;border:1px solid var(--accent);background:transparent;color:var(--accent);cursor:pointer;font-size:11px;white-space:nowrap;${btnDisabled}" title="${ver ? '选择版本并下载前置模组' : '无兼容版本'}">下载前置</button>
      ${ver ? '<span style="font-size:11px;color:#22c55e;">✓</span>' : '<span style="font-size:11px;color:#ef4444;">✗</span>'}
    </div>`;
  }).join('');

  const downloadableCount = deps.filter(d => d.compatibleVersion).length;

  modal.innerHTML = `<div style="background:var(--bg-primary);border-radius:12px;padding:24px;max-width:460px;width:90%;box-shadow:0 20px 60px rgba(0,0,0,0.3);">
    <h3 style="margin:0 0 8px;font-size:16px;font-weight:700;">检测到前置依赖</h3>
    <p style="margin:0 0 16px;font-size:13px;color:var(--text-secondary);">该模组需要以下前置模组才能正常运行：</p>
    <div style="max-height:280px;overflow-y:auto;margin-bottom:16px;">${depListHtml}</div>
    <div style="display:flex;gap:10px;justify-content:flex-end;">
      <button id="dep-cancel-btn" style="padding:8px 20px;border-radius:8px;border:1px solid var(--border);background:transparent;color:var(--text-primary);cursor:pointer;font-size:13px;">取消</button>
      <button id="dep-download-btn" style="padding:8px 20px;border-radius:8px;border:none;background:var(--primary);color:#fff;cursor:pointer;font-size:13px;font-weight:600;${downloadableCount === 0 ? 'opacity:0.5;pointer-events:none;' : ''}">一键下载全部（${downloadableCount} 个前置）</button>
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
  const iconUrl = currentModDetailData?.icon || '';
  dlManager.add(pendingTaskId, '准备下载...', 'mod', '', iconUrl);
  dlManager.update(pendingTaskId, { progress: 0, status: 'downloading', message: '正在获取下载信息...' });
  try {
    const currentGameVersion = gameVersion || getCustomSelectValue('mod-filter-version') || '';
    const currentLoader = loader || getCustomSelectValue('mod-filter-loader') || '';
    const result = await API.downloadModVersion(versionId || '', projectId, source, fileId || '', currentGameVersion, currentLoader, savePath, includeDeps);
    if (result.success) {
      mdDepsCache.clear();
      dlManager.remove(pendingTaskId);
      showModDownloadModal(result.fileName, result.sessionId, savePath, iconUrl);
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

function quickInstallCurrentMod() {
  if (!currentModDetailData) return;
  const versionId = currentModDetailData.selectedVersionId || currentModDetailData.versionId || '';
  showModInstallConfirm(currentModDetailData.id || currentModDetailId, currentModDetailSource, versionId);
}

function copyModName() {
  if (!currentModDetailData) return;
  window.electronAPI.clipboard.writeText(currentModDetailData.title).then(() => showToast('已复制名称', 'success'));
}

function openModSourceUrl() {
  if (!currentModDetailData) return;
  let url = '';
  if (currentModDetailSource === 'curseforge') {
    url = `https://www.curseforge.com/minecraft-mc-mods/${currentModDetailId}`;
  } else {
    url = `https://modrinth.com/mod/${currentModDetailId}`;
  }
  window.electronAPI.openExternal(url);
}

async function quickInstallMod(projectId, source, versionId, fileId) {
  const pendingTaskId = 'mod-' + Date.now();
  const iconUrl = currentModDetailData?.icon || '';
  dlManager.add(pendingTaskId, '准备下载...', 'mod', '', iconUrl);
  dlManager.update(pendingTaskId, { progress: 0, status: 'downloading', message: '正在获取下载信息...' });
  try {
    const currentGameVersion = getCustomSelectValue('mod-filter-version') || '';
    const currentLoader = getCustomSelectValue('mod-filter-loader') || '';
    const savePath = await resolveModSavePath();
    const result = await API.downloadModVersion(versionId || '', projectId, source, fileId || '', currentGameVersion, currentLoader, savePath);
    if (result.success) {
      mdDepsCache.clear();
      dlManager.remove(pendingTaskId);
      showModDownloadModal(result.fileName, result.sessionId);
    } else {
      dlManager.update(pendingTaskId, { status: 'failed', message: result.error || '下载失败' });
      showToast(result.error || '下载失败', 'error');
    }
  } catch (e) {
    console.error('quickInstallMod error:', e);
    dlManager.update(pendingTaskId, { status: 'failed', message: e.message || '请求失败' });
    showToast('下载请求失败: ' + (e.message || '未知错误'), 'error');
  }
}

function showModDownloadModal(fileName, sessionId, savePath, iconUrl) {
  const taskId = 'mod-' + sessionId;
  const resolvedIcon = iconUrl || currentModDetailData?.icon || '';
  dlManager.add(taskId, fileName || '模组下载', 'mod', sessionId, resolvedIcon);

  modDownloadPollTimers.forEach(t => clearTimeout(t));
  modDownloadPollTimers = [];

  let unknownRetries = 0;
  const poll = async () => {
    try {
      const data = await API.getModDownloadStatus(sessionId);
      dlManager.update(taskId, {
        progress: data.progress || 0,
        status: data.status === 'completed' ? 'completed' : data.status === 'failed' ? 'failed' : 'downloading',
        message: data.message || '下载中...'
      });
      if (data.status === 'completed') {
        showToast(`${fileName} 下载完成`, 'success');
        loadInstalledMods();
        return;
      }
      if (data.status === 'failed') {
        showToast(`下载失败: ${data.message}`, 'error');
        return;
      }
      if (data.status === 'unknown' || !data.status) {
        unknownRetries++;
        if (unknownRetries <= 1) {
          const timer = setTimeout(poll, 3000);
          modDownloadPollTimers.push(timer);
          return;
        }
        dlManager.update(taskId, { status: 'failed', message: '会话已失效' });
        return;
      }
      const timer = setTimeout(poll, 500);
      modDownloadPollTimers.push(timer);
    } catch (e) {
      const timer = setTimeout(poll, 1000);
      modDownloadPollTimers.push(timer);
    }
  };
  const timer = setTimeout(poll, 500);
  modDownloadPollTimers.push(timer);
}

function toggleModMultiSelect() {
  modMultiSelectMode = !modMultiSelectMode;
  const toggleBtn = document.getElementById('mod-multiselect-toggle');
  const bar = document.getElementById('mod-multiselect-bar');
  const hintEl = document.getElementById('mod-filter-hint');
  
  if (modMultiSelectMode) {
    toggleBtn.classList.add('btn-primary');
    toggleBtn.classList.remove('btn-secondary');
    bar.style.display = 'flex';
    modSelectedIds.clear();
    modSelectedVersions.clear();
    
    const gv = getCustomSelectValue('mod-filter-version') || '';
    const ld = getCustomSelectValue('mod-filter-loader') || '';
    let hintParts = [];
    if (gv) hintParts.push(gv);
    if (ld) hintParts.push(ld.charAt(0).toUpperCase() + ld.slice(1));
    if (hintEl) hintEl.textContent = hintParts.length > 0 ? `将下载 ${hintParts.join(' + ')} 版本` : '建议先选择游戏版本和加载器';
    
    updateModSelectUI();
  } else {
    toggleBtn.classList.remove('btn-primary');
    toggleBtn.classList.add('btn-secondary');
    bar.style.display = 'none';
    modSelectedIds.clear();
    modSelectedVersions.clear();
  }
  loadMods();
}

function toggleModSelect(modId) {
  if (modSelectedIds.has(modId)) {
    modSelectedIds.delete(modId);
  } else {
    modSelectedIds.add(modId);
  }
  updateModSelectUI();
  
  const safeId = CSS.escape(modId);
  const checkbox = document.querySelector(`.mod-checkbox[data-mod-id="${safeId}"]`);
  if (checkbox) {
    checkbox.classList.toggle('checked', modSelectedIds.has(modId));
  }
}

function toggleSelectAllMods(checked) {
  const container = document.getElementById('mod-browse-list');
  const items = container.querySelectorAll('.mod-item');
  
  if (checked) {
    items.forEach(item => {
      const checkbox = item.querySelector('.mod-checkbox');
      if (checkbox) {
        const modId = checkbox.dataset.modId;
        modSelectedIds.add(modId);
        checkbox.classList.add('checked');
      }
    });
  } else {
    modSelectedIds.clear();
    items.forEach(item => {
      const checkbox = item.querySelector('.mod-checkbox');
      if (checkbox) checkbox.classList.remove('checked');
    });
  }
  updateModSelectUI();
}

function updateModSelectUI() {
  const countEl = document.getElementById('mod-selected-count');
  const batchBtn = document.getElementById('mod-batch-download-btn');
  const selectAll = document.getElementById('mod-select-all');
  
  if (countEl) countEl.textContent = `已选 ${modSelectedIds.size} 个`;
  if (batchBtn) batchBtn.disabled = modSelectedIds.size === 0;
  
  const container = document.getElementById('mod-browse-list');
  const totalItems = container.querySelectorAll('.mod-checkbox').length;
  if (selectAll) selectAll.checked = totalItems > 0 && modSelectedIds.size >= totalItems;
}

async function batchDownloadMods() {
  if (modSelectedIds.size === 0) return;

  const defaultPath = await resolveModSavePath();
  const folderResult = await API.selectSaveFolder(defaultPath);
  if (folderResult.cancelled) return;
  const savePath = folderResult.path;
  if (!savePath) {
    showToast('未选择文件夹', 'error');
    return;
  }
  localStorage.setItem('lastModSavePath', savePath);

  const currentGameVersion = getCustomSelectValue('mod-filter-version');
  const currentLoader = getCustomSelectValue('mod-filter-loader');
  
  const modIds = Array.from(modSelectedIds);
  const total = modIds.length;
  
  const modInfoMap = {};
  modSearchResults.forEach(m => { modInfoMap[m.id] = m; });

  const batchTaskId = 'batch-' + Date.now();
  const files = modIds.map(id => {
    const info = modInfoMap[id];
    const displayName = info ? formatModNameWithChinese(info.slug || id, info.title) : id;
    return { name: displayName, status: 'pending', size: '' };
  });
  dlManager.add(batchTaskId, `批量下载 ${total} 个模组`, 'mod', '');
  dlManager.update(batchTaskId, { files: files });

  let completed = 0;
  let failed = 0;
  
  for (let i = 0; i < modIds.length; i++) {
    const modId = modIds[i];
    const info = modInfoMap[modId];
    const displayName = info ? formatModNameWithChinese(info.slug || modId, info.title) : modId;

    files[i].status = 'downloading';
    dlManager.update(batchTaskId, {
      progress: Math.round((i / total) * 100),
      message: `正在下载 ${i + 1}/${total}`,
      files: [...files]
    });
    
    try {
      const selectedVer = modSelectedVersions.get(modId);
      const versionId = selectedVer?.versionId || '';
      const fileId = selectedVer?.fileId || '';
      const source = selectedVer?.source || 'modrinth';
      
      const result = await API.downloadModVersion(versionId, modId, source, fileId, currentGameVersion, currentLoader, savePath);
      
      if (result.success) {
        await pollBatchModDownload(result.sessionId, modId);
        completed++;
        files[i].status = 'completed';
      } else {
        failed++;
        files[i].status = 'failed';
      }
    } catch (e) {
      failed++;
      files[i].status = 'failed';
    }
    
    dlManager.update(batchTaskId, {
      progress: Math.round(((i + 1) / total) * 100),
      message: `下载完成 ${completed}/${total}${failed > 0 ? `，失败 ${failed}` : ''}`,
      status: (i + 1 === total) ? (failed === total ? 'failed' : 'completed') : 'downloading',
      files: [...files]
    });
  }
  
  modSelectedIds.clear();
  modSelectedVersions.clear();
  updateModSelectUI();
  
  if (currentSettingsVersionId) {
    loadInstalledModsForSettings();
  }
  loadInstalledMods();
}

function pollBatchModDownload(sessionId, modId) {
  return new Promise((resolve) => {
    const poll = async () => {
      try {
        const data = await API.getModDownloadStatus(sessionId);
        if (data.status === 'completed') {
          resolve();
          return;
        }
        if (data.status === 'failed') {
          resolve();
          return;
        }
        setTimeout(poll, 500);
      } catch (e) {
        setTimeout(poll, 1000);
      }
    };
    setTimeout(poll, 500);
  });
}
