/* plugin-runtime.js - 插件前端运行时
 * 说明：扫描已安装插件，按其 plugin.json 的 ui 声明动态注册：
 *   main   : 插件主脚本（与启动器前端同一环境执行，可任意自定义前端）
 *   style  : 注入全局 CSS（主题/外观覆盖）
 *   pages  : 新增整页（侧边栏按钮 + SVG + Vue 组件）
 *   cards  : 向现有页面注入卡片（Vue 组件）
 *   pet    : 宠物/挂件（个性化浮层组件）
 * 页面走现有 navigateToPage 路由；cards/pet/main 均依赖同一 webview 环境的全局对象。
 */
(function () {
  if (!window.bridge || !window.bridge.invoke) return;

  const COMP_PREFIX = 'Plugin_';
  const PAGE_PREFIX = 'plugin_';

  function waitForVue(cb) {
    if (window.Vue && window._vueMountDone) { cb(); return; }
    const t = setInterval(() => {
      if (window.Vue && window._vueMountDone) { clearInterval(t); cb(); }
    }, 200);
    setTimeout(() => clearInterval(t), 20000);
  }

  // 兼容三种返回：Base64 字符串 / number[] / Uint8Array
  async function bytesToText(res) {
    if (typeof res === 'string') {
      const bin = atob(res);
      const bytes = new Uint8Array(bin.length);
      for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
      return new TextDecoder('utf-8').decode(bytes);
    }
    const arr = res instanceof Uint8Array ? res : new Uint8Array(res || []);
    return new TextDecoder('utf-8').decode(arr);
  }

  async function readText(dir, file) {
    if (!file) return '';
    // 正确入口是顶层 window.bridge.readFileBuffer（system 子命名空间未实现）
    const res = await window.bridge.readFileBuffer(dir + '\\' + file);
    return bytesToText(res);
  }

  function execPluginScript(code) {
    (0, eval)(code);
  }

  // 读取插件内 JS 并执行（使插件能注册组件/做任意定制）
  async function loadPluginScriptFile(dir, file) {
    const code = await readText(dir, file);
    execPluginScript(code);
  }

  // 注入全局 CSS
  function injectStyle(cssText, id) {
    if (!cssText) return;
    if (id && document.getElementById(id)) return;
    const el = document.createElement('style');
    if (id) el.id = id;
    el.textContent = cssText;
    document.head.appendChild(el);
  }

  // 轮询等待选择器出现（cards 目标容器可能在某页面渲染后才存在）
  function waitForSelector(selector, cb, timeout) {
    const found = document.querySelector(selector);
    if (found) { cb(found); return; }
    let tries = 0;
    const t = setInterval(() => {
      tries++;
      const el = document.querySelector(selector);
      if (el || tries > (timeout || 30)) {
        clearInterval(t);
        if (el) cb(el);
      }
    }, 400);
  }

  // 加载并挂载 Vue 组件到容器（组件注册名 Plugin_<id>_<key>）
  async function mountComponent(pid, key, container) {
    const compKey = COMP_PREFIX + pid + '_' + key;
    if (!window.VersePC[compKey]) return false;
    if (container.__plugin_mounted) return true;
    try {
      container.__plugin_mounted = true;
      window.Vue.createApp(window.VersePC[compKey]).mount(container);
      return true;
    } catch (e) {
      console.error('[plugin-runtime] mount 失败 ' + compKey, e);
      return false;
    }
  }

  async function buildPages(plugin, dir) {
    const pages = Array.isArray(plugin.ui.pages) ? plugin.ui.pages : [];
    if (!pages.length) return;
    const pid = plugin.id;
    const contentArea = document.querySelector('.content-area');
    const navAnchor = document.getElementById('nav-plugins-btn');

    for (const p of pages) {
      const pageId = PAGE_PREFIX + pid + '_' + p.id;
      const compKey = COMP_PREFIX + pid + '_' + p.id;

      let pageEl = document.getElementById('page-' + pageId);
      if (!pageEl) {
        pageEl = document.createElement('div');
        pageEl.id = 'page-' + pageId;
        pageEl.className = 'page';
        contentArea.appendChild(pageEl);
      }

      if (navAnchor) {
        let btn = document.getElementById('nav-btn-' + pageId);
        if (!btn) {
          btn = document.createElement('button');
          btn.id = 'nav-btn-' + pageId;
          btn.className = 'nav-btn plugin-page-btn';
          btn.setAttribute('data-page', pageId);
          btn.setAttribute('title', p.title || p.id);
          btn.innerHTML = '<svg class="nav-icon-svg plugin-page-icon"></svg><span>' + (p.title || p.id) + '</span>';
          navAnchor.insertAdjacentElement('afterend', btn);
          btn.addEventListener('click', function () {
            if (typeof navigateToPage === 'function') navigateToPage(pageId);
          });
          if (p.icon) {
            readText(dir, p.icon).then(function (svg) {
              const ico = btn.querySelector('.plugin-page-icon');
              if (ico && svg) ico.innerHTML = svg;
            }).catch(function () {});
          }
        }
      }

      try {
        if (!window.VersePC[compKey] && p.component) {
          await loadPluginScriptFile(dir, p.component);
        }
        mountComponent(pid, p.id, pageEl);
      } catch (e) {
        console.error('[plugin-runtime] 页面组件加载失败 ' + compKey, e);
      }
    }
  }

  async function buildCards(plugin, dir) {
    const cards = Array.isArray(plugin.ui.cards) ? plugin.ui.cards : [];
    const pid = plugin.id;
    for (const c of cards) {
      const key = 'card_' + (c.id || c.component);
      try {
        if (c.component) await loadPluginScriptFile(dir, c.component);
      } catch (e) {
        console.error('[plugin-runtime] 卡片组件加载失败 ' + c.component, e);
        continue;
      }
      if (!c.anchor) continue;
      const slotId = 'plugin-card-' + pid + '-' + (c.id || c.component);
      waitForSelector(c.anchor, function (target) {
        if (target.querySelector('#' + slotId)) return;
        const slot = document.createElement('div');
        slot.id = slotId;
        slot.className = 'plugin-card-slot';
        target.appendChild(slot);
        mountComponent(pid, key, slot);
      }, 30);
    }
  }

  async function buildPet(plugin, dir) {
    const pet = plugin.ui.pet;
    if (!pet || !pet.component) return;
    const pid = plugin.id;
    try {
      await loadPluginScriptFile(dir, pet.component);
    } catch (e) {
      console.error('[plugin-runtime] 宠物挂件组件加载失败', e);
      return;
    }
    let host = document.getElementById('plugin-pet-layer');
    if (!host) {
      host = document.createElement('div');
      host.id = 'plugin-pet-layer';
      host.style.cssText = 'position:fixed;right:16px;bottom:16px;z-index:9999;pointer-events:none;';
      document.body.appendChild(host);
    }
    mountComponent(pid, 'pet', host);
  }

  async function buildPlugin(plugin) {
    const dir = plugin.installedDir || '';
    const ui = plugin.ui || {};

    // main：插件主脚本，先进环境，可做任意前端定制
    if (ui.main) {
      try { await loadPluginScriptFile(dir, ui.main); } catch (e) { console.error('[plugin-runtime] main 脚本失败 ' + plugin.id, e); }
    }
    // style：注入全局样式
    if (ui.style) {
      try {
        const css = await readText(dir, ui.style);
        injectStyle(css, 'plugin-style-' + plugin.id);
      } catch (e) { console.error('[plugin-runtime] style 注入失败 ' + plugin.id, e); }
    }
    // pages / cards / pet（可并行，按需 await）
    const jobs = [];
    jobs.push(buildPages(plugin, dir));
    jobs.push(buildCards(plugin, dir));
    jobs.push(buildPet(plugin, dir));
    await Promise.all(jobs);
  }

  // 重扫前清理所有由插件运行时创建/注入的 UI 与组件，
  // 使卸载插件后按钮/页面/卡片/挂件/样式都能被移除（再按当前已装重建）。
  function cleanupAllRuntimeUI() {
    if (window.VersePC) {
      Object.keys(window.VersePC)
        .filter(function (k) { return k.indexOf('Plugin_') === 0; })
        .forEach(function (k) { delete window.VersePC[k]; });
    }
    document.querySelectorAll('.plugin-page-btn').forEach(function (e) { e.remove(); });
    document.querySelectorAll('[id^="page-plugin_"]').forEach(function (e) { e.remove(); });
    document.querySelectorAll('[id^="plugin-card-"]').forEach(function (e) { e.remove(); });
    document.querySelectorAll('[id^="plugin-style-"]').forEach(function (e) { e.remove(); });
    var pet = document.getElementById('plugin-pet-layer');
    if (pet) pet.remove();
  }

  async function init() {
    cleanupAllRuntimeUI();
    let res;
    try { res = await window.bridge.invoke('plugin_list'); }
    catch (e) { console.error('[plugin-runtime] 插件列表失败', e); return; }
    const plugins = (res && res.plugins) ? res.plugins : [];
    for (const pl of plugins) {
      if (pl && pl.ui) {
        try { await buildPlugin(pl); } catch (e) { console.error('[plugin-runtime] buildPlugin 失败 ' + pl.id, e); }
      }
    }
  }

  window.addEventListener('DOMContentLoaded', function () {
    waitForVue(function () { init(); });
  });

  // 暴露重扫入口：插件安装/卸载后可调用，使侧边栏按钮/页面/卡片/挂件即时生效
  window.VersePC = window.VersePC || {};
  window.VersePC.reloadPluginUI = init;
})();