// VersePC2 下载接管 —— 后台 Service Worker
//
// 流程：拦截浏览器下载 → 取消浏览器下载 → 通过原生消息宿主交给 VersePC2
//       → 启动器把「地址 + 文件名 + 目录」填进主页下载卡 → 由用户点「开始下载」
//
// 拦截用两个钩子，谁先到用谁（Edge/Chrome 不同版本触发点不一样）：
//   1. chrome.downloads.onCreated            —— 下载一创建就拦，最早、最可靠
//   2. chrome.downloads.onDeterminingFilename —— 兜底，部分场景只触发这个
// 同一个下载只会被处理一次（按 downloadId 去重）。
//
// 红线：扩展永不代按"开始下载"。
'use strict';

const HOST_NAME = 'cc.versepc.download_bridge';

const DEFAULTS = {
  enabled: true,          // 总开关
  silent: true,           // true = 不弹确认窗，直接交给启动器填表
  useBrowserFolder: true, // true = 用浏览器原定的保存目录
  pclPath: '',            // 兼容旧字段，VersePC2 不需要填路径
};

let lastDownload = null;

// 已处理过的 downloadId（onCreated / onDeterminingFilename 会拿到同一个 id）
const handledIds = new Set();
// 我们自己交回浏览器的 URL，避免二次拦截造成死循环
const selfDownloads = new Set();

// ─────────────────────────────────────────────
// 安装/启动初始化
// ─────────────────────────────────────────────
chrome.runtime.onInstalled.addListener(async (details) => {
  const cur = await chrome.storage.local.get(DEFAULTS);
  await chrome.storage.local.set({ ...DEFAULTS, ...cur });
  console.log('[VersePC2] 扩展已安装/更新:', details.reason);
  probeHost().catch(() => {});
});

chrome.runtime.onStartup.addListener(() => {
  console.log('[VersePC2] 浏览器启动');
});

// ─────────────────────────────────────────────
// 1. 拦截下载（两个钩子）
// ─────────────────────────────────────────────
chrome.downloads.onCreated.addListener((item) => {
  intercept(item, 'onCreated').catch((err) => {
    console.error('[VersePC2] onCreated 处理失败:', err);
  });
});

chrome.downloads.onDeterminingFilename.addListener((item, suggest) => {
  intercept(item, 'onDeterminingFilename').catch((err) => {
    console.error('[VersePC2] onDeterminingFilename 处理失败:', err);
  });
  // 不调用 suggest()：保持浏览器原本的文件名，真正的文件由启动器下载
});

/** 记一条拦截结果，供弹窗诊断显示 */
function record(entry) {
  const payload = Object.assign({ ts: Date.now() }, entry);
  try { chrome.storage.local.set({ lastAttempt: payload }); } catch (e) {}
  return payload;
}

async function intercept(item, from) {
  if (!item || typeof item.id !== 'number') return;

  // 去重：同一 id 只处理一次
  if (handledIds.has(item.id)) return;
  handledIds.add(item.id);
  if (handledIds.size > 300) handledIds.clear();

  const url = item.finalUrl || item.url || '';
  // 注意：onCreated 阶段 item.filename 经常还是空的（浏览器还没定名），
  // 这时千万不要编一个 download.bin 出来 —— 那会让启动器卡片显示错的格式。
  // 宁可发空串，让启动器按 URL / Content-Disposition 去解析真实名字。
  let filename = baseName(item.filename || '');
  if (/^(download|file|unknown)(\.bin)?$/i.test(filename)) filename = '';

  if (isExtensionUrl(url)) return;
  if (selfDownloads.has(url)) { selfDownloads.delete(url); return; }

  const cfg = { ...DEFAULTS, ...(await chrome.storage.local.get(DEFAULTS)) };

  // 非 http(s)：blob:/data: 之类启动器拿不到，直接放行，不动浏览器下载
  if (!/^https?:/i.test(url)) {
    record({ from, url, filename, result: 'pass', error: '非 http(s) 链接，交给浏览器' });
    return;
  }
  if (!cfg.enabled) {
    record({ from, url, filename, result: 'pass', error: '接管开关已关闭' });
    return;
  }

  // 决定接管之后才取消浏览器下载，避免"开关关着还把下载干掉"
  cancelDownload(item.id);

  // 浏览器有时会在取消后的一瞬间把文件名定下来（尤其是 Content-Disposition），
  // 补查一次，能拿到就别用空名字。
  if (!filename) {
    try {
      const hits = await chrome.downloads.search({ id: item.id });
      const nm = hits && hits[0] && baseName(hits[0].filename || '');
      if (nm && !/^(download|file|unknown)(\.bin)?$/i.test(nm)) filename = nm;
    } catch (e) {}
  }

  lastDownload = { url, filename };
  chrome.storage.local.set({ lastUrl: url, lastName: filename }).catch(() => {});
  const folder = cfg.useBrowserFolder ? folderFromPath(item.filename) : '';

  // 冷启动场景下宿主要先拉起启动器，给足时间（默认 45s）
  const res = await callHost({ action: 'download', url, filename, folder, userAgent: '' }, 45000);

  if (res && res.ok) {
    const via = res.via || 'pipe';
    record({ from, url, filename, result: 'ok', via, error: '' });
    notifyBadge('✓', '#22c55e');

    if (via === 'cold-start') {
      // 启动器本来是关着的，是这次点下载把它拉起来的：
      // 窗口要十几秒才出得来，给个通知，别让用户以为没反应。
      try {
        await chrome.notifications.create('versepc-cold-' + Date.now(), {
          type: 'basic',
          iconUrl: 'icons/128.png',
          title: '正在启动 VersePC2…',
          message: '下载已交给启动器，窗口出现后地址会自动填好，点「开始下载」即可。',
        });
      } catch (e) {}
    }
    return;
  }

  const err = (res && (res.error || res.message)) || '原生宿主无响应';
  record({ from, url, filename, result: 'fail', error: err });
  notifyBadge('!', '#ef4444');
  console.warn('[VersePC2] 交接失败:', err);

  // 交接失败才把下载还给浏览器，绝不吞掉用户的文件
  reDownloadToBrowser(url, filename);
  try {
    await chrome.notifications.create('versepc-fail-' + Date.now(), {
      type: 'basic',
      iconUrl: 'icons/128.png',
      title: 'VersePC2 接管失败，已交回浏览器',
      message: err,
    });
  } catch (e) {}
}

function cancelDownload(id) {
  try { chrome.downloads.cancel(id).catch(() => {}); } catch (e) {}
  try { chrome.downloads.erase({ id }).catch(() => {}); } catch (e) {}
}

// ─────────────────────────────────────────────
// 2. 小工具
// ─────────────────────────────────────────────
function notifyBadge(text, color, ms) {
  try {
    chrome.action.setBadgeBackgroundColor({ color: color || '#3b82f6' });
    chrome.action.setBadgeText({ text: text || '' });
    if (text) {
      setTimeout(() => {
        try { chrome.action.setBadgeText({ text: '' }); } catch (e) {}
      }, ms || 4000);
    }
  } catch (e) {}
}

function reDownloadToBrowser(url, filename) {
  if (!url) return;
  selfDownloads.add(url);
  setTimeout(() => selfDownloads.delete(url), 20000);
  try {
    chrome.downloads
      .download({ url, filename: baseName(filename), conflictAction: 'uniquify' })
      .catch((e) => console.error('[VersePC2] 交还浏览器下载失败:', e));
  } catch (e) {
    console.error('[VersePC2] 交还浏览器下载失败:', e);
  }
}

function isExtensionUrl(url) {
  return typeof url === 'string' && (url.startsWith('chrome-extension://') || url.startsWith('edge-extension://'));
}

function folderFromPath(fullPath) {
  if (!fullPath) return '';
  const norm = String(fullPath).replace(/[/]/g, '\\');
  const idx = norm.lastIndexOf('\\');
  if (idx < 0) return '';
  return norm.slice(0, idx + 1);
}

function baseName(p) {
  if (!p) return 'download.bin';
  const norm = String(p).replace(/\\/g, '/');
  return norm.slice(norm.lastIndexOf('/') + 1) || 'download.bin';
}

// ─────────────────────────────────────────────
// 3. 与弹窗/设置页通信
// ─────────────────────────────────────────────
chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  (async () => {
    switch (msg && msg.type) {
      case 'getSettings':
        sendResponse({ ok: true, settings: await chrome.storage.local.get(DEFAULTS) });
        break;

      case 'saveSettings':
        await chrome.storage.local.set(msg.settings || {});
        sendResponse({ ok: true });
        break;

      case 'testHost':
        sendResponse(await probeHost());
        break;

      case 'scanLauncher': {
        const res = await callHost({ action: 'scanLauncher' }, 30000);
        if (res && res.ok) {
          // 记住扫描结果，弹窗/设置页都能用
          await chrome.storage.local.set({ scanResult: res });
          sendResponse({ ok: true, result: res });
        } else {
          sendResponse({ ok: false, error: (res && res.error) || '扫描失败：原生宿主不可用' });
        }
        break;
      }

      case 'probeLauncher': {
        const res = await callHost({ action: 'probeLauncher', path: msg.path }, 15000);
        sendResponse(res || { ok: false, error: '无响应' });
        break;
      }

      case 'useLauncher': {
        const res = await callHost({ action: 'useLauncher', path: msg.path }, 20000);
        if (res && res.ok) await chrome.storage.local.set({ launcherPath: res.launcher || msg.path });
        sendResponse(res || { ok: false, error: '无响应' });
        break;
      }

      case 'getLauncherPath':
        sendResponse({ ok: true, path: (await chrome.storage.local.get('launcherPath')).launcherPath || '' });
        break;

      case 'getLastAttempt':
        sendResponse({ ok: true, attempt: (await chrome.storage.local.get('lastAttempt')).lastAttempt || null });
        break;

      case 'downloadInBrowser':
        if (lastDownload) reDownloadToBrowser(lastDownload.url, lastDownload.filename);
        sendResponse({ ok: true });
        break;

      case 'retryLast':
        if (!lastDownload) { sendResponse({ ok: false, error: '没有可重试的下载' }); break; }
        sendResponse(await callHost({ action: 'download', ...lastDownload }));
        break;

      default:
        sendResponse({ ok: false, error: '未知消息: ' + (msg && msg.type) });
    }
  })().catch((e) => sendResponse({ ok: false, error: String(e) }));
  return true; // 保持消息通道开放（异步响应）
});

// ─────────────────────────────────────────────
// 4. 原生消息宿主调用
// ─────────────────────────────────────────────
function probeHost() {
  return callHost({ action: 'ping' }, 6000);
}

function callHost(payload, timeoutMs) {
  return new Promise((resolve) => {
    let port;
    try {
      port = chrome.runtime.connectNative(HOST_NAME);
    } catch (e) {
      resolve({ ok: false, error: '无法连接原生宿主：' + e.message });
      return;
    }

    let settled = false;
    const finish = (r) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      try { port.disconnect(); } catch (e) {}
      resolve(r);
    };

    const timer = setTimeout(() => {
      finish({ ok: false, error: '原生宿主响应超时' });
    }, timeoutMs || 20000);

    port.onMessage.addListener((resp) => finish(resp));

    port.onDisconnect.addListener(() => {
      const err = chrome.runtime.lastError;
      finish({
        ok: false,
        error: err
          ? err.message
          : '原生宿主未注册或扩展 ID 不匹配。请打开 VersePC2 → 设置 → 其他设置 → 浏览器下载接管 → 点「一键安装」。',
      });
    });

    try {
      port.postMessage(payload);
    } catch (e) {
      finish({ ok: false, error: String(e) });
    }
  });
}
