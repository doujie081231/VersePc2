import { reactive } from 'vue';
import { API, apiPost } from '../../core/api.js';
import { bridge } from '../../core/tauri-bridge.js';

import { showToast } from '../misc/ui-components.js';

import { dlManager, clearCompletedDownloads } from './custom-download-tasks.js';

let _customDlSessionId = null;
let _customDlPollTimer = null;
let _customDlSavePath = '';

// 自定义下载：状态与 DOM 绑定（工具箱卡片与主页控件共用；DOM 查询全部空值保护）

/** 当前下载任务快照（响应式），供主页控件/工具箱卡片订阅 */
const customDlState = reactive({
    sessionId: null,
    url: '',
    savePath: '',
    fileName: '',
    status: '',        // '' | running | completed | failed | cancelled | pending
    progress: 0,
    message: '',
    speed: 0,
    downloaded: 0,
    total: 0,
});

const _customDlListeners = new Set();

/** 订阅状态变化，返回取消函数 */
function onCustomDownloadChange(fn) {
    if (typeof fn !== 'function') return () => {};
    _customDlListeners.add(fn);
    try { fn({ ...customDlState }); } catch (e) {}
    return () => _customDlListeners.delete(fn);
}

function _emitCustomDl() {
    const snap = { ...customDlState };
    _customDlListeners.forEach((fn) => {
        try { fn(snap); } catch (e) { console.error('[custom-dl] listener', e); }
    });
}

/** 写 DOM（可能不存在，全部容错） */
function _setText(id, text) {
    const el = document.getElementById(id);
    if (el) el.textContent = text;
}

function _setDisplay(id, value) {
    const el = document.getElementById(id);
    if (el) el.style.display = value;
}

/** 选择保存目录 */
async function browseCustomDlPath() {
    const result = await bridge.showOpenDialog({ properties: ['openDirectory'], title: '选择保存位置' });
    if (result && result.filePaths && result.filePaths.length > 0) {
        const dir = result.filePaths[0];
        _customDlSavePath = dir;
        customDlState.savePath = dir;
        const input = document.getElementById('custom-dl-path');
        if (input) input.value = dir;
        API.saveCustomDownloadFolder(dir).catch(() => {});
        _emitCustomDl();
    }
}

/** 打开保存目录 */
function openCustomDlFolder() {
    const dir = customDlState.savePath || _customDlSavePath || document.getElementById('custom-dl-path')?.value || '';
    if (!dir) { showToast('请先选择保存位置', 'info'); return; }
    // 统一走 API 代理（Tauri 下是 invoke，浏览器下是 fetch）
    apiPost('/api/open-folder', { folder: 'custom', customPath: dir }).catch(() => {});
}

/**
 * 开始下载
 * @param {{url?:string, savePath?:string, fileName?:string}} [override]
 *        不传则从工具箱卡片的输入框读取（保持旧调用方式兼容）
 */
async function startCustomDownload(override) {
    const opts = override || {};
    const url = (opts.url ?? document.getElementById('custom-dl-url')?.value ?? '').trim();
    const savePath = (opts.savePath ?? document.getElementById('custom-dl-path')?.value ?? '').trim();
    const fileName = (opts.fileName ?? document.getElementById('custom-dl-filename')?.value ?? '').trim();

    if (!url) { showToast('请输入下载地址', 'error'); return; }
    if (!savePath) { showToast('请选择保存位置', 'error'); return; }
    try { new URL(url); } catch (e) { showToast('请输入有效的下载地址', 'error'); return; }

    _setDisplay('custom-dl-start-btn', 'none');
    _setDisplay('custom-dl-cancel-btn', '');
    _setDisplay('custom-dl-progress', '');
    const fill = document.getElementById('custom-dl-progress-fill');
    if (fill) fill.style.width = '0%';
    _setText('custom-dl-progress-text', '0%');
    _setText('custom-dl-status', '正在连接...');

    try {
        const result = await API.startCustomDownload({ url, savePath, fileName: withExtension(fileName, url) });
        if (!result || result.error) {
            showToast((result && result.error) || '创建下载任务失败', 'error');
            resetCustomDlUI();
            return;
        }
        _customDlSessionId = result.sessionId;
        _customDlSavePath = savePath;
        // 后端可能根据 Content-Disposition / 重定向 / Content-Type 解析出了更准确的文件名，
        // 回填到输入框，让「界面上显示的名字」= 「磁盘上真正的名字」
        const resolvedName = result.fileName || fileName;
        const nameEl = document.getElementById('custom-dl-filename');
        if (nameEl && resolvedName) nameEl.value = resolvedName;
        Object.assign(customDlState, {
            sessionId: result.sessionId,
            url,
            savePath,
            fileName: resolvedName,
            status: 'running',
            progress: 0,
            message: '正在连接...',
            speed: 0,
            downloaded: 0,
            total: 0,
        });
        _emitCustomDl();
        pollCustomDlProgress();
        return result;
    } catch (e) {
        showToast('下载请求失败: ' + (e && e.message ? e.message : e), 'error');
        resetCustomDlUI();
    }
}

/** 暂停当前下载（分块/单流都会保留 .downloading 断点） */
async function pauseCustomDownload() {
    if (!_customDlSessionId) return;
    try {
        const res = await API.pauseCustomDownload(_customDlSessionId);
        if (res && res.session) {
            Object.assign(customDlState, res.session);
            _emitCustomDl();
        }
    } catch (e) {
        showToast('暂停失败: ' + ((e && e.message) || e), 'error');
    }
}

/** 继续当前下载（Range 断点续传） */
async function resumeCustomDownload() {
    if (!_customDlSessionId) return;
    try {
        const res = await API.resumeCustomDownload(_customDlSessionId);
        if (res && res.session) {
            Object.assign(customDlState, res.session);
            _emitCustomDl();
        }
        pollCustomDlProgress();
    } catch (e) {
        showToast('继续失败: ' + ((e && e.message) || e), 'error');
    }
}

/** 轮询进度（500ms） */
function pollCustomDlProgress() {
    if (_customDlPollTimer) clearInterval(_customDlPollTimer);
    _customDlPollTimer = setInterval(async () => {
        if (!_customDlSessionId) { clearInterval(_customDlPollTimer); return; }
        try {
            const data = await API.getCustomDownloadStatus(_customDlSessionId);
            if (!data || data.status === 'not_found') {
                clearInterval(_customDlPollTimer);
                resetCustomDlUI();
                return;
            }

            Object.assign(customDlState, {
                status: data.status,
                progress: data.progress || 0,
                message: data.message || '',
                speed: data.speed || 0,
                downloaded: data.downloaded || 0,
                total: data.total || 0,
                fileName: data.fileName || customDlState.fileName,
                savePath: data.savePath || customDlState.savePath,
            });

            const fill = document.getElementById('custom-dl-progress-fill');
            if (fill) fill.style.width = customDlState.progress + '%';
            _setText('custom-dl-progress-text', customDlState.progress + '%');
            _setText('custom-dl-status', customDlState.message);
            _emitCustomDl();

            if (data.status === 'completed') {
                clearInterval(_customDlPollTimer);
                showToast('下载完成！', 'success');
                resetCustomDlUI(true);
            } else if (data.status === 'failed' || data.status === 'cancelled') {
                clearInterval(_customDlPollTimer);
                showToast(data.message || '下载失败', 'error');
                resetCustomDlUI(true);
            }
        } catch (e) {
            // 单次查询失败不终止轮询（后端忙时可能偶发超时）
        }
    }, 500);
}

/** 取消下载 */
async function cancelCustomDownload() {
    if (_customDlSessionId) {
        try { await API.cancelCustomDownload(_customDlSessionId); } catch (e) {}
    }
    if (_customDlPollTimer) clearInterval(_customDlPollTimer);
    // 取消是"这次不下了"：连同已下载的半成品和表单里的地址/文件名一起清掉
    clearCustomDlForm();
    resetCustomDlUI(false);
}

/** 清空下载卡片的输入（地址/文件名）与内存态 */
function clearCustomDlForm() {
    const urlEl = document.getElementById('custom-dl-url');
    if (urlEl) urlEl.value = '';
    const nameEl = document.getElementById('custom-dl-filename');
    if (nameEl) nameEl.value = '';
    // 保存目录保留：下次下载沿用同一个目录更顺手
    Object.assign(customDlState, {
        sessionId: null,
        url: '',
        fileName: '',
        status: '',
        progress: 0,
        message: '',
        speed: 0,
        downloaded: 0,
        total: 0,
    });
    _emitCustomDl();
}

/**
 * 复位 UI
 * @param {boolean} keepState 为 true 时保留最终状态供控件展示（完成后仍显示文件名/100%）
 */
function resetCustomDlUI(keepState) {
    _customDlSessionId = null;
    _setDisplay('custom-dl-start-btn', '');
    _setDisplay('custom-dl-cancel-btn', 'none');
    if (!keepState) {
        customDlState.status = '';
        customDlState.progress = 0;
        customDlState.message = '';
        _emitCustomDl();
    }
}

// ─────────────────────────────────────────────────────────────
// 浏览器接管：取走启动器塞进来的待确认请求
// ─────────────────────────────────────────────────────────────

/**
 * 拉取一条待处理请求并填进下载控件（不自动开始）。
 * @returns {Promise<object|null>} 取到的请求，没有则 null
 */
async function fetchPendingDownload() {
    try {
        const res = await API.getPendingDownload();
        const req = res && res.request;
        if (!req || !req.url) return null;
        applyPendingDownload(req);
        return req;
    } catch (e) {
        console.error('[custom-dl] 拉取待处理请求失败:', e);
        return null;
    }
}



/** 扩展/浏览器给的占位名（对用户没意义），一律当成"还不知道" */
function isPlaceholderName(name) {
    const s = String(name || '').trim().toLowerCase();
    return !s || s === 'download.bin' || s === 'download' || s === 'file' || s === 'unknown';
}

/** 从 URL 猜文件名（含扩展名），用于扩展只给了 url 没给 filename 的情况 */
function guessNameFromUrl(url) {
    try {
        const u = new URL(url);
        const seg = decodeURIComponent(u.pathname.split('/').filter(Boolean).pop() || '');
        if (seg && /\.[A-Za-z0-9]{1,8}$/.test(seg)) return seg;
    } catch (e) {}
    return '';
}

/** 用户填的名字没有扩展名时，用 URL 里的扩展名补齐 */
function withExtension(name, url) {
    const n = isPlaceholderName(name) ? '' : String(name || '').trim();
    if (!n) return guessNameFromUrl(url);
    if (/\.[A-Za-z0-9]{1,8}$/.test(n)) return n;
    const g = guessNameFromUrl(url);
    const m = /\.([A-Za-z0-9]{1,8})$/.exec(g);
    return m ? n + '.' + m[1] : n;
}


/** 向后端要真实文件名并回填（只在名字未知时调用） */
let _resolveSeq = 0;
async function resolvePendingName(req) {
    const url = (req && req.url) || '';
    if (!url) return;
    const seq = ++_resolveSeq;
    const nameInput = document.getElementById('custom-dl-filename');
    const statusEl = document.getElementById('custom-dl-status');
    if (statusEl && (!nameInput || !nameInput.value)) _setText('custom-dl-status', '正在识别文件名…');
    try {
        const res = await API.resolveCustomDownloadName({ url });
        if (seq !== _resolveSeq) return; // 期间又来了新请求，丢弃旧结果
        const name = (res && res.fileName) || '';
        if (!name) {
            if (statusEl) _setText('custom-dl-status', customDlState.message || '已收到浏览器下载请求');
            return;
        }
        customDlState.fileName = name;
        if (nameInput) nameInput.value = name;
        if (statusEl) _setText('custom-dl-status', customDlState.message || '已收到浏览器下载请求');
        _emitCustomDl();
    } catch (e) {
        if (statusEl) _setText('custom-dl-status', customDlState.message || '');
    }
}

/**
 * 把一条下载请求填进界面（只填表，绝不自动开始）
 * 由主页下载控件与工具箱卡片共用。
 */
function applyPendingDownload(req) {
    if (!req || !req.url) return;

    Object.assign(customDlState, {
        sessionId: null,
        url: req.url,
        savePath: req.folder || customDlState.savePath || _customDlSavePath || '',
        fileName: withExtension(req.filename, req.url),
        status: 'pending',
        progress: 0,
        message: '已收到浏览器下载请求，确认后点「开始下载」',
        speed: 0,
        downloaded: 0,
        total: 0,
    });

    const urlInput = document.getElementById('custom-dl-url');
    if (urlInput) urlInput.value = req.url;
    const nameInput = document.getElementById('custom-dl-filename');
    if (nameInput) nameInput.value = withExtension(req.filename, req.url);

    // 名字还是未知（URL 里没有文件名，扩展也没给）：
    // 让后端去问服务器（Content-Disposition / 302 / Content-Type），
    // 这样卡片在点「开始下载」之前就能显示 correctname.exe 而不是 download.bin。
    if (!nameInput || isPlaceholderName(nameInput.value) || !/\.[A-Za-z0-9]{1,8}$/.test(nameInput.value)) {
        resolvePendingName(req);
    }
    const pathInput = document.getElementById('custom-dl-path');
    if (pathInput && req.folder) pathInput.value = req.folder;

    _setDisplay('custom-dl-progress', 'none');
    _setText('custom-dl-status', customDlState.message);
    _emitCustomDl();
}

/** 补齐保存目录：没有就用后端记住的默认目录 */
async function ensureCustomDlFolder() {
    if (customDlState.savePath || _customDlSavePath) return customDlState.savePath || _customDlSavePath;
    try {
        const res = await API.getCustomDownloadFolder();
        const dir = (res && res.folder) || '';
        if (dir) {
            customDlState.savePath = dir;
            _customDlSavePath = dir;
            const input = document.getElementById('custom-dl-path');
            if (input) input.value = dir;
            _emitCustomDl();
        }
        return dir;
    } catch (e) {
        return '';
    }
}

// 迁移过渡：保持旧脚本与模板可用的全局名（Phase 5 移除）
export {
    clearCompletedDownloads, browseCustomDlPath, openCustomDlFolder, startCustomDownload,
    pollCustomDlProgress, cancelCustomDownload, resetCustomDlUI, dlManager, _customDlSessionId,
    _customDlPollTimer, _customDlSavePath,
    // 主页下载控件与浏览器接管所需
    customDlState, onCustomDownloadChange, fetchPendingDownload, applyPendingDownload,
    ensureCustomDlFolder, pauseCustomDownload, resumeCustomDownload,
};
