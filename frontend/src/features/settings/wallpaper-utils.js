// 壁纸路径与全局下载进度：独立成模块，供引擎、渲染器与主流程共用，
// 避免渲染器为了工具函数反向依赖壁纸总入口。
import { bridge } from '../../core/tauri-bridge.js';
import { S } from './wallpaper-state.js';

export function wpfilePath(filePath) {
    if (!filePath) return '';
    if (filePath.startsWith('wpfile://') || filePath.startsWith('blob:')) return filePath;
    const normalized = filePath.replace(/\\/g, '/');
    const segs = normalized.split('/').map(encodeURIComponent).join('/');
    // 与 wewp 同理：WebView2 把自定义协议映射成 http://<scheme>.localhost，
    // <video>/<img> 直接用 http 地址才能在 Windows 上正常加载。
    const isWindows = !bridge || bridge.isWindows;
    return isWindows ? 'http://wpfile.localhost/' + segs : 'wpfile:///' + segs;
}

export function wewpVideoPath(workshopId, relFile) {
    if (!workshopId || !relFile) return '';
    const segs = String(relFile).split(/[\\/]/).map(encodeURIComponent).join('/');
    // WebView2 会把自定义协议映射成 http://<scheme>.localhost；
    // <video> 不能直接吃 wewp://，必须用映射后的 http 地址（Windows 专属功能）。
    const isWindows = !bridge || bridge.isWindows;
    const origin = isWindows ? 'http://wewp.localhost' : 'wewp://localhost';
    return origin + '/' + encodeURIComponent(workshopId) + '/' + segs;
}

export function _ensureGlobalDlBar() {
    if (S._globalDlBarEl) return S._globalDlBarEl;
    const el = document.createElement('div');
    el.id = 'global-download-progress-bar';
    el.style.cssText = 'position:fixed;left:50%;bottom:28px;transform:translateX(-50%);z-index:99999;' +
        'width:min(420px,80vw);background:var(--bg-card,#14171d);border:1px solid var(--border,#2a2f3a);' +
        'border-radius:12px;padding:12px 16px;box-shadow:0 8px 30px rgba(0,0,0,.35);display:none;';
    el.innerHTML =
        '<div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:8px;font-size:13px;font-weight:600;">' +
        '<span id="gdlb-title">正在下载</span>' +
        '<span id="gdlb-pct" style="color:var(--text-muted);font-weight:400;">0%</span></div>' +
        '<div style="height:6px;background:var(--bg-secondary,#1a1d24);border-radius:3px;overflow:hidden;">' +
        '<div id="gdlb-fill" style="height:100%;width:0%;background:var(--accent,#4c8dff);border-radius:3px;transition:width .2s;"></div></div>' +
        '<div id="gdlb-info" style="margin-top:6px;font-size:11px;color:var(--text-muted);"></div>';
    document.body.appendChild(el);
    S._globalDlBarEl = el;
    return el;
}

export function showGlobalDownloadProgress(title, percent, detail) {
    const el = _ensureGlobalDlBar();
    const titleEl = el.querySelector('#gdlb-title');
    const pctEl = el.querySelector('#gdlb-pct');
    const fillEl = el.querySelector('#gdlb-fill');
    const infoEl = el.querySelector('#gdlb-info');
    if (title) titleEl.textContent = title;
    const p = Math.max(0, Math.min(100, Math.round(percent || 0)));
    pctEl.textContent = p + '%';
    fillEl.style.width = p + '%';
    if (detail) infoEl.textContent = detail;
    el.style.display = 'block';
}

export function hideGlobalDownloadProgress() {
    if (S._globalDlBarEl) S._globalDlBarEl.style.display = 'none';
}
