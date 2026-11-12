// popup.js — 工具栏弹窗：状态灯、开关、最近一次下载
'use strict';

const $ = (id) => document.getElementById(id);

function setStatus(text, isError) {
  const el = $('status');
  el.textContent = text || '';
  el.className = isError ? 'err' : '';
}

function setDot(id, state) {
  const el = $(id);
  el.className = 'dot' + (state === 'ok' ? ' ok' : state === 'bad' ? ' bad' : '');
}

function send(msg) {
  return new Promise((resolve) => {
    chrome.runtime.sendMessage(msg, (resp) => {
      if (chrome.runtime.lastError) {
        resolve({ ok: false, error: chrome.runtime.lastError.message });
        return;
      }
      resolve(resp || { ok: false, error: '无响应' });
    });
  });
}

async function loadSettings() {
  const res = await send({ type: 'getSettings' });
  if (res.ok) {
    $('enabled').checked = !!res.settings.enabled;
    $('useBrowserFolder').checked = !!res.settings.useBrowserFolder;
  }
}

async function saveSettings() {
  await send({
    type: 'saveSettings',
    settings: {
      enabled: $('enabled').checked,
      useBrowserFolder: $('useBrowserFolder').checked,
    },
  });
  setStatus('已保存');
  setTimeout(() => setStatus(''), 1600);
}

async function testHost() {
  setDot('dotHost', '');
  $('txtHost').textContent = '正在检测原生宿主…';
  setDot('dotLauncher', '');
  $('txtLauncher').textContent = '正在检查启动器…';

  const res = await send({ type: 'testHost' });

  if (res.ok) {
    // ping 成功 = 宿主正常 + 启动器在跑（ping 是转发给启动器管道再回来的）
    setDot('dotHost', 'ok');
    $('txtHost').textContent = '原生宿主正常';
    setDot('dotLauncher', 'ok');
    const pid = res.pid ? ' (PID ' + res.pid + ')' : '';
    const ver = res.version ? ' v' + res.version : '';
    $('txtLauncher').textContent = '启动器已就绪' + ver + pid;
    setStatus('');
  } else {
    const err = res.error || '';
    if (err.indexOf('未注册') >= 0 || err.indexOf('not found') >= 0 || err.indexOf('Access') >= 0) {
      setDot('dotHost', 'bad');
      $('txtHost').textContent = '原生宿主未注册';
      setDot('dotLauncher', '');
      $('txtLauncher').textContent = '需先注册宿主';
    } else {
      setDot('dotHost', 'ok');
      $('txtHost').textContent = '原生宿主正常';
      setDot('dotLauncher', 'bad');
      $('txtLauncher').textContent = '启动器未运行';
    }
    setStatus(err, true);
  }
}

async function loadLastAttempt() {
  const res = await send({ type: 'getLastAttempt' });
  const el = $('lastAttempt');
  if (!el) return;
  const a = res && res.attempt;
  if (!a) { el.textContent = ''; return; }
  const time = new Date(a.ts).toLocaleTimeString();
  if (a.result === 'ok') {
    el.textContent = time + ' · 已交给启动器';
    el.style.color = '#7fd18a';
  } else if (a.result === 'pass') {
    el.textContent = time + ' · 放行';
    el.style.color = '#9fb4cc';
  } else {
    el.textContent = time + ' · 失败：' + (a.error || '未知原因');
    el.style.color = '#ff8f8f';
  }
};

async function loadLast() {
  const res = await chrome.storage.local.get(['lastUrl', 'lastName']);
  if (res.lastUrl) {
    $('lastInfo').textContent = res.lastName || res.lastUrl;
  } else {
    $('lastInfo').textContent = '暂无';
  }
}


// ─────────────────────────────────────────────
// 启动器位置（一键扫描 / 手动指定）
// ─────────────────────────────────────────────
function escPop(s) {
  return String(s == null ? '' : s).replace(/[&<>"']/g, (c) => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
  }[c]));
}

async function refreshLauncherPathPop() {
  const res = await send({ type: 'getLauncherPath' });
  const el = $('popLauncherPath');
  if (!el) return;
  if (res && res.path) { el.textContent = res.path; el.classList.add('ok'); }
  else { el.textContent = '跟随系统注册项'; el.classList.remove('ok'); }
}

async function scanLauncherPop() {
  const btn = $('btnPopScan');
  const box = $('popScanBox');
  const hint = $('popHint');
  btn.disabled = true; btn.textContent = '扫描中…';
  hint.style.color = '#7f8ea3'; hint.textContent = '正在扫描…';
  try {
    const res = await send({ type: 'scanLauncher' });
    if (!res || !res.ok) {
      box.style.display = 'none';
      hint.style.color = '#ff8f8f';
      hint.textContent = (res && res.error) || '扫描失败';
      return;
    }
    const list = (res.result && res.result.candidates) || [];
    box.innerHTML = '';
    if (!list.length) {
      box.style.display = 'none';
      hint.style.color = '#ff8f8f';
      hint.textContent = '没扫到，点「手动指定」粘贴路径';
      return;
    }
    box.style.display = 'block';
    hint.style.color = '#7f8ea3';
    hint.textContent = '点一行选用';
    for (const c of list) {
      const row = document.createElement('div');
      row.className = 'scan-item';
      row.innerHTML = (c.recommended ? '<span class="rec">★</span>' : '') + '<span>' + escPop(c.path) + '</span>';
      row.addEventListener('click', () => useLauncherPop(c.path));
      box.appendChild(row);
    }
  } finally {
    btn.disabled = false; btn.textContent = '一键扫描';
  }
}

async function useLauncherPop(path) {
  const hint = $('popHint');
  const res = await send({ type: 'useLauncher', path });
  if (res && res.ok) {
    hint.style.color = '#7fd18a';
    hint.textContent = '已切换';
    const el = $('popLauncherPath');
    if (el) { el.textContent = res.launcher || path; el.classList.add('ok'); }
    await refreshLauncherPathPop();
    testHost();
  } else {
    hint.style.color = '#ff8f8f';
    hint.textContent = (res && res.error) || '失败';
  }
}

async function manualPickPop() {
  const hint = $('popHint');
  hint.style.color = '#9fb4cc';
  // 弹窗里不能放输入框，直接跳到设置页（那里有扫描/手填/拖拽）
  const path = window.prompt('VersePC2.exe 完整路径', '');
  if (!path) { hint.textContent = ''; return; }
  hint.style.color = '#7f8ea3'; hint.textContent = '正在校验…';
  const ok = await send({ type: 'probeLauncher', path });
  if (!ok || !ok.ok) { hint.style.color = '#ff8f8f'; hint.textContent = (ok && ok.error) || '无效路径'; return; }
  if (ok.supportsBridge === false) { hint.style.color = '#ff8f8f'; hint.textContent = '这个 exe 没有接管模块'; return; }
  await useLauncherPop(ok.path || path);
}

document.addEventListener('DOMContentLoaded', async () => {
  await loadSettings();
  await loadLast();
  await loadLastAttempt();
  await refreshLauncherPathPop();
  testHost();

  $('enabled').addEventListener('change', saveSettings);
  $('useBrowserFolder').addEventListener('change', saveSettings);
  $('btnTest').addEventListener('click', async () => { await testHost(); await loadLastAttempt(); });
  $('btnPopScan').addEventListener('click', scanLauncherPop);
  $('btnPopPick').addEventListener('click', manualPickPop);

  $('btnRetry').addEventListener('click', async () => {
    const res = await send({ type: 'retryLast' });
    setStatus(res.ok ? '已重新交给启动器' : (res.error || '重试失败'), !res.ok);
  });

  $('btnBrowser').addEventListener('click', async () => {
    await send({ type: 'downloadInBrowser' });
    setStatus('已交回浏览器');
  });

  $('more').addEventListener('click', () => {
    chrome.runtime.openOptionsPage();
  });
});
