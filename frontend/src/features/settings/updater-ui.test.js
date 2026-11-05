// @vitest-environment jsdom
// updater-ui.test.js — 更新状态机、进度展示与弹窗交互单元测试
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { getCommandCalls, installTauriMock } from '../../test-utils/tauri-mock.js';

let mod;
const listeners = {};

function buildDom() {
  document.body.innerHTML = `
    <div id="updater-version-info"></div>
    <div id="updater-status-area"></div>
    <button id="updater-check-btn"></button>
    <button id="updater-skip-btn"></button>
    <button id="updater-release-btn"></button>
    <button id="updater-download-btn"></button>
    <button id="updater-install-btn"></button>
    <div id="updater-release-notes" style="display:none"><pre id="updater-release-notes-content"></pre></div>
    <div class="card"><div id="updater-btn-group"></div></div>
    <div id="update-card"></div>
    <div id="window-update-pill"><span id="window-update-pill-text"></span><span id="window-update-pill-spinner"></span></div>
    <div id="update-modal">
      <h3 id="update-modal-title"></h3>
      <div id="update-modal-version"></div>
      <div id="update-modal-content"></div>
      <button id="update-modal-goto-btn"></button>
      <button id="update-modal-quark-btn"></button>
      <button id="update-modal-acknowledge-btn"></button>
    </div>
    <div id="settings-submenu-group"><button class="nav-btn"></button></div>
    <div id="toast-container"></div>
  `;
}

const baseCommands = () => ({
  get_app_version: () => '0.4.0',
  updater_get_pending_notice: () => null,
  updater_check_for_updates: () => ({ status: 'not-available' }),
  updater_download_update: () => ({ success: true }),
  updater_install_update: () => ({ success: true }),
  updater_skip_version: () => ({}),
});

beforeAll(async () => {
  installTauriMock({ commands: baseCommands() });
  window.__TAURI_INTERNALS__.listen = async (name, cb) => {
    listeners[name] = cb;
    return () => {};
  };
  buildDom();
  vi.stubGlobal('marked', { parse: (text) => `<p>${text}</p>` });
  mod = await import('./updater-ui.js');
  await new Promise((resolve) => setTimeout(resolve, 20));
});

beforeEach(() => {
  installTauriMock({ commands: baseCommands() });
  buildDom();
  vi.stubGlobal('marked', { parse: (text) => `<p>${text}</p>` });
  Element.prototype.scrollIntoView = vi.fn();
});

afterEach(() => {
  vi.restoreAllMocks();
  document.body.innerHTML = '';
});

function emit(channel, data = {}) {
  const listener = listeners['updater:status'];
  if (!listener) throw new Error('未捕获 updater:status 监听');
  listener({ payload: { channel, data } });
}

describe('formatBytes / escapeHtml', () => {
  it('格式化不同数量级', () => {
    expect(mod.formatBytes(0)).toBe('0 B');
    expect(mod.formatBytes(1024)).toBe('1 KB');
    expect(mod.formatBytes(1536)).toBe('1.5 KB');
    expect(mod.formatBytes(1024 * 1024)).toBe('1 MB');
  });

  it('转义 HTML 特殊字符', () => {
    expect(mod.escapeHtml(`<a href="x">'&'</a>`)).toBe('&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;');
    expect(mod.escapeHtml(null)).toBe('');
  });
});

describe('更新胶囊', () => {
  it('显示/隐藏胶囊与 ready 状态', async () => {
    mod.showUpdatePill('发现新版本 v1.0.0', { spinner: true });
    const pill = document.getElementById('window-update-pill');
    expect(pill.style.display).toBe('flex');
    expect(document.getElementById('window-update-pill-text').textContent).toBe('发现新版本 v1.0.0');
    expect(document.getElementById('window-update-pill-spinner').style.display).toBe('');
    expect(pill.classList.contains('is-ready')).toBe(false);

    mod.showUpdatePill('重启以更新', { ready: true });
    expect(pill.classList.contains('is-ready')).toBe(true);
    expect(pill.title).toBe('重启并安装更新');

    mod.hideUpdatePill();
    expect(pill.style.display).toBe('none');
    // 隐藏后点击不应再触发安装
    await mod.handleWindowUpdatePillClick();
    expect(getCommandCalls().some((c) => c.cmd === 'updater_install_update')).toBe(false);
  });

  it('未 ready 时点击胶囊不触发安装', async () => {
    mod.showUpdatePill('下载中', { spinner: true });
    await mod.handleWindowUpdatePillClick();
    expect(getCommandCalls().some((c) => c.cmd === 'updater_install_update')).toBe(false);
  });

  it('ready 时点击胶囊调用安装', async () => {
    mod.showUpdatePill('重启以更新', { ready: true });
    await mod.handleWindowUpdatePillClick();
    expect(getCommandCalls().some((c) => c.cmd === 'updater_install_update')).toBe(true);
    expect(document.getElementById('window-update-pill-text').textContent).toBe('正在重启安装…');
  });

  it('胶囊元素缺失时安全返回', () => {
    document.getElementById('window-update-pill').remove();
    expect(() => mod.showUpdatePill('x')).not.toThrow();
    expect(() => mod.hideUpdatePill()).not.toThrow();
  });
});

describe('release notes / resetButtons', () => {
  it('有更新日志时渲染内容并展开', async () => {
    await mod.showReleaseNotes('  - 修复问题  ');
    expect(document.getElementById('updater-release-notes').style.display).toBe('');
    expect(document.getElementById('updater-release-notes-content').innerHTML).toContain('修复问题');
  });

  it('无更新日志时收起区域', async () => {
    await mod.showReleaseNotes('   ');
    expect(document.getElementById('updater-release-notes').style.display).toBe('none');
  });

  it('更新日志中的危险 HTML 会被消毒', async () => {
    await mod.showReleaseNotes('<img src=x onerror="window.__xss=1"><script>window.__xss=2</script>');
    const html = document.getElementById('updater-release-notes-content').innerHTML;
    expect(html).not.toContain('onerror');
    expect(html).not.toContain('<script');
    expect(window.__xss).toBeUndefined();
  });

  it('重置按钮显示态', () => {
    document.getElementById('updater-check-btn').disabled = true;
    mod.resetButtons();
    expect(document.getElementById('updater-check-btn').style.display).toBe('');
    expect(document.getElementById('updater-check-btn').disabled).toBe(false);
    ['updater-skip-btn', 'updater-release-btn', 'updater-download-btn', 'updater-install-btn', 'updater-release-notes']
      .forEach((id) => expect(document.getElementById(id).style.display).toBe('none'));
  });
});

describe('更新状态事件', () => {
  it('checking-for-update 禁用检查按钮并隐藏胶囊', () => {
    mod.showUpdatePill('checking', { spinner: true });
    emit('checking-for-update');
    expect(document.getElementById('updater-check-btn').disabled).toBe(true);
    expect(document.getElementById('updater-status-area').textContent).toContain('正在检查更新...');
    expect(document.getElementById('window-update-pill').style.display).toBe('none');
  });

  it('update-available 展示新版本、日志与胶囊', async () => {
    emit('update-available', { version: '1.2.0', releaseNotes: '新功能' });

    expect(document.getElementById('updater-status-area').textContent).toContain('发现新版本 v1.2.0');
    expect(document.getElementById('updater-check-btn').style.display).toBe('none');
    expect(document.getElementById('updater-skip-btn').style.display).toBe('');
    expect(document.getElementById('updater-release-btn').style.display).toBe('');
    expect(document.getElementById('updater-download-btn').style.display).toBe('');
    expect(document.getElementById('updater-install-btn').style.display).toBe('none');
    expect(document.getElementById('window-update-pill-text').textContent).toBe('发现新版本 v1.2.0');
    expect(document.querySelectorAll('.update-dot').length).toBeGreaterThan(0);
    await vi.waitFor(() => {
      expect(document.getElementById('updater-release-notes').style.display).toBe('');
    });
  });

  it('update-not-available 恢复检查按钮', () => {
    emit('update-not-available');
    expect(document.getElementById('updater-status-area').textContent).toContain('已是最新版本');
    expect(document.getElementById('updater-check-btn').disabled).toBe(false);
  });

  it('update-skipped 展示跳过版本且转义', () => {
    emit('update-skipped', { version: '<b>1.0</b>' });
    expect(document.getElementById('updater-status-area').textContent).toContain('已跳过 v<b>1.0</b>');
    expect(document.getElementById('updater-check-btn').disabled).toBe(false);
  });

  it('update-error 展示转义后的错误信息', () => {
    emit('update-error', { message: '<bad>', hint: '检查网络' });
    const area = document.getElementById('updater-status-area');
    expect(area.textContent).toContain('检查更新失败');
    expect(area.textContent).toContain('<bad>');
    expect(area.textContent).toContain('检查网络');
    expect(document.getElementById('updater-check-btn').disabled).toBe(false);
  });

  it('start-download 禁用下载按钮并更新胶囊', () => {
    emit('start-download');
    const btn = document.getElementById('updater-download-btn');
    expect(btn.disabled).toBe(true);
    expect(btn.textContent).toBe('下载中...');
    expect(document.getElementById('window-update-pill-text').textContent).toBe('正在下载更新 0%');
  });

  it('download-progress 百分比单调递增且展示速度与体积', () => {
    emit('download-progress', { percent: 40, bytesPerSecond: 2048, transferred: 4096, total: 8192 });
    let area = document.getElementById('updater-status-area');
    expect(area.innerHTML).toContain('40.0%');
    expect(area.textContent).toContain('2 KB/s');
    expect(area.textContent).toContain('4 KB / 8 KB');

    emit('download-progress', { percent: 20 });
    area = document.getElementById('updater-status-area');
    expect(area.innerHTML).toContain('40.0%');

    emit('download-progress', { percent: 85.5 });
    expect(document.getElementById('updater-status-area').innerHTML).toContain('85.5%');
    expect(document.getElementById('window-update-pill-text').textContent).toBe('正在下载更新 85.5%');
  });

  it('update-downloaded 显示安装按钮与可重启胶囊', () => {
    emit('update-downloaded', { version: '1.2.0' });
    expect(document.getElementById('updater-status-area').textContent).toContain('更新已下载完成 (v1.2.0)');
    expect(document.getElementById('updater-install-btn').style.display).toBe('');
    expect(document.getElementById('updater-download-btn').style.display).toBe('none');
    const pill = document.getElementById('window-update-pill');
    expect(pill.textContent).toBe('重启以更新');
    expect(pill.classList.contains('is-ready')).toBe(true);
  });
});

describe('更新按钮动作', () => {
  it('检查更新调用后端命令', async () => {
    await mod.handleCheckUpdate();
    expect(getCommandCalls().some((c) => c.cmd === 'updater_check_for_updates')).toBe(true);
  });

  it('下载更新调用后端命令并禁用按钮', async () => {
    await mod.handleDownloadUpdate();
    expect(getCommandCalls().some((c) => c.cmd === 'updater_download_update')).toBe(true);
    expect(document.getElementById('updater-download-btn').disabled).toBe(true);
  });

  it('安装更新调用后端命令', async () => {
    await mod.handleInstallUpdate();
    expect(getCommandCalls().some((c) => c.cmd === 'updater_install_update')).toBe(true);
  });

  it('跳过版本时携带当前新版本号并重置按钮', async () => {
    emit('update-available', { version: '1.3.0', releaseNotes: '' });
    await mod.handleSkipVersion();

    const call = getCommandCalls().find((c) => c.cmd === 'updater_skip_version');
    expect(call.args.version).toBe('1.3.0');
    expect(document.getElementById('updater-status-area').textContent).toContain('已跳过 v1.3.0');
    expect(document.getElementById('updater-check-btn').style.display).toBe('');
  });

  it('未检测到新版本时跳过不调用接口', async () => {
    // update-downloaded 会清空待跳过版本
    emit('update-downloaded', { version: '1.4.0' });
    await mod.handleSkipVersion();
    expect(getCommandCalls().some((c) => c.cmd === 'updater_skip_version')).toBe(false);
  });

  it('下载卡片可关闭', () => {
    mod.dismissUpdateCard();
    expect(document.getElementById('update-card').style.display).toBe('none');
  });

  it('外部下载入口打开官网与网盘', () => {
    mod.downloadFromGitHub();
    mod.downloadFromQuark();
    const calls = getCommandCalls().filter((c) => c.cmd === 'open_external');
    expect(calls.map((c) => c.args.url)).toEqual([
      'https://www.verselauncher.cn/',
      'https://pan.quark.cn/s/04ec149d8a93',
    ]);
  });
});

describe('更新弹窗', () => {
  it('showUpdatedNotice 展示更新完成信息', async () => {
    mod.showUpdatedNotice({ version: '0.5.0', notes: '- 修复 A' });

    expect(document.getElementById('update-modal-title').textContent).toBe('更新完成');
    expect(document.getElementById('update-modal-version').textContent).toBe('软件已更新至 v0.5.0 版本');
    expect(document.getElementById('update-modal-goto-btn').style.display).toBe('none');
    expect(document.getElementById('update-modal-quark-btn').style.display).toBe('none');
    expect(document.getElementById('update-modal-acknowledge-btn').style.display).toBe('');
    expect(document.getElementById('update-modal').style.display).toBe('flex');
    await vi.waitFor(() => {
      expect(document.getElementById('update-modal-content').innerHTML).toContain('修复 A');
    });
  });

  it('showUpdatePopup 展示版本跨度与更新按钮', async () => {
    mod.showUpdatePopup({ currentVersion: '0.4.0', version: '0.5.0', releaseNotes: '更新内容' });

    expect(document.getElementById('update-modal-title').textContent).toBe('发现新版本');
    expect(document.getElementById('update-modal-version').textContent).toBe('v0.4.0 → v0.5.0');
    expect(document.getElementById('update-modal-goto-btn').style.display).toBe('');
    expect(document.getElementById('update-modal-quark-btn').style.display).toBe('');
    expect(document.getElementById('update-modal-acknowledge-btn').style.display).toBe('none');
    await vi.waitFor(() => {
      expect(document.getElementById('update-modal-content').innerHTML).toContain('更新内容');
    });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(document.getElementById('update-modal').classList.contains('modal-visible')).toBe(true);
  });

  it('无更新日志时显示加载占位', () => {
    mod.showUpdatePopup({ currentVersion: '0.4.0', version: '0.5.0' });
    expect(document.getElementById('update-modal-content').innerHTML).toContain('更新内容加载中');
  });

  it('Esc 关闭弹窗并在动画后隐藏', async () => {
    mod.showUpdatePopup({ currentVersion: '0.4.0', version: '0.5.0' });
    const modal = document.getElementById('update-modal');
    modal.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    expect(modal.getAttribute('data-state')).toBe('closed');
    await new Promise((resolve) => setTimeout(resolve, 260));
    expect(modal.style.display).toBe('none');
  });

  it('gotoUpdateSettings 关闭弹窗并滚动到更新卡片', async () => {
    mod.showUpdatePopup({ currentVersion: '0.4.0', version: '0.5.0' });
    mod.gotoUpdateSettings();
    expect(document.getElementById('update-modal').getAttribute('data-state')).toBe('closed');
    await new Promise((resolve) => setTimeout(resolve, 360));
    expect(Element.prototype.scrollIntoView).toHaveBeenCalled();
  });

  it('addUpdateDots 幂等添加更新红点', () => {
    mod.addUpdateDots();
    mod.addUpdateDots();
    expect(document.querySelectorAll('#settings-submenu-group .update-dot').length).toBe(1);
    expect(document.querySelectorAll('#updater-check-btn .update-dot').length).toBe(1);
  });
});
