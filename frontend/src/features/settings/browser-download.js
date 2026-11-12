/**
 * browser-download.js — 设置页「浏览器下载接管」区块
 *
 * 一键流程（点一个按钮全搞定）：
 *   1. 把内嵌在 exe 里的扩展释放到 <data>/extension/
 *   2. 注册原生消息宿主（写注册表，无需管理员）
 *   3. 写浏览器「外部扩展」注册表
 *   4. 带 --load-extension 拉起浏览器 → 扩展立刻可用
 *
 * 对应后端：src-tauri/src/bridge/extension.rs 与 register.rs
 */

import { reactive } from 'vue';
import { bridge } from '../../core/tauri-bridge.js';
import { showToast } from '../misc/ui-components.js';

let lastHint = '';

const state = reactive({
    ready: false,          // 扩展文件已释放
    hostInstalled: false,  // 原生宿主已注册
    registryCount: 0,      // 外部扩展注册表项数
    dir: '',
    extensionId: '',
    version: '',
    storeUrl: '',
    busy: false,
});

/** 商店链接存在 localStorage，避免为了一个 URL 改后端设置结构 */
const STORE_URL_KEY = 'versepc_extension_store_url';

export function loadStoreUrl() {
    try {
        const v = localStorage.getItem(STORE_URL_KEY) || '';
        state.storeUrl = v;
        return v;
    } catch (e) {
        return '';
    }
}

export function saveStoreUrl(url) {
    const v = String(url || '').trim();
    try {
        if (v) localStorage.setItem(STORE_URL_KEY, v);
        else localStorage.removeItem(STORE_URL_KEY);
    } catch (e) {}
    state.storeUrl = v;
    return v;
}

/** 调后端 Tauri 命令（统一走 core/tauri-bridge 的 invoke，不直接访问宿主全局） */
async function invoke(cmd, args) {
    try {
        if (!bridge || typeof bridge.invoke !== 'function') {
            return { ok: false, error: '需要在 VersePC2 客户端内操作' };
        }
        const res = await bridge.invoke(cmd, args || {});
        return res === undefined || res === null ? { ok: false, error: '后端无响应' } : res;
    } catch (e) {
        return { ok: false, error: String((e && e.message) || e) };
    }
}

/** 刷新状态 */
async function refreshBrowserBridgeStatus() {
    const res = await invoke('bridge_extension_status');
    Object.assign(state, {
        ready: !!res.extracted,
        hostInstalled: !!res.hostInstalled,
        registryCount: res.registryCount || 0,
        dir: res.dir || '',
        extensionId: res.extensionId || '',
        version: res.version || '',
    });
    renderBrowserBridgeStatus(res.error);
    return state;
}

/** 画状态 */
function renderBrowserBridgeStatus(error) {
    const dot = document.getElementById('bridge-status-dot');
    const text = document.getElementById('bridge-status-text');
    const detail = document.getElementById('bridge-status-detail');
    const installBtn = document.getElementById('bridge-install-btn');
    const uninstallBtn = document.getElementById('bridge-uninstall-btn');
    const dirWrap = document.getElementById('bridge-dir-wrap');
    const dirText = document.getElementById('bridge-dir-text');

    const allReady = state.ready && state.hostInstalled;

    if (dot) {
        dot.classList.remove('is-ok', 'is-warn', 'is-bad');
        dot.classList.add(error ? 'is-bad' : allReady ? 'is-ok' : state.ready ? 'is-warn' : 'is-bad');
    }

    if (text) {
        if (error) text.textContent = '检测失败：' + error;
        else if (allReady) text.textContent = '已就绪，浏览器可以召唤 VersePC2';
        else if (state.ready) text.textContent = '扩展已释放，但原生宿主未注册';
        else text.textContent = '尚未安装';
    }

    if (detail) {
        if (error) detail.textContent = '';
        else if (lastHint) detail.textContent = lastHint;
        else if (allReady) detail.textContent = '扩展 v' + state.version + ' · 宿主已注册';
        else detail.textContent = '点「一键安装」会自动完成全部步骤并打开浏览器';
    }

    if (dirWrap) dirWrap.style.display = state.ready ? '' : 'none';
    if (dirText) dirText.textContent = state.dir;

    if (installBtn) installBtn.textContent = allReady ? '重新安装' : '一键安装';
    if (uninstallBtn) uninstallBtn.style.display = state.ready ? '' : 'none';
}

/** 一键安装：释放 → 注册宿主 → 写扩展注册表 → 拉起浏览器 */
async function installBrowserBridge() {
    if (state.busy) return;
    state.busy = true;
    const btn = document.getElementById('bridge-install-btn');
    if (btn) { btn.disabled = true; btn.textContent = '安装中...'; }

    try {
        const res = await invoke('bridge_install_extension');
        if (res.ok) {
            // 后端会如实告知「是否真的自动加载成功」，据实提示，绝不谎报
            if (res.autoLoaded) {
                showToast('扩展已安装并已加载，可以开始接管下载了', 'success');
            } else if (res.needManual) {
                showToast('文件与宿主已就绪，请在打开的浏览器页里点「加载解压缩的扩展」', 'warning');
            } else {
                showToast('扩展已安装', 'success');
            }
            lastHint = res.hint || '';
            renderBrowserBridgeStatus();
        } else {
            showToast('安装失败：' + (res.error || '未知错误'), 'error');
        }
        await refreshBrowserBridgeStatus();
    } finally {
        state.busy = false;
        if (btn) btn.disabled = false;
    }
}

/** 卸载 */
async function uninstallBrowserBridge() {
    if (state.busy) return;
    state.busy = true;
    const btn = document.getElementById('bridge-uninstall-btn');
    if (btn) btn.disabled = true;
    try {
        const res = await invoke('bridge_uninstall_extension');
        if (res.ok) {
            showToast('已移除扩展文件与注册表项', 'success');
        } else {
            showToast('卸载失败：' + (res.error || '未知错误'), 'error');
        }
        await refreshBrowserBridgeStatus();
    } finally {
        state.busy = false;
        if (btn) btn.disabled = false;
    }
}

/** 打开扩展所在文件夹（手动加载时用） */
async function openExtensionDir() {
    const ok = await invoke('bridge_open_extension_dir');
    if (!ok) showToast('扩展目录还不存在，请先点「一键安装」', 'info');
}

/** 复制扩展目录路径 */
async function copyExtensionDir() {
    if (!state.dir) { showToast('还没有扩展目录', 'info'); return; }
    try {
        if (bridge && bridge.clipboard && bridge.clipboard.writeText) {
            await bridge.clipboard.writeText(state.dir);
        } else {
            await navigator.clipboard.writeText(state.dir);
        }
        showToast('路径已复制', 'success');
    } catch (e) {
        showToast('复制失败，请手动复制', 'error');
    }
}

/** 进页面即刷新 */
function initBrowserBridgeCard() {
    refreshBrowserBridgeStatus().catch(() => {});
}

export {
    state as browserBridgeState,
    refreshBrowserBridgeStatus,
    renderBrowserBridgeStatus,
    installBrowserBridge,
    uninstallBrowserBridge,
    openExtensionDir,
    copyExtensionDir,
    initBrowserBridgeCard,
};
