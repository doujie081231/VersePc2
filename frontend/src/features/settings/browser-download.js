/**
 * browser-download.js — 设置页「浏览器下载接管」区块
 *
 * 现在的设计只有两件事：
 *   1. 显示「是否就绪」（扩展装没装 + 本机宿主注册没注册）
 *   2. 点安装 → 注册本机宿主，然后跳浏览器扩展商店让用户装扩展
 *
 * 扩展本体走 Edge 加载项商店分发，不再从 exe 里释放文件、不再往浏览器写
 * 「外部扩展」注册表（那条路 Chrome/Edge 137+ 已经不放行了）。
 *
 * 对应后端：src-tauri/src/bridge/register.rs、lib.rs 的 open_extension_store
 */

import { reactive } from 'vue';
import { bridge } from '../../core/tauri-bridge.js';
import { showToast } from '../misc/ui-components.js';

export const EXTENSION_STORE_URL =
    'https://microsoftedge.microsoft.com/addons/detail/hjimmjkhbblbbbbmhlagchfdohoelbbi';

const state = reactive({
    hostInstalled: false,   // 本机原生消息宿主已注册
    statusError: '',
    busy: false,
});

/** 调后端 Tauri 命令（统一走 core/tauri-bridge，不直接摸宿主全局） */
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

/** 打开扩展商店（优先 Edge，没装 Edge 用默认浏览器） */
export function openExtensionStore() {
    try {
        if (bridge && typeof bridge.openExtensionStore === 'function') {
            bridge.openExtensionStore().catch(() => bridge.openExternal(EXTENSION_STORE_URL));
        } else {
            bridge.openExternal(EXTENSION_STORE_URL);
        }
    } catch (e) {
        showToast('打开扩展商店失败，请手动访问 ' + EXTENSION_STORE_URL, 'error');
    }
}

/** 刷新就绪状态 */
export async function refreshBrowserBridgeStatus() {
    const res = await invoke('bridge_native_host_status');
    state.hostInstalled = !!res.installed;
    state.statusError = res.error || '';
    return state;
}

/** 安装：注册本机宿主 → 打开扩展商店装扩展 */
export async function installBrowserBridge() {
    if (state.busy) return;
    state.busy = true;
    try {
        const res = await invoke('bridge_install_native_host');
        if (res && res.ok === false) {
            showToast('注册本机通信失败：' + (res.error || '未知错误'), 'error');
            return;
        }
        await refreshBrowserBridgeStatus();
        showToast('正在打开扩展商店，装好后回到这里点「重新检测」', 'success');
        openExtensionStore();
    } finally {
        state.busy = false;
    }
}

/** 卸载本机宿主注册 */
export async function uninstallBrowserBridge() {
    if (state.busy) return;
    state.busy = true;
    try {
        const res = await invoke('bridge_uninstall_native_host');
        if (res.ok) showToast('已移除本机通信注册', 'success');
        else showToast('移除失败：' + (res.error || '未知错误'), 'error');
        await refreshBrowserBridgeStatus();
    } finally {
        state.busy = false;
    }
}

/** 进页面即刷新 */
export function initBrowserBridgeCard() {
    refreshBrowserBridgeStatus().catch(() => {});
}

export { state as browserBridgeState };
