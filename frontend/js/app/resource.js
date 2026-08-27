/**
 * @file resource.js
 * @description 资源管理 - 整合包、数据包、材质包、光影包的浏览、详情与安装
 */
const resourceState = {
  modpack: { offset: 0, total: 0, query: '' },
  datapack: { offset: 0, total: 0, query: '' },
  resourcepack: { offset: 0, total: 0, query: '' },
  shader: { offset: 0, total: 0, query: '' },
};

// 资源列表请求竞态保护：每次请求递增 _reqId，await 后校验是否为最新请求
// 避免快速切换时旧请求覆盖新请求导致图标/列表错乱
const _resourceReqId = { modpack: 0, datapack: 0, resourcepack: 0, shader: 0 };

const typeNames = {
  modpack: '整合包', datapack: '数据包',
  resourcepack: '材质包', shader: '光影包'
};

const typeIcons = {
  modpack: '📦', datapack: '🗄️',
  resourcepack: '🎨', shader: '☀️'
};

// 整合包命名弹窗：让用户自定义版本名称，与下载版本时的命名弹窗保持一致
// 支持回调形式和 Promise 形式（不传 onConfirm 时返回 Promise）
function showImportNameModal(defaultName, onConfirm) {
  const existing = document.getElementById('import-name-modal');
  if (existing) existing.remove();

  const modal = document.createElement('div');
  modal.id = 'import-name-modal';
  modal.className = 'modal-overlay';
  modal.style.cssText = 'position:fixed;inset:0;z-index:10001;display:flex;align-items:center;justify-content:center;background:rgba(0,0,0,0.5);';

  const card = document.createElement('div');
  card.style.cssText = 'max-width:420px;width:90%;background:var(--bg-secondary);border-radius:12px;box-shadow:0 20px 60px rgba(0,0,0,0.3);overflow:hidden;';

  card.innerHTML = `
    <div style="padding:16px 20px;border-bottom:1px solid var(--border);display:flex;align-items:center;justify-content:space-between;">
      <span style="font-size:15px;font-weight:600;color:var(--text-primary);">设置版本名称</span>
      <button id="inm-close" style="background:none;border:none;color:var(--text-muted);cursor:pointer;font-size:18px;padding:4px;">✕</button>
    </div>
    <div style="padding:20px;">
      <div style="margin-bottom:12px;">
        <label style="display:block;font-size:13px;color:var(--text-secondary);margin-bottom:6px;">版本名称</label>
        <input id="inm-input" type="text" value="${defaultName.replace(/"/g, '&quot;')}"
          style="width:100%;padding:10px 12px;border:1px solid var(--border);border-radius:8px;background:var(--bg-primary);color:var(--text-primary);font-size:14px;outline:none;box-sizing:border-box;" />
        <div id="inm-hint" style="margin-top:6px;font-size:12px;color:var(--text-muted);"></div>
      </div>
      <div id="inm-warn" style="display:none;padding:10px 12px;border-radius:8px;background:rgba(255,193,7,0.1);border:1px solid rgba(255,193,7,0.3);margin-bottom:12px;">
        <span style="font-size:13px;color:#e6a817;">⚠ 已有相同名称的版本</span>
      </div>
    </div>
    <div style="padding:12px 20px;border-top:1px solid var(--border);display:flex;justify-content:flex-end;gap:10px;">
      <button id="inm-cancel" style="padding:8px 20px;border-radius:8px;border:1px solid var(--border);background:transparent;color:var(--text-primary);cursor:pointer;font-size:13px;">取消</button>
      <button id="inm-confirm" style="padding:8px 20px;border-radius:8px;border:none;background:var(--accent);color:#fff;cursor:pointer;font-size:13px;font-weight:500;">确认安装</button>
    </div>
  `;

  modal.appendChild(card);
  document.body.appendChild(modal);

  const input = document.getElementById('inm-input');
  const hint = document.getElementById('inm-hint');
  const warn = document.getElementById('inm-warn');
  const confirmBtn = document.getElementById('inm-confirm');

  // 当未传 onConfirm 时，使用 Promise 模式
  const usePromise = typeof onConfirm !== 'function';
  let _resolve = null;

  async function checkName() {
    const name = input.value.trim();
    if (!name) {
      hint.textContent = '';
      warn.style.display = 'none';
      confirmBtn.disabled = true;
      confirmBtn.style.opacity = '0.5';
      return;
    }
    try {
      const result = await API.checkVersionName(name);
      if (!result.available) {
        warn.style.display = 'block';
        warn.textContent = '⚠ ' + (result.reason || '版本名称不可用');
        hint.textContent = '';
        confirmBtn.disabled = true;
        confirmBtn.style.opacity = '0.5';
        confirmBtn.style.cursor = 'not-allowed';
      } else {
        warn.style.display = 'none';
        hint.textContent = '✓ 名称可用';
        confirmBtn.disabled = false;
        confirmBtn.style.opacity = '1';
        confirmBtn.style.cursor = 'pointer';
      }
    } catch (e) {
      warn.style.display = 'none';
      hint.textContent = '';
      confirmBtn.disabled = false;
      confirmBtn.style.opacity = '1';
      confirmBtn.style.cursor = 'pointer';
    }
  }

  input.addEventListener('input', checkName);
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !confirmBtn.disabled) confirmBtn.click();
  });

  function closeModal(result) {
    modal.remove();
    if (usePromise && _resolve) {
      _resolve(result);
    } else if (!usePromise && onConfirm) {
      onConfirm(result);
    }
  }

  document.getElementById('inm-close').onclick = () => closeModal(null);
  document.getElementById('inm-cancel').onclick = () => closeModal(null);
  modal.onclick = (e) => { if (e.target === modal) closeModal(null); };

  confirmBtn.onclick = async () => {
    const name = input.value.trim();
    if (!name || confirmBtn.disabled) return;
    closeModal(name);
  };

  checkName();
  input.focus();
  input.select();

  if (usePromise) {
    return new Promise((resolve) => { _resolve = resolve; });
  }
}

function getImportStageText(msg) {
  if (!msg) return '处理中...';
  if (msg.includes('download') || msg.includes('下载')) return '下载整合包内容...';
  if (msg.includes('read') || msg.includes('读取') || msg.includes('分析')) return '分析整合包...';
  if (msg.includes('mod') || msg.includes('模组')) return '下载整合包模组...';
  if (msg.includes('override') || msg.includes('配置')) return '解压整合包配置...';
  if (msg.includes('install') || msg.includes('安装')) return '安装整合包...';
  return msg;
}

async function importModpackFromFile() {
  if (window._modpackImporting) {
    showToast('整合包正在导入中，请等待完成', 'warning');
    return;
  }
  var _useVIsland = typeof DynamicIsland !== 'undefined' && DynamicIsland.isEnabled();
  try {
    const result = await API.selectModpackFile();
    if (result && result.filePath) {
      const filePath = result.filePath;
      // 用文件名（去掉扩展名）作为默认版本名
      const fileBaseName = (result.name || result.filePath).replace(/\.(mrpack|zip|cursemodpack)$/i, '');
      showImportNameModal(fileBaseName, async function(customName) {
        window._modpackImporting = true;
        try {
          var sessionId = 'local-modpack-' + Date.now();
          var taskId = 'modpack-' + sessionId;
          if (_useVIsland) {
            DynamicIsland.show(result.name || '整合包导入');
          } else if (typeof dlManager !== 'undefined') {
            dlManager.add(taskId, result.name || '整合包导入', 'modpack', sessionId, '');
          }
          if (window.electronAPI?.onImportProgress) {
            if (window.electronAPI.removeImportProgressListener) window.electronAPI.removeImportProgressListener();
            // 节流控制：避免高频进度回调打爆主线程
            var _ipThrottleTimer = null;
            var _ipLastData = null;
            var _ipLastTime = 0;
            var IP_THROTTLE_MS = 250;
            function _doImportProgress(data) {
              var stageText = getImportStageText(data.message);
              var pct = data.progress || 0;
              var filesMapped = null;
              if (data.files && data.files.length > 0) {
                var totalSpeed = 0;
                for (var i = 0; i < data.files.length; i++) {
                  var f = data.files[i];
                  if ((f.status === 'downloading' || f.s === 'downloading') && (f.speed || f.sp || 0) > 0) totalSpeed += (f.speed || f.sp || 0);
                }
                filesMapped = data.files.map(function (f) {
                  return { name: f.name || f.filename || f.n || '', status: f.status || f.s || 'pending', progress: f.progress || f.p || 0, speed: f.speed || f.sp || 0 };
                });
              }
              if (_useVIsland) {
                DynamicIsland.update({ progress: pct, status: 'downloading', message: stageText, name: result.name || '整合包导入', speed: totalSpeed || data.speed || 0, files: filesMapped || [], stageHistory: data.stageHistory || [], currentFile: data.currentFile || '' });
              } else if (typeof dlManager !== 'undefined') {
                var speedText = '';
                if (totalSpeed > 0) {
                  speedText = totalSpeed > 1024 * 1024 ? ' | ' + (totalSpeed / 1024 / 1024).toFixed(1) + ' MB/s' : ' | ' + (totalSpeed / 1024).toFixed(0) + ' KB/s';
                }
                var u = { progress: pct, status: 'downloading', message: stageText + speedText, stageHistory: data.stageHistory || [], currentFile: data.currentFile || '' };
                if (filesMapped) u.files = filesMapped;
                dlManager.update(taskId, u);
              }
            }
            window.electronAPI.onImportProgress(function (data) {
              var isTerminal = (data.status === 'completed' || data.status === 'failed' || (data.progress || 0) >= 100);
              if (isTerminal) {
                if (_ipThrottleTimer) { clearTimeout(_ipThrottleTimer); _ipThrottleTimer = null; }
                _doImportProgress(data);
                return;
              }
              _ipLastData = data;
              var now = Date.now();
              if (now - _ipLastTime >= IP_THROTTLE_MS) {
                _ipLastTime = now;
                _doImportProgress(data);
              } else {
                if (!_ipThrottleTimer) {
                  _ipThrottleTimer = setTimeout(function () {
                    _ipThrottleTimer = null;
                    _ipLastTime = Date.now();
                    if (_ipLastData) _doImportProgress(_ipLastData);
                  }, IP_THROTTLE_MS);
                }
              }
            });
          }
          if (!_useVIsland) showToast('正在导入整合包...', 'info');
          const importResult = await window.electronAPI.importModpack(filePath, customName);
          if (importResult && importResult.success) {
            if (_useVIsland) {
              DynamicIsland.update({ progress: 100, status: 'completed', message: '导入完成' });
            } else if (typeof dlManager !== 'undefined') {
              dlManager.update(taskId, { status: 'completed', progress: 100, message: '导入完成' });
            }
            if (!_useVIsland) showToast(`整合包 "${importResult.name || '未知'}" 导入成功！`, 'success');
          } else {
            var errMsg = importResult?.error || '未知错误';
            if (_useVIsland) {
              DynamicIsland.update({ status: 'failed', message: errMsg });
            } else if (typeof dlManager !== 'undefined') {
              dlManager.update(taskId, { status: 'failed', progress: 100, message: errMsg, stageHistory: importResult?.stageHistory || [] });
            }
            if (!_useVIsland) showToast(`导入失败: ${errMsg}`, 'error');
          }
        } finally {
          window._modpackImporting = false;
        }
      });
    }
  } catch (e) {
    showToast('导入失败: ' + (e.message || ''), 'error');
  }
}

const __resIsTauri = !!(window.__TAURI__ || window.__TAURI_INTERNALS__);
// Tauri 环境下由 modpack-import.js 的 tauri://drag-drop 事件处理拖拽导入，
// 这里不能 preventDefault 默认拖拽行为，否则会阻止 Tauri 的拖拽事件触发。
document.addEventListener('dragover', (e) => {
  if (__resIsTauri) return;
  e.preventDefault(); e.stopPropagation();
});
document.addEventListener('drop', (e) => {
  e.preventDefault();
  e.stopPropagation();
  if (__resIsTauri) return; // Tauri 环境交给 modpack-import.js 处理
  if (window._modpackImporting) return;
  const files = e.dataTransfer?.files;
  if (!files || files.length === 0) return;
  const file = files[0];
  const name = (file.name || '').toLowerCase();
  const isModpackFile = name.endsWith('.mrpack') || name.endsWith('.cursemodpack') || name.endsWith('.zip');
  if (isModpackFile) {
    let filePath = file.path;
    if (!filePath && window.electronAPI?.getDroppedFilePath) {
      filePath = window.electronAPI.getDroppedFilePath(file);
    }
    if (filePath) {
      // 用文件名（去掉扩展名）作为默认版本名
      const fileBaseName = (file.name || '').replace(/\.(mrpack|zip|cursemodpack)$/i, '');
      showImportNameModal(fileBaseName, function(customName) {
        window._modpackImporting = true;
        var _vi = typeof DynamicIsland !== 'undefined' && DynamicIsland.isEnabled();
        var sessionId = 'local-modpack-' + Date.now();
        var taskId = 'modpack-' + sessionId;
        if (_vi) {
          DynamicIsland.show(name || '整合包导入');
        } else if (typeof dlManager !== 'undefined') {
          dlManager.add(taskId, name || '整合包导入', 'modpack', sessionId, '');
        }
        if (window.electronAPI?.onImportProgress) {
          if (window.electronAPI.removeImportProgressListener) window.electronAPI.removeImportProgressListener();
          // 节流控制：避免高频进度回调打爆主线程
          var _progThrottleTimer = null;
          var _progLastData = null;
          var _progLastTime = 0;
          var PROG_THROTTLE_MS = 250;
          function _doHandleProgress(data) {
            var stageText = getImportStageText(data.message);
            var pct = data.progress || 0;
            if (_vi) {
              var filesMapped = data.files ? data.files.map(function (f) { return { name: f.name || f.filename || f.n || '', status: f.status || f.s || 'pending', progress: f.progress || f.p || 0, speed: f.speed || f.sp || 0 }; }) : [];
              DynamicIsland.update({ progress: pct, status: 'downloading', message: stageText, name: name || '整合包导入', speed: data.speed || 0, files: filesMapped, stageHistory: data.stageHistory || [], currentFile: data.currentFile || '' });
            } else if (typeof dlManager !== 'undefined') {
              dlManager.update(taskId, { progress: pct, status: 'downloading', message: stageText, stageHistory: data.stageHistory || [], currentFile: data.currentFile || '' });
            }
          }
          window.electronAPI.onImportProgress(function (data) {
            var isTerminal = (data.status === 'completed' || data.status === 'failed' || (data.progress || 0) >= 100);
            if (isTerminal) {
              if (_progThrottleTimer) { clearTimeout(_progThrottleTimer); _progThrottleTimer = null; }
              _doHandleProgress(data);
              return;
            }
            _progLastData = data;
            var now = Date.now();
            if (now - _progLastTime >= PROG_THROTTLE_MS) {
              _progLastTime = now;
              _doHandleProgress(data);
            } else {
              if (!_progThrottleTimer) {
                _progThrottleTimer = setTimeout(function () {
                  _progThrottleTimer = null;
                  _progLastTime = Date.now();
                  if (_progLastData) _doHandleProgress(_progLastData);
                }, PROG_THROTTLE_MS);
              }
            }
          });
        }
        if (!_vi) showToast('正在导入整合包...', 'info');
        window.electronAPI.importModpack(filePath, customName).then(result => {
          window._modpackImporting = false;
          if (result && result.success) {
            if (_vi) { DynamicIsland.update({ progress: 100, status: 'completed', message: '导入完成' }); }
            else if (typeof dlManager !== 'undefined') { dlManager.update(taskId, { status: 'completed', progress: 100, message: '导入完成' }); }
            if (!_vi) showToast(`整合包 "${result.name || '未知'}" 导入成功！`, 'success');
          } else {
            var errMsg = result?.error || '未知错误';
            if (_vi) { DynamicIsland.update({ status: 'failed', message: errMsg }); }
            else if (typeof dlManager !== 'undefined') { dlManager.update(taskId, { status: 'error', message: errMsg }); }
            if (!_vi) showToast(`导入失败: ${errMsg}`, 'error');
          }
        }).catch(err => {
          window._modpackImporting = false;
          var catchMsg = err.message || '';
          if (_vi) { DynamicIsland.update({ status: 'failed', message: catchMsg }); }
          else if (typeof dlManager !== 'undefined') { dlManager.update(taskId, { status: 'error', message: catchMsg }); }
          if (!_vi) showToast('导入失败: ' + catchMsg, 'error');
        });
      });
    }
  }
});

function loadResourcePage(type) {
  const state = resourceState[type];
  state.offset = 0;
  state.query = '';
  loadResourceList(type);
  setupResourceEvents(type);
}

function setupResourceEvents(type) {
  const searchInput = document.getElementById(`${type === 'resourcepack' ? 'resourcepack' : type === 'shader' ? 'shader' : type === 'datapack' ? 'datapack' : 'modpack'}-search-input`);
  const searchBtn = document.getElementById(`${type === 'resourcepack' ? 'resourcepack' : type === 'shader' ? 'shader' : type === 'datapack' ? 'datapack' : 'modpack'}-search-btn`);
  const prevBtn = document.getElementById(`${type === 'resourcepack' ? 'resourcepack' : type === 'shader' ? 'shader' : type === 'datapack' ? 'datapack' : 'modpack'}-prev-btn`);
  const nextBtn = document.getElementById(`${type === 'resourcepack' ? 'resourcepack' : type === 'shader' ? 'shader' : type === 'datapack' ? 'datapack' : 'modpack'}-next-btn`);

  const prefix = type === 'resourcepack' ? 'resourcepack' : type === 'shader' ? 'shader' : type === 'datapack' ? 'datapack' : 'modpack';

  if (searchBtn && !searchBtn._bound) {
    searchBtn._bound = true;
    searchBtn.addEventListener('click', () => {
      resourceState[type].query = searchInput.value.trim();
      resourceState[type].offset = 0;
      loadResourceList(type);
    });
  }
  if (searchInput && !searchInput._bound) {
    searchInput._bound = true;
    searchInput.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' && !e.isComposing) {
        resourceState[type].query = searchInput.value.trim();
        resourceState[type].offset = 0;
        loadResourceList(type);
      }
    });
  }
  if (prevBtn && !prevBtn._bound) {
    prevBtn._bound = true;
    prevBtn.addEventListener('click', () => {
      if (resourceState[type].offset >= 15) {
        resourceState[type].offset -= 15;
        loadResourceList(type);
      }
    });
  }
  if (nextBtn && !nextBtn._bound) {
    nextBtn._bound = true;
    nextBtn.addEventListener('click', () => {
      resourceState[type].offset += 15;
      loadResourceList(type);
    });
  }

  const loaderInstance = customSelectInstances[`${prefix}-filter-loader`];
  const versionInstance = customSelectInstances[`${prefix}-filter-version`];
  if (loaderInstance && !loaderInstance._resourceBound) {
    loaderInstance._resourceBound = true;
    loaderInstance.onChange = () => {
      resourceState[type].offset = 0;
      loadResourceList(type);
    };
  }
  if (versionInstance && !versionInstance._resourceBound) {
    versionInstance._resourceBound = true;
    const origOnChange = versionInstance.onChange;
    versionInstance.onChange = () => {
      if (origOnChange) origOnChange();
      resourceState[type].offset = 0;
      loadResourceList(type);
    };
  }
  const sourceInstance = customSelectInstances[`${prefix}-filter-source`];
  if (sourceInstance && !sourceInstance._resourceBound) {
    sourceInstance._resourceBound = true;
    sourceInstance.onChange = () => {
      resourceState[type].offset = 0;
      loadResourceList(type);
    };
  }
  if (type === 'resourcepack') {
    const resolutionInstance = customSelectInstances['resourcepack-filter-resolution'];
    if (resolutionInstance && !resolutionInstance._resourceBound) {
      resolutionInstance._resourceBound = true;
      resolutionInstance.onChange = () => {
        resourceState[type].offset = 0;
        loadResourceList(type);
      };
    }
  }
}

async function loadResourceList(type) {
  const prefix = type === 'resourcepack' ? 'resourcepack' : type === 'shader' ? 'shader' : type === 'datapack' ? 'datapack' : 'modpack';
  const container = document.getElementById(`${prefix}-browse-list`);
  if (!container) return;

  // 递增请求 ID，用于 await 后校验是否为最新请求
  const myReqId = ++_resourceReqId[type];

  container.innerHTML = `<div class="loading-spinner"><div class="spinner"></div><p>正在获取${typeNames[type] || '资源'}列表...</p></div>`;

  const state = resourceState[type];
  const loader = getCustomSelectValue(`${prefix}-filter-loader`);
  const version = getCustomSelectValue(`${prefix}-filter-version`);
  const resolution = type === 'resourcepack' ? getCustomSelectValue('resourcepack-filter-resolution') : '';
  const source = getCustomSelectValue(`${prefix}-filter-source`) || '';

  try {
    const data = await API.searchResources(state.query, type, loader, version, resolution, 'downloads', 15, state.offset, source);

    // 竞态校验：如果不是最新请求，丢弃结果（避免旧请求覆盖新请求）
    if (_resourceReqId[type] !== myReqId) return;

    const hits = data.hits || [];
    state.total = data.total || 0;
    hits.forEach(item => _projectDataCache.set(item.id, item));

    if (hits.length === 0) {
      if (state.query) {
        container.innerHTML = `<p class="empty-text">暂无匹配的${typeNames[type]}</p><p class="empty-hint">试试其他关键词吧</p>`;
      } else {
        container.innerHTML = `<p class="empty-text">暂无${typeNames[type]}</p>`;
      }
    } else {
      container.innerHTML = hits.map(item => `
        <div class="mod-item mod-item-clickable" onclick="openResourceDetail('${item.id}', '${type}', '${item.source || 'modrinth'}')" onmouseenter="preloadResourceVersions('${item.id}', '${item.source || 'modrinth'}')">
          ${item.icon ? `<div class="mod-icon"><img src="${item.icon}" alt="" onerror="this.parentElement.remove()"></div>` : ''}
          <div class="mod-info">
            <div class="mod-name">${escapeHtml(formatModNameWithChinese(item.slug || item.id, item.title))}
              ${(source === 'any' || !source) ? `<span style="font-size:10px;padding:1px 5px;border-radius:3px;margin-left:6px;background:${item.source === 'curseforge' ? '#f1643620;color:#f16436;border:1px solid #f1643630' : '#4caf5020;color:#4caf50;border:1px solid #4caf5030'};font-weight:500;vertical-align:middle">${item.source === 'curseforge' ? 'CF' : 'MR'}</span>` : ''}
            </div>
            <div class="mod-desc">${escapeHtml(item.description)}</div>
            <div class="mod-meta">
              <span>⬇ ${formatNumber(item.downloads)}</span>
              <span>❤ ${escapeHtml(item.author)}</span>
              <span>${(item.categories || []).slice(0, 3).join(', ')}</span>
            </div>
          </div>
          <div class="mod-actions" onclick="event.stopPropagation()">
            <button class="btn btn-primary btn-sm" onclick="event.stopPropagation();openResourceDetail('${item.id}', '${type}')">安装</button>
          </div>
        </div>
      `).join('');
    }

    const pageInfo = document.getElementById(`${prefix}-page-info`);
    const totalPages = Math.max(1, Math.ceil(state.total / 15));
    const currentPage = Math.floor(state.offset / 15) + 1;
    if (pageInfo) pageInfo.textContent = `${currentPage}/${totalPages}`;
  } catch (e) {
    // 竞态校验：旧请求报错不覆盖新请求
    if (_resourceReqId[type] !== myReqId) return;
    container.innerHTML = `<p class="empty-text">加载失败</p><button class="btn btn-secondary btn-sm" onclick="loadResourceList('${type}')" style="margin-top:8px">重试</button>`;
  }
}

// 资源专用版本预加载：与详情页使用同一接口（getResourceVersions），避免缓存数据不一致
function preloadResourceVersions(projectId, source) {
  if (_versionPreloadCache.has(projectId) || _versionPreloadInFlight.has(projectId)) return;
  _versionPreloadInFlight.add(projectId);
  API.getResourceVersions(projectId, source || 'modrinth').then(data => {
    _versionPreloadCache.set(projectId, data);
    _versionPreloadInFlight.delete(projectId);
  }).catch(() => { _versionPreloadInFlight.delete(projectId); });
}

async function openResourceDetail(projectId, type, source) {
  pushModDetailHistory();
  currentModDetailId = projectId;
  currentModDetailSource = source || 'modrinth';
  currentModDetailType = type;

  // 资源详情（材质/光影/数据包/整合包）不展示加载器/版本兼容卡片
  const compatSection = document.getElementById('md-compat-section');
  if (compatSection) compatSection.style.display = 'none';

  navigateToPage('mod-detail');
  resetMdDetailView();

  const depsSection = document.getElementById('md-deps-section');
  if (depsSection) depsSection.style.display = 'none';
  if (type !== 'mod' && modMultiSelectMode) {
    modMultiSelectMode = false;
  }
  mdCurrentDeps = [];
  mdDepsResolved = {};
  mdDepsVersionInfo = {};

  const backBtn = document.querySelector('#page-mod-detail .moddetail-page-header .btn-icon');
  if (backBtn) backBtn.setAttribute('onclick', 'goBackFromDetail()');

  const mdName = document.getElementById('md-name');
  const mdDesc = document.getElementById('md-desc');
  const mdIconImg = document.getElementById('md-icon-img');
  const mdIconFallback = document.getElementById('md-icon-fallback');
  const mdVersionList = document.getElementById('md-version-list');
  const mdVersionTabs = document.getElementById('md-version-tabs');

  if (!mdName || !mdVersionList) return;

  // 立即清空旧内容，防止切换整合包时短暂显示上一个整合包的版本列表
  mdVersionList.innerHTML = '';
  if (mdVersionTabs) mdVersionTabs.innerHTML = '';

  // 竞态保护：记录本次请求的 ID，API 返回时检查是否仍是最新的
  const _reqId = projectId;

  const typeNames = { mod: '模组', modpack: '整合包', resourcepack: '材质包', shader: '光影包', datapack: '数据包' };
  const typeIcons = { mod: '🧩', modpack: '📦', resourcepack: '🎨', shader: '✨', datapack: '📊' };

  const cachedRaw = _projectDataCache.get(projectId);
  const cached = (cachedRaw && typeof cachedRaw.body === 'string') ? cachedRaw : null;
  if (cached) {
    currentModDetailData = cached;
    mdName.textContent = formatModNameWithChinese(cached.slug || cached.id, cached.title || typeNames[type] || '未知');
    if (mdDesc) mdDesc.textContent = (cached.description || '').substring(0, 200);
    if (cached.icon && mdIconImg && mdIconFallback) { mdIconImg.style.display = ''; mdIconFallback.style.display = 'none'; setLocalImage(mdIconImg, cached.icon, source); }
    const mdDownloads = document.getElementById('md-downloads');
    const mdFollowers = document.getElementById('md-followers');
    if (mdDownloads) mdDownloads.textContent = `⬇ ${formatNumber(cached.downloads || 0)}`;
    if (mdFollowers) mdFollowers.textContent = `❤ ${formatNumber(cached.followers || 0)}`;
    const srcBadge = document.getElementById('md-source-badge');
    if (srcBadge) { srcBadge.textContent = typeNames[type] || type; srcBadge.style.color = '#f59e0b'; srcBadge.style.background = 'rgba(245,158,11,0.12)'; }
    renderModDescription(cached);
    renderModGallery(cached);
    renderMdSidebar(cached);
  } else {
    mdName.textContent = '加载中...';
  }

  const _hasPreloaded = _versionPreloadCache.has(projectId);
  let _resLoadingTimer = null;
  if (!_hasPreloaded) {
    _resLoadingTimer = setTimeout(() => {
      if (mdVersionList && !mdVersionList.querySelector('.mdv-group')) {
        mdVersionList.innerHTML = '<p class="empty-text" style="padding:30px 0;text-align:center;color:var(--text-muted)">加载版本列表...</p>';
      }
    }, 400);
  }
  if (mdVersionTabs) mdVersionTabs.innerHTML = '';

  try {
    const versionsPromise = _hasPreloaded
      ? Promise.resolve(_versionPreloadCache.get(projectId))
      : API.getResourceVersions(projectId, source || 'modrinth').catch(e => { console.error('[ResDetail] getResourceVersions failed:', e); return null; });
    _versionPreloadCache.delete(projectId);
    const detailPromise = cached ? Promise.resolve(cached) : API.getModDetail(projectId, source || 'modrinth').catch(e => { console.error('[ResDetail] getModDetail failed:', e); return null; });

    const [detail, data] = await Promise.all([detailPromise, versionsPromise]);
    if (_resLoadingTimer) { clearTimeout(_resLoadingTimer); _resLoadingTimer = null; }
    // 竞态保护：如果在等待 API 期间用户已经打开了另一个整合包，丢弃本次结果
    if (currentModDetailId !== _reqId) { return; }
    if (!detail) {
      mdName.textContent = '加载失败';
      mdVersionList.innerHTML = `<p class="empty-text" style="padding:30px 0;text-align:center;color:var(--text-muted)">无法加载详情: API请求失败，请检查网络连接</p>`;
      return;
    }
    if (!cached) {
      _projectDataCache.set(projectId, detail);
      currentModDetailData = detail;
      mdName.textContent = formatModNameWithChinese(detail.slug || detail.id, detail.title || typeNames[type] || '未知');
      if (mdDesc) mdDesc.textContent = (detail.description || '').substring(0, 200);
      if (detail.icon && mdIconImg && mdIconFallback) { mdIconImg.style.display = ''; mdIconFallback.style.display = 'none'; setLocalImage(mdIconImg, detail.icon, source); }
      const mdDownloads = document.getElementById('md-downloads');
      const mdFollowers = document.getElementById('md-followers');
      if (mdDownloads) mdDownloads.textContent = `⬇ ${formatNumber(detail.downloads || 0)}`;
      if (mdFollowers) mdFollowers.textContent = `❤ ${formatNumber(detail.followers || 0)}`;
      const srcBadge = document.getElementById('md-source-badge');
      if (srcBadge) { srcBadge.textContent = typeNames[type] || type; srcBadge.style.color = '#f59e0b'; srcBadge.style.background = 'rgba(245,158,11,0.12)'; }
      renderModDescription(detail);
      renderModGallery(detail);
      renderMdSidebar(detail);
    }

    mdAllVersions = data ? (data.versions || []) : [];
    if (!Array.isArray(mdAllVersions)) mdAllVersions = [];
    console.error('[ResDetail][debug] projectId=', projectId, 'type=', type, 'source=', source, 'versions=', mdAllVersions.length, 'first=', JSON.stringify(mdAllVersions[0]));

    // 详情页优先展示该资源的全部版本，不被列表页筛选条件过滤
    const tabsContainer = document.getElementById('md-version-tabs');
    const gameVersions = new Set();
    mdAllVersions.forEach(v => {
      (v.gameVersions || []).forEach(gv => gameVersions.add(gv));
    });

    let tabsHtml = '<button class="md-vtab active" data-ver="" onclick="switchMdVersionTab(\'\')">全部</button>';
    [...gameVersions].sort().reverse().forEach(gv => {
      // 使用 exact 模式确保精确版本号匹配（如 1.20.1 不会被 _versionToMajor 转为 1.20）
      tabsHtml += `<button class="md-vtab" data-ver="${escapeHtml(gv)}" onclick="switchMdVersionTab('${escapeOnclick(gv)}', 'exact')">${escapeHtml(gv)}</button>`;
    });
    if (tabsContainer) tabsContainer.innerHTML = tabsHtml;

    renderMdVersionList(mdAllVersions);
  } catch (e) {
    mdName.textContent = '加载失败';
    mdVersionList.innerHTML = `<p class="empty-text" style="padding:30px 0;text-align:center;color:var(--text-muted)">无法加载详情: ${e.message || e}</p>`;
  }
}

// 全局变量：当前整合包详情的目标版本
async function quickInstallResource(projectId, type) {
  if (type === 'modpack') {
    // 整合包需要创建新版本，先弹窗让用户自定义版本名
    const defaultName = (typeof currentModDetailData !== 'undefined' && (currentModDetailData?.title || currentModDetailData?.name)) || projectId;
    showImportNameModal(defaultName, async function(customName) {
      showToast('正在下载整合包，将创建为新版本...', 'info');
      try {
        const result = await API.downloadResource('', projectId, type, '', '', customName, currentModDetailSource);
        if (result.success) {
          showModpackInstallModal(result.fileName, result.sessionId);
        } else {
          showToast(result.error || '安装失败', 'error');
        }
      } catch (e) {
        showToast('安装失败', 'error');
      }
    });
  } else {
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
      showToast(`正在安装${typeNames[type]}...`, 'info');
      const result = await API.downloadResource('', projectId, type, '', savePath, '', currentModDetailSource);
      if (result.success) {
        showModDownloadModal(result.fileName, result.sessionId);
      } else {
        showToast(result.error || '安装失败', 'error');
      }
    } catch (e) {
      showToast('安装失败', 'error');
    }
  }
}

// 显示版本选择对话框
async function showVersionSelectDialog() {
  return new Promise((resolve) => {
    const modal = document.createElement('div');
    modal.className = 'modal-overlay';
    modal.style.cssText = 'position:fixed;top:0;left:0;right:0;bottom:0;background:rgba(0,0,0,0.7);display:flex;align-items:center;justify-content:center;z-index:9999;';
    
    modal.innerHTML = `
      <div style="background:var(--bg-secondary,#1a1a2e);border-radius:12px;padding:24px;min-width:320px;max-width:400px;border:1px solid var(--border-color,rgba(255,255,255,0.1));">
        <h3 style="margin:0 0 16px;color:var(--text-primary,#fff);">选择目标版本</h3>
        <p style="margin:0 0 16px;color:var(--text-muted,#aaa);font-size:13px;">整合包将安装到所选版本中</p>
        <select id="version-select-dialog" style="width:100%;padding:10px 12px;background:var(--bg-input,#252540);border:1px solid var(--border-color,rgba(255,255,255,0.15));border-radius:8px;color:var(--text-primary,#fff);font-size:14px;">
          <option value="">加载中...</option>
        </select>
        <div style="display:flex;gap:12px;margin-top:20px;justify-content:flex-end;">
          <button id="version-select-cancel" style="padding:8px 16px;background:transparent;border:1px solid var(--border-color,rgba(255,255,255,0.2));border-radius:6px;color:var(--text-secondary,#ccc);cursor:pointer;">取消</button>
          <button id="version-select-confirm" style="padding:8px 16px;background:var(--accent,#60a5fa);border:none;border-radius:6px;color:#fff;cursor:pointer;font-weight:500;">确认安装</button>
        </div>
      </div>
    `;
    
    document.body.appendChild(modal);
    
    const select = modal.querySelector('#version-select-dialog');
    const cancelBtn = modal.querySelector('#version-select-cancel');
    const confirmBtn = modal.querySelector('#version-select-confirm');
    
    API.getVersions(true).then(data => {
      select.innerHTML = '';
      const installed = (data?.installed || []).filter(v => v.id && v.type !== '(old)');
      if (installed.length === 0) {
        select.innerHTML = '<option value="">没有已安装的版本</option>';
      } else {
        installed.forEach(v => {
          const opt = document.createElement('option');
          opt.value = v.id;
          let label = v.id;
          if (v.isModpack) label += ` [${v.modpackLoader || '整合包'}]`;
          else if (v.isFabric) label += ' [Fabric]';
          else if (v.isForge) label += ' [Forge]';
          else if (v.isNeoForge) label += ' [NeoForge]';
          else if (v.isOptiFine) label += ' [OptiFine]';
          opt.textContent = label;
          select.appendChild(opt);
        });
      }
    }).catch(() => {
      select.innerHTML = '<option value="">加载失败</option>';
    });
    
    const close = (result) => {
      document.body.removeChild(modal);
      resolve(result);
    };
    
    cancelBtn.addEventListener('click', () => close(''));
    confirmBtn.addEventListener('click', () => close(select.value));
    modal.addEventListener('click', (e) => {
      if (e.target === modal) close('');
    });
  });
}
