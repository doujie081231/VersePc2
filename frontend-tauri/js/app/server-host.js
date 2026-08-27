/**
 * js/app/server-host.js - 开服页业务逻辑（实验性）
 * - 稳定进度条（可见态 class，不被 list 刷新冲掉）
 * - 主题化输入
 * - 模组端创建 + 一键同步模组
 */

let _shUnsubLog = null;
let _shUnsubStatus = null;
let _shActiveId = null;
let _shServers = [];
let _shVersions = [];
let _shProgressVisible = false;
let _shProgressValue = 0;
let _shProgressText = '';
let _shProgressIndeterminate = false;
let _shListRefreshTimer = null;
let _shCreating = false;
const SH_CONSOLE_MAX = 500;

function escapeHtmlText(s) {
  return String(s)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

function escapeHtmlAttr(s) {
  return escapeHtmlText(s).replace(/'/g, '&#39;');
}

function _shApi() {
  return window.electronAPI && window.electronAPI.serverHost;
}

function _shSetStatusText(text) {
  const el = document.getElementById('server-host-status');
  if (el) el.textContent = text || '';
}

function _shShowProgress(visible, { progress, text, indeterminate } = {}) {
  const wrap = document.getElementById('server-host-progress');
  const bar = document.getElementById('server-host-progress-bar');
  const meta = document.getElementById('server-host-progress-meta');
  const textEl = document.getElementById('server-host-progress-text');
  const pctEl = document.getElementById('server-host-progress-pct');
  if (!wrap || !bar) return;

  if (typeof progress === 'number' && !Number.isNaN(progress)) {
    _shProgressValue = Math.min(100, Math.max(0, progress));
  }
  if (typeof text === 'string') _shProgressText = text;
  if (typeof indeterminate === 'boolean') _shProgressIndeterminate = indeterminate;
  if (typeof visible === 'boolean') _shProgressVisible = visible;

  // downloading / installing / syncing 始终保持可见，直到 ready/error
  if (_shProgressVisible) {
    wrap.classList.add('is-visible');
    wrap.setAttribute('aria-hidden', 'false');
    if (meta) meta.classList.add('is-active');
  } else {
    wrap.classList.remove('is-visible');
    wrap.setAttribute('aria-hidden', 'true');
    if (meta) meta.classList.remove('is-active');
  }

  if (_shProgressIndeterminate) {
    wrap.classList.add('is-indeterminate');
    bar.style.width = '35%';
    if (pctEl) pctEl.textContent = '';
  } else {
    wrap.classList.remove('is-indeterminate');
    bar.style.width = _shProgressValue + '%';
    if (pctEl) pctEl.textContent = _shProgressVisible ? (Math.round(_shProgressValue) + '%') : '';
  }
  if (textEl) textEl.textContent = _shProgressVisible ? (_shProgressText || '') : '';
}

function _shStatusBadge(status) {
  const map = {
    running: { t: '运行中', c: '#22c55e' },
    starting: { t: '启动中', c: '#eab308' },
    downloading: { t: '下载中', c: '#3b82f6' },
    installing: { t: '安装中', c: '#8b5cf6' },
    'syncing-mods': { t: '同步模组', c: '#06b6d4' },
    stopping: { t: '关闭中', c: '#f97316' },
    ready: { t: '已就绪', c: '#8b5cf6' },
    stopped: { t: '已停止', c: 'var(--text-muted)' },
    error: { t: '错误', c: '#ef4444' }
  };
  const m = map[status] || map.stopped;
  return `<span class="sh-badge" style="color:${m.c}"><span class="sh-badge-dot" style="background:${m.c};box-shadow:0 0 0 3px ${m.c}33"></span>${m.t}</span>`;
}

function _shLoaderChip(loader, ver) {
  if (!loader || loader === 'vanilla') return `<span class="sh-chip vanilla">原版</span>`;
  const cls = loader === 'forge' ? 'forge' : loader === 'neoforge' ? 'neoforge' : loader === 'fabric' ? 'fabric' : '';
  const label = loader === 'neoforge' ? 'NeoForge' : loader.charAt(0).toUpperCase() + loader.slice(1);
  return `<span class="sh-chip ${cls}">${label}${ver ? ' ' + escapeHtmlText(ver) : ''}</span>`;
}

async function initServerHostPage() {
  // 确保自定义下拉框已初始化（Vue 挂载后 DOM 才存在）
  if (typeof initCustomSelects === 'function') initCustomSelects();
  _shBindEvents();
  await serverHostRefreshVersions();
  await serverHostRefreshList();
  _shAttachListeners();
  _shShowProgress(false);
}

function _shBindEvents() {
  const cmdInput = document.getElementById('server-host-cmd');
  if (cmdInput && !cmdInput._bound) {
    cmdInput._bound = true;
    cmdInput.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') {
        e.preventDefault();
        serverHostSendCommand();
      }
    });
  }
  const sel = document.getElementById('server-host-version');
  if (sel && !sel._boundLoader) {
    sel._boundLoader = true;
    sel.addEventListener('change', () => serverHostDetectLoader());
  }
}

function _shAttachListeners() {
  const api = _shApi();
  _shDetach();
  if (!api) return;

  if (api.onLog) {
    _shUnsubLog = api.onLog((data) => {
      if (!data) return;
      if (data.id && !_shActiveId) _shActiveId = data.id;
      if (data.id && data.id !== _shActiveId) return;
      _shAppendConsole(data.line, data.stream);
    });
  }
  if (api.onStatus) {
    _shUnsubStatus = api.onStatus((data) => {
      if (!data) return;
      // 进度事件始终处理（创建时可能还没选中）
      if (data.id && (!_shActiveId || data.id === _shActiveId || _shCreating)) {
        if (!_shActiveId) _shActiveId = data.id;
        _shUpdateActiveStatus(data);
      }
      // 列表刷新节流，避免每个 progress tick 重绘导致进度条闪烁
      if (_shListRefreshTimer) clearTimeout(_shListRefreshTimer);
      _shListRefreshTimer = setTimeout(() => {
        _shListRefreshTimer = null;
        serverHostRefreshList(true);
      }, 400);
    });
  }
}

function _shDetach() {
  if (_shUnsubLog) { try { _shUnsubLog(); } catch (_) {} _shUnsubLog = null; }
  if (_shUnsubStatus) { try { _shUnsubStatus(); } catch (_) {} _shUnsubStatus = null; }
  if (_shListRefreshTimer) { clearTimeout(_shListRefreshTimer); _shListRefreshTimer = null; }
}

function _shAppendConsole(line, stream) {
  const consoleEl = document.getElementById('server-host-console');
  if (!consoleEl) return;
  // clear placeholder
  if (consoleEl.children.length === 1 && consoleEl.lastChild.textContent && consoleEl.lastChild.textContent.includes('等待服务器输出')) {
    consoleEl.innerHTML = '';
  }
  let cls = 'sh-console-line';
  if (stream === 'err') cls += ' err';
  else if (stream === 'cmd') cls += ' cmd';
  else if (/\[VersePC\]/.test(line)) cls += ' sys';
  else if (/Done \(|For help/i.test(line)) cls += ' ok';
  const div = document.createElement('div');
  div.className = cls;
  div.textContent = line;
  consoleEl.appendChild(div);
  while (consoleEl.children.length > SH_CONSOLE_MAX) {
    consoleEl.removeChild(consoleEl.firstChild);
  }
  consoleEl.scrollTop = consoleEl.scrollHeight;
}

function _shClearConsole() {
  const consoleEl = document.getElementById('server-host-console');
  if (consoleEl) {
    consoleEl.innerHTML = '<div class="sh-console-line" style="opacity:.6">等待服务器输出…</div>';
  }
}

function _shUpdateActiveStatus(data) {
  const badge = document.getElementById('server-host-active-badge');
  if (badge) badge.innerHTML = _shStatusBadge(data.status || 'stopped');
  const msg = document.getElementById('server-host-active-msg');
  if (msg) {
    let t = data.message || data.status || '';
    if (data.status === 'running' && data.port) {
      const ips = (data.localIps || []).slice(0, 3);
      t = `端口 ${data.port}` + (ips.length ? ` · 局域网 ${ips.map(ip => ip + ':' + data.port).join(', ')}` : '');
    }
    msg.textContent = t;
  }

  const startBtn = document.getElementById('server-host-start-btn');
  const stopBtn = document.getElementById('server-host-stop-btn');
  const busy = ['running', 'starting', 'stopping', 'downloading', 'installing', 'syncing-mods'].includes(data.status);
  if (startBtn) startBtn.disabled = busy && !['ready', 'stopped', 'error'].includes(data.status);
  if (stopBtn) stopBtn.disabled = !(data.status === 'running' || data.status === 'starting');

  // 进度条：下载/安装/同步时保持可见；ready/error/stopped 后延迟隐藏
  if (['downloading', 'installing', 'syncing-mods'].includes(data.status)) {
    _shShowProgress(true, {
      progress: data.progress != null ? data.progress : _shProgressValue,
      text: data.message || data.status,
      indeterminate: data.indeterminate === true || data.progress == null
    });
  } else if (data.status === 'ready') {
    _shShowProgress(true, { progress: 100, text: data.message || '完成', indeterminate: false });
    setTimeout(() => {
      if (!_shCreating) _shShowProgress(false);
    }, 1200);
  } else if (data.status === 'error') {
    _shShowProgress(true, { progress: _shProgressValue, text: data.message || '失败', indeterminate: false });
  } else if (data.status === 'starting' || data.status === 'running') {
    // 启动后可隐藏进度
    if (!_shCreating) _shShowProgress(false);
  }
}

async function serverHostRefreshVersions() {
  if (!customSelectInstances['server-host-version']) {
    if (typeof initCustomSelects === 'function') initCustomSelects();
  }
  const inst = customSelectInstances['server-host-version'];
  if (!inst) return;
  inst.setOptions([{ value: '', text: '加载中...' }]);
  try {
    let installed = (typeof installedVersions !== 'undefined' && Array.isArray(installedVersions))
      ? installedVersions
      : [];
    if ((!installed || installed.length === 0) && typeof API !== 'undefined' && API.getVersions) {
      const data = await API.getVersions(false);
      installed = data.installed || [];
    }
    _shVersions = Array.isArray(installed) ? installed.slice() : [];
    if (_shVersions.length === 0) {
      inst.setOptions([{ value: '', text: '暂无已安装版本' }]);
      _shSetStatusText('没有找到已安装版本，请先在「下载」页安装版本。');
      return;
    }

    const scored = _shVersions.map(v => {
      const id = v.id || v.versionId || v.name || '';
      const type = (v.type || v.loader || '').toLowerCase();
      const pure = /^\d+\.\d+/.test(id) && !/forge|fabric|quilt|neoforge|optifine/i.test(id);
      let tag = '原版';
      if (/neoforge/i.test(id) || type.includes('neoforge')) tag = 'NeoForge';
      else if (/forge/i.test(id) || type.includes('forge')) tag = 'Forge';
      else if (/fabric|quilt/i.test(id) || type.includes('fabric') || type.includes('quilt')) tag = 'Fabric';
      else if (!pure) tag = type || '整合包';
      return { id, tag, pure };
    }).sort((a, b) => (b.pure - a.pure) || a.id.localeCompare(b.id, undefined, { numeric: true }));

    inst.setOptions(scored.map(({ id, tag }) => ({ value: id, text: id + '（' + tag + '）' })));
    _shSetStatusText(`已加载 ${_shVersions.length} 个本地版本。模组端版本将自动识别并安装对应服务端。`);
    await serverHostDetectLoader();
  } catch (e) {
    console.error('[ServerHost] refresh versions failed:', e);
    inst.setOptions([{ value: '', text: '加载失败' }]);
    _shSetStatusText('版本列表加载失败：' + (e.message || e));
  }
}

async function serverHostDetectLoader() {
  const hint = document.getElementById('server-host-loader-hint');
  const versionId = (getCustomSelectValue('server-host-version') || '').trim();
  if (!versionId) return;
  const api = _shApi();
  if (!api || !api.detectLoader) {
    if (hint) hint.textContent = '选择本地版本后将自动识别加载器类型。';
    return;
  }
  try {
    const r = await api.detectLoader({ versionId });
    if (!r || !r.ok) {
      if (hint) hint.textContent = '加载器识别失败：' + (r && r.error || 'unknown');
      return;
    }
    const map = { vanilla: '原版', forge: 'Forge', fabric: 'Fabric', neoforge: 'NeoForge' };
    const name = map[r.loader] || r.loader;
    if (hint) {
      hint.innerHTML = `识别结果：<strong>${escapeHtmlText(name)}</strong>`
        + (r.loaderVersion ? ` ${escapeHtmlText(r.loaderVersion)}` : '')
        + (r.mcVersion ? ` · MC ${escapeHtmlText(r.mcVersion)}` : '')
        + (r.loader === 'vanilla' ? ' · 将下载官方 server.jar' : ' · 将安装模组服务端并同步模组');
    }
  } catch (e) {
    if (hint) hint.textContent = '加载器识别异常：' + e.message;
  }
}

async function serverHostRefreshList(silent) {
  const api = _shApi();
  const listEl = document.getElementById('server-host-list');
  if (!listEl) return;
  if (!api) {
    listEl.innerHTML = '<p class="form-hint">主进程 serverHost API 不可用（需重启应用）。</p>';
    return;
  }
  try {
    const r = await api.list();
    if (!r || !r.ok) {
      if (!silent) listEl.innerHTML = `<p class="form-hint">加载失败：${escapeHtmlText(r && r.error || 'unknown')}</p>`;
      return;
    }
    _shServers = r.servers || [];
    if (_shServers.length === 0) {
      listEl.innerHTML = '<p class="form-hint" style="margin:0">还没有服务器。填写上方信息后点「创建并准备服务端」。</p>';
      return;
    }
    if (!_shActiveId || !_shServers.some(s => s.id === _shActiveId)) {
      _shActiveId = _shServers[0].id;
    }
    listEl.innerHTML = _shServers.map(s => {
      const active = s.id === _shActiveId;
      return `
        <div class="server-host-card${active ? ' is-active' : ''}" data-id="${escapeHtmlAttr(s.id)}">
          <div style="display:flex;justify-content:space-between;align-items:center;gap:8px">
            <div class="title">${escapeHtmlText(s.name || s.id)}</div>
            ${_shStatusBadge(s.status || (s.running ? 'running' : 'stopped'))}
          </div>
          <div class="form-hint" style="margin-top:6px;display:flex;gap:6px;flex-wrap:wrap;align-items:center">
            ${_shLoaderChip(s.loader || 'vanilla', s.loaderVersion)}
            <span>${escapeHtmlText(s.versionId || '')}</span>
            <span>· 端口 ${s.port || 25565}</span>
            ${s.maxMem ? `<span>· ${s.maxMem}MB</span>` : ''}
          </div>
        </div>`;
    }).join('');

    listEl.querySelectorAll('.server-host-card').forEach(card => {
      card.addEventListener('click', () => {
        _shActiveId = card.getAttribute('data-id');
        _shClearConsole();
        serverHostRefreshList(true);
        serverHostSelectActive();
      });
    });

    // 刷新列表后恢复进度条 DOM 状态（innerHTML 不会动进度条，但确保 class 仍在）
    _shShowProgress(_shProgressVisible, {
      progress: _shProgressValue,
      text: _shProgressText,
      indeterminate: _shProgressIndeterminate
    });
    serverHostSelectActive();
  } catch (e) {
    if (!silent) listEl.innerHTML = `<p class="form-hint">加载失败：${escapeHtmlText(e.message || e)}</p>`;
  }
}

async function serverHostSelectActive() {
  const api = _shApi();
  if (!api || !_shActiveId) return;
  const s = _shServers.find(x => x.id === _shActiveId);
  const title = document.getElementById('server-host-active-title');
  if (title) title.textContent = s ? (s.name || s.id) : '未选择';
  const loaderEl = document.getElementById('server-host-active-loader');
  if (loaderEl) loaderEl.innerHTML = s ? _shLoaderChip(s.loader || 'vanilla', s.loaderVersion) : '';
  try {
    const st = await api.status({ id: _shActiveId });
    if (st && st.ok) {
      // 创建/下载中列表轮询的 status 常为 stopped，避免冲掉进度条与下载态
      if (_shCreating || _shProgressVisible) {
        const badge = document.getElementById('server-host-active-badge');
        // keep progress; only update run-state badge if truly running
        if (st.status === 'running' || st.status === 'starting') {
          _shUpdateActiveStatus({
            id: _shActiveId,
            status: st.status,
            port: st.port,
            localIps: st.localIps,
            message: st.status === 'running' ? '运行中' : '启动中'
          });
        } else if (badge && !_shCreating) {
          // no-op during progress
        }
      } else {
        _shUpdateActiveStatus({
          id: _shActiveId,
          status: st.status,
          port: st.port,
          localIps: st.localIps,
          message: st.status === 'running' ? '运行中' : ''
        });
      }
    }
  } catch (_) {}
  if (s) {
    const portEl = document.getElementById('server-host-port');
    const memEl = document.getElementById('server-host-mem');
    const nameEl = document.getElementById('server-host-name');
    if (portEl && document.activeElement !== portEl) portEl.value = s.port || 25565;
    if (memEl && document.activeElement !== memEl) memEl.value = s.maxMem || 2048;
    if (nameEl && document.activeElement !== nameEl && !nameEl.value) nameEl.value = s.name || 'MyServer';
  }
}

function _shGetCreateInputs() {
  const versionId = (getCustomSelectValue('server-host-version') || '').trim();
  const name = (document.getElementById('server-host-name')?.value || 'MyServer').trim() || 'MyServer';
  const port = parseInt(document.getElementById('server-host-port')?.value || '25565', 10) || 25565;
  const maxMem = parseInt(document.getElementById('server-host-mem')?.value || '2048', 10) || 2048;
  const onlineMode = document.getElementById('server-host-online')?.checked !== false;
  const syncMods = document.getElementById('server-host-sync-mods')?.checked !== false;
  return { versionId, name, port, maxMem, onlineMode, syncMods };
}

async function serverHostCreate() {
  const api = _shApi();
  if (!api) {
    if (typeof showToast === 'function') showToast('请重启应用以加载开服 API', 'error');
    return;
  }
  const opts = _shGetCreateInputs();
  if (!opts.versionId) {
    if (typeof showToast === 'function') showToast('请先选择版本', 'error');
    return;
  }
  _shCreating = true;
  _shSetStatusText('正在创建服务端，请稍候…');
  _shClearConsole();
  _shShowProgress(true, { progress: 2, text: '准备中…', indeterminate: true });
  const btn = document.getElementById('server-host-create-btn');
  if (btn) btn.disabled = true;
  // 显示取消按钮
  const cancelBtn = document.getElementById('server-host-cancel-btn');
  if (cancelBtn) cancelBtn.style.display = '';
  try {
    const r = await api.create(opts);
    if (!r || !r.ok) {
      const msg = (r && r.error) || '创建失败';
      if (typeof showToast === 'function') showToast(msg, 'error');
      _shSetStatusText(msg);
      _shAppendConsole('[错误] ' + msg, 'err');
      _shShowProgress(true, { progress: _shProgressValue, text: msg, indeterminate: false });
      return;
    }
    _shActiveId = r.server.id;
    if (typeof showToast === 'function') showToast('服务端已准备就绪', 'success');
    _shSetStatusText(`已就绪：${r.server.path || r.server.dir || ''}` + (r.modSync ? ' · ' + r.modSync.message : ''));
    _shShowProgress(true, { progress: 100, text: '完成', indeterminate: false });
    await serverHostRefreshList();
    setTimeout(() => _shShowProgress(false), 1500);
  } catch (e) {
    console.error('[ServerHost] create failed:', e);
    if (typeof showToast === 'function') showToast('创建失败: ' + e.message, 'error');
    _shSetStatusText('创建失败: ' + (e.message || e));
  } finally {
    _shCreating = false;
    if (btn) btn.disabled = false;
    if (cancelBtn) cancelBtn.style.display = 'none';
  }
}

/** 取消正在进行的创建任务 */
async function serverHostCancelCreate() {
  const api = _shApi();
  if (!api || !api.cancelCreate) return;
  try {
    await api.cancelCreate();
    _shAppendConsole('[VersePC] 用户已取消创建', 'sys');
    _shSetStatusText('已取消创建');
    _shShowProgress(true, { progress: _shProgressValue, text: '已取消', indeterminate: false });
    if (typeof showToast === 'function') showToast('已取消创建', 'info');
  } catch (e) {
    if (typeof showToast === 'function') showToast('取消失败: ' + e.message, 'error');
  }
}

async function serverHostStart() {
  const api = _shApi();
  if (!api || !_shActiveId) {
    if (typeof showToast === 'function') showToast('请先创建并选择一个服务器', 'error');
    return;
  }
  const port = parseInt(document.getElementById('server-host-port')?.value || '25565', 10) || 25565;
  const maxMem = parseInt(document.getElementById('server-host-mem')?.value || '2048', 10) || 2048;
  _shClearConsole();
  _shAppendConsole('[VersePC] 请求启动服务器 ' + _shActiveId, 'sys');
  try {
    const r = await api.start({ id: _shActiveId, port, maxMem });
    if (!r || !r.ok) {
      const msg = (r && r.error) || '启动失败';
      if (typeof showToast === 'function') showToast(msg, 'error');
      _shAppendConsole('[错误] ' + msg, 'err');
      return;
    }
    if (typeof showToast === 'function') showToast('服务器启动中', 'success');
    _shUpdateActiveStatus({ id: _shActiveId, status: 'starting', port: r.port, localIps: r.localIps });
  } catch (e) {
    if (typeof showToast === 'function') showToast('启动失败: ' + e.message, 'error');
  }
}

async function serverHostStop() {
  const api = _shApi();
  if (!api || !_shActiveId) return;
  try {
    const r = await api.stop({ id: _shActiveId });
    if (!r || !r.ok) {
      if (typeof showToast === 'function') showToast((r && r.error) || '停止失败', 'error');
      return;
    }
    if (typeof showToast === 'function') showToast(r.forced ? '已强制停止' : '已发送 stop', 'success');
  } catch (e) {
    if (typeof showToast === 'function') showToast('停止失败: ' + e.message, 'error');
  }
}

async function serverHostSendCommand() {
  const api = _shApi();
  const input = document.getElementById('server-host-cmd');
  if (!api || !_shActiveId || !input) return;
  const command = (input.value || '').trim();
  if (!command) return;
  try {
    const r = await api.command({ id: _shActiveId, command });
    if (!r || !r.ok) {
      _shAppendConsole('[错误] ' + ((r && r.error) || '发送失败'), 'err');
    } else {
      input.value = '';
    }
  } catch (e) {
    _shAppendConsole('[错误] ' + e.message, 'err');
  }
}

async function serverHostSyncMods() {
  const api = _shApi();
  if (!api || !api.syncMods) {
    if (typeof showToast === 'function') showToast('请重启应用以加载同步模组 API', 'error');
    return;
  }
  if (!_shActiveId) {
    if (typeof showToast === 'function') showToast('请先选择服务器', 'error');
    return;
  }
  _shShowProgress(true, { progress: 0, text: '同步模组…', indeterminate: true });
  try {
    const r = await api.syncMods({ id: _shActiveId });
    if (!r || !r.ok) {
      if (typeof showToast === 'function') showToast((r && r.error) || '同步失败', 'error');
      _shShowProgress(true, { text: (r && r.error) || '同步失败', indeterminate: false });
      return;
    }
    if (typeof showToast === 'function') showToast(r.message || '同步完成', 'success');
    _shSetStatusText(r.message || '同步完成');
    _shShowProgress(true, { progress: 100, text: r.message || '同步完成', indeterminate: false });
    setTimeout(() => _shShowProgress(false), 1200);
  } catch (e) {
    if (typeof showToast === 'function') showToast('同步失败: ' + e.message, 'error');
  }
}

async function serverHostDelete() {
  const api = _shApi();
  if (!api || !_shActiveId) return;
  if (!confirm('确定删除该服务器及其所有文件？此操作不可恢复。')) return;
  try {
    const r = await api.delete({ id: _shActiveId });
    if (!r || !r.ok) {
      if (typeof showToast === 'function') showToast((r && r.error) || '删除失败', 'error');
      return;
    }
    _shActiveId = null;
    _shClearConsole();
    if (typeof showToast === 'function') showToast('已删除', 'success');
    await serverHostRefreshList();
  } catch (e) {
    if (typeof showToast === 'function') showToast('删除失败: ' + e.message, 'error');
  }
}

async function serverHostOpenFolder() {
  const api = _shApi();
  if (!api) {
    if (typeof showToast === 'function') showToast('API 不可用，请重启应用', 'error');
    return;
  }
  try {
    const r = await api.openDir({ id: _shActiveId || undefined });
    if (!(r && r.ok) && typeof showToast === 'function') {
      showToast(r?.error || '打开目录失败', 'error');
    }
  } catch (e) {
    if (typeof showToast === 'function') showToast('打开失败: ' + e.message, 'error');
  }
}

window.initServerHostPage = initServerHostPage;
window.serverHostRefreshVersions = serverHostRefreshVersions;
window.serverHostRefreshList = serverHostRefreshList;
window.serverHostCreate = serverHostCreate;
window.serverHostStart = serverHostStart;
window.serverHostStop = serverHostStop;
window.serverHostSendCommand = serverHostSendCommand;
window.serverHostDelete = serverHostDelete;
window.serverHostOpenFolder = serverHostOpenFolder;
window.serverHostClearConsole = _shClearConsole;
window.serverHostSyncMods = serverHostSyncMods;
window.serverHostDetectLoader = serverHostDetectLoader;
window._shDetach = _shDetach;
window._shShowProgress = _shShowProgress;
