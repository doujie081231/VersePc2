/**
 * js/tauri-bridge.js - Tauri 环境下的完整 electronAPI 等价适配层
 *
 * 职责：
 *   在 Tauri 环境下，提供与 Electron preload.cjs 中 window.electronAPI 等价的接口，
 *   内部通过 window.__TAURI__.core.invoke / event.listen 调用 Rust 后端。
 *
 *   包含以下模块（按原 Electron preload.cjs 结构排列）：
 *     - 窗口控制 (minimize/maximize/close/restore/state 等)
 *     - 存储 (store.get / set)
 *     - 事件 (窗口状态/关闭动画/导入进度)
 *     - 对话框 (showOpenDialog / selectFile / selectFolder)
 *     - 剪贴板 (clipboard.writeText)
 *     - 系统 (platform / memoryOptimize / openExternal / 等)
 *     - 更新器 (updater.getVersion)
 *     - 私人服务器 / 本地开服 / 红石联机（已有）
 *
 * 注意：本文件在 `frontendDist` 下直接可用，不需要打包，
 *       且在 index.html 中以第一顺位加载（早于所有业务 JS）。
 */

(function () {
  'use strict';

  // ============== 环境检测 ==============

  var isTauri = !!(window.__TAURI__ || window.__TAURI_INTERNALS__);
  if (!isTauri) return;

  function _core() {
    if (window.__TAURI__ && window.__TAURI__.core) return window.__TAURI__.core;
    // Tauri v2: __TAURI_INTERNALS__ 没有 .core，invoke/event 直接在自身
    if (window.__TAURI_INTERNALS__) return window.__TAURI_INTERNALS__;
    return null;
  }

  function _event() {
    if (window.__TAURI__ && window.__TAURI__.event) return window.__TAURI__.event;
    // Tauri v2: __TAURI_INTERNALS__ 没有 .event，listen 等直接在自身
    if (window.__TAURI_INTERNALS__) return window.__TAURI_INTERNALS__;
    return null;
  }

  async function invoke(cmd, args) {
    var c = _core();
    if (!c || !c.invoke) throw new Error('[tauri-bridge] core.invoke 不可用: ' + cmd);
    return await c.invoke(cmd, args || {});
  }

  // 事件订阅（返回取消函数）
  function onTauriEvent(eventName, callback, transform) {
    var extract = transform || function (p) { return p; };
    var disposed = false;
    var unlisten = null;

    var evt = _event();
    if (!evt || !evt.listen) return function () {};

    evt.listen(eventName, function (event) {
      if (disposed) return;
      try { callback(extract(event.payload)); } catch (e) { console.error('[tauri-bridge] event cb', eventName, e); }
    }).then(function (fn) {
      if (disposed) { try { fn(); } catch (_) {} } else { unlisten = fn; }
    }).catch(function (e) {
      console.warn('[tauri-bridge] listen failed', eventName, e);
    });

    return function () {
      disposed = true;
      if (unlisten) { try { unlisten(); } catch (_) {} unlisten = null; }
    };
  }

  // ============== 窗口控制 ==============

  var _windowStateListeners = [];

  // 监听 Tauri 窗口大小变化 → 转换为 windowState / windowMode 回调
  function _initWindowStateTracking() {
    if (typeof window.__TAURI__ === 'undefined') return;

    var resizing = false;
    onTauriEvent('tauri://resize', function () {
      if (resizing) return;
      resizing = true;
      // 防抖：resize 可能频繁触发，统一在下一帧查询状态
      requestAnimationFrame(function () {
        resizing = false;
        Promise.all([
          invoke('window_is_maximized'),
          invoke('window_is_fullscreen')
        ]).then(function (results) {
          var maximized = !!results[0];
          var fullscreen = !!results[1];
          var data = { maximized: maximized, fullscreen: fullscreen, windowMode: !fullscreen };
          for (var i = 0; i < _windowStateListeners.length; i++) {
            try { _windowStateListeners[i](data); } catch (_) {}
          }
        }).catch(function () {});
      });
    });
  }

  // 延迟初始化（等 __TAURI__ 就绪）
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', _initWindowStateTracking);
  } else {
    _initWindowStateTracking();
  }

  var windowControls = {
    minimize: function () { invoke('window_minimize'); },
    maximize: function () { invoke('window_maximize'); },
    close: function () { invoke('window_close'); },
    destroy: function () { invoke('window_destroy'); },
    isMaximized: function () { return invoke('window_is_maximized'); },
    isFullscreen: function () { return invoke('window_is_fullscreen'); },
    restore: function () { invoke('window_restore'); },
    isMinimized: function () { return invoke('window_is_minimized'); },
    // 窗口模式切换：windowMode=true 退出全屏；false 恢复全屏/最大化
    setWindowMode: function (windowMode) {
      if (windowMode) {
        invoke('window_restore');
      } else {
        invoke('window_maximize');
      }
    },
    // 设置启动器窗口尺寸 — Tauri 中可通过自定义命令实现
    setLauncherSize: function (width, height) {
      invoke('window_set_size', { width: width, height: height }).catch(function () {
        // 如果 Rust 端暂无此命令，静默忽略
      });
    },
    quitApp: function () { invoke('window_close'); },
    // launch.js 中使用的别名
    windowMinimize: function () { invoke('window_minimize'); },
    windowRestore: function () { invoke('window_restore'); },
    showWindowEarly: function () {
      // Tauri 中窗口构建时即显示，无需额外操作
    },
    openDevtools: function () { invoke('open_devtools'); }
  };

  // ============== 存储（KV Store） ==============

  var store = {
    get: function (key) { return invoke('store_get', { key: key }); },
    set: function (key, value) { return invoke('store_set', { key: key, value: value }); },
    delete: function (key) { return invoke('store_delete', { key: key }); }
  };

  // ============== 对话框 ==============

  var dialogs = {
    // 通用打开对话框，兼容 Electron dialog.showOpenDialog 格式
    showOpenDialog: function (options) {
      return invoke('dialog_open', { options: options || {} });
    },
    // 选择文件（兼容 personalize.js 中的 electronAPI.selectFile）
    selectFile: function (opts) {
      opts = opts || {};
      return invoke('select_file', {
        title: opts.title || null,
        default_path: opts.defaultPath || null
      }).then(function (result) {
        // 转换为前端期望的格式：兼容 { canceled, filePaths } 和 { cancelled, path } 两种格式
        if (result && result.cancelled === false && result.path) {
          return { canceled: false, cancelled: false, filePaths: [result.path], path: result.path };
        }
        return { canceled: true, cancelled: true, filePaths: [], path: undefined };
      });
    },
    // 选择文件夹
    selectFolder: function (opts) {
      opts = opts || {};
      return invoke('select_folder', {
        title: opts.title || opts.prompt || null,
        default_path: opts.defaultPath || null
      }).then(function (result) {
        // 兼容 { canceled, filePaths } 和 { cancelled, path } 两种格式
        if (result && result.cancelled === false && result.path) {
          return { canceled: false, cancelled: false, filePaths: [result.path], path: result.path };
        }
        return { canceled: true, cancelled: true, filePaths: [], path: undefined };
      });
    },
    // 选择保存文件夹（前端 api.js 中 selectSaveFolder 使用）
    // 返回与 Electron / 服务端一致的 { cancelled, path, error } 对象格式
    selectSaveFolder: function (defaultPath) {
      return invoke('select_folder', {
        title: '选择保存位置',
        default_path: defaultPath || null
      }).then(function (result) {
        if (result && result.cancelled === false && result.path) {
          return { cancelled: false, path: result.path, error: null };
        }
        return { cancelled: true, path: null, error: null };
      });
    }
  };

  // ============== 剪贴板 ==============

  var clipboard = {
    writeText: function (text) {
      // Tauri 中优先使用 Web Clipboard API
      try {
        return navigator.clipboard.writeText(text);
      } catch (e) {
        console.warn('[tauri-bridge] clipboard.writeText fallback failed:', e);
        return Promise.resolve();
      }
    }
  };

  // ============== 事件订阅 ==============

  var _closeAnimateCleanup = null;
  var _windowStateCleanup = null;

  var events = {
    // 窗口状态变化（maximized / fullscreen / windowMode）
    onWindowStateChanged: function (callback) {
      _windowStateListeners.push(callback);
      // 返回移除函数
      return function () {
        var idx = _windowStateListeners.indexOf(callback);
        if (idx >= 0) _windowStateListeners.splice(idx, 1);
      };
    },
    // 窗口模式变化
    onWindowModeChanged: function (callback) {
      _windowStateListeners.push(function (data) {
        callback({ windowMode: data.windowMode, maximized: data.maximized });
      });
      return function () {
        // 从 _windowStateListeners 中移除
        _windowStateListeners = _windowStateListeners.filter(function (f) { return f !== callback; });
      };
    },
    // 关闭动画
    onRequestCloseAnimate: function (callback) {
      if (_closeAnimateCleanup) _closeAnimateCleanup();
      _closeAnimateCleanup = onTauriEvent('request-close-animate', callback);
      return _closeAnimateCleanup;
    },
    // 整合包导入进度
    onImportProgress: function (callback) {
      return onTauriEvent('import:progress', callback);
    },
    removeImportProgressListener: function () {
      // stub — Tauri 事件监听由 onImportProgress 返回的取消函数管理
    }
  };

  // ============== 系统工具 ==============

  var platform;
  try {
    platform = navigator.platform || 'win32';
  } catch (e) {
    platform = 'win32';
  }

  var system = {
    platform: platform,
    memoryOptimize: function () {
      // Tauri 中通过 api_proxy 调用 /api/system/memory
      return invoke('api_proxy', {
        method: 'GET',
        path: '/api/system/memory',
        params: {},
        body: null
      }).then(function (result) {
        return result && result.body;
      });
    },
    openExternal: function (url) {
      return invoke('open_external', { url: url });
    },
    getDefaultModPath: function () {
      return invoke('get_default_mod_path').then(function (result) {
        if (result && result.success) return result.path;
        return '';
      });
    },
    // 拖拽文件路径解析 — 从 File 对象中提取路径
    getDroppedFilePath: function (file) {
      return file ? (file.path || file.name || '') : '';
    },
    readFileBuffer: function (filePath) {
      // 读取本地文件原始字节（返回 ArrayBuffer），前端转 blob URL 即可显示
      return invoke('read_file_buffer', { path: filePath });
    },
    getAuroraVideoPath: function () {
      // 极光视频壁纸 — Tauri 暂不支持，回退
      return Promise.resolve('');
    }
  };

  // ============== 更新器 ==============
  // Tauri 环境下更新器暂未完整实现，提供 stub 避免前端报错

  var updater = {
    getVersion: function () {
      return invoke('get_app_version').catch(function () {
        return '0.1.0';
      });
    },
    checkForUpdates: function () {
      return invoke('updater_check_for_updates').catch(function (e) {
        console.warn('[tauri-bridge] updater.checkForUpdates error:', e);
        return { status: 'not-available', error: String(e) };
      });
    },
    downloadUpdate: function () {
      return invoke('updater_download_update').catch(function (e) {
        console.warn('[tauri-bridge] updater.downloadUpdate error:', e);
        return { success: false, error: String(e) };
      });
    },
    installUpdate: function () {
      return invoke('updater_install_update').catch(function (e) {
        console.warn('[tauri-bridge] updater.installUpdate error:', e);
        return { success: false, error: String(e) };
      });
    },
    skipVersion: function (version) {
      return invoke('updater_skip_version', { version: version }).catch(function (e) {
        console.warn('[tauri-bridge] updater.skipVersion error:', e);
        return {};
      });
    },
    openReleasePage: function () {
      return invoke('updater_open_release_page').catch(function () {});
    },
    getPendingNotice: function () {
      // 自更新重启后的一次性"已更新"提示
      return invoke('updater_get_pending_notice').catch(function () {
        return null;
      });
    },
    onStatusChanged: function (callback) {
      // 订阅 Rust 端 updater:status 事件，payload 为 { channel, data }
      return onTauriEvent('updater:status', function (payload) {
        if (payload && typeof payload === 'object' && payload.channel) {
          callback({ channel: payload.channel, data: payload.data || {} });
        }
      });
    },
    removeStatusListener: function () {
      // 由 onStatusChanged 返回的取消函数管理
    }
  };

  // ============== AI / TTS（V岛功能） ==============

  var aiStub = {
    tts: {
      speak: function (text, voice) {
        // 通过 Tauri invoke 调用 Rust 后端 Edge TTS 合成
        return invoke('tts_speak', { text: text, voice: voice || 'zh-CN-XiaoxiaoNeural' })
          .then(function (data) {
            // data 是 Vec<u8> 直接返回的 Uint8Array
            if (!data || data.length === 0) {
              return { ok: false, error: '合成结果为空' };
            }
            return { ok: true, data: data };
          })
          .catch(function (err) {
            console.warn('[tauri-bridge] TTS 合成失败:', err);
            return { ok: false, error: String(err) };
          });
      }
    },
    ai: {
      chat: function (reqConfig) {
        console.log('[tauri-bridge] AI chat stub:', reqConfig);
        return Promise.resolve({ text: '(Tauri 环境暂不支持 AI 对话)' });
      }
    }
  };

  // ============== 原本已有的三个模块 ==============

  // 下面的代码直接从原 tauri-bridge.js 保留 privateServer / serverHost / redstoneOnline

  // ── invoke 封装（复用上面的 invoke） ──

  // ── 私人服务器（7 个命令） ──
  var privateServer = {
    list: function () { return invoke('private_server_list'); },
    save: function (servers) { return invoke('private_server_save', { servers: servers }); },
    add: function (server) { return invoke('private_server_add', { server: server }); },
    update: function (server) { return invoke('private_server_update', { server: server }); },
    delete: function (id) { return invoke('private_server_delete', { id: id }); },
    check: function (address) { return invoke('private_server_check', { address: address }); },
    copyAddress: function (address) { return invoke('private_server_copy_address', { address: address }); },
    getIcon: function (icon) { return invoke('private_server_icon', { icon: icon }); }
  };

  // ── 本地开服（11 个命令 + 2 个事件） ──
  var serverHost = {
    list: function () { return invoke('server_host_list'); },
    create: function (opts) {
      opts = opts || {};
      return invoke('server_host_create', {
        versionId: opts.versionId,
        name: opts.name,
        port: opts.port,
        options: {
          maxMem: opts.maxMem,
          onlineMode: opts.onlineMode,
          syncMods: opts.syncMods
        }
      });
    },
    start: function (opts) {
      opts = opts || {};
      return invoke('server_host_start', { serverId: opts.id });
    },
    stop: function (opts) {
      opts = opts || {};
      return invoke('server_host_stop', { serverId: opts.id });
    },
    command: function (opts) {
      opts = opts || {};
      return invoke('server_host_command', { serverId: opts.id, cmd: opts.command });
    },
    status: function (opts) {
      var serverId = (opts && opts.id) ? opts.id : null;
      return invoke('server_host_status', { serverId: serverId });
    },
    delete: function (opts) {
      opts = opts || {};
      return invoke('server_host_delete', { serverId: opts.id });
    },
    openDir: function (opts) {
      var serverId = (opts && opts.id) ? opts.id : null;
      return invoke('server_host_open_dir', { serverId: serverId });
    },
    resolveVersion: function (opts) {
      opts = opts || {};
      return invoke('server_host_resolve_version', { versionId: opts.versionId });
    },
    detectLoader: function (opts) {
      opts = opts || {};
      return invoke('server_host_detect_loader', { versionId: opts.versionId });
    },
    syncMods: function (opts) {
      opts = opts || {};
      return invoke('server_host_sync_mods', {
        serverId: opts.id,
        clientVersionId: opts.clientVersionId || opts.versionId || ''
      });
    },
    onLog: function (cb) {
      return onTauriEvent('server-host:log', cb, function (p) { return p; });
    },
    onStatus: function (cb) {
      return onTauriEvent('server-host:status', cb, function (p) { return p; });
    }
  };

  // ── 红石联机（9 个命令 + 4 个事件） ──
  var _rsUnlisteners = [];

  var redstoneOnline = {
    getServers: function () { return invoke('redstone_servers'); },
    getApikey: function () { return invoke('redstone_apikey'); },
    resetApikey: function () { return invoke('redstone_apikey_reset'); },
    scanPort: function () { return invoke('redstone_scan_port'); },
    start: function (params) { return invoke('redstone_start', { params: params }); },
    stop: function () { return invoke('redstone_stop'); },
    getStatus: function () { return invoke('redstone_status'); },
    onLog: function (callback) {
      var unsub = onTauriEvent('redstone:log', callback, function (payload) {
        if (typeof payload === 'string') return payload;
        return (payload && payload.message) || '';
      });
      _rsUnlisteners.push(unsub);
      return unsub;
    },
    onDisconnected: function (callback) {
      var unsub = onTauriEvent('redstone:disconnected', callback, function () { return undefined; });
      _rsUnlisteners.push(unsub);
      return unsub;
    },
    onReconnecting: function (callback) {
      var unsub = onTauriEvent('redstone:reconnecting', callback, function (payload) { return payload || {}; });
      _rsUnlisteners.push(unsub);
      return unsub;
    },
    onReconnected: function (callback) {
      var unsub = onTauriEvent('redstone:reconnected', callback, function (payload) { return payload || {}; });
      _rsUnlisteners.push(unsub);
      return unsub;
    },
    removeListeners: function () {
      while (_rsUnlisteners.length) {
        var fn = _rsUnlisteners.pop();
        try { fn(); } catch (_) {}
      }
    }
  };

  // ============== 注入到 window.electronAPI ==============

  var electronAPI = {
    // 窗口控制
    minimize: windowControls.minimize,
    maximize: windowControls.maximize,
    close: windowControls.close,
    destroy: windowControls.destroy,
    isMaximized: windowControls.isMaximized,
    isFullscreen: windowControls.isFullscreen,
    restore: windowControls.restore,
    isMinimized: windowControls.isMinimized,
    setWindowMode: windowControls.setWindowMode,
    setLauncherSize: windowControls.setLauncherSize,
    quitApp: windowControls.quitApp,
    windowMinimize: windowControls.windowMinimize,
    windowRestore: windowControls.windowRestore,
    showWindowEarly: windowControls.showWindowEarly,
    openDevtools: windowControls.openDevtools,

    // 存储
    store: store,

    // 对话框
    showOpenDialog: dialogs.showOpenDialog,
    selectFile: dialogs.selectFile,
    selectFolder: dialogs.selectFolder,
    selectSaveFolder: dialogs.selectSaveFolder,

    // 剪贴板
    clipboard: clipboard,

    // 事件
    onWindowStateChanged: events.onWindowStateChanged,
    onWindowModeChanged: events.onWindowModeChanged,
    onRequestCloseAnimate: events.onRequestCloseAnimate,
    onImportProgress: events.onImportProgress,
    removeImportProgressListener: events.removeImportProgressListener,

    // 系统
    platform: system.platform,
    memoryOptimize: system.memoryOptimize,
    openExternal: system.openExternal,
    getDefaultModPath: system.getDefaultModPath,
    getDroppedFilePath: system.getDroppedFilePath,
    readFileBuffer: system.readFileBuffer,
    getAuroraVideoPath: system.getAuroraVideoPath,

    // 更新器
    updater: updater,

    // AI / TTS
    tts: aiStub.tts,
    ai: aiStub.ai,

    // 三大模块
    privateServer: privateServer,
    serverHost: serverHost,
    redstoneOnline: redstoneOnline
  };

  window.electronAPI = electronAPI;

  // ============== 额外：暴露 api_proxy 让 api.js 使用 ==============
  // 在 window.__TAURI_PROXY__ 上暴露一个统一调用函数
  if (!window.__TAURI_PROXY__) {
    window.__TAURI_PROXY__ = {
      apiProxy: function (method, path, params, body) {
        return invoke('api_proxy', {
          method: method,
          path: path,
          params: params || {},
          body: body || null
        }).then(function (result) {
          // 兼容 api.js 对 fetch Response 的模拟：
          // 返回 { ok, status, json() }
          return {
            ok: result.status < 400,
            status: result.status,
            json: function () { return Promise.resolve(result.body); },
            // 如果 API 返回错误，让上层也能拿到完整响应信息
            _raw: result
          };
        });
      }
    };
  }

  // ============== 额外的全局函数存根 ==============

  // 如果某些页面直接调用了 window.electronAPI.xxx 而不在预置列表中，
  // 提供兜底 fallback
  if (typeof Proxy !== 'undefined') {
    window.electronAPI = new Proxy(window.electronAPI, {
      get: function (target, prop) {
        if (prop in target) return target[prop];
        // 对于未定义的方法返回存根函数（避免 "is not a function" 崩溃）
        if (typeof prop === 'string' && !prop.startsWith('__')) {
          console.warn('[tauri-bridge] 访问未实现的 electronAPI.' + prop + '，返回空函数');
          return function () { return Promise.resolve(null); };
        }
        return target[prop];
      }
    });
  }

  console.log('[tauri-bridge] Tauri 环境已检测到，electronAPI 桥接层已加载');
})();
