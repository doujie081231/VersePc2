import { bridge } from '../../core/tauri-bridge.js';

import { apiGet } from '../../core/api.js';
import { showToast } from './ui-components.js';

import { actionAttrs, registerActions } from '../../core/actions.js';

/* cloud.js — Verse云盘页交互逻辑
 * 登录：统一UID+密码（官网 /api/cloud/pc-login 签发24h token）
 * 文件操作：cloudapi 中继（overview/redeem/delete）+ Rust 打包/上传/下载
 */
var BUY_URL = 'https://afdian.com/item/143beddcc5fa11f189925254001e7c00';
var SKIN_URL = 'https://skin.verselauncher.cn';

var S = {
  token: localStorage.getItem('verse-cloud-token') || '',
  name: localStorage.getItem('verse-cloud-name') || '',
  uid: localStorage.getItem('verse-cloud-uid') || '',
  ov: null,
  busy: false,
  dialogCb: null,
  sortKey: 'uploadedAt',
  sortDir: 'desc'
};

var ZIP_ICON = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><path d="M14 2v6h6"/><path d="M10 12h1M10 16h1M13 12h1M13 16h1"/></svg>';

var EMPTY_ICON = '<svg class="cloud-empty-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M17.5 19a4.5 4.5 0 0 0 .5-8.97A6 6 0 0 0 6.2 9.2 4 4 0 0 0 6 17.5"/><path d="M12 12v7"/><path d="M9 15l3-3 3 3"/></svg>';

/* ---------- 基础工具 ---------- */
function $(id) { return document.getElementById(id); }
function toast(msg, type) { if (typeof showToast === 'function') showToast(msg, type || 'info'); else console.log('[cloud]', msg); }

function cloudInvoke(path, body) {
  var core = (window.__TAURI__ && window.__TAURI__.core) || window.__TAURI_INTERNALS__;
  if (!core || !core.invoke) return Promise.reject(new Error('Tauri 桥接不可用'));
  return core.invoke('api_proxy', { method: 'POST', path: path, params: {}, body: body || {} });
}

/* invoke 返回 ApiResult：成功为 {status:200, body:{status:inner, body:j}}，Rust层错误为 {status:err, body:{error}}
 * 统一解包为 {status:inner, body:j} */
function unwrapOuter(r) {
  if (!r) throw new Error('云盘服务无响应');
  if (r.body && typeof r.body.status === 'number' && r.body.body !== undefined) {
    return { status: r.body.status, body: r.body.body || {} };
  }
  return { status: r.status || 500, body: r.body || {} };
}

function listenProgress(cb) {
  try {
    var evt = (window.__TAURI__ && window.__TAURI__.event) || window.__TAURI_INTERNALS__;
    if (evt && evt.listen) return evt.listen('cloud:progress', function (e) { cb(e.payload || {}); });
  } catch (e) { /* ignore */ }
  return function () {};
}

function fmtSize(bytes) {
  if (!bytes && bytes !== 0) return '--';
  if (bytes < 1024) return bytes + ' B';
  if (bytes < 1048576) return (bytes / 1024).toFixed(1) + ' KB';
  if (bytes < 1073741824) return (bytes / 1048576).toFixed(1) + ' MB';
  return (bytes / 1073741824).toFixed(2) + ' GB';
}

function pad2(n) { return String(n).padStart(2, '0'); }

function fmtTime(ts) {
  if (!ts) return '--';
  var d = new Date(ts);
  return d.getFullYear() + '-' + pad2(d.getMonth() + 1) + '-' + pad2(d.getDate()) +
    ' ' + pad2(d.getHours()) + ':' + pad2(d.getMinutes());
}

function esc(s) {
  return String(s == null ? '' : s).replace(/[&<>"']/g, function (c) {
    return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c];
  });
}

/* 云盘剩余可用字节；容量未知时返回 null（不做拦截） */
function freeBytes() {
  var b = S.ov;
  if (!b) return null;
  var cap = b.capacityBytes || 0;
  if (cap <= 0) return null;
  return Math.max(0, cap - (b.usedBytes || 0));
}

/* ---------- 页面显示 ---------- */
function onCloudPageShow() {
  var logged = !!S.token;
  $('cloud-login-card').hidden = logged;
  $('cloud-main').hidden = !logged;
  $('cloud-buy-btn').hidden = false;
  ['cloud-redeem-btn', 'cloud-refresh-btn', 'cloud-logout-btn'].forEach(function (id) { $(id).hidden = !logged; });
  bindOverlayOnce();
  if (logged) cloudRefresh();
}

/* 点击遮罩空白处关闭弹层（只绑一次） */
function bindOverlayOnce() {
  var ov = $('cloud-overlay');
  if (!ov || ov._cloudBound) return;
  ov._cloudBound = true;
  ov.addEventListener('click', function (e) {
    if (e.target === ov) cloudDialogClose();
  });
}

/* ---------- 登录/退出 ---------- */
function cloudUpdateLoginState() {
  var uidEl = $('cloud-uid');
  var pwdEl = $('cloud-password');
  var btn = $('cloud-login-btn');
  if (!uidEl || !pwdEl || !btn) return;
  var ok = uidEl.value.trim().length >= 4 && pwdEl.value.length >= 1;
  btn.disabled = !ok;
}

async function cloudLogin() {
  var uid = $('cloud-uid').value.trim();
  var password = $('cloud-password').value;
  if (!uid || !password) return;
  var btn = $('cloud-login-btn');
  btn.disabled = true;
  $('cloud-login-hint').textContent = '登录中...';
  try {
    var r = await cloudInvoke('/api/cloud/login', { uid: uid, password: password });
    var u = unwrapOuter(r);
    var b = u.body || {};
    if (u.status === 200 && b.ok && b.token) {
      S.token = b.token; S.name = b.name || ''; S.uid = b.uid || uid;
      localStorage.setItem('verse-cloud-token', S.token);
      localStorage.setItem('verse-cloud-name', S.name);
      localStorage.setItem('verse-cloud-uid', S.uid);
      $('cloud-password').value = '';
      $('cloud-login-hint').textContent = '';
      onCloudPageShow();
      toast('登录成功，欢迎 ' + (S.name || S.uid), 'success');
    } else {
      $('cloud-login-hint').textContent = b.msg || b.error || '登录失败，请检查UID和密码';
    }
  } catch (e) {
    $('cloud-login-hint').textContent = '登录失败：' + (e.message || e);
  } finally {
    btn.disabled = false;
    cloudUpdateLoginState();
  }
}

function cloudLogout() {
  S.token = ''; S.ov = null;
  localStorage.removeItem('verse-cloud-token');
  localStorage.removeItem('verse-cloud-name');
  localStorage.removeItem('verse-cloud-uid');
  onCloudPageShow();
}

/* ---------- 概览 ---------- */
async function relay(method, sub, body) {
  var outer = await cloudInvoke('/api/cloud/relay', { method: method, path: sub, token: S.token, body: body || null });
  return unwrapOuter(outer);
}

async function cloudRefresh() {
  if (!S.token) return;
  var btn = $('cloud-refresh-btn');
  var card = document.querySelector('.cloud-cap-card');
  if (btn) btn.disabled = true;
  if (card && !S.ov) card.classList.add('is-loading');
  try {
    var r = await relay('GET', '/overview');
    if (r.status === 401) { cloudLogout(); toast('登录已过期，请重新登录', 'warning'); return; }
    var b = r.body || {};
    if (!b.ok) { toast(b.msg || '获取云盘信息失败', 'error'); return; }
    S.ov = b;
    renderOverview();
  } catch (e) {
    toast('云盘服务无法连接：' + (e.message || e), 'error');
  } finally {
    if (btn) btn.disabled = false;
    if (card) card.classList.remove('is-loading');
  }
}

function renderOverview() {
  var b = S.ov;
  if (!b) return;
  var cap = b.capacityBytes || 0, used = b.usedBytes || 0;
  $('cloud-cap-nums').innerHTML =
    '<span class="cloud-cap-used">' + fmtSize(used) + '</span>' +
    '<span class="cloud-cap-sep">/</span>' +
    '<span class="cloud-cap-total">' + fmtSize(cap) + '</span>';

  var pct = cap > 0 ? Math.min(100, (used / cap) * 100) : 0;
  var bar = $('cloud-cap-bar');
  bar.style.width = pct.toFixed(1) + '%';
  bar.className = 'cloud-cap-bar' + (pct >= 95 ? ' cloud-cap-danger' : pct >= 80 ? ' cloud-cap-warn' : '');
  $('cloud-cap-percent').textContent = cap > 0 ? '已用 ' + pct.toFixed(1) + '%' : '';

  // 订单剩余天数
  var days = '';
  (b.orders || []).forEach(function (o) {
    var msLeft = (o.expiresAt || 0) - Date.now();
    if (msLeft <= 0) {
      days += '<span class="cloud-order-chip cloud-order-expired">+' + o.gb + 'G · 已过期</span>';
    } else {
      days += '<span class="cloud-order-chip">+' + o.gb + 'G · 剩 ' + Math.ceil(msLeft / 86400000) + ' 天</span>';
    }
  });
  if (!days) days = '<span class="cloud-order-chip cloud-order-free">免费版 100MB</span>';
  $('cloud-cap-days').innerHTML = days;

  var pool = b.poolRemainingBytes || 0, total = b.poolBytes || 0;
  $('cloud-cap-pool').textContent = '全网剩余可兑换空间 ' + fmtSize(pool) + '（总池 ' + fmtSize(total) + '）';

  renderFiles(b.files || []);
}

function sortFiles(files) {
  var key = S.sortKey, dir = S.sortDir === 'asc' ? 1 : -1;
  return files.slice().sort(function (a, b) {
    if (key === 'name') {
      return String(a.name || '').localeCompare(String(b.name || ''), 'zh-Hans-CN') * dir;
    }
    return ((a[key] || 0) - (b[key] || 0)) * dir;
  });
}

function sortTh(key, label) {
  var active = S.sortKey === key;
  var mark = active ? (S.sortDir === 'desc' ? ' ↓' : ' ↑') : '';
  return '<th class="cloud-th-sort' + (active ? ' is-active' : '') + '"' + actionAttrs('cloudSort', key) + '>' +
    label + mark + '</th>';
}

function renderFiles(files) {
  var box = $('cloud-file-list');
  var countEl = $('cloud-files-count');
  if (countEl) countEl.textContent = files.length ? files.length + ' 个文件' : '';
  if (!files.length) {
    box.innerHTML = '<div class="cloud-empty">' + EMPTY_ICON +
      '<div class="cloud-empty-title">云盘还是空的</div>' +
      '<div class="cloud-empty-sub">点上方「上传存档」，把当前版本的世界备份上来</div></div>';
    return;
  }
  var list = sortFiles(files);
  var html = '<table class="cloud-table"><thead><tr>' +
    sortTh('name', '文件名') + sortTh('size', '大小') + sortTh('uploadedAt', '上传时间') +
    '<th>操作</th></tr></thead><tbody>';
  list.forEach(function (f) {
    html += '<tr>' +
      '<td class="cloud-fname-cell"><div class="cloud-fname">' +
      '<span class="cloud-fname-icon">' + ZIP_ICON + '</span>' +
      '<span class="cloud-fname-text" title="' + esc(f.name) + '">' + esc(f.name) + '</span></div></td>' +
      '<td class="cloud-fsize">' + fmtSize(f.size) + '</td>' +
      '<td class="cloud-ftime">' + fmtTime(f.uploadedAt) + '</td>' +
      '<td class="cloud-fops">' +
      '<button class="btn btn-secondary btn-sm"' + actionAttrs('cloudDownload', f.id) + '>下载</button>' +
      '<button class="btn btn-secondary btn-sm cloud-btn-danger"' + actionAttrs('cloudDelete', f.id, f.name) + '>删除</button>' +
      '</td></tr>';
  });
  html += '</tbody></table>';
  box.innerHTML = html;
}

/* ---------- 兑换 ---------- */
function cloudRedeemOpen() {
  cloudDialog('兑换套餐', '<div class="form-group">' +
    '<label>爱发电订单号</label>' +
    '<input type="text" id="cloud-redeem-input" class="text-input" placeholder="在爱发电App-订单详情里复制订单号" maxlength="64">' +
    '</div><div class="form-hint">购买「Verse云盘资源」后，用下单的爱发电账号登录本云盘，粘贴订单号即可到账（¥3=1G，¥13.88=5G，30天有效）</div>' +
    '<div class="fb-submit-bar"><button class="btn btn-primary"' + actionAttrs('cloudRedeemSubmit') + '>确认兑换</button></div>');
  setTimeout(function () { var i = $('cloud-redeem-input'); if (i) i.focus(); }, 50);
}

async function cloudRedeemSubmit() {
  var no = ($('cloud-redeem-input') && $('cloud-redeem-input').value.trim()) || '';
  if (!no) { toast('请输入订单号', 'warning'); return; }
  try {
    var r = await relay('POST', '/redeem', { outTradeNo: no });
    var b = r.body || {};
    toast(b.msg || (b.ok ? '兑换成功' : '兑换失败'), b.ok ? 'success' : 'error');
    if (b.ok) { cloudDialogClose(); cloudRefresh(); }
  } catch (e) {
    toast('兑换失败：' + (e.message || e), 'error');
  }
}

/* ---------- 上传 ---------- */
async function cloudUploadFlow() {
  if (S.busy) return;
  var versions;
  try {
    var r = await apiGet('/api/versions', { installed: 'true' });
    versions = (r && (r.installed || r.versions)) || [];
  } catch (e) {
    toast('获取版本列表失败：' + (e.message || e), 'error');
    return;
  }
  var withSaves = versions.filter(function (v) { return v.hasSaves; });
  if (!withSaves.length) { toast('没有找到带存档的已安装版本', 'warning'); return; }
  var html = '<div class="cloud-version-list">';
  withSaves.forEach(function (v) {
    html += '<button class="cloud-version-item"' + actionAttrs('cloudUploadPick', v.id) + '>' +
      '<span class="cloud-version-name">' + esc(v.id) + '</span>' +
      '<span class="cloud-version-meta">' + esc(v.worldsCount || '有存档') + '</span></button>';
  });
  html += '</div><div class="form-hint">会把该版本的 saves 目录打包成 zip 再上传</div>';
  cloudDialog('选择要上传存档的版本', html);
}

async function cloudUploadPick(versionId) {
  if (S.busy) return;
  cloudDialogClose();
  S.busy = true;
  var uploadBtn = $('cloud-upload-btn');
  if (uploadBtn) uploadBtn.disabled = true;
  showProgress(true, '正在打包存档...', null);
  try {
    var packed = unwrapOuter(await cloudInvoke('/api/cloud/pack-saves', { versionId: versionId }));
    if (packed.status !== 200 || !packed.body || !packed.body.path) {
      throw new Error((packed.body && (packed.body.error || packed.body.msg)) || '打包失败');
    }
    var zipSize = packed.body.size || 0;
    var free = freeBytes();
    if (free != null && zipSize > free) {
      throw new Error('存档压缩后 ' + fmtSize(zipSize) + '，超过云盘剩余空间 ' + fmtSize(free) + '，请先删除云盘文件或扩容');
    }
    var stamp = new Date().toISOString().slice(0, 10).replace(/-/g, '');
    var name = packed.body.versionId + '-存档-' + stamp + '.zip';
    showProgress(true, '准备上传...', 0);
    var stop = listenProgress(function (p) {
      if (!p || p.stage !== 'upload') return;
      showProgress(true, p.message || '上传中...', typeof p.percent === 'number' ? p.percent : null);
    });
    try {
      var up = unwrapOuter(await cloudInvoke('/api/cloud/upload', { path: packed.body.path, name: name, token: S.token }));
      if (up.status !== 200 || !up.body || !up.body.ok) {
        throw new Error((up.body && (up.body.msg || up.body.error)) || '上传失败');
      }
      showProgress(true, '上传完成', 100);
      toast('上传成功：' + name, 'success');
    } finally { stop(); }
    cloudRefresh();
  } catch (e) {
    toast('上传失败：' + (e.message || e), 'error');
  } finally {
    S.busy = false;
    if (uploadBtn) uploadBtn.disabled = false;
    hideProgressSoon();
  }
}

/* ---------- 下载 ---------- */
async function cloudDownload(fileId) {
  if (S.busy) return;
  var versions;
  try {
    var r = await apiGet('/api/versions', { installed: 'true' });
    versions = (r && (r.installed || r.versions)) || [];
  } catch (e) {
    toast('获取版本列表失败：' + (e.message || e), 'error');
    return;
  }
  if (!versions.length) { toast('没有已安装版本，请先安装一个版本', 'warning'); return; }
  var html = '<div class="cloud-version-list">';
  versions.forEach(function (v) {
    html += '<button class="cloud-version-item"' + actionAttrs('cloudDownloadPick', fileId, v.id) + '>' +
      '<span class="cloud-version-name">' + esc(v.id) + '</span>' +
      '<span class="cloud-version-meta">' + (v.hasSaves ? '已有存档' : '无存档') + '</span></button>';
  });
  html += '</div><div class="form-hint cloud-warn">存档将解压还原到所选版本的 saves 目录</div>';
  cloudDialog('把云盘存档还原到哪个版本？', html);
}

async function cloudDownloadPick(fileId, versionId) {
  if (S.busy) return;
  var f = (S.ov && S.ov.files || []).find(function (x) { return x.id === fileId; });
  var fname = (f && f.name) || fileId;
  cloudDialog('确认还原存档',
    '<div class="cloud-confirm-text">把 <b>' + esc(fname) + '</b> 还原到版本 <b>' + esc(versionId) + '</b>？</div>' +
    '<div class="cloud-warn">同名世界文件夹会被覆盖，且无法恢复</div>',
    { confirmText: '继续还原', danger: true, onConfirm: function () { cloudDownloadRun(fileId, versionId); } });
}

async function cloudDownloadRun(fileId, versionId) {
  if (S.busy) return;
  var f = (S.ov && S.ov.files || []).find(function (x) { return x.id === fileId; });
  S.busy = true;
  var uploadBtn = $('cloud-upload-btn');
  if (uploadBtn) uploadBtn.disabled = true;
  showProgress(true, '下载中...', 0);
  var stop = listenProgress(function (p) {
    if (!p) return;
    var pct = typeof p.percent === 'number' ? p.percent : null;
    if (p.stage === 'download') showProgress(true, p.message || '下载中...', pct);
    else if (p.stage === 'extract') showProgress(true, p.message || '正在解压...', pct);
    else if (p.stage === 'done') showProgress(true, p.message || '已完成', 100);
  });
  try {
    var r = unwrapOuter(await cloudInvoke('/api/cloud/download', { fileId: fileId, name: (f && f.name) || '', token: S.token, versionId: versionId }));
    if (r.status !== 200) throw new Error((r.body && (r.body.error || r.body.msg)) || '下载失败');
    var worlds = (r.body && r.body.worlds) || [];
    toast('已还原到 ' + versionId + ' 的存档目录' + (worlds.length ? '：' + worlds.join('、') : ''), 'success');
    cloudRefresh();
  } catch (e) {
    toast('下载失败：' + (e.message || e), 'error');
  } finally {
    stop();
    S.busy = false;
    if (uploadBtn) uploadBtn.disabled = false;
    hideProgressSoon();
  }
}

/* ---------- 删除 ---------- */
function cloudDelete(fileId, name) {
  cloudDialog('删除文件',
    '<div class="cloud-confirm-text">确定删除云盘文件 <b>' + esc(name || fileId) + '</b>？</div>' +
    '<div class="cloud-warn">删除后不可恢复</div>',
    { confirmText: '删除', danger: true, onConfirm: function () { cloudDeleteRun(fileId); } });
}

async function cloudDeleteRun(fileId) {
  try {
    var r = await relay('POST', '/files/' + fileId + '/delete', {});
    if (r.body && r.body.ok) { toast('已删除', 'success'); cloudRefresh(); }
    else toast((r.body && r.body.msg) || '删除失败', 'error');
  } catch (e) {
    toast('删除失败：' + (e.message || e), 'error');
  }
}

/* ---------- 外链与弹层 ---------- */
function cloudBuy() {
  var api = bridge;
  if (api && typeof api.openExternal === 'function') { api.openExternal(BUY_URL).catch(function () {}); return; }
  window.open(BUY_URL, '_blank');
}

function cloudOpenSkin() {
  var api = bridge;
  if (api && typeof api.openExternal === 'function') { api.openExternal(SKIN_URL).catch(function () {}); return; }
  window.open(SKIN_URL, '_blank');
}

/* opts: { confirmText, cancelText, danger, onConfirm } */
function cloudDialog(title, bodyHtml, opts) {
  opts = opts || {};
  $('cloud-dialog-title').textContent = title;
  $('cloud-dialog-body').innerHTML = bodyHtml;
  var foot = $('cloud-dialog-foot');
  if (foot) {
    var html = '<button class="btn btn-secondary btn-sm"' + actionAttrs('cloudDialogClose') + '>' +
      (opts.cancelText || '取消') + '</button>';
    if (opts.confirmText) {
      html += '<button class="btn ' + (opts.danger ? 'cloud-btn-danger-solid' : 'btn-primary') + ' btn-sm"' +
        actionAttrs('cloudDialogConfirm') + '>' + opts.confirmText + '</button>';
    }
    foot.innerHTML = html;
  }
  S.dialogCb = typeof opts.onConfirm === 'function' ? opts.onConfirm : null;
  $('cloud-overlay').hidden = false;
  bindOverlayOnce();
}

function cloudDialogClose() {
  $('cloud-overlay').hidden = true;
  S.dialogCb = null;
}

function cloudDialogConfirm() {
  var cb = S.dialogCb;
  cloudDialogClose();
  if (typeof cb === 'function') cb();
}

function cloudSort(key) {
  if (S.sortKey === key) S.sortDir = S.sortDir === 'desc' ? 'asc' : 'desc';
  else { S.sortKey = key; S.sortDir = 'desc'; }
  renderFiles((S.ov && S.ov.files) || []);
}

/* pct 为数字时更新进度条，null 表示只改文案 */
var progressHideTimer = null;

function showProgress(show, label, pct) {
  var box = $('cloud-progress');
  if (!box) return;
  if (show && progressHideTimer) {
    clearTimeout(progressHideTimer);
    progressHideTimer = null;
  }
  box.hidden = !show;
  if (label != null) $('cloud-progress-label').textContent = label;
  if (typeof pct === 'number' && isFinite(pct)) {
    var p = Math.max(0, Math.min(100, pct));
    $('cloud-progress-bar').style.width = p.toFixed(1) + '%';
    $('cloud-progress-pct').textContent = Math.round(p) + '%';
  }
  if (!show) {
    $('cloud-progress-bar').style.width = '0%';
    $('cloud-progress-pct').textContent = '';
  }
}

/* 让 100% 停留一下再收起，同时避免影响下一次进度 */
function hideProgressSoon() {
  if (progressHideTimer) clearTimeout(progressHideTimer);
  progressHideTimer = setTimeout(function () {
    progressHideTimer = null;
    showProgress(false);
  }, 700);
}

document.addEventListener('keydown', function (e) {
  if (e.key !== 'Escape') return;
  var ov = $('cloud-overlay');
  if (ov && !ov.hidden) cloudDialogClose();
});

registerActions({
  cloudDownload: (event, el, fileId) => cloudDownload(fileId),
  cloudDelete: (event, el, fileId, name) => cloudDelete(fileId, name),
  cloudRedeemSubmit,
  cloudUploadPick: (event, el, versionId) => cloudUploadPick(versionId),
  cloudDownloadPick: (event, el, fileId, versionId) => cloudDownloadPick(fileId, versionId),
  cloudSort: (event, el, key) => cloudSort(key),
  cloudDialogClose: () => cloudDialogClose(),
  cloudDialogConfirm: () => cloudDialogConfirm(),
});

export { onCloudPageShow, cloudUpdateLoginState, cloudLogin, cloudLogout, cloudRefresh, cloudRedeemOpen, cloudRedeemSubmit, cloudUploadFlow, cloudUploadPick, cloudDownload, cloudDownloadPick, cloudDelete, cloudBuy, cloudOpenSkin, cloudDialogClose, cloudDialogConfirm, cloudSort };
