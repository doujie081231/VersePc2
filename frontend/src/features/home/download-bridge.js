/**
 * download-bridge.js — 浏览器下载接管的前端接入层
 *
 * 职责：
 *   1. 启动后拉一次「待处理下载请求」（来自扩展或启动参数）
 *   2. 监听后端推送的 download-request 事件
 *   3. 把请求交给下载控件填表（绝不自动开始下载）
 *   4. 自动把「下载」控件挂到主页（若用户还没添加）
 *
 * 约束：
 *   - 不直接操作下载业务，只做「收取 → 转交」
 *   - 事件订阅失败不抛错（非 Tauri 环境预览时也要能跑）
 */

import { bridge } from '../../core/tauri-bridge.js';
import { showToast } from '../misc/ui-components.js';
import { fetchPendingDownload, applyPendingDownload } from '../versions/custom-download.js';
import { ensureWidgetVisible, onPendingDownloadRequest } from './home-widgets.js';

let _initialized = false;
let _unlisten = null;

/**
 * 处理一条下载请求：确保主页有下载控件 → 填表 → 切到主页 → 提示用户
 * @param {object} req { url, filename, folder, source }
 */
export async function handleDownloadRequest(req) {
    if (!req || !req.url) return;

    // 1. 确保主页上有下载控件（用户没添加过就自动加上）
    const added = ensureWidgetVisible('download');

    // 2. 广播给订阅者（主页下载控件、工具箱卡片都会收到）
    onPendingDownloadRequest(req);

    // 3. 填进界面
    try {
        applyPendingDownload(req);
    } catch (e) {
        console.error('[download-bridge] 填充请求失败:', e);
    }

    // 4. 回到主页，让用户看见
    try {
        const { navigateToPage } = await import('../../core/navigation.js');
        if (typeof navigateToPage === 'function') navigateToPage('home');
    } catch (e) {}

    if (added) {
        showToast('已收到浏览器下载请求，并把「下载」控件添加到主页', 'info');
    } else {
        showToast('已收到浏览器下载请求，确认后点「开始下载」', 'info');
    }
}

/**
 * 初始化：只在启动时调用一次
 */
export function initDownloadBridge() {
    if (_initialized) return;
    _initialized = true;

    // ① 后端主动推送（已运行实例收到管道请求 / 窗口被唤起时）
    try {
        const onReq = bridge && bridge.onDownloadRequest;
        if (typeof onReq === 'function') {
            const un = onReq((payload) => {
                handleDownloadRequest(payload).catch((e) => console.error('[download-bridge]', e));
            });
            _unlisten = typeof un === 'function' ? un : null;
        }
    } catch (e) {
        console.warn('[download-bridge] 事件订阅失败（非 Tauri 环境？）:', e);
    }

    // ② 拉取一次队列（冷启动：参数已排队，窗口出来时取走）
    Promise.resolve()
        .then(() => fetchPendingDownload())
        .then((req) => {
            if (req) return handleDownloadRequest(req);
        })
        .catch((e) => console.error('[download-bridge] 拉取待处理请求失败:', e));

    // ③ 兜底轮询：管道转发的请求可能早于前端订阅就绪。
    //    前 30 秒内每 2 秒补拉一次；拉到就继续把队列抽干（可能一次积压多条），
    //    而不是拉一条就停——否则后面的请求会被白白丢掉。
    let ticks = 0;
    const drain = async () => {
        for (let i = 0; i < 8; i++) {
            const req = await fetchPendingDownload();
            if (!req) break;
            await handleDownloadRequest(req);
        }
    };
    const timer = setInterval(() => {
        ticks += 1;
        if (ticks > 15) { clearInterval(timer); return; }
        drain().catch(() => {});
    }, 2000);
}

/** 卸载监听（热重载/调试用） */
export function disposeDownloadBridge() {
    if (_unlisten) { try { _unlisten(); } catch (e) {} _unlisten = null; }
    _initialized = false;
}
