// options.js — 完整设置页
'use strict';

const $ = (id) => document.getElementById(id);

function send(msg) {
  return new Promise((resolve) => {
    chrome.runtime.sendMessage(msg, (resp) => {
      if (chrome.runtime.lastError) return resolve({ ok: false, error: chrome.runtime.lastError.message });
      resolve(resp || { ok: false, error: '无响应' });
    });
  });
}

function setDot(id, state) {
  $(id).className = 'dot' + (state === 'ok' ? ' ok' : state === 'bad' ? ' bad' : '');
}

async function load() {
  const res = await send({ type: 'getSettings' });
  if (res.ok) {
    $('enabled').checked = !!res.settings.enabled;
    $('useBrowserFolder').checked = !!res.settings.useBrowserFolder;
  }
}

async function save() {
  await send({
    type: 'saveSettings',
    settings: { enabled: $('enabled').checked, useBrowserFolder: $('useBrowserFolder').checked },
  });
}

async function test() {
  setDot('dotHost', '');
  $('txtHost').textContent = '正在检测…';
  setDot('dotLauncher', '');
  $('txtLauncher').textContent = '正在检查…';

  const res = await send({ type: 'testHost' });
  if (res.ok) {
    setDot('dotHost', 'ok');
    $('txtHost').textContent = '原生宿主正常';
    setDot('dotLauncher', 'ok');
    $('txtLauncher').textContent = '启动器已就绪' + (res.version ? ' v' + res.version : '') + (res.pid ? ' (PID ' + res.pid + ')' : '');
  } else {
    const err = res.error || '';
    if (/未注册|not found|Access/i.test(err)) {
      setDot('dotHost', 'bad');
      $('txtHost').textContent = '原生宿主未注册';
      setDot('dotLauncher', '');
      $('txtLauncher').textContent = '需先在启动器里注册';
    } else {
      setDot('dotHost', 'ok');
      $('txtHost').textContent = '原生宿主正常';
      setDot('dotLauncher', 'bad');
      $('txtLauncher').textContent = '启动器未运行';
    }
  }
}


// ─────────────────────────────────────────────
// 启动器位置：一键扫描 / 手动选择 / 直接粘路径
// ─────────────────────────────────────────────

function esc(s) {
  return String(s == null ? '' : s).replace(/[&<>"']/g, (c) => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
  }[c]));
}

async function refreshLauncherPath() {
  const res = await send({ type: 'getLauncherPath' });
  const el = $('txtLauncherPath');
  if (!el) return;
  if (res && res.path) {
    el.textContent = res.path;
    el.classList.add('ok');
  } else {
    el.textContent = '跟随系统注册项';
    el.classList.remove('ok');
  }
}

function renderScanList(result) {
  const box = $('scanBox');
  const list = $('scanList');
  const hint = $('pickHint');
  list.innerHTML = '';

  const candidates = (result && result.candidates) || [];
  if (!candidates.length) {
    box.style.display = 'none';
    hint.style.color = '#ffb3b3';
    hint.textContent = '没扫到，点「手动选择」粘贴路径';
    return;
  }

  box.style.display = 'block';
  hint.style.color = '#7f8ea3';
  hint.textContent = '点一行选用';

  for (const c of candidates) {
    const row = document.createElement('div');
    row.className = 'scan-item';
    const tags = [];
    if (c.recommended) tags.push('<span class="tag rec">推荐</span>');
    if (c.supportsBridge === false) tags.push('<span class="tag warn">无接管模块</span>');
    row.innerHTML = tags.join('') + '<span>' + esc(c.path) + '</span>';
    row.addEventListener('click', () => useLauncherPath(c.path));
    list.appendChild(row);
  }
}

async function scanLauncher() {
  const btn = $('btnScan');
  const hint = $('pickHint');
  btn.disabled = true;
  btn.textContent = '扫描中…';
  hint.style.color = '#7f8ea3';
  hint.textContent = '扫描中…';
  try {
    const res = await send({ type: 'scanLauncher' });
    if (!res || !res.ok) {
      $('scanBox').style.display = 'none';
      hint.style.color = '#ffb3b3';
      hint.textContent = (res && res.error) || '扫描失败';
      return;
    }
    renderScanList(res.result);
  } finally {
    btn.disabled = false;
    btn.textContent = '一键扫描';
  }
}

async function useLauncherPath(path) {
  const hint = $('pickHint');
  const res = await send({ type: 'useLauncher', path });
  if (res && res.ok) {
    hint.style.color = '#86e7a8';
    hint.textContent = '已切换';
    // 直接拿响应里的路径刷新显示，不完全依赖下次读 storage
    const el = $('txtLauncherPath');
    if (el) { el.textContent = res.launcher || path; el.classList.add('ok'); }
    await refreshLauncherPath();
    test();
  } else {
    hint.style.color = '#ffb3b3';
    hint.textContent = (res && res.error) || '设置失败';
  }
}

async function probeAndUse(path) {
  const hint = $('pickHint');
  if (!path || !path.trim()) {
    hint.style.color = '#ffb3b3';
    hint.textContent = '路径不能为空';
    return;
  }
  hint.style.color = '#7f8ea3';
  hint.textContent = '校验中…';
  const ok = await send({ type: 'probeLauncher', path });
  if (!ok || !ok.ok) {
    hint.style.color = '#ffb3b3';
    hint.textContent = (ok && ok.error) || '不是有效的启动器';
    return;
  }
  if (ok.supportsBridge === false) {
    hint.style.color = '#ffb3b3';
    hint.textContent = '这个 exe 没有接管模块';
    return;
  }
  await useLauncherPath(ok.path || path);
}

document.addEventListener('DOMContentLoaded', () => {
  load();
  $('enabled').addEventListener('change', save);
  $('useBrowserFolder').addEventListener('change', save);
  $('btnTest').addEventListener('click', test);
  test();

  refreshLauncherPath();
  $('btnScan').addEventListener('click', scanLauncher);
  $('btnUseManual').addEventListener('click', () => probeAndUse($('manualPath').value));
  $('manualPath').addEventListener('keydown', (e) => { if (e.key === 'Enter') probeAndUse($('manualPath').value); });
  $('btnClearPath').addEventListener('click', async () => {
    await chrome.storage.local.remove('launcherPath');
    $('scanBox').style.display = 'none';
    $('pickHint').textContent = '';
    $('manualPath').value = '';
    await refreshLauncherPath();
  });

  // 手动选择：扩展没有文件系统 API，只能让用户在对话框里选，
  // 浏览器不会把真实路径回传给扩展 —— 于是退回"选目录里的 exe"提示，
  // 并提供可直接粘贴路径的输入框。
  $('btnPick').addEventListener('click', () => {
    const hint = $('pickHint');
    hint.style.color = '#9fb4cc';
    hint.textContent = '把 VersePC2.exe 拖到输入框，或粘贴路径';
    const input = $('manualPath');
    input.focus();
    input.placeholder = '把 VersePC2.exe 拖到这里';
  });

  // 支持把 exe 拖到这个输入框里，自动填路径
  const input = $('manualPath');
  input.addEventListener('dragover', (e) => { e.preventDefault(); input.style.borderColor = '#4f8cff'; });
  input.addEventListener('dragleave', () => { input.style.borderColor = '#2c3644'; });
  input.addEventListener('drop', (e) => {
    e.preventDefault();
    input.style.borderColor = '#2c3644';
    const f = e.dataTransfer && e.dataTransfer.files && e.dataTransfer.files[0];
    if (f && f.path) {
      input.value = f.path;
      probeAndUse(f.path);
    }
  });
});
