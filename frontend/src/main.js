// main.js — 前端模块入口
// 1. 按原加载顺序导入功能模块；
// 2. 安装统一事件委托与错误提示通道；
// 3. 启动 DOMContentLoaded 初始化流程。
// 样式入口：Tailwind 变量与工具类（含 shadcn 语义 token 映射），需最先执行
import './styles/tailwind.css';
import './features/misc/rose-loader.js';
import './features/registry.js';
import './features/misc/translate-modal-tabs.js';
import './core/plugin-compat.js';
import { installImageFallback } from './core/image-fallback.js';
import './vue/mount.js';
import { init } from './features/launch/init-setup.js';
import { initSettingsPages } from './features/settings/settings-misc.js';
import { renderSponsors } from './features/misc/sponsors.js';
import { showToast } from './features/misc/ui-components.js';
import { setApiNotifier } from './core/api.js';
import { bridge } from './core/tauri-bridge.js';
import { installActionDelegation, registerActions } from './core/actions.js';
import { mountStaticModals } from './vue/modals/static-modals.js';
import { mountStaticIcons } from './core/static-icons.js';
import { startGameClock } from './core/game-store.js';

installActionDelegation();
installImageFallback();
setApiNotifier(showToast);
registerActions({
  openExternal: (event, el, url) => bridge.openExternal(url),
});

// 平台功能显隐：仅隐藏 Windows 专属入口
function hideWinOnlyFeatures() {
  document.querySelectorAll('.win-only').forEach((el) => (el.style.display = 'none'));
}

document.addEventListener('DOMContentLoaded', () => {
  // 静态资源在初始化前挂载，保证旧逻辑按 ID 查找元素时已存在
  mountStaticIcons();
  mountStaticModals();
  startGameClock();
  init();
  setTimeout(initSettingsPages, 500);
  renderSponsors();

  if (!bridge.isWindows) {
    hideWinOnlyFeatures();
    // 设置页模板延迟插入，稍后再补一次，确保平台专属入口被隐藏
    setTimeout(hideWinOnlyFeatures, 700);
  }
});

export const buildMode = 'vite';
