/**
 * @file feedback.js
 * @description 问题反馈页 - 分类选择、日志自动收集、附件、环境信息与站点提交
 *
 * 链路：
 *   1. 用户在「问题分类」选择 启动问题 / 下载问题 / 其他
 *   2. 选择后自动扫描 data/logs 收集对应日志（launch -> 启动日志；download -> 下载日志）
 *   3. 提交时加载站点验证码 -> HMAC 防伪签名 -> multipart POST 到 verselauncher.cn
 */

/** 站点地址 */
var FEEDBACK_SITE_URL = 'https://verselauncher.cn';

/** 附件限制 */
var FEEDBACK_MAX_FILES = 10;
var FEEDBACK_MAX_FILE_BYTES = 20 * 1024 * 1024;

/** 分类 -> 自动收集的日志文件名规则 */
var FEEDBACK_AUTO_LOGS = {
  launch: [
    { prefix: 'launch-debug.log', match: 'exact' },
    { prefix: 'launch-fail-', match: 'prefix' },
    { prefix: 'startup-timing.log', match: 'exact' }
  ],
  download: [
    { prefix: 'modpack-import.log', match: 'exact' },
    { prefix: 'updater-download.log', match: 'exact' },
    { prefix: 'loader-install.log', match: 'exact' },
    { prefix: '.install', match: 'ends' }
  ]
};

/** 表单状态 */
var feedbackState = {
  type: 'bug',
  category: 'other',
  files: [],          // 截图/视频（手动选择）
  autoLogs: [],       // 日志（自动收集）
  env: null,
  envLoaded: false,
  submitting: false,
  captchaId: '',
  signKey: ''
};

/* ============================================================================
   初始化
   ============================================================================ */

/** 绑定拖拽区、采集环境信息并同步按钮状态（Vue 挂载完成后调用） */
function setupFeedback() {
  var drop = document.getElementById('fb-drop');
  if (drop) {
    drop.addEventListener('dragover', function (e) {
      e.preventDefault();
      drop.classList.add('drag-over');
    });
    drop.addEventListener('dragleave', function () {
      drop.classList.remove('drag-over');
    });
    drop.addEventListener('drop', function (e) {
      e.preventDefault();
      drop.classList.remove('drag-over');
      if (e.dataTransfer && e.dataTransfer.files) {
        addFeedbackFiles(e.dataTransfer.files);
      }
    });
  }

  onFeedbackDetailInput();
  renderFeedbackFiles();
  renderAutoLogFiles();
  updateFeedbackSubmitState();
  loadFeedbackEnv();
  loadFeedbackCaptcha();
}

/* ============================================================================
   反馈类型
   ============================================================================ */

function selectFeedbackType(btn) {
  if (!btn) return;
  var group = document.getElementById('fb-type-group');
  if (group) {
    group.querySelectorAll('.fb-type-btn').forEach(function (b) { b.classList.remove('active'); });
  }
  btn.classList.add('active');
  feedbackState.type = btn.dataset.type || 'other';
  updateFeedbackSubmitState();
}

function getFeedbackTypeLabel(type) {
  if (type === 'bug') return '问题反馈';
  if (type === 'feature') return '功能建议';
  return '其他';
}

/* ============================================================================
   问题分类 + 日志自动收集
   ============================================================================ */

function selectFeedbackCategory(btn) {
  if (!btn) return;
  var group = document.getElementById('fb-category-group');
  if (group) {
    group.querySelectorAll('.fb-category-btn').forEach(function (b) { b.classList.remove('active'); });
  }
  btn.classList.add('active');
  feedbackState.category = btn.dataset.cat || 'other';
  // 分类变化后重新扫描日志
  feedbackState.autoLogs = [];
  renderAutoLogFiles();
  collectAutoLogs();
  updateFeedbackSubmitState();
}

function getFeedbackCategoryLabel(cat) {
  if (cat === 'launch') return '启动问题';
  if (cat === 'download') return '下载问题';
  return '其他';
}

/** 扫描 data/logs 并读取匹配日志，追加到 autoLogs（自动收集最多 8 个） */
async function collectAutoLogs() {
  var rules = FEEDBACK_AUTO_LOGS[feedbackState.category];
  var tip = document.getElementById('fb-auto-log-tip');
  var badge = document.getElementById('fb-auto-log-badge');

  if (!rules) {
    if (tip) { tip.textContent = '「其他」分类无需收集日志，可直接补充截图或描述'; tip.className = 'fb-auto-log-tip empty'; }
    if (badge) badge.hidden = true;
    return;
  }
  if (tip) { tip.textContent = '正在扫描 data/logs 收集日志...'; tip.className = 'fb-auto-log-tip loading'; }
  if (badge) badge.hidden = false;

  try {
    var dataDir = null;
    if (typeof API !== 'undefined' && typeof API.getDataDir === 'function') {
      var res = await API.getDataDir();
      if (res && res.dataDir) dataDir = res.dataDir;
    }
    if (!dataDir) {
      if (tip) { tip.textContent = '获取数据目录失败，无法自动收集日志'; tip.className = 'fb-auto-log-tip empty'; }
      return;
    }

    // logs 目录
    var logsPath = dataDir.replace(/[\\/]+$/, '') + '/logs';
    var entries = [];
    if (typeof API !== 'undefined' && typeof API.browseDirectory === 'function') {
      var browse = await API.browseDirectory(logsPath, true);
      // 兼容 items / files / folders 三种返回结构
      if (browse && browse.items) entries = browse.items;
      else if (browse && browse.files) entries = browse.files;
      else if (browse && browse.folders) entries = browse.folders;
    }

    // 按规则过滤，排除目录，按修改时间倒序
    var matched = entries.filter(function (item) {
      if (item.isDirectory) return false;
      var name = String(item.name || '');
      return rules.some(function (rule) {
        if (rule.match === 'exact') return name === rule.prefix;
        if (rule.match === 'prefix') return name.indexOf(rule.prefix) === 0;
        if (rule.match === 'ends') return name.lastIndexOf(rule.prefix) === name.length - rule.prefix.length;
        return false;
      });
    });
    matched.sort(function (a, b) { return (b.modified || 0) - (a.modified || 0); });

    // 读取文件内容转 File（最多 8 个）
    var picked = matched.slice(0, 8);
    var added = [];
    for (var i = 0; i < picked.length; i++) {
      var item = picked[i];
      try {
        var bytes = await readFeedbackFileAsBytes(item.path);
        if (!bytes || bytes.byteLength > FEEDBACK_MAX_FILE_BYTES) continue;
        var file = new File([bytes], item.name, { type: 'text/plain' });
        added.push({ name: item.name, size: file.size, file: file, auto: true });
      } catch (e) { /* 单个日志读取失败跳过 */ }
    }
    feedbackState.autoLogs = added;
  } catch (e) {
    if (tip) { tip.textContent = '日志收集失败：' + (e && e.message ? e.message : '未知错误'); tip.className = 'fb-auto-log-tip empty'; }
  }

  renderAutoLogFiles();
  if (tip) {
    if (!feedbackState.autoLogs.length) {
      tip.textContent = '未在 data/logs 找到相关日志（可能尚未复现过问题）';
      tip.className = 'fb-auto-log-tip empty';
    } else {
      tip.textContent = '已自动收集 ' + feedbackState.autoLogs.length + ' 个日志文件（不可移除，随反馈一并提交）';
      tip.className = 'fb-auto-log-tip';
    }
  }
}

/** 通过 IPC 读取本地文件为 Uint8Array */
async function readFeedbackFileAsBytes(filePath) {
  var api = window.electronAPI;
  if (!api || typeof api.readFileBuffer !== 'function') throw new Error('readFileBuffer unavailable');
  var buffer = await api.readFileBuffer(filePath);
  if (typeof window.decodeFileBuffer === 'function') return window.decodeFileBuffer(buffer);
  if (buffer instanceof Uint8Array) return buffer;
  if (buffer) return new Uint8Array(buffer);
  return null;
}

function renderAutoLogFiles() {
  var list = document.getElementById('fb-auto-log-list');
  if (!list) return;

  if (!feedbackState.autoLogs.length) {
    list.innerHTML = '';
    return;
  }

  list.innerHTML = feedbackState.autoLogs.map(function (item, index) {
    return '<div class="fb-file-item">' +
      '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="fb-file-icon">' +
      '<path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/></svg>' +
      '<span class="fb-file-name" title="' + escapeFeedbackText(item.name) + '">' + escapeFeedbackText(item.name) + '</span>' +
      '<span class="fb-log-tag is-auto">日志</span>' +
      '<span class="fb-file-size">' + formatFeedbackSize(item.size) + '</span>' +
      '</div>';
  }).join('');
}

/* ============================================================================
   附件（截图 / 视频）
   ============================================================================ */

function pickFeedbackFiles() {
  var input = document.getElementById('fb-file-input');
  if (input) input.click();
}

function onFeedbackFilesPicked(fileList) {
  addFeedbackFiles(fileList);
  var input = document.getElementById('fb-file-input');
  if (input) input.value = '';
}

function addFeedbackFiles(fileList) {
  var incoming = Array.prototype.slice.call(fileList || []);
  var rejected = [];

  incoming.forEach(function (file) {
    if (feedbackState.files.length >= FEEDBACK_MAX_FILES) {
      rejected.push(file.name + '（已达数量上限）');
      return;
    }
    if (file.size > FEEDBACK_MAX_FILE_BYTES) {
      rejected.push(file.name + '（超过 20 MB）');
      return;
    }
    var duplicated = feedbackState.files.some(function (item) {
      return item.name === file.name && item.size === file.size;
    });
    if (duplicated) return;
    feedbackState.files.push({ name: file.name, size: file.size, file: file });
  });

  if (rejected.length && typeof showToast === 'function') {
    showToast('已跳过：' + rejected.join('、'), 'error');
  }
  renderFeedbackFiles();
  updateFeedbackSubmitState();
}

function removeFeedbackFile(index) {
  feedbackState.files.splice(index, 1);
  renderFeedbackFiles();
  updateFeedbackSubmitState();
}

function renderFeedbackFiles() {
  var list = document.getElementById('fb-file-list');
  if (!list) return;

  if (!feedbackState.files.length) {
    list.innerHTML = '';
    return;
  }

  list.innerHTML = feedbackState.files.map(function (item, index) {
    return '<div class="fb-file-item">' +
      '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="fb-file-icon">' +
      '<path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/></svg>' +
      '<span class="fb-file-name" title="' + escapeFeedbackText(item.name) + '">' + escapeFeedbackText(item.name) + '</span>' +
      '<span class="fb-file-size">' + formatFeedbackSize(item.size) + '</span>' +
      '<button class="fb-file-remove" title="移除" onclick="removeFeedbackFile(' + index + ')">' +
      '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>' +
      '</button>' +
      '</div>';
  }).join('');
}

function formatFeedbackSize(bytes) {
  if (bytes < 1024) return bytes + ' B';
  if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + ' KB';
  return (bytes / 1024 / 1024).toFixed(1) + ' MB';
}

function escapeFeedbackText(text) {
  return String(text == null ? '' : text).replace(/[&<>"']/g, function (ch) {
    return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[ch];
  });
}

/* ============================================================================
   环境信息
   ============================================================================ */

function onFeedbackDetailInput() {
  var detail = document.getElementById('fb-detail');
  var counter = document.getElementById('fb-detail-counter');
  if (detail && counter) counter.textContent = detail.value.length + ' / 2000';
  updateFeedbackSubmitState();
}

async function loadFeedbackEnv() {
  if (feedbackState.envLoaded) {
    renderFeedbackEnv();
    return;
  }

  var env = {
    launcher: '未知',
    system: '未知',
    java: '未知'
  };

  try {
    var api = window.electronAPI;
    if (api && api.updater && typeof api.updater.getVersion === 'function') {
      var version = await api.updater.getVersion();
      if (version) env.launcher = 'VersePC2 ' + version;
    }
  } catch (e) {}

  try {
    var platform = (navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || '';
    var arch = /x64|Win64|WOW64|x86_64/i.test(navigator.userAgent) ? '64 位' : '';
    env.system = platform ? (platform + (arch ? ' · ' + arch : '')) : navigator.userAgent;
  } catch (e) {}

  try {
    if (typeof API !== 'undefined' && typeof API.detectJava === 'function') {
      var result = await API.detectJava();
      var list = (result && result.javaList) || [];
      if (list.length) {
        var best = list.find(function (j) { return j.majorVersion >= 17; }) || list[0];
        env.java = 'Java ' + (best.version || best.majorVersion || '') +
          (best.is64Bit ? ' (64 位)' : '') +
          (best.path ? ' · ' + best.path : '');
      } else {
        env.java = '未检测到';
      }
    }
  } catch (e) {}

  feedbackState.env = env;
  feedbackState.envLoaded = true;
  renderFeedbackEnv();
}

function renderFeedbackEnv() {
  var list = document.getElementById('fb-env-list');
  if (!list) return;
  var env = feedbackState.env;
  if (!env) {
    list.innerHTML = '<div class="fb-env-empty">正在收集环境信息...</div>';
    return;
  }

  var rows = [
    ['启动器版本', env.launcher],
    ['操作系统', env.system],
    ['Java', env.java]
  ];

  list.innerHTML = rows.map(function (row) {
    return '<div class="fb-env-row">' +
      '<span class="fb-env-key">' + row[0] + '</span>' +
      '<span class="fb-env-value" title="' + escapeFeedbackText(row[1]) + '">' + escapeFeedbackText(row[1]) + '</span>' +
      '</div>';
  }).join('');
}

/* ============================================================================
   人机验证（站点验证码）
   ============================================================================ */

/** 从站点获取验证码 SVG 与签名密钥 */
async function loadFeedbackCaptcha() {
  try {
    var data = await fetchFeedbackJson(FEEDBACK_SITE_URL + '/api/feedback/captcha');
    if (data && data.id && data.svg) {
      feedbackState.captchaId = data.id;
      var img = document.getElementById('fb-captcha-image');
      if (img) img.innerHTML = data.svg;
      var group = document.getElementById('fb-captcha-group');
      if (group) group.hidden = false;
    }
  } catch (e) {}

  // 签名密钥：同源仅站点页面可用；启动器从 /signinfo 获取（host 为 verselauncher.cn）
  if (!feedbackState.signKey) {
    try {
      var keyRes = await fetchFeedbackJson(FEEDBACK_SITE_URL + '/api/feedback/signinfo');
      if (keyRes && keyRes.key) feedbackState.signKey = keyRes.key;
    } catch (e) {}
  }
  updateFeedbackSubmitState();
}

function refreshFeedbackCaptcha() {
  loadFeedbackCaptcha();
}

async function fetchFeedbackJson(url) {
  var res = await fetch(url, { method: 'GET', credentials: 'omit' });
  if (!res.ok) throw new Error('HTTP ' + res.status);
  return res.json();
}

/* ============================================================================
   提交
   ============================================================================ */

function getFeedbackForm() {
  return {
    type: feedbackState.type,
    typeLabel: getFeedbackTypeLabel(feedbackState.type),
    category: feedbackState.category,
    categoryLabel: getFeedbackCategoryLabel(feedbackState.category),
    title: (document.getElementById('fb-title') || {}).value || '',
    detail: (document.getElementById('fb-detail') || {}).value || '',
    contact: (document.getElementById('fb-contact') || {}).value || '',
    captcha: (document.getElementById('fb-captcha-input') || {}).value || ''
  };
}

/** 返回第一条校验错误，全部通过时返回空字符串 */
function validateFeedback(form) {
  if (!form.title.trim()) return '请填写反馈标题';
  if (form.title.trim().length < 4) return '标题太短，请至少填写 4 个字';
  if (!form.detail.trim()) return '请填写详细描述';
  if (form.detail.trim().length < 10) return '描述太短，请至少填写 10 个字';

  var captchaGroup = document.getElementById('fb-captcha-group');
  if (captchaGroup && !captchaGroup.hidden && !form.captcha.trim()) {
    return '请完成人机验证';
  }
  return '';
}

function updateFeedbackSubmitState() {
  var button = document.getElementById('fb-submit-btn');
  var hint = document.getElementById('fb-submit-hint');
  if (!button) return;

  if (feedbackState.submitting) {
    button.disabled = true;
    return;
  }

  var error = validateFeedback(getFeedbackForm());
  button.disabled = !!error;
  if (hint) hint.textContent = error || '';
  if (hint) hint.classList.toggle('is-error', !!error);
}

function setFeedbackSubmitting(submitting) {
  feedbackState.submitting = submitting;
  var button = document.getElementById('fb-submit-btn');
  if (button) {
    button.disabled = submitting;
    button.textContent = submitting ? '提交中...' : '提交反馈';
  }
}

function resetFeedbackForm() {
  var title = document.getElementById('fb-title');
  var detail = document.getElementById('fb-detail');
  var contact = document.getElementById('fb-contact');
  var captchaInput = document.getElementById('fb-captcha-input');
  if (title) title.value = '';
  if (detail) detail.value = '';
  if (contact) contact.value = '';
  if (captchaInput) captchaInput.value = '';

  feedbackState.files = [];
  feedbackState.type = 'bug';
  var group = document.getElementById('fb-type-group');
  if (group) {
    group.querySelectorAll('.fb-type-btn').forEach(function (b) {
      b.classList.toggle('active', b.dataset.type === 'bug');
    });
  }

  // 分类重置为「其他」并清空自动日志
  feedbackState.category = 'other';
  feedbackState.autoLogs = [];
  var catGroup = document.getElementById('fb-category-group');
  if (catGroup) {
    catGroup.querySelectorAll('.fb-category-btn').forEach(function (b) {
      b.classList.toggle('active', b.dataset.cat === 'other');
    });
  }
  var tip = document.getElementById('fb-auto-log-tip');
  if (tip) { tip.textContent = '选择「启动问题」或「下载问题」后自动收集 data/logs 下的相关日志'; tip.className = 'fb-auto-log-tip'; }
  var badge = document.getElementById('fb-auto-log-badge');
  if (badge) badge.hidden = true;

  renderFeedbackFiles();
  renderAutoLogFiles();
  onFeedbackDetailInput();

  // 验证码一次一用，重置后刷新
  loadFeedbackCaptcha();
}

async function submitFeedback() {
  if (feedbackState.submitting) return;

  var form = getFeedbackForm();
  var error = validateFeedback(form);
  if (error) {
    updateFeedbackSubmitState();
    if (typeof showToast === 'function') showToast(error, 'error');
    return;
  }

  var includeEnv = document.getElementById('fb-include-env');

  setFeedbackSubmitting(true);
  try {
    await sendFeedbackToSite({
      category: form.category,
      categoryLabel: form.categoryLabel,
      title: form.title.trim(),
      detail: form.detail.trim(),
      contact: form.contact.trim(),
      captcha: form.captcha.trim(),
      env: (includeEnv && includeEnv.checked) ? feedbackState.env : null,
      autoLogs: feedbackState.autoLogs,
      files: feedbackState.files
    });
    setFeedbackSubmitting(false);
    resetFeedbackForm();
    loadFeedbackCaptcha();
    if (typeof showToast === 'function') showToast('反馈已提交，感谢你的反馈！', 'success');
  } catch (e) {
    setFeedbackSubmitting(false);
    updateFeedbackSubmitState();
    // 429 时提示明日再试
    if (e && e.status === 429) {
      if (typeof showToast === 'function') showToast('今日已提交过反馈，请明日再试', 'error');
      return;
    }
    if (e && e.dailyLimit) {
      if (typeof showToast === 'function') showToast('今日已提交过反馈，请明日再试', 'error');
      return;
    }
    if (typeof showToast === 'function') showToast(e && e.message ? e.message : '提交失败，请稍后重试', 'error');
  }
}

/**
 * 提交到站点：multipart POST + HMAC-SHA256 防伪签名 + 验证码 + nonce 防重放
 *
 * 签名载荷：verse-feedback-v1|captchaId|nonce|ts|client|category|title|description
 * （与站点 lib/sign.ts 校验规则一致）
 */
async function sendFeedbackToSite(payload) {
  if (!feedbackState.signKey) {
    throw new Error('验证码与签名信息尚未就绪，请稍后重试');
  }

  // 拼接提交描述：正文 + 分类 + 环境信息
  var description = payload.detail;
  if (payload.env) {
    description += '\n\n【环境信息】启动器：' + (payload.env.launcher || '未知') +
      '；操作系统：' + (payload.env.system || '未知') +
      '；Java：' + (payload.env.java || '未知');
  }

  var captchaId = feedbackState.captchaId;
  var nonce = randomFeedbackNonce();
  var ts = String(Date.now());
  var client = 'versepc2';
  var category = payload.categoryLabel;
  var title = payload.title;

  var parts = ['verse-feedback-v1', captchaId, nonce, ts, client, category, title, description];
  var sig = await hmacFeedbackSha256(feedbackState.signKey, parts.join('|'));

  var formData = new FormData();
  formData.append('client', client);
  formData.append('title', title);
  formData.append('description', description);
  formData.append('category', category);
  formData.append('email', payload.contact);
  formData.append('captchaId', captchaId);
  formData.append('captchaAnswer', payload.captcha);
  formData.append('nonce', nonce);
  formData.append('ts', ts);
  formData.append('sig', sig);

  // 日志（自动收集）在前，截图/视频在后
  var allFiles = payload.autoLogs.concat(payload.files).slice(0, FEEDBACK_MAX_FILES);
  for (var i = 0; i < allFiles.length; i++) {
    formData.append('file' + i, allFiles[i].file, allFiles[i].name);
  }

  var res = await fetch(FEEDBACK_SITE_URL + '/api/feedback', {
    method: 'POST',
    body: formData,
    credentials: 'omit'
  });

  if (res.status === 429) {
    var e429 = new Error('今日已提交过反馈，请明日再试');
    e429.status = 429;
    e429.dailyLimit = true;
    throw e429;
  }
  if (!res.ok) {
    var body = null;
    try { body = await res.json(); } catch (e) {}
    if (body && body.error === 'captcha incorrect') throw new Error('验证码不正确，请点击验证码刷新后重试');
    if (body && body.error === 'invalid signature') throw new Error('提交校验失败，请刷新验证码后重试');
    if (body && body.error === 'replay denied') throw new Error('请勿重复提交，请刷新验证码后重试');
    throw new Error('提交失败（' + res.status + '），请稍后重试');
  }
  return true;
}

/** HMAC-SHA256 hex，密钥为 hex 字符串（与站点 Web Crypto 实现对应） */
async function hmacFeedbackSha256(keyHex, message) {
  var keyBytes = hexToBytes(keyHex);
  var key = await crypto.subtle.importKey('raw', keyBytes, { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
  var sig = await crypto.subtle.sign('HMAC', key, new TextEncoder().encode(message));
  return bytesToHex(new Uint8Array(sig));
}

function hexToBytes(hex) {
  var clean = String(hex).replace(/[^0-9a-fA-F]/g, '');
  var bytes = new Uint8Array(clean.length / 2);
  for (var i = 0; i < bytes.length; i++) {
    bytes[i] = parseInt(clean.substr(i * 2, 2), 16);
  }
  return bytes;
}

function bytesToHex(bytes) {
  return Array.from(bytes).map(function (b) { return b.toString(16).padStart(2, '0'); }).join('');
}

/** 随机一次性 nonce（8-64 位字母数字） */
function randomFeedbackNonce() {
  var bytes = crypto.getRandomValues(new Uint8Array(16));
  return Array.from(bytes).map(function (b) { return b.toString(36); }).join('').slice(0, 32);
}

function openFeedbackSite() {
  var api = window.electronAPI;
  if (api && typeof api.openExternal === 'function') {
    api.openExternal(FEEDBACK_SITE_URL).catch(function () {});
    return;
  }
  window.open(FEEDBACK_SITE_URL, '_blank');
}