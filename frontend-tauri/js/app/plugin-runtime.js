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

  // ===== 插件沙箱（权限模型） =====
  // 按 plugin.json 声明的 permissions 决定放行能力，未授权的一律拦截：
  //   - 无条件放行（低风险）：get_app_version、store_*、clipboard、selectFile/showOpenDialog、
  //       openExternal、readFileBuffer（仅限插件自身目录）。
  //   - permissions.network === true        → 放行 fetch / apiProxy / XHR / WebSocket 等外部网络。
  //   - permissions.native 含 "exec"        → 放行进程托管（启动/停止/状态/日志/退出事件）。
  // 说明：JS 无强隔离，本沙箱能阻断普通第三方插件的越权行为，不防护刻意构造的作用域逃逸。

  // 后端命令白名单（无害/只读/本地配置类）
  const PLUGIN_ALLOWLIST = ['get_app_version', 'store_get', 'store_set', 'store_delete'];

  function _pluginDeny(pid, what) {
    console.warn('[plugin-sandbox] 拦截插件 ' + pid + ' 的越权调用: ' + what);
    return Promise.reject(new Error('permission denied by sandbox: ' + what));
  }

  function _hasPerm(plugin, key) {
    return !!(plugin && plugin.permissions && plugin.permissions[key]);
  }
  function _hasNativeExec(plugin) {
    var native = plugin && plugin.permissions && plugin.permissions.native;
    return Array.isArray(native) && native.indexOf('exec') >= 0;
  }

  // 受限 bridge：只暴露低风险 + 已授权能力；未列出的桥能力一律拒绝
  function _restrictedBridge(pid, dir, plugin) {
    var base = String(dir || '').toLowerCase().replace(/[\\/]+$/, '');
    var allowNetwork = _hasPerm(plugin, 'network');
    var hasExec = _hasNativeExec(plugin);
    var native = window.bridge || {};
    var restricted = {
      invoke: function (cmd, args) {
        cmd = String(cmd || '');
        if (PLUGIN_ALLOWLIST.indexOf(cmd) >= 0) return native.invoke(cmd, args);
        if (allowNetwork && cmd === 'api_proxy') {
          return native.invoke('api_proxy', Object.assign({}, args || {}, { method: (args && args.method) || 'GET' }));
        }
        if (hasExec && (cmd === 'plugin_process_exec' || cmd === 'plugin_process_stop' || cmd === 'plugin_process_status')) {
          // 闭包锁定本插件 id，防伪造他人进程；探针返回真值
          if (args && args.__probe) return Promise.resolve({ ok: true, _probe: true });
          return native.invoke(cmd, Object.assign({}, args || {}, { pluginId: pid }));
        }
        return _pluginDeny(pid, 'invoke:' + cmd);
      },
      readFileBuffer: function (path) {
        path = String(path || '');
        if (base && path.toLowerCase().indexOf(base + '\\') === 0) {
          return native.invoke('read_file_buffer', { path: path });
        }
        return _pluginDeny(pid, 'readFileBuffer:' + path);
      },
      apiProxy: function (method, path, params, body) {
        if (!allowNetwork) return _pluginDeny(pid, 'apiProxy');
        return native.invoke('api_proxy', {
          method: method || 'GET',
          path: path || '',
          params: params || {},
          body: body || null
        }).then(function (r) {
          // 兼容 api.js 对 fetch Response 的模拟
          var status = (r && r.status) || 500;
          return { ok: status < 400, status: status, json: function () { return Promise.resolve(r && r.body); }, _raw: r };
        });
      },
      // 本地/用户触发/只读能力：直接透传
      store: native.store,
      clipboard: native.clipboard,
      openExternal: native.openExternal,
      selectFile: native.selectFile,
      showOpenDialog: native.showOpenDialog,
      showSaveDialog: native.showSaveDialog,
      showMessageDialog: native.showMessageDialog
    };
    // exec 专属方法（声明 native:exec 才注入）
    if (hasExec) {
      restricted.exec = function (opts) {
        return native.invoke('plugin_process_exec', Object.assign({}, opts || {}, { pluginId: pid }));
      };
      restricted.stopExec = function (opts) {
        return native.invoke('plugin_process_stop', Object.assign({}, opts || {}, { pluginId: pid }));
      };
      restricted.execStatus = function (opts) {
        return native.invoke('plugin_process_status', Object.assign({}, opts || {}, { pluginId: pid }));
      };
      restricted.onExecLog = function (cb) { return _listenTauri('plugin:' + pid + ':exec-log', cb); };
      restricted.onExecExit = function (cb) { return _listenTauri('plugin:' + pid + ':exec-exit', cb); };
    }
    // 未显式提供的桥能力（窗口/开服/联机/AI/更新器等）一律拦截
    return new Proxy(restricted, {
      get: function (t, prop) {
        if (prop in t) return t[prop];
        if (typeof prop !== 'symbol') return function () { return _pluginDeny(pid, 'bridge.' + prop); };
        return undefined;
      }
    });
  }

  // 受限 window：替换 bridge/electronAPI/__TAURI__；未授权则禁用网络 API；DOM 渲染能力保留
  function _restrictedWindow(pid, dir, plugin) {
    var br = _restrictedBridge(pid, dir, plugin);
    var allowNetwork = _hasPerm(plugin, 'network');
    var guard = {
      get: function (t, prop) {
        if (prop === 'bridge' || prop === 'electronAPI' || prop === '__TAURI__' ||
            prop === '__TAURI_INTERNALS__' || prop === '__TAURI_PROXY__') return br;
        if (allowNetwork) return t[prop];
        if (prop === 'fetch') return function () { return _pluginDeny(pid, 'fetch'); };
        if (prop === 'XMLHttpRequest' || prop === 'WebSocket' || prop === 'EventSource') {
          return function () { throw new Error('network denied by sandbox'); };
        }
        if (prop === 'sendBeacon') return function () { return false; };
        return t[prop];
      },
      set: function (t, prop, val) {
        // 禁止插件把真实 bridge 写回 window，防止覆盖沙箱
        if (prop === 'bridge' || prop === 'electronAPI' || prop === '__TAURI__' ||
            prop === '__TAURI_PROXY__') return false;
        t[prop] = val;
        return true;
      },
      has: function (t, prop) { return true; }
    };
    return new Proxy(window, guard);
  }

  // 事件监听 helper（onExecLog/onExecExit 用；window.bridge 未直接暴露 listen）
  function _listenTauri(eventName, cb) {
    var evt = (window.__TAURI__ && window.__TAURI__.event)
      || (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.event)
      || window.__TAURI_INTERNALS__
      || null;
    if (!evt || typeof evt.listen !== 'function') return function () {};
    var disposed = false, un = null;
    evt.listen(eventName, function (ev) { if (disposed) return; try { cb(ev.payload); } catch (e) { console.error(e); } })
      .then(function (fn) { if (disposed) { try { fn(); } catch (_) {} } else { un = fn; } })
      .catch(function () {});
    return function () { disposed = true; if (un) { try { un(); } catch (_) {} } un = null; };
  }

  // 在隔离作用域执行插件脚本：window/self/globalThis 传受限版；fetch/XHR/WS 按权限注入
  function execPluginScript(code, plugin) {
    var pid = plugin && plugin.id;
    var dir = plugin && plugin.installedDir;
    var allowNetwork = _hasPerm(plugin, 'network');
    var w = _restrictedWindow(pid, dir, plugin);
    var br = w.bridge;
    var blockedCtor = function () { throw new Error('network denied by sandbox'); };
    var runner = new Function(
      'window', 'self', 'globalThis', 'navigator', 'document',
      'fetch', 'XMLHttpRequest', 'WebSocket', 'EventSource', 'PluginBridge',
      code
    );
    var f = allowNetwork ? window.fetch.bind(window) : function () { return _pluginDeny(pid, 'fetch'); };
    var xhr = allowNetwork ? window.XMLHttpRequest : blockedCtor;
    var ws = allowNetwork ? window.WebSocket : blockedCtor;
    var es = allowNetwork ? window.EventSource : blockedCtor;
    try {
      runner(w, w, w, window.navigator, window.document, f, xhr, ws, es, br);
    } catch (e) {
      console.error('[plugin-sandbox] 插件 ' + pid + ' 执行失败', e);
    }
  }

  // 读取插件内 JS 并在沙箱内执行（使插件能注册组件/做任意定制）
  async function loadPluginScriptFile(plugin, dir, file) {
    const code = await readText(dir, file);
    execPluginScript(code, plugin);
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
          await loadPluginScriptFile(plugin, dir, p.component);
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
        if (c.component) await loadPluginScriptFile(plugin, dir, c.component);
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
      await loadPluginScriptFile(plugin, dir, pet.component);
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
      try { await loadPluginScriptFile(plugin, dir, ui.main); } catch (e) { console.error('[plugin-runtime] main 脚本失败 ' + plugin.id, e); }
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