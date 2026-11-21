// features/misc/horizon.js — Rust 版（Horizon 桌面端）页面
//
// 桌面端不需要内置引擎：查官方 release → 下载安装包 → 唤起安装，其余由 Horizon 自己维护。
// 全部网络与进程操作走 Rust 命令（horizon_latest / horizon_download / horizon_run / horizon_cached），
// 渲染层只做状态展示。

import { bridge } from '../../core/tauri-bridge.js';

const invoke = (cmd, args) => bridge.invoke(cmd, args);

const listen = (event, handler) => bridge.onEvent(event, handler);

const humanSize = (bytes) => {
  if (!bytes) return '—';
  const mb = bytes / 1048576;
  return mb >= 1 ? `${mb.toFixed(1)} MB` : `${(bytes / 1024).toFixed(0)} KB`;
};

let wired = false;
let latest = null;
let downloadedPath = null;

const $ = (id) => document.getElementById(id);

const setState = (text) => {
  const el = $('rust-state');
  if (el) el.textContent = text;
};

const setProgress = (fraction) => {
  const fill = $('rust-bar-fill');
  if (fill) fill.style.width = `${Math.max(0, Math.min(1, fraction)) * 100}%`;
};

async function refreshCached() {
  try {
    const cached = await invoke('horizon_cached');
    if (cached) {
      downloadedPath = cached;
      setState('已下载安装包');
      setProgress(1);
      const run = $('rust-run');
      if (run) run.disabled = false;
    }
  } catch (_) {
    /* 首次运行时目录可能还不存在，忽略 */
  }
}

async function loadLatest() {
  try {
    latest = await invoke('horizon_latest');
    const ver = $('rust-version');
    if (ver) ver.textContent = `${latest.version}（${latest.published}）`;
    const asset = $('rust-asset');
    if (asset) asset.textContent = `${latest.asset} · ${humanSize(latest.size)}`;
  } catch (e) {
    const ver = $('rust-version');
    if (ver) ver.textContent = '检测失败（网络）';
    setState(String(e && e.message ? e.message : e));
  }
}

async function startDownload() {
  if (!latest) return;
  const btn = $('rust-download');
  if (btn) btn.disabled = true;
  setState('下载中…');
  setProgress(0);
  try {
    const path = await invoke('horizon_download', { url: latest.url, name: latest.asset });
    downloadedPath = path;
    setProgress(1);
    setState('下载完成');
    const run = $('rust-run');
    if (run) run.disabled = false;
  } catch (e) {
    setState(`下载失败：${e && e.message ? e.message : e}`);
  } finally {
    if (btn) btn.disabled = false;
  }
}

async function runInstaller() {
  if (!downloadedPath) return;
  try {
    await invoke('horizon_run', { path: downloadedPath });
    setState('已唤起安装程序');
  } catch (e) {
    setState(`启动失败：${e && e.message ? e.message : e}`);
  }
}

export function initHorizon() {
  if (wired) return;
  wired = true;

  listen('horizon-progress', (event) => {
    const { done, total } = event.payload || {};
    if (total) {
      setProgress(done / total);
      setState(`下载中 ${humanSize(done)} / ${humanSize(total)}`);
    } else {
      setState(`下载中 ${humanSize(done)}`);
    }
  });

  const ensurePage = () => {
    if (!$('page-rust')) return false;
    if (!$('rust-version').dataset.ready) {
      $('rust-version').dataset.ready = '1';
      loadLatest();
      refreshCached();
    }
    return true;
  };

  document.addEventListener('click', (event) => {
    const nav = event.target.closest && event.target.closest('[data-page="rust"]');
    if (nav) setTimeout(ensurePage, 30);
  });

  const dl = $('rust-download');
  if (dl) dl.addEventListener('click', startDownload);
  const run = $('rust-run');
  if (run) run.addEventListener('click', runInstaller);
  const site = $('rust-site');
  if (site) {
    site.addEventListener('click', () => {
      window.open('https://playhorizon.dev', '_blank');
    });
  }
}

// registry 以副作用方式引入本模块，这里直接完成初始化（内部有 wired 去重）
initHorizon();
