// vue/mount.js — Vue 页面挂载入口
// 主页立即挂载；其余页面按每批 4 个动态加载，保留原有的分批挂载节奏。
import { createApp } from 'vue';
import PageHome from './pages/page-home.js';
import { markVueMountDone } from '../core/vue-ready.js';

const OTHER_PAGES = [
  ['PageVersions', '#page-versions', () => import('./pages/page-versions.js')],
  ['PageInstalledVersions', '#page-installed-versions', () => import('./pages/page-installed-versions.js')],
  ['PageVersionDetail', '#page-version-detail', () => import('./pages/page-version-detail.js')],
  ['PageVersionSettings', '#page-version-settings', () => import('./pages/page-version-settings.js')],
  ['PageMods', '#page-mods', () => import('./pages/page-mods.js')],
  ['PageModDetail', '#page-mod-detail', () => import('./pages/page-mod-detail.js')],
  ['PageModFavorites', '#page-mod-favorites', () => import('./pages/page-mod-favorites.js')],
  ['PageModpacks', '#page-modpacks', () => import('./pages/page-modpacks.js')],
  ['PageDatapacks', '#page-datapacks', () => import('./pages/page-datapacks.js')],
  ['PageResourcepacks', '#page-resourcepacks', () => import('./pages/page-resourcepacks.js')],
  ['PageShaders', '#page-shaders', () => import('./pages/page-shaders.js')],
  ['PageLanTerracotta', '#page-lan-terracotta', () => import('./pages/page-lan-terracotta.js')],
  ['PageLanPortmap', '#page-lan-portmap', () => import('./pages/page-lan-portmap.js')],
  ['PageLanEnderlink', '#page-lan-enderlink', () => import('./pages/page-lan-enderlink.js')],
  ['PagePlugins', '#page-plugins', () => import('./pages/page-plugins.js')],
  ['PagePrivateServer', '#page-private-server', () => import('./pages/page-private-server.js')],
  ['PageServerHost', '#page-server-host', () => import('./pages/page-server-host.js')],
  ['PageToolbox', '#page-toolbox', () => import('./pages/page-toolbox.js')],
  ['PageCommunity', '#page-community', () => import('./pages/page-community.js')],
  ['PageSkin', '#page-skin', () => import('./pages/page-skin.js')],
  ['PageAccounts', '#page-accounts', () => import('./pages/page-accounts.js')],
  ['PageSettingsLaunch', '#page-settings-launch', () => import('./pages/page-settings-launch.js')],
  ['PageSettingsPersonalize', '#page-settings-personalize', () => import('./pages/page-settings-personalize.js')],
  ['PageSettingsOther', '#page-settings-other', () => import('./pages/page-settings-other.js')],
  ['PageJava', '#page-java', () => import('./pages/page-java.js')],
  ['PageDownloads', '#page-downloads', () => import('./pages/page-downloads.js')],
  ['PageFeedback', '#page-feedback', () => import('./pages/page-feedback.js')],
  ['PageCloud', '#page-cloud', () => import('./pages/page-cloud.js')],
];

function mountBatch(state) {
  const { list, indexRef } = state;
  const batch = list.slice(indexRef.value, indexRef.value + 4);
  indexRef.value += batch.length;

  Promise.all(
    batch.map(async ([name, selector, loader]) => {
      try {
        if (!loader) return;
        const mod = await loader();
        const component = mod && (mod.default || mod);
        if (component) createApp(component).mount(selector);
      } catch (e) {
        console.error('Vue mount failed:', name, e);
      }
    })
  ).then(() => {
    if (indexRef.value >= list.length) {
      markVueMountDone();
      return;
    }
    const schedule = window.requestIdleCallback || ((cb) => setTimeout(cb, 16));
    schedule(() => mountBatch(state), { timeout: 100 });
  });
}

window.addEventListener('DOMContentLoaded', function () {
  // 主页优先同步挂载，保证首屏最快可见
  createApp(PageHome).mount('#page-home');

  const state = { list: OTHER_PAGES, indexRef: { value: 0 } };
  setTimeout(() => {
    const schedule = window.requestIdleCallback || ((cb) => setTimeout(cb, 16));
    schedule(() => mountBatch(state), { timeout: 100 });
  }, 0);

  // 开发环境追加组件基座预览页（生产构建由 DEV 判断短路）
  if (import.meta.env.DEV) {
    import('./dev-preview.js').then((m) => m.mountDevPreview()).catch(() => {});
  }

  // 内置网页登录会在主窗口临时跳到站点登录页，回来后自动回到原来的面板页
  try {
    const back = window.localStorage.getItem('versePanelReturn');
    if (back) {
      window.localStorage.removeItem('versePanelReturn');
      setTimeout(() => {
        import('../features/versions/versions-nav.js')
          .then((m) => m.navigateToPage(back))
          .catch(() => {});
      }, 600);
    }
  } catch (e) {}
});
