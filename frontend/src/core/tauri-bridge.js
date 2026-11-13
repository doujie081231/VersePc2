/**
 * js/tauri-bridge.js - Tauri 环境下的完整 electronAPI 等价适配层
 *
 * 职责：
 *   在 Tauri 环境下，提供与 Electron preload.cjs 中 bridge 等价的接口，
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

import { createBridgeModules } from './tauri-bridge-modules.js';

export const bridge = (function () {
  'use strict';

  // ============== 环境检测 ==============

  var isTauri = !!(window.__TAURI__ || window.__TAURI_INTERNALS__);
  if (!isTauri) return {};

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
    quitApp: function () { invoke('window_close'); },
    // launch.js 中使用的别名
    windowMinimize: function () { invoke('window_minimize'); },
    windowRestore: function () { invoke('window_restore'); },
    showWindowEarly: function () {
      // 窗口默认隐藏（visible:false），splash 首屏渲染后调用此命令显示，避免启动黑屏闪一下
      invoke('window_show').catch(function () {});
    }
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
    // 选择保存文件夹（前端 api.js 中 selectSaveFolder 使用）
    // 返回与 Electron / 服务端一致的 { cancelled, path, error } 对象格式
    selectSaveFolder: function (defaultPath) {
      return invoke('select_folder', {
        title: '选择保存位置',
        defaultPath: defaultPath || null
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

  // 规范化平台命名：统一为 'windows' / 'macos' / 'linux' / 'ios' / 'android' / 'unknown'。
  // navigator.platform 由 WebView2 / WebKit 内核决定，Win 上为 "Win32"、macOS 为 "MacIntel"，
  // 与 Tauri 运行时名（windows/macos/linux）不一致，必须归一化后再给前端判断。
  function _detectPlatform() {
    var ua = (navigator.userAgent || '').toLowerCase();
    var p = (navigator.platform || '').toLowerCase();
    if (ua.indexOf('mac os') >= 0 || ua.indexOf('macintosh') >= 0 || p.indexOf('mac') >= 0) return 'macos';
    if (window.navigator.platform && /iphone|ipad|ipod/i.test(window.navigator.platform)) return 'ios';
    if (ua.indexOf('android') >= 0) return 'android';
    if (ua.indexOf('linux') >= 0 || p.indexOf('linux') >= 0) return 'linux';
    if (ua.indexOf('win') >= 0 || p.indexOf('win') >= 0) return 'windows';
    return 'unknown';
  }

  var _platformKey = _detectPlatform();

  var system = {
    // 归一化平台键：windows / macos / linux / ios / android / unknown
    platform: _platformKey,
    // 便捷布尔判断，供前端按平台隐藏不可用功能入口
    isWindows: _platformKey === 'windows',
    isMac: _platformKey === 'macos',
    isLinux: _platformKey === 'linux',
    memoryOptimize: function () {
      // Tauri 中通过 api_proxy 调用 POST /api/memory-optimize 执行内存优化
      return invoke('api_proxy', {
        method: 'POST',
        path: '/api/memory-optimize',
        params: {},
        body: {}
      }).then(function (result) {
        return result && result.body;
      });
    },
    openExternal: function (url) {
      return invoke('open_external', { url: url });
    },
    // 打开浏览器扩展商店：优先 Edge，没装 Edge 时用默认浏览器（Rust 侧决定）
    openExtensionStore: function () {
      return invoke('open_extension_store', {});
    },
    // 打开任意 URL，但优先用 Edge（用于"这个链接在 Edge 里打开更合适"的场景）
    openExternalInEdge: function (url) {
      return invoke('open_external_in_edge', { url: url });
    },
    getDefaultModPath: function (versionId) {
      return invoke('get_default_mod_path', versionId ? { versionId: versionId } : {}).then(function (result) {
        if (result && result.success) return result.path;
        return '';
      });
    },
    getDefaultModSaveFolder: function (gameVersion, loader) {
      return invoke('get_default_mod_save_folder', { gameVersion: gameVersion || '', loader: loader || '' }).then(function (result) {
        if (result && result.success) return result;
        return null;
      }).catch(function () { return null; });
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

  // ============== 场景渲染器组件（Wallpaper Engine 场景壁纸，可选下载） ==============

  var sceneRendererStub = {
    installed: function () {
      return invoke('scene_renderer_installed').catch(function () {
        return { ok: false, installed: false };
      });
    },
    download: function () {
      return invoke('scene_renderer_download').catch(function (err) {
        return { ok: false, error: String(err) };
      });
    },
    onProgress: function (callback) {
      // 订阅 scene-renderer:progress 事件，payload: { stage, percent, transferred, total, speed, message? }
      return onTauriEvent('scene-renderer:progress', function (payload) {
        try { callback(payload || {}); } catch (e) { console.error('[tauri-bridge] scene renderer progress cb', e); }
      });
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
        // 通过 Rust 后端代理发起 AI 请求，绕过 WebView CORS 限制
        return invoke('ai_chat', { config: reqConfig || {} }).then(function (res) {
          if (!res) return { ok: false, error: 'AI 请求无响应' };
          return res;
        }).catch(function (err) {
          console.warn('[tauri-bridge] AI chat 调用失败:', err);
          return { ok: false, error: String(err) };
        });
      }
    }
  };

  // ============== 联机与本地开服模块 ==============
  var _bridgeModules = createBridgeModules({ invoke: invoke, onTauriEvent: onTauriEvent });
  var privateServer = _bridgeModules.privateServer;
  var serverHost = _bridgeModules.serverHost;
  var redstoneOnline = _bridgeModules.redstoneOnline;
  var enderlinkOnline = _bridgeModules.enderlinkOnline;

  // ============== 注入到 bridge ==============

  var electronAPI = {
    // 窗口控制
    minimize: windowControls.minimize,
    maximize: windowControls.maximize,
    close: windowControls.close,
    isMaximized: windowControls.isMaximized,
    isFullscreen: windowControls.isFullscreen,
    restore: windowControls.restore,
    isMinimized: windowControls.isMinimized,
    setWindowMode: windowControls.setWindowMode,
    quitApp: windowControls.quitApp,
    windowMinimize: windowControls.windowMinimize,
    windowRestore: windowControls.windowRestore,
    showWindowEarly: windowControls.showWindowEarly,
    // 启动计时诊断（写入 logs/startup-timing.log）
    writeStartupTiming: function (content) {
      invoke('write_startup_timing', { content: String(content) }).catch(function () {});
    },

    // 存储
    store: store,

    // 对话框
    showOpenDialog: dialogs.showOpenDialog,
    selectFile: dialogs.selectFile,
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
    isWindows: system.isWindows,
    isMac: system.isMac,
    isLinux: system.isLinux,
    memoryOptimize: system.memoryOptimize,
    openExternal: system.openExternal,
    openExtensionStore: system.openExtensionStore,
    openExternalInEdge: system.openExternalInEdge,
    getDefaultModPath: system.getDefaultModPath,
    getDefaultModSaveFolder: system.getDefaultModSaveFolder,
    getDroppedFilePath: system.getDroppedFilePath,
    readFileBuffer: system.readFileBuffer,
    getAuroraVideoPath: system.getAuroraVideoPath,

    // 更新器
    updater: updater,

    // 场景渲染器组件（可选下载）
    sceneRenderer: sceneRendererStub,

    // AI / TTS
    tts: aiStub.tts,
    ai: aiStub.ai,

    // 三大模块
    privateServer: privateServer,
    serverHost: serverHost,
    redstoneOnline: redstoneOnline,
    enderlinkOnline: enderlinkOnline
  };

  // ============== 统一 IPC 面 ==============
  // 单一通信面：底层 invoke、业务传输(apiProxy)、能力方法与业务 API 全部收敛在一个对象上。
  // 能力方法（窗口/系统/对话框/剪贴板/存储/事件/更新器/AI/联机模块）先平铺进来，
  // 业务大对象由 api.js 在加载后挂到 bridge.api 上。
  const bridgeObj = {};
  bridgeObj.invoke = invoke;
  bridgeObj.api = null;
  Object.keys(electronAPI).forEach(function (k) {
    bridgeObj[k] = electronAPI[k];
  });

  // ============== 额外：暴露 api_proxy 让 api.js 使用 ==============
  // 通用命令调用：features 层需要调用自定义 #[tauri::command] 时统一走这里，
  // 避免各模块直接摸 window.__TAURI__（架构护栏会拦）。
  bridgeObj.invoke = function (cmd, args) {
    return invoke(cmd, args || {});
  };

  // 浏览器下载接管：订阅后端推送的 download-request
  bridgeObj.onDownloadRequest = function (callback) {
    return onTauriEvent('download-request', function (payload) {
      if (typeof callback === 'function') callback(payload);
    });
  };

  bridgeObj.apiProxy = function (method, path, params, body) {
    return invoke('api_proxy', {
      method: method,
      path: path,
      params: params || {},
      body: body || null
    }).then(function (result) {
      // 兼容 api.js 对 fetch Response 的模拟：返回 { ok, status, json() }
      return {
        ok: result.status < 400,
        status: result.status,
        json: function () { return Promise.resolve(result.body); },
        // 如果 API 返回错误，让上层也能拿到完整响应信息
        _raw: result
      };
    });
  };

  // ============== 兜底存根（避免未知方法崩溃） ==============
  function _withFallback(target) {
    if (typeof Proxy === 'undefined') return target;
    return new Proxy(target, {
      get: function (t, prop) {
        if (prop in t) return t[prop];
        // thenable 探测：不能把 bridge 伪装成 Promise，否则 await/return 会永久挂起
        if (prop === 'then') return undefined;
        if (typeof prop === 'string' && !prop.startsWith('__')) {
          console.warn('[tauri-bridge] 访问未实现的视桥接口 ' + String(prop) + '，返回空函数');
          return function () { return Promise.resolve(null); };
        }
        return t[prop];
      }
    });
  }

  // 统一通信面以 bridge 为准；bridge / bridge 兼容别名
  // 由 legacy/globals.js 在入口统一挂载。
  console.log('[tauri-bridge] Tauri 环境已检测到，统一桥接层已加载');
  return _withFallback(bridgeObj);
})();
