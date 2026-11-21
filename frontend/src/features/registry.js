// features/registry.js — 首屏必需模块
// 页面专属功能不再首屏静态加载，由页面组件或 versions-nav 的动态加载表按需引入。
import './settings/wallpaper-engine.js';
import './misc/dynamic-island.js';
import './misc/sponsors.js';
import './settings/custom-theme.js';
import './settings/liquid-glass.js';
import './settings/personalize.js';
import './settings/window-theme.js';
import './misc/announcement.js';
import './settings/settings-misc.js';
import './settings/updater-ui.js';
import './misc/ui-components.js';
import './misc/utils.js';
import './misc/horizon.js';
import './misc/custom-select.js';
import './misc/console-log.js';
import './misc/feature-flags.js';
import './misc/plugin-runtime.js';
import './launch/launch.js';
import './launch/java.js';
import './launch/init-setup.js';
import './settings/settings-load-save.js';
import './versions/versions-actions.js';
import './accounts/accounts-actions.js';
// 拖拽导入整合包依赖该模块在启动时注册 Tauri 拖放监听
import './mods/modpack-import.js';

export const featureModuleCount = 22;
