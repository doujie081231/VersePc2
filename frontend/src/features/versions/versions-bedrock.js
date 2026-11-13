/* versions-bedrock.js — 基岩版下载页
 * 清单来自 MineBBS（Windows 基岩版），下载走 123 云盘分享解析（登录态只保存在本机）。
 */
import { API } from '../../core/api.js';
import { escapeHtml, formatDate, formatSize, formatSpeed } from '../../core/utils.js';
import { showToast } from '../misc/ui-components.js';

const S = {
  list: null,
  loading: false,
  sessionId: '',
  pollTimer: null,
  loginTimer: null,
  pendingInstall: null,
};

export function resetBedrockCache() {
  S.list = null;
}

function closeBedrockModal(id) {
  const modal = document.getElementById(id);
  if (modal) {
    modal.classList.remove('modal-visible');
    setTimeout(() => modal.remove(), 250);
  }
}

function showModal(id, html) {
  closeBedrockModal(id);
  document.body.insertAdjacentHTML('beforeend',
    `<div class="modal" id="${id}" style="display:flex">${html}</div>`);
  requestAnimationFrame(() => document.getElementById(id)?.classList.add('modal-visible'));
}

export async function renderBedrockVersions(container) {
  if (!container) return;
  if (!S.list || S.loading) {
    if (!S.loading) {
      S.loading = true;
      container.innerHTML = '<div class="loading-spinner"><div class="spinner"></div><p>正在获取基岩版列表...</p></div>';
      try {
        const data = await API.getBedrockVersions(false);
        S.list = (data && data.versions) || [];
      } catch (e) {
        S.loading = false;
        container.innerHTML = `
          <p class="empty-text">加载基岩版列表失败：${escapeHtml(e.message || String(e))}</p>
          <button class="btn btn-secondary btn-sm" id="bedrock-retry-btn" style="margin-top:12px">重试</button>`;
        document.getElementById('bedrock-retry-btn')?.addEventListener('click', () => {
          S.list = null;
          renderBedrockVersions(container);
        });
        return;
      }
      S.loading = false;
    }
  }

  const versions = S.list || [];
  if (versions.length === 0) {
    container.innerHTML = '<p class="empty-text">暂无基岩版版本</p>';
    return;
  }

  container.innerHTML = `
    <div class="bedrock-list-tip" style="margin-bottom:12px;padding:10px 14px;border-radius:8px;background:var(--bg-secondary);border:1px solid var(--border-color);font-size:12px;color:var(--text-muted);line-height:1.6">
      下载需要登录一次 123 云盘。登录态只保存在本机，启动器不会上传账号信息；免费额度用完时 123 会要求开通会员，这种情况启动器无法绕开。
    </div>
    ${versions.map(v => {
      const link = (v.links && v.links[0]) || '';
      const abstract = (v.abstract || '').replace(/\s+/g, ' ');
      return `
      <div class="version-item" data-bedrock-version="${escapeHtml(v.version)}">
        <div class="version-item-left">
          <div class="version-item-icon release" style="display:flex;align-items:center;justify-content:center;font-size:11px;font-weight:700;color:var(--accent)">BE</div>
          <div class="version-item-info">
            <span class="version-item-name">${escapeHtml(v.version)}</span>
            <span class="version-item-meta">基岩版 · ${formatDate(v.time)} · ${formatSize(v.size)}</span>
            ${abstract ? `<span class="version-item-meta" style="display:block;margin-top:2px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;max-width:520px" title="${escapeHtml(abstract)}">${escapeHtml(abstract)}</span>` : ''}
          </div>
        </div>
        <div class="version-item-actions">
          ${v.installed
            ? '<span style="font-size:12px;color:var(--text-muted);padding:6px 10px">已安装</span>'
            : `<button class="btn btn-accent btn-sm" data-bedrock-download="${escapeHtml(v.version)}" data-bedrock-link="${escapeHtml(link)}">下载</button>`}
        </div>
      </div>`;
    }).join('')}
  `;

  container.querySelectorAll('[data-bedrock-download]').forEach(btn => {
    btn.addEventListener('click', () => requestBedrockDownload(btn.dataset.bedrockDownload, btn.dataset.bedrockLink));
  });
}

async function requestBedrockDownload(version, link) {
  if (!link) {
    showToast('这个版本没有可用下载链接', 'error');
    return;
  }
  try {
    const status = await API.bedrockLoginStatus();
    if (status.loggedIn) {
      startBedrockInstall(version, link);
    } else {
      S.pendingInstall = { version, link };
      showLoginModal();
    }
  } catch (e) {
    showToast('检查登录状态失败：' + (e.message || e), 'error');
  }
}

function showLoginModal() {
  showModal('bedrock-login-modal', `
    <div class="modal-content" style="max-width:520px">
      <div class="modal-header">
        <h3>登录 123 云盘</h3>
        <button class="modal-close" id="bedrock-login-close">&times;</button>
      </div>
      <div class="modal-body">
        <p style="font-size:13px;color:var(--text-secondary);line-height:1.7;margin-bottom:10px">
          基岩版文件放在 123 云盘分享里。点下方按钮打开登录窗口，登录成功后会自动读回登录态并关闭窗口。
        </p>
        <p style="font-size:12px;color:var(--text-muted);line-height:1.7;margin-bottom:14px">
          登录信息只保存在本机（123pan-token.txt），不经过任何服务器。
        </p>
        <div id="bedrock-login-status" style="font-size:12px;color:var(--text-muted);min-height:20px"></div>
      </div>
      <div class="modal-footer">
        <button class="btn btn-secondary" id="bedrock-login-cancel">取消</button>
        <button class="btn btn-primary" id="bedrock-login-open">打开登录窗口</button>
      </div>
    </div>`);

  const statusEl = document.getElementById('bedrock-login-status');
  const close = () => {
    if (S.loginTimer) { clearInterval(S.loginTimer); S.loginTimer = null; }
    S.pendingInstall = null;
    closeBedrockModal('bedrock-login-modal');
  };
  document.getElementById('bedrock-login-close')?.addEventListener('click', close);
  document.getElementById('bedrock-login-cancel')?.addEventListener('click', close);

  document.getElementById('bedrock-login-open')?.addEventListener('click', async () => {
    const btn = document.getElementById('bedrock-login-open');
    if (btn) btn.disabled = true;
    if (statusEl) statusEl.textContent = '正在打开登录窗口...';
    try {
      await API.bedrockLoginStart();
    } catch (e) {
      if (statusEl) statusEl.textContent = '打开登录窗口失败：' + (e.message || e);
      if (btn) btn.disabled = false;
      return;
    }
    if (S.loginTimer) clearInterval(S.loginTimer);
    S.loginTimer = setInterval(async () => {
      try {
        const st = await API.bedrockLoginStatus();
        if (st.loggedIn) {
          clearInterval(S.loginTimer);
          S.loginTimer = null;
          if (statusEl) statusEl.textContent = '登录成功';
          showToast('123 云盘登录成功', 'success');
          const pending = S.pendingInstall;
          S.pendingInstall = null;
          closeBedrockModal('bedrock-login-modal');
          if (pending) startBedrockInstall(pending.version, pending.link);
        } else if (st.message) {
          if (statusEl) statusEl.textContent = st.message;
        }
      } catch (e) {
        clearInterval(S.loginTimer);
        S.loginTimer = null;
        if (statusEl) statusEl.textContent = '读取登录状态失败：' + (e.message || e);
        if (btn) btn.disabled = false;
      }
    }, 1000);
  });
}

function showProgressModal(version) {
  showModal('bedrock-progress-modal', `
    <div class="modal-content" style="max-width:520px">
      <div class="modal-header">
        <h3>下载基岩版 ${escapeHtml(version)}</h3>
        <button class="modal-close" id="bedrock-progress-close">&times;</button>
      </div>
      <div class="modal-body">
        <div class="install-progress-container">
          <div class="install-info">
            <span id="bedrock-stage" style="font-weight:600">准备中...</span>
            <span id="bedrock-percent" style="font-weight:600;color:var(--accent)">0%</span>
          </div>
          <div class="progress-bar" style="margin-bottom:12px">
            <div class="progress-fill" id="bedrock-progress-fill" style="width:0%"></div>
          </div>
          <div id="bedrock-message" class="install-message" style="margin-bottom:4px"></div>
          <div id="bedrock-speed" class="install-speed"></div>
        </div>
      </div>
      <div class="modal-footer">
        <button id="bedrock-cancel-btn" class="btn btn-danger">取消下载</button>
      </div>
    </div>`);

  document.getElementById('bedrock-progress-close')?.addEventListener('click', closeProgress);
  document.getElementById('bedrock-cancel-btn')?.addEventListener('click', async () => {
    const btn = document.getElementById('bedrock-cancel-btn');
    if (btn) { btn.disabled = true; btn.textContent = '正在取消...'; }
    try { await API.bedrockInstallCancel(S.sessionId); } catch (e) { /* 忽略 */ }
    setTimeout(closeProgress, 400);
  });
}

function closeProgress() {
  if (S.pollTimer) { clearInterval(S.pollTimer); S.pollTimer = null; }
  S.sessionId = '';
  closeBedrockModal('bedrock-progress-modal');
}

async function startBedrockInstall(version, link) {
  try {
    const r = await API.bedrockInstall(version, link);
    if (!r.success) {
      showToast(r.message || r.error || '无法开始下载', r.alreadyInstalled ? 'info' : 'error');
      return;
    }
    S.sessionId = r.sessionId;
    showProgressModal(version);
    pollBedrockInstall();
  } catch (e) {
    showToast('下载请求失败：' + (e.message || e), 'error');
  }
}

function pollBedrockInstall() {
  if (S.pollTimer) clearInterval(S.pollTimer);
  S.pollTimer = setInterval(async () => {
    try {
      const st = await API.bedrockInstallProgress(S.sessionId);
      const fill = document.getElementById('bedrock-progress-fill');
      const percent = document.getElementById('bedrock-percent');
      const stage = document.getElementById('bedrock-stage');
      const message = document.getElementById('bedrock-message');
      const speed = document.getElementById('bedrock-speed');

      const pct = Math.max(0, Math.min(100, st.progress || 0));
      if (fill) fill.style.width = pct + '%';
      if (percent) percent.textContent = pct + '%';
      if (stage) {
        const map = { running: '正在下载', completed: '安装完成', failed: '安装失败', cancelled: '已取消' };
        stage.textContent = map[st.status] || '正在处理...';
      }
      if (message) message.textContent = st.message || '';
      if (speed) {
        const parts = [];
        if (st.totalBytes > 0) parts.push(`${formatSize(st.downloadedBytes || 0)} / ${formatSize(st.totalBytes)}`);
        else if (st.downloadedBytes > 0) parts.push(formatSize(st.downloadedBytes));
        if (st.speed > 0) parts.push(formatSpeed(st.speed));
        speed.textContent = parts.join(' · ');
      }

      if (st.status === 'completed') {
        clearInterval(S.pollTimer);
        S.pollTimer = null;
        S.sessionId = '';
        showToast('基岩版安装完成', 'success');
        S.list = null;
        document.dispatchEvent(new CustomEvent('versepc:bedrock-installed'));
        setTimeout(() => closeBedrockModal('bedrock-progress-modal'), 1200);
      } else if (st.status === 'failed' || st.status === 'cancelled') {
        clearInterval(S.pollTimer);
        S.pollTimer = null;
        if (st.status === 'failed') {
          const msg = st.error || st.message || '下载失败';
          if (/登录|过期|401|未授权|token/i.test(msg)) {
            API.bedrockLogout().catch(() => {});
            showToast('123 云盘登录态已失效，已清除，请重新点下载并登录', 'error');
          } else {
            showToast(msg, 'error');
          }
          const btn = document.getElementById('bedrock-cancel-btn');
          if (btn) { btn.disabled = false; btn.textContent = '关闭'; btn.onclick = closeProgress; }
        } else {
          setTimeout(closeProgress, 500);
        }
      }
    } catch (e) {
      clearInterval(S.pollTimer);
      S.pollTimer = null;
      showToast('获取下载进度失败：' + (e.message || e), 'error');
    }
  }, 500);
}