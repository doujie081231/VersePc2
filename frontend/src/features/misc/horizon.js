// features/misc/horizon.js — Rust 版（Horizon）在启动器内安装 / 准备 / 启动
//
// 与手机端同一套思路：运行时不单独安装成另一个应用，而是收进本启动器的数据目录，
// 由这里驱动它自己的 CLI 完成
//   下载运行时（28MB 便携 zip）→ install latest → prepare --download → play
// 所有进度与输出都回到本页面（horizon-progress / horizon-log / horizon-step-done）。
//
// 后端命令见 src-tauri/src/horizon.rs。
//
// 注意：按钮状态用 `data-rust-action`，不能用 `data-action` —— 那是 core/actions.js
// 的全局委托属性，未注册的名字会在控制台刷「未注册的动作」并可能抢走点击。

import { bridge } from '../../core/tauri-bridge.js';

const invoke = (cmd, args) => bridge.invoke(cmd, args);
const listen = (event, handler) => bridge.onEvent(event, handler);

const $ = (id) => document.getElementById(id);

const humanSize = (bytes) => {
  if (!bytes) return '—';
  const mb = bytes / 1048576;
  return mb >= 1 ? `${mb.toFixed(1)} MB` : `${(bytes / 1024).toFixed(0)} KB`;
};

let wired = false;
let status = null;
let busy = false;
let playing = false;

function setBusy(value, label) {
  busy = value;
  const primary = $('rust-primary');
  if (primary && value && label) {
    primary.disabled = true;
    primary.textContent = label;
  }
  if (!value) render();
}

function appendLog(line, kind) {
  const box = $('rust-log');
  if (!box) return;
  const prefix = kind === 'err' ? '! ' : kind === 'info' ? '· ' : '';
  box.textContent += `${prefix}${line}\n`;
  // 只保留最后 400 行，长时间 install 也不会把 DOM 撑爆
  const lines = box.textContent.split('\n');
  if (lines.length > 400) box.textContent = lines.slice(-400).join('\n');
  box.scrollTop = box.scrollHeight;
}

function setProgress(fraction) {
  const fill = $('rust-bar-fill');
  if (fill) fill.style.width = `${Math.max(0, Math.min(1, fraction)) * 100}%`;
}

/** 根据后端状态决定主按钮：下载运行时 → 安装版本 → 准备资源 → 启动 */
function render() {
  if (!status) return;
  const primary = $('rust-primary');
  const detail = $('rust-detail');
  const prepare = $('rust-prepare');
  const signin = $('rust-signin');
  const stop = $('rust-stop');
  if (!primary || !detail) return;

  primary.disabled = false;
  if (stop) stop.disabled = !playing;
  if (signin) signin.disabled = busy;
  if (prepare) prepare.disabled = busy || !status.runtime || !!status.prepared || !status.account;

  // 顺序照 CLI 的真实前置条件走：运行时 → 正版账号（prepare --download 与 play 都要求）
  // → 安装版本 → 准备资源 → 启动。没有账号时不让用户点「准备资源」再吃一条 CLI 报错。
  if (!status.runtime) {
    detail.textContent = '尚未下载运行时 · 首次约 28 MB';
    primary.textContent = '下载运行环境';
    primary.dataset.rustAction = 'runtime';
  } else if (!status.account) {
    detail.textContent = `运行时已就绪 · 需要登录拥有 Minecraft: Java 版的正版账号${
      status.installed && status.installed.length ? ` · 已装 ${status.installed.join(' / ')}` : ''
    }`;
    primary.textContent = '登录 Microsoft 账号';
    primary.dataset.rustAction = 'signin';
  } else if (!status.installed || status.installed.length === 0) {
    detail.textContent = `运行时已就绪 · 账号 ${status.account} · 还没有安装 Horizon 版本`;
    primary.textContent = '安装版本';
    primary.dataset.rustAction = 'install';
  } else if (!status.prepared) {
    detail.textContent = `版本 ${status.installed.join(' / ')} · 账号 ${status.account} · 资源未准备`;
    primary.textContent = '准备资源';
    primary.dataset.rustAction = 'prepare';
  } else {
    detail.textContent = `已安装 · ${status.installed.join(' / ')}${
      status.account ? ` · ${status.account}` : ''
    }`;
    primary.textContent = playing ? '运行中' : '启动游戏';
    primary.dataset.rustAction = 'play';
  }
}

async function refresh() {
  try {
    status = await invoke('horizon_status');
    render();
  } catch (e) {
    const detail = $('rust-detail');
    if (detail) detail.textContent = `状态读取失败：${e && e.message ? e.message : e}`;
  }
}

async function runStep(step, label) {
  if (busy) return;
  setBusy(true, label);
  try {
    const code = await invoke('horizon_step', { step });
    if (code !== 0) appendLog(`步骤 ${step} 退出码 ${code}`, 'err');
  } catch (e) {
    const msg = e && e.message ? e.message : String(e);
    appendLog(`${label}失败：${msg}`, 'err');
    const detail = $('rust-detail');
    if (detail) detail.textContent = `${label}失败：${msg}`;
  } finally {
    if (step === 'play') playing = false;
    setBusy(false);
    await refresh();
  }
}

async function primaryAction() {
  const action = $('rust-primary').dataset.rustAction;
  if (action === 'runtime') {
    if (busy) return;
    setBusy(true, '下载中…');
    setProgress(0);
    try {
      status = await invoke('horizon_install_runtime');
      setProgress(1);
    } catch (e) {
      const msg = e && e.message ? e.message : String(e);
      appendLog(`下载运行时失败：${msg}`, 'err');
      const detail = $('rust-detail');
      if (detail) detail.textContent = `下载运行时失败：${msg}`;
    } finally {
      setBusy(false);
      await refresh();
    }
    return;
  }
  if (action === 'install') return runStep('install', '安装中…');
  if (action === 'signin') return runStep('signin', '等待登录…');
  if (action === 'prepare') return runStep('prepare', '准备中…');
  if (action === 'play') {
    playing = true;
    render();
    return runStep('play', '启动中…');
  }
}

export function initHorizon() {
  if (wired) return;
  wired = true;

  listen('horizon-progress', (event) => {
    const { done, total } = event.payload || {};
    if (total) {
      setProgress(done / total);
      const primary = $('rust-primary');
      if (primary && primary.dataset.rustAction === 'runtime') {
        primary.textContent = `${humanSize(done)} / ${humanSize(total)}`;
      }
    }
  });

  listen('horizon-log', (event) => {
    const { line, kind } = event.payload || {};
    if (line) appendLog(line, kind);
  });

  listen('horizon-step-done', (event) => {
    const { step, code } = event.payload || {};
    if (step === 'play') playing = false;
    appendLog(`[${step}] 结束，退出码 ${code}`, code === 0 ? 'info' : 'err');
    refresh();
  });

  const ensurePage = () => {
    if (!$('page-rust')) return false;
    if (!$('rust-detail').dataset.ready) {
      $('rust-detail').dataset.ready = '1';
      refresh();
    }
    return true;
  };

  document.addEventListener('click', (event) => {
    const nav = event.target.closest && event.target.closest('[data-page="rust"]');
    if (nav) setTimeout(ensurePage, 30);
  });

  $('rust-primary')?.addEventListener('click', primaryAction);
  $('rust-signin')?.addEventListener('click', () => runStep('signin', '等待登录…'));
  $('rust-prepare')?.addEventListener('click', () => runStep('prepare', '准备中…'));
  $('rust-stop')?.addEventListener('click', async () => {
    try {
      await invoke('horizon_stop');
      appendLog('已请求停止游戏', 'info');
      playing = false;
      render();
    } catch (e) {
      appendLog(`停止失败：${e && e.message ? e.message : e}`, 'error');
    }
  });
  $('rust-site')?.addEventListener('click', () => {
    window.open('https://playhorizon.dev', '_blank');
  });
}

// registry 以副作用方式引入本模块，这里直接完成初始化（内部有 wired 去重）
initHorizon();
