/* cloud.js — Verse云盘页交互逻辑
 * 登录：统一UID+密码（官网 /api/cloud/pc-login 签发24h token）
 * 文件操作：cloudapi 中继（overview/redeem/delete）+ Rust 打包/上传/下载
 */
(function () {
  'use strict';

  var BUY_URL = 'https://afdian.com/item/143beddcc5fa11f189925254001e7c00';
  var SKIN_URL = 'https://skin.verselauncher.cn';

  var S = {
    token: localStorage.getItem('verse-cloud-token') || '',
    name: localStorage.getItem('verse-cloud-name') || '',
    uid: localStorage.getItem('verse-cloud-uid') || '',
    ov: null,
    busy: false,
    dialogCb: null
  };

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
  function fmtTime(ts) {
    if (!ts) return '--';
    var d = new Date(ts);
    return d.getFullYear() + '-' + String(d.getMonth() + 1).padStart(2, '0') + '-' + String(d.getDate()).padStart(2, '0');
  }
  function esc(s) {
    return String(s == null ? '' : s).replace(/[&<>"']/g, function (c) {
      return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c];
    });
  }

  /* ---------- 页面显示 ---------- */
  window.onCloudPageShow = function () {
    var logged = !!S.token;
    $('cloud-login-card').hidden = logged;
    $('cloud-main').hidden = !logged;
    $('cloud-buy-btn').hidden = false;
    ['cloud-redeem-btn', 'cloud-refresh-btn', 'cloud-logout-btn'].forEach(function (id) { $(id).hidden = !logged; });
    if (logged) cloudRefresh();
  };

  /* ---------- 登录/退出 ---------- */
  window.cloudUpdateLoginState = function () {
    var ok = $('cloud-uid').value.trim().length >= 4 && $('cloud-password').value.length >= 1;
    $('cloud-login-btn').disabled = !ok;
  };

  window.cloudLogin = async function () {
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
  };

  window.cloudLogout = function () {
    S.token = ''; S.ov = null;
    localStorage.removeItem('verse-cloud-token');
    localStorage.removeItem('verse-cloud-name');
    localStorage.removeItem('verse-cloud-uid');
    onCloudPageShow();
  };

  /* ---------- 概览 ---------- */
  async function relay(method, sub, body) {
    var outer = await cloudInvoke('/api/cloud/relay', { method: method, path: sub, token: S.token, body: body || null });
    return unwrapOuter(outer);
  }

  window.cloudRefresh = async function () {
    if (!S.token) return;
    try {
      var r = await relay('GET', '/overview');
      if (r.status === 401) { cloudLogout(); toast('登录已过期，请重新登录', 'warning'); return; }
      var b = r.body || {};
      if (!b.ok) { toast(b.msg || '获取云盘信息失败', 'error'); return; }
      S.ov = b;
      renderOverview();
    } catch (e) {
      toast('云盘服务无法连接：' + (e.message || e), 'error');
    }
  };

  function renderOverview() {
    var b = S.ov;
    if (!b) return;
    var cap = b.capacityBytes || 0, used = b.usedBytes || 0;
    $('cloud-cap-nums').textContent = fmtSize(used) + ' / ' + fmtSize(cap);
    var pct = cap > 0 ? Math.min(100, (used / cap) * 100) : 0;
    $('cloud-cap-bar').style.width = pct.toFixed(1) + '%';

    // 订单剩余天数
    var days = '';
    (b.orders || []).forEach(function (o) {
      var left = Math.max(0, Math.ceil((o.expiresAt - Date.now()) / 86400000));
      days += '<span class="cloud-order-chip">+' + o.gb + 'G · 剩' + left + '天</span>';
    });
    if (!days) days = '<span class="cloud-order-chip cloud-order-free">免费版 100MB</span>';
    $('cloud-cap-days').innerHTML = days;

    var pool = b.poolRemainingBytes || 0, total = b.poolBytes || 0;
    $('cloud-cap-pool').textContent = '全网剩余可兑换空间 ' + fmtSize(pool) + '（总池 ' + fmtSize(total) + '）';

    renderFiles(b.files || []);
  }

  function renderFiles(files) {
    var box = $('cloud-file-list');
    if (!files.length) {
      box.innerHTML = '<div class="cloud-empty">云盘还是空的，点上方「上传存档」把当前版本的世界备份上来吧</div>';
      return;
    }
    var html = '<table class="cloud-table"><thead><tr><th>文件名</th><th>大小</th><th>上传时间</th><th>操作</th></tr></thead><tbody>';
    files.forEach(function (f) {
      html += '<tr>' +
        '<td class="cloud-fname" title="' + esc(f.name) + '">' + esc(f.name) + '</td>' +
        '<td>' + fmtSize(f.size) + '</td>' +
        '<td>' + fmtTime(f.uploadedAt) + '</td>' +
        '<td class="cloud-fops">' +
        '<button class="btn btn-secondary btn-sm" onclick="cloudDownload(\'' + f.id + '\')">下载</button>' +
        '<button class="btn btn-secondary btn-sm cloud-btn-danger" onclick="cloudDelete(\'' + f.id + '\',\'' + esc(f.name).replace(/'/g, '') + '\')">删除</button>' +
        '</td></tr>';
    });
    html += '</tbody></table>';
    box.innerHTML = html;
  }

  /* ---------- 兑换 ---------- */
  window.cloudRedeemOpen = function () {
    cloudDialog('兑换套餐', '<div class="form-group">' +
      '<label>爱发电订单号</label>' +
      '<input type="text" id="cloud-redeem-input" class="text-input" placeholder="在爱发电App-订单详情里复制订单号" maxlength="64">' +
      '</div><div class="form-hint">购买「Verse云盘资源」后，用下单的爱发电账号登录本云盘，粘贴订单号即可到账（¥3=1G，¥13.88=5G，30天有效）</div>' +
      '<div class="fb-submit-bar"><button class="btn btn-primary" onclick="cloudRedeemSubmit()">确认兑换</button></div>');
    setTimeout(function () { var i = $('cloud-redeem-input'); if (i) i.focus(); }, 50);
  };

  window.cloudRedeemSubmit = async function () {
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
  };

  /* ---------- 上传 ---------- */
  window.cloudUploadFlow = async function () {
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
      html += '<button class="cloud-version-item" onclick="cloudUploadPick(\'' + esc(v.id).replace(/'/g, "\\'") + '\')">' +
        '<span class="cloud-version-name">' + esc(v.id) + '</span>' +
        '<span class="cloud-version-meta">' + (v.worldsCount || (v.hasSaves ? '有存档' : '')) + '</span></button>';
    });
    html += '</div><div class="form-hint">将打包该版本 saves 目录为 zip 上传</div>';
    cloudDialog('选择要上传存档的版本', html);
  };

  window.cloudUploadPick = async function (versionId) {
    if (S.busy) return;
    cloudDialogClose();
    S.busy = true;
    showProgress(true, '正在打包存档...');
    try {
      var packed = unwrapOuter(await cloudInvoke('/api/cloud/pack-saves', { versionId: versionId }));
      if (packed.status !== 200 || !packed.body || !packed.body.path) {
        throw new Error((packed.body && (packed.body.error || packed.body.msg)) || '打包失败');
      }
      var stamp = new Date().toISOString().slice(0, 10).replace(/-/g, '');
      var name = packed.body.versionId + '-存档-' + stamp + '.zip';
      showProgress(true, '上传中 0%');
      var stop = listenProgress(function (p) {
        if (p && p.stage === 'upload') showProgress(true, (p.message || '上传中') + (p.percent != null ? ' ' + Math.round(p.percent) + '%' : ''));
      });
      try {
        var up = unwrapOuter(await cloudInvoke('/api/cloud/upload', { path: packed.body.path, name: name, token: S.token }));
        if (up.status !== 200 || !up.body || !up.body.ok) {
          throw new Error((up.body && (up.body.msg || up.body.error)) || '上传失败');
        }
        toast('上传成功：' + name, 'success');
      } finally { stop(); }
      cloudRefresh();
    } catch (e) {
      toast('上传失败：' + (e.message || e), 'error');
    } finally {
      S.busy = false;
      showProgress(false);
    }
  };

  /* ---------- 下载 ---------- */
  window.cloudDownload = async function (fileId) {
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
      html += '<button class="cloud-version-item" onclick="cloudDownloadPick(\'' + fileId + '\',\'' + esc(v.id).replace(/'/g, "\\'") + '\')">' +
        '<span class="cloud-version-name">' + esc(v.id) + '</span>' +
        '<span class="cloud-version-meta">' + (v.hasSaves ? '已有存档' : '无存档') + '</span></button>';
    });
    html += '</div><div class="form-hint cloud-warn">存档将解压还原到所选版本的 saves 目录，同名世界会被覆盖</div>';
    cloudDialog('把云盘存档还原到哪个版本？', html);
    S._dlFileId = fileId;
  };

  window.cloudDownloadPick = async function (fileId, versionId) {
    if (S.busy) return;
    cloudDialogClose();
    var f = (S.ov && S.ov.files || []).find(function (x) { return x.id === fileId; });
    S.busy = true;
    showProgress(true, '下载中 0%');
    var stop = listenProgress(function (p) {
      if (p && (p.stage === 'download' || p.stage === 'extract' || p.stage === 'done')) showProgress(true, (p.message || '') + (p.stage === 'download' && p.percent != null ? ' ' + Math.round(p.percent) + '%' : ''));
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
      showProgress(false);
    }
  };

  /* ---------- 删除 ---------- */
  window.cloudDelete = async function (fileId, name) {
    if (!confirm('确定删除云盘文件「' + (name || fileId) + '」？删除后不可恢复')) return;
    try {
      var r = await relay('POST', '/files/' + fileId + '/delete', {});
      if (r.body && r.body.ok) { toast('已删除', 'success'); cloudRefresh(); }
      else toast((r.body && r.body.msg) || '删除失败', 'error');
    } catch (e) {
      toast('删除失败：' + (e.message || e), 'error');
    }
  };

  /* ---------- 外链与弹层 ---------- */
  window.cloudBuy = function () {
    var api = window.electronAPI;
    if (api && typeof api.openExternal === 'function') { api.openExternal(BUY_URL).catch(function () {}); return; }
    window.open(BUY_URL, '_blank');
  };
  window.cloudOpenSkin = function () {
    var api = window.electronAPI;
    if (api && typeof api.openExternal === 'function') { api.openExternal(SKIN_URL).catch(function () {}); return; }
    window.open(SKIN_URL, '_blank');
  };

  function cloudDialog(title, bodyHtml) {
    $('cloud-dialog-title').textContent = title;
    $('cloud-dialog-body').innerHTML = bodyHtml;
    $('cloud-overlay').hidden = false;
  }
  window.cloudDialogClose = function () { $('cloud-overlay').hidden = true; };

  function showProgress(show, label) {
    var box = $('cloud-progress');
    box.hidden = !show;
    if (label != null) $('cloud-progress-label').textContent = label;
    if (!show) $('cloud-progress-bar').style.width = '0%';
  }
})();
