/**
 * @file accounts.js
 * @description 账户管理 - 陶瓦联机(P2P组网)的状态管理、下载、主机/客户端模式、皮肤展示
 */
let accPollTimer = null;
let accDlSessionId = null;
let accDlPollTimer = null;

function accUpdateHeroBadge(statusType, text) {
  const badge = document.getElementById('acc-hero-badge');
  if (!badge) return;
  const dot = badge.querySelector('.acc-badge-dot');
  const textEl = document.getElementById('acc-badge-text');
  badge.className = 'acc-hero-status-badge';
  if (statusType === 'running') badge.classList.add('running');
  if (statusType === 'installed') badge.classList.add('installed');
  if (textEl) textEl.textContent = text;
}

async function accLoadStatus() {
  try {
    const status = await API.easytierStatus();
    const installPanel = document.getElementById('acc-install-panel');
    const controlPanel = document.getElementById('acc-control-panel');
    const joinPanel = document.getElementById('acc-join-panel');
    const peersPanel = document.getElementById('acc-peers-panel');
    const statusGrid = document.getElementById('acc-status-grid');
    const configSection = document.getElementById('acc-config-section');
    const startBtn = document.getElementById('acc-start-btn');
    const stopBtn = document.getElementById('acc-stop-btn');

    if (status.running) {
      installPanel.style.display = 'none';
      controlPanel.style.display = '';
      joinPanel.style.display = 'none';
      peersPanel.style.display = '';
      statusGrid.style.display = '';
      configSection.style.display = 'none';
      startBtn.style.display = 'none';
      stopBtn.style.display = '';

      accUpdateHeroBadge('running', '运行中');
      document.getElementById('acc-hero-desc').textContent = 'P2P 虚拟组网已启动';

      document.getElementById('acc-status-mode').textContent = status.mode === 'host' ? '主机模式' : '客户端模式';
      document.getElementById('acc-status-ip').textContent = status.virtualIP || '等待分配...';
      document.getElementById('acc-status-peers').textContent = '0';
      document.getElementById('acc-status-port').textContent = status.gamePort || 25565;

      if (status.mode === 'host') {
        document.getElementById('acc-card-invitation').style.display = '';
        document.getElementById('acc-card-connect').style.display = 'none';
        document.getElementById('acc-status-invitation').textContent = status.roomCode || '等待分配...';
      } else {
        document.getElementById('acc-card-invitation').style.display = 'none';
        document.getElementById('acc-card-connect').style.display = '';
        document.getElementById('acc-status-connect').textContent = status.virtualIP || '等待分配...';
      }

      if (status.state) {
        const stateType = status.state.state;
        if (stateType === 'host-ok' && status.state.room) {
          const roomVal = status.state.room;
          const roomStr = (typeof roomVal === 'object' && roomVal !== null) ? (roomVal.code || '') : roomVal;
          document.getElementById('acc-status-invitation').textContent = roomStr;
          document.getElementById('acc-status-peers').textContent = '1';
        } else if (stateType === 'guest-ok' && status.state.url) {
          document.getElementById('acc-status-connect').textContent = status.state.url;
          document.getElementById('acc-status-ip').textContent = status.state.url;
          document.getElementById('acc-status-peers').textContent = '1';
        }
      }

      if (accPollTimer) clearInterval(accPollTimer);
      accPollTimer = setInterval(accRefreshStatus, 3000);
    } else if (status.installed || status.downloading) {
      installPanel.style.display = status.downloading ? 'none' : '';
      controlPanel.style.display = '';
      joinPanel.style.display = '';
      peersPanel.style.display = 'none';
      statusGrid.style.display = 'none';
      configSection.style.display = 'none';
      startBtn.style.display = '';
      stopBtn.style.display = 'none';
      document.getElementById('acc-download-btn').style.display = 'none';
      document.getElementById('acc-download-progress').style.display = 'none';

      if (status.downloading) {
        accUpdateHeroBadge('installed', '下载中...');
      } else {
        accUpdateHeroBadge('installed', '已就绪');
      }
      document.getElementById('acc-hero-desc').textContent = 'P2P 虚拟组网加速，降低 Minecraft 联机延迟';
    } else {
      installPanel.style.display = '';
      controlPanel.style.display = 'none';
      joinPanel.style.display = 'none';
      peersPanel.style.display = 'none';
      document.getElementById('acc-download-btn').style.display = '';
      document.getElementById('acc-download-progress').style.display = 'none';
      accUpdateHeroBadge('', '未安装');
      document.getElementById('acc-hero-desc').textContent = 'P2P 虚拟组网加速，降低 Minecraft 联机延迟';
    }
  } catch (e) {
    console.error('[Acc] Load status error:', e);
  }
}

async function accRefreshStatus() {
  try {
    const status = await API.easytierStatus();
    if (!status.running) {
      if (accPollTimer) { clearInterval(accPollTimer); accPollTimer = null; }
      accLoadStatus();
      return;
    }

    if (status.state) {
      const stateType = status.state.state;
      if (stateType === 'host-ok') {
        const roomCode = status.state.room || status.roomCode || '';
        document.getElementById('acc-status-invitation').textContent = roomCode;
        document.getElementById('acc-status-peers').textContent = '1';
      } else if (stateType === 'guest-ok') {
        const connectUrl = status.state.url || status.virtualIP || '';
        document.getElementById('acc-status-connect').textContent = connectUrl;
        document.getElementById('acc-status-ip').textContent = connectUrl;
        document.getElementById('acc-status-peers').textContent = '1';
      } else if (stateType === 'host-scanning' || stateType === 'host-starting') {
        document.getElementById('acc-status-peers').textContent = '...';
      } else if (stateType === 'guest-connecting' || stateType === 'guest-starting') {
        document.getElementById('acc-status-peers').textContent = '...';
      } else if (stateType === 'exception') {
        document.getElementById('acc-status-peers').textContent = '!';
      }
    }

    const peersResult = await API.easytierPeers();
    if (peersResult.state && peersResult.state.state === 'host-ok' && peersResult.state.room) {
      document.getElementById('acc-status-invitation').textContent = peersResult.state.room;
    }
    if (peersResult.state && peersResult.state.state === 'guest-ok' && peersResult.state.url) {
      document.getElementById('acc-status-connect').textContent = peersResult.state.url;
      document.getElementById('acc-status-ip').textContent = peersResult.state.url;
    }
  } catch (e) {
    console.error('[Acc] Refresh status error:', e);
  }
}

async function accDownload() {
  const btn = document.getElementById('acc-download-btn');
  btn.disabled = true;
  btn.textContent = '准备下载...';
  document.getElementById('acc-download-progress').style.display = '';

  try {
    const result = await API.easytierDownload();
    accDlSessionId = result.sessionId;

    if (accDlPollTimer) clearInterval(accDlPollTimer);
    accDlPollTimer = setInterval(async () => {
      try {
        const status = await API.easytierDownloadStatus(accDlSessionId);
        document.getElementById('acc-progress-fill').style.width = status.progress + '%';
        document.getElementById('acc-progress-pct').textContent = status.progress + '%';
        document.getElementById('acc-progress-status').textContent = status.status === 'downloading' ? '下载中' : status.status === 'extracting' ? '解压中' : status.status;
        document.getElementById('acc-progress-msg').textContent = status.message || '';

        if (status.status === 'completed') {
          clearInterval(accDlPollTimer);
          accDlPollTimer = null;
          showToast('陶瓦联机安装完成！', 'success');
          await accLoadStatus();
        } else if (status.status === 'error') {
          clearInterval(accDlPollTimer);
          accDlPollTimer = null;
          showToast('安装失败: ' + (status.message || '未知错误'), 'error');
          btn.disabled = false;
          btn.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:18px;height:18px"><path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>重新下载';
        }
      } catch (e) {
        console.warn('[Terracotta] 安装进度轮询失败:', e);
      }
    }, 500);
  } catch (e) {
    showToast('启动下载失败: ' + e.message, 'error');
    btn.disabled = false;
    btn.textContent = '下载并安装';
  }
}

async function accStartHost() {
  const portEl = document.getElementById('acc-game-port');
  const gamePort = (portEl && parseInt(portEl.value, 10)) || 25565;
  try {
    showToast('正在初始化陶瓦联机...', 'info');
    document.getElementById('acc-start-btn').disabled = true;
    document.getElementById('acc-start-btn').textContent = '初始化中...';

    const result = await API.easytierHost(gamePort);

    document.getElementById('acc-start-btn').style.display = 'none';
    document.getElementById('acc-stop-btn').style.display = '';
    document.getElementById('acc-status-grid').style.display = '';
    document.getElementById('acc-config-section').style.display = 'none';
    document.getElementById('acc-join-panel').style.display = 'none';

    accUpdateHeroBadge('running', '运行中');
    document.getElementById('acc-hero-desc').textContent = 'P2P 虚拟组网已启动';

    document.getElementById('acc-status-mode').textContent = '主机模式';
    document.getElementById('acc-status-ip').textContent = '等待分配...';
    document.getElementById('acc-status-peers').textContent = '0';
    document.getElementById('acc-status-port').textContent = gamePort;
    document.getElementById('acc-card-invitation').style.display = '';
    document.getElementById('acc-card-connect').style.display = 'none';
    document.getElementById('acc-status-invitation').textContent = '等待分配...';

    if (accPollTimer) clearInterval(accPollTimer);
    accPollTimer = setInterval(accRefreshStatus, 3000);

    showToast('陶瓦联机已启动', 'success');
  } catch (e) {
    showToast('启动失败: ' + e.message, 'error');
    document.getElementById('acc-start-btn').disabled = false;
    document.getElementById('acc-start-btn').innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:16px;height:16px"><polygon points="5 3 19 12 5 21 5 3"/></svg>启动加速';
  }
}

async function accJoin() {
  const codeText = document.getElementById('acc-join-code').value.trim();
  if (!codeText) {
    showToast('请输入房间码', 'error');
    return;
  }

  try {
    showToast('正在加入联机网络...', 'info');
    const joinBtn = document.querySelector('#acc-join-panel .btn-primary');
    joinBtn.disabled = true;
    joinBtn.textContent = '连接中...';

    const result = await API.easytierGuest(codeText);

    document.getElementById('acc-control-panel').style.display = '';
    document.getElementById('acc-join-panel').style.display = 'none';
    document.getElementById('acc-install-panel').style.display = 'none';
    document.getElementById('acc-peers-panel').style.display = '';
    document.getElementById('acc-status-grid').style.display = '';
    document.getElementById('acc-config-section').style.display = 'none';
    document.getElementById('acc-start-btn').style.display = 'none';
    document.getElementById('acc-stop-btn').style.display = '';

    accUpdateHeroBadge('running', '运行中');
    document.getElementById('acc-hero-desc').textContent = '已加入 P2P 联机网络';

    document.getElementById('acc-status-mode').textContent = '客户端模式';
    document.getElementById('acc-status-ip').textContent = '等待分配...';
    document.getElementById('acc-status-peers').textContent = '0';
    document.getElementById('acc-status-port').textContent = '--';
    document.getElementById('acc-card-invitation').style.display = 'none';
    document.getElementById('acc-card-connect').style.display = '';
    document.getElementById('acc-status-connect').textContent = '等待分配...';

    if (accPollTimer) clearInterval(accPollTimer);
    accPollTimer = setInterval(accRefreshStatus, 3000);

    showToast('已加入联机网络，正在连接...', 'success');
  } catch (e) {
    showToast('加入失败: ' + e.message, 'error');
    const joinBtn = document.querySelector('#acc-join-panel .btn-primary');
    if (joinBtn) {
      joinBtn.disabled = false;
      joinBtn.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:16px;height:16px"><path d="M15 3h4a2 2 0 012 2v14a2 2 0 01-2 2h-4"/><polyline points="10 17 15 12 10 7"/><line x1="15" y1="12" x2="3" y2="12"/></svg>加入联机网络';
    }
  }
}

async function accStop() {
  if (accPollTimer) { clearInterval(accPollTimer); accPollTimer = null; }
  try {
    await API.easytierStop();
    showToast('加速器已停止', 'info');
  } catch (e) {
    console.warn('[Acc] 停止加速器失败:', e);
  }
  await accLoadStatus();
}

function accCopyInvitation() {
  const code = document.getElementById('acc-status-invitation').textContent;
  if (code && code !== '--' && code !== '等待分配...') {
    window.electronAPI.clipboard.writeText(code).then(() => {
      showToast('房间码已复制', 'success');
    });
  }
}

function accCopyConnect() {
  const addr = document.getElementById('acc-status-connect').textContent;
  if (addr && addr !== '--' && addr !== '等待分配...') {
    window.electronAPI.clipboard.writeText(addr).then(() => {
      showToast('连接地址已复制', 'success');
    });
  }
}

async function loadSettingsFromLocal() {
  try {
    const raw = await window.electronAPI.store.get('versepc_settings');
    return raw ? (typeof raw === 'string' ? JSON.parse(raw) : raw) : null;
  } catch (e) { return null; }
}

async function toggleMod(modId, enabled) {
  try {
    await API.toggleMod(modId, enabled);
    await loadInstalledMods();
    showToast(enabled ? '模组已启用' : '模组已禁用', 'info');
  } catch (e) { showToast('操作失败', 'error'); }
}

async function deleteMod(modId) {
  const confirmed = await showConfirmDialog('删除模组', '确定要删除此模组吗？', '删除', '取消');
  if (!confirmed) return;
  try {
    await API.deleteMod(modId);
    showToast('模组已删除', 'success');
    await loadInstalledMods();
  } catch (e) { showToast('删除失败', 'error'); }
}

function _refreshAccountAvatars() {
  const ts = Date.now();
  document.querySelectorAll('.account-avatar-img').forEach(img => {
    const src = img.src;
    if (src && src.includes('/api/avatar')) {
      img.src = src.replace(/&_=\d+/, '') + '&_=' + ts;
    }
  });
  try {
    const selectedId = localStorage.getItem('versepc_selected_account');
    if (selectedId) {
      API.getAccounts().then(accounts => {
        const selected = accounts.find(a => a.id === selectedId);
        if (selected) {
          const accUuid = (selected.uuid || '').replace(/-/g, '');
          if (accUuid) {
            const serverParam = selected.serverUrl ? `&serverUrl=${encodeURIComponent(selected.serverUrl)}` : '';
            const usernameParam = selected.username ? `&username=${encodeURIComponent(selected.username)}` : '';
            const offlineParam = (selected.type === 'offline' && !selected.serverUrl) ? '&offline=1' : '';
            const newUrl = `/api/avatar?uuid=${accUuid}${serverParam}${usernameParam}${offlineParam}&_=${ts}`;
            const homeAvatar = document.getElementById('home-avatar');
            if (homeAvatar) {
              const existingImg = homeAvatar.querySelector('.account-avatar-img');
              if (existingImg && existingImg.src && existingImg.src.includes('/api/avatar')) {
                existingImg.src = newUrl;
              }
            }
            try { localStorage.setItem('cachedAvatarUrl', newUrl); } catch(e) {}
          }
        }
      }).catch(() => {});
    }
  } catch (e) {}
}

/** 加载头像图片到容器中（兼容 Tauri invoke 和旧 HTTP） */
async function _loadAvatarImg(container, uuid, serverUrl, username, type, cacheVersion) {
  if (!container || !uuid) return;
  container.innerHTML = '';
  container.style.backgroundImage = '';
  const img = document.createElement('img');
  img.className = 'account-avatar-img';
  img.width = 64;
  img.height = 64;

  const offline = (type === 'offline' && !serverUrl) ? '1' : '0';

  if (window.__TAURI__ && window.__TAURI__.core) {
    try {
      const result = await window.__TAURI__.core.invoke('get_avatar', {
        uuid,
        serverUrl: serverUrl || undefined,
        username: username || undefined,
        offline: offline === '1'
      });
      if (result && result.success && result.data_url) {
        img.onload = function() {
          try {
            if (result.is_full_skin) {
              const cropped = cropSkinHeadCanvas(this, 64);
              if (cropped) { this.onload = null; this.src = cropped; return; }
            }
            const canvas = document.createElement('canvas');
            canvas.width = 64; canvas.height = 64;
            const ctx = canvas.getContext('2d');
            ctx.drawImage(this, 0, 0, 64, 64);
            const dataUrl = canvas.toDataURL('image/png');
            if (dataUrl && dataUrl.length > 100) {
              try { localStorage.setItem('cachedAvatarData', dataUrl); } catch(e) {}
            }
          } catch(e) {}
        };
        img.src = result.data_url;
        container.appendChild(img);
        return;
      }
    } catch(e) {
      // 回退到后续逻辑
    }
  }

  // 非 Tauri 或 invoke 失败：HTTP 回退
  const httpUrl = `/api/avatar?uuid=${uuid}${serverUrl ? '&serverUrl=' + encodeURIComponent(serverUrl) : ''}${username ? '&username=' + encodeURIComponent(username) : ''}${offline === '1' ? '&offline=1' : ''}&_=${cacheVersion || Date.now()}`;
  fetch(httpUrl).then(resp => {
    const isFullSkin = resp.headers.get('X-Is-Full-Skin') === 'true';
    return resp.blob().then(blob => ({ blob, isFullSkin }));
  }).then(({ blob, isFullSkin }) => {
    const objUrl = URL.createObjectURL(blob);
    img.onload = function() {
      try {
        if (isFullSkin) {
          const cropped = cropSkinHeadCanvas(this, 64);
          if (cropped) { this.onload = null; this.src = cropped; URL.revokeObjectURL(objUrl); return; }
        }
        URL.revokeObjectURL(objUrl);
        const canvas = document.createElement('canvas');
        canvas.width = 64; canvas.height = 64;
        const ctx = canvas.getContext('2d');
        ctx.drawImage(this, 0, 0, 64, 64);
        const dataUrl = canvas.toDataURL('image/png');
        if (dataUrl && dataUrl.length > 100) {
          try { localStorage.setItem('cachedAvatarData', dataUrl); } catch(e) {}
        }
      } catch(e) {}
    };
    img.src = objUrl;
    container.appendChild(img);
  }).catch(() => {
    img.src = httpUrl;
    img.onerror = function() {
      img.style.display = 'none';
      setTimeout(() => {
        const retryImg = document.createElement('img');
        retryImg.src = httpUrl.split('&_=')[0] + '&_=' + Date.now();
        retryImg.className = 'account-avatar-img';
        retryImg.width = 64;
        retryImg.height = 64;
        retryImg.onload = function() {
          container.innerHTML = '';
          container.appendChild(retryImg);
        };
      }, 2000);
    };
    container.appendChild(img);
  });
}

async function loadAccounts() {
  try {
    const [accounts, settings] = await Promise.all([
      API.getAccounts(),
      API.getSettings(),
    ]);
    const container = document.getElementById('accounts-list');

    if (accounts.length === 0) {
      container.innerHTML = '<p class="empty-text">暂无账户，请添加账户</p>';
    } else {
      container.innerHTML = accounts.map(acc => {
        const isSelected = acc.id === settings.selectedAccount;
        const typeLabel = acc.type === 'microsoft' ? '微软账户' : acc.type === 'thirdparty' ? '外置登录' : '离线账户';
        const typeClass = acc.type === 'microsoft' ? 'microsoft' : acc.type === 'thirdparty' ? 'thirdparty' : 'offline';
        const accUuid = (acc.uuid || '').replace(/-/g, '');
        const avatarHtml = accUuid
          ? `<img src="" alt="" class="account-avatar-img" data-uuid="${accUuid}" data-server-url="${acc.serverUrl || ''}" data-username="${acc.username || ''}" data-offline="${(acc.type === 'offline' && !acc.serverUrl) ? '1' : '0'}">`
          : `<span class="account-avatar-text">${acc.username.charAt(0).toUpperCase()}</span>`;
        return `<div class="account-item ${isSelected ? 'selected' : ''}" onclick="showAccountDetail('${acc.id}')">
          <div class="account-avatar">${avatarHtml}</div>
          <div class="account-item-info">
            <div class="account-item-name">${escapeHtml(acc.username)}</div>
            <div class="account-item-uuid">${acc.uuid}</div>
            <div class="account-item-type ${typeClass}">${typeLabel}</div>
          </div>
          <div class="mod-actions">
            ${!isSelected ? `<button class="btn btn-primary btn-sm" onclick="event.stopPropagation(); selectAccount('${acc.id}')">选择</button>` : '<span style="color: var(--accent); font-size: 12px; padding: 4px 10px; display: inline-flex; align-items: center;">当前使用</span>'}
            <button class="btn btn-danger btn-sm" onclick="event.stopPropagation(); deleteAccount('${acc.id}')">删除</button>
          </div>
        </div>`;
      }).join('');
      
      container.querySelectorAll('.account-avatar-img').forEach(img => {
        const uuid = img.dataset.uuid;
        if (!uuid) return;
        const serverUrl = img.dataset.serverUrl || undefined;
        const username = img.dataset.username || undefined;
        const offline = img.dataset.offline === '1';

        if (window.__TAURI__ && window.__TAURI__.core) {
          window.__TAURI__.core.invoke('get_avatar', { uuid, serverUrl, username, offline }).then(result => {
            if (result && result.success && result.data_url) {
              img.onload = function() {
                if (result.is_full_skin) {
                  const cropped = cropSkinHeadCanvas(this, 64);
                  if (cropped) { this.onload = null; this.src = cropped; return; }
                }
              };
              img.src = result.data_url;
            }
          }).catch(() => {});
        } else {
          // 非 Tauri 环境回退到 HTTP
          const httpUrl = `/api/avatar?uuid=${uuid}${serverUrl ? '&serverUrl=' + encodeURIComponent(serverUrl) : ''}${username ? '&username=' + encodeURIComponent(username) : ''}${offline ? '&offline=1' : ''}`;
          img.dataset.originalSrc = httpUrl;
          fetch(httpUrl).then(resp => {
            const isFullSkin = resp.headers.get('X-Is-Full-Skin') === 'true';
            return resp.blob().then(blob => ({ blob, isFullSkin }));
          }).then(({ blob, isFullSkin }) => {
            const objUrl = URL.createObjectURL(blob);
            img.onload = function() {
              if (isFullSkin) {
                const cropped = cropSkinHeadCanvas(this, 64);
                if (cropped) { this.onload = null; this.src = cropped; URL.revokeObjectURL(objUrl); return; }
              }
              URL.revokeObjectURL(objUrl);
            };
            img.onerror = function() {
              const origSrc = this.dataset.originalSrc;
              if (!origSrc) return;
              const avatarDiv = this.parentElement;
              if (avatarDiv) {
                this.style.display = 'none';
                setTimeout(() => {
                  const retryImg = document.createElement('img');
                  retryImg.src = origSrc + (origSrc.includes('?') ? '&' : '?') + '_=' + Date.now();
                  retryImg.className = 'account-avatar-img';
                  retryImg.dataset.originalSrc = origSrc;
                  retryImg.onerror = function() { this.style.display = 'none'; };
                  retryImg.onload = function() {
                    avatarDiv.innerHTML = '';
                    avatarDiv.appendChild(retryImg);
                  };
                  avatarDiv.innerHTML = '';
                  avatarDiv.appendChild(retryImg);
                }, 2000);
              }
            };
            img.src = objUrl;
          }).catch(() => {
            img.onload = null;
            img.onerror = function() {
              const origSrc = this.dataset.originalSrc;
              if (!origSrc) return;
              const avatarDiv = this.parentElement;
              if (avatarDiv) {
                this.style.display = 'none';
                setTimeout(() => {
                  const retryImg = document.createElement('img');
                  retryImg.src = origSrc + (origSrc.includes('?') ? '&' : '?') + '_=' + Date.now();
                  retryImg.className = 'account-avatar-img';
                  retryImg.dataset.originalSrc = origSrc;
                  retryImg.onerror = function() { this.style.display = 'none'; };
                  retryImg.onload = function() {
                    avatarDiv.innerHTML = '';
                    avatarDiv.appendChild(retryImg);
                  };
                  avatarDiv.innerHTML = '';
                  avatarDiv.appendChild(retryImg);
                }, 2000);
              }
            };
            img.src = httpUrl;
          });
        }
      });
    }

    const selectedAccount = accounts.find(a => a.id === settings.selectedAccount) || accounts[0];
    if (selectedAccount) {
      const accUuid = (selectedAccount.uuid || '').replace(/-/g, '');
      let accSkinUrl = '';
      if (accUuid) {
        const serverParam = selectedAccount.serverUrl ? `&serverUrl=${encodeURIComponent(selectedAccount.serverUrl)}` : '';
        const usernameParam = selectedAccount.username ? `&username=${encodeURIComponent(selectedAccount.username)}` : '';
        const offlineParam = (selectedAccount.type === 'offline' && !selectedAccount.serverUrl) ? '&offline=1' : '';
        accSkinUrl = `/api/avatar?uuid=${accUuid}${serverParam}${usernameParam}${offlineParam}&_=${AVATAR_CACHE_VERSION}`;
      }
      
      document.getElementById('home-player-name').textContent = selectedAccount.username;
      const accountTypeText = selectedAccount.type === 'microsoft' ? '微软账户' : selectedAccount.type === 'thirdparty' ? '外置登录' : '离线模式';
      document.getElementById('home-account-type').textContent = accountTypeText;
      try { localStorage.setItem('cachedPlayerName', selectedAccount.username); localStorage.setItem('cachedAccountType', accountTypeText); } catch(e) {}
      
      // 使用统一函数加载首页头像
      _loadAvatarImg(
        document.getElementById('home-avatar'),
        accUuid,
        selectedAccount.serverUrl,
        selectedAccount.username,
        selectedAccount.type,
        AVATAR_CACHE_VERSION
      );

      document.getElementById('launch-player-name').textContent = selectedAccount.username;
      // 使用统一函数加载启动页头像
      _loadAvatarImg(
        document.getElementById('launch-avatar'),
        accUuid,
        selectedAccount.serverUrl,
        selectedAccount.username,
        selectedAccount.type,
        AVATAR_CACHE_VERSION
      );
    } else {
      const homeAvatar = document.getElementById('home-avatar');
      homeAvatar.className = 'account-avatar account-avatar--empty';
      homeAvatar.innerHTML = '';
      document.getElementById('home-player-name').textContent = '未登录';
      document.getElementById('home-account-type').textContent = '离线模式';
      const launchAvatar = document.getElementById('launch-avatar');
      launchAvatar.innerHTML = '<img src="img/icon.png" alt="" class="account-avatar-img">';
      document.getElementById('launch-player-name').textContent = 'Player';
    }
  } catch (e) { console.error('[Accounts] Failed to update account display:', e); }
}

async function selectAccount(accountId) {
  try {
    await API.selectAccount(accountId);
    await loadAccounts();
    showToast('已切换账户', 'info');
  } catch (e) { showToast('切换失败', 'error'); }
}

async function deleteAccount(accountId) {
  const confirmed = await showConfirmDialog('删除账户', '确定要删除此账户吗？', '删除', '取消');
  if (!confirmed) return;
  try {
    await API.deleteAccount(accountId);
    await loadAccounts();
    showToast('账户已删除', 'success');
  } catch (e) { showToast('删除失败', 'error'); }
}

let _currentDetailAccount = null;
let _skinViewer = null;
let _skinResizeObserver = null;
let _currentSkinBg = 'white';

function showAccountDetail(accountId) {
  API.getAccounts().then(async accounts => {
    const acc = accounts.find(a => a.id === accountId);
    if (!acc) return;
    _currentDetailAccount = acc;
    const accUuid = (acc.uuid || '').replace(/-/g, '');
    document.getElementById('detail-username').textContent = acc.username;
    document.getElementById('detail-uuid').textContent = acc.uuid || '-';
    const typeMap = { microsoft: '正版', thirdparty: '外置登录', offline: '离线' };
    const badgeLabel = typeMap[acc.type] || '离线';
    document.getElementById('detail-skin-type').textContent = badgeLabel;
    const typeEl = document.getElementById('detail-account-type');
    if (typeEl) typeEl.textContent = badgeLabel;
    document.getElementById('accounts-list').style.display = 'none';
    const header = document.querySelector('#page-accounts .page-header');
    if (header) header.style.display = 'none';
    const pageAccounts = document.getElementById('page-accounts');
    const activePage = pageAccounts.closest('.page.active') || pageAccounts.parentElement;
    if (activePage) {
      pageAccounts.style.height = activePage.clientHeight + 'px';
    }
    pageAccounts.style.overflow = 'hidden';
    document.getElementById('page-account-detail').style.display = '';
    setSkinBg(_currentSkinBg);

    // 获取皮肤纹理 data URL
    let skinDataUrl = '';
    let skinModel = ((acc.skinModel || '').toLowerCase() === 'slim') ? 'slim' : 'default';
    if (accUuid) {
      if (window.__TAURI__ && window.__TAURI__.core) {
        try {
          const result = await window.__TAURI__.core.invoke('get_skin_texture', {
            uuid: accUuid,
            serverUrl: acc.serverUrl || undefined,
            username: acc.username || undefined
          });
          if (result && result.success && result.data_url) {
            skinDataUrl = result.data_url;
            if (result.model === 'slim' || result.model === 'default') skinModel = result.model;
          }
        } catch(e) {
          console.warn('[SkinViewer] invoke get_skin_texture failed, fallback to HTTP', e);
        }
      }
      // 非 Tauri 或 invoke 失败，回退到 HTTP
      if (!skinDataUrl) {
        skinDataUrl = `/api/skin-texture?uuid=${accUuid}${acc.serverUrl ? '&serverUrl=' + encodeURIComponent(acc.serverUrl) : ''}${acc.username ? '&username=' + encodeURIComponent(acc.username) : ''}`;
        // 尝试通过 HEAD 探测皮肤模型
        try {
          const controller = new AbortController();
          const timeoutId = setTimeout(() => controller.abort(), 5000);
          const probe = await fetch(skinDataUrl.replace(/&_=\d+/, ''), { method: 'HEAD', signal: controller.signal });
          clearTimeout(timeoutId);
          const headerModel = probe.headers.get('X-Skin-Model');
          if (headerModel === 'slim' || headerModel === 'default') skinModel = headerModel;
        } catch (e) {}
      }
    }
    if (_currentDetailAccount) _currentDetailAccount._resolvedSkinModel = skinModel;

    await initSkinViewer(skinDataUrl, skinModel);
    loadSkinSelector(acc);
  });
}

function showAccountList() {
  document.getElementById('page-account-detail').style.display = 'none';
  document.getElementById('accounts-list').style.display = '';
  const header = document.querySelector('#page-accounts .page-header');
  if (header) header.style.display = '';
  const pageAccounts = document.getElementById('page-accounts');
  pageAccounts.style.height = '';
  pageAccounts.style.overflow = '';
  destroySkinViewer();
  _currentDetailAccount = null;
}

function destroySkinViewer() {
  if (_skinResizeObserver) {
    try { _skinResizeObserver.disconnect(); } catch (e) {}
    _skinResizeObserver = null;
  }
  if (_skinViewer) {
    try { _skinViewer.dispose(); } catch (e) {}
    _skinViewer = null;
  }
  const container = document.getElementById('skin-3d-container');
  if (container) container.innerHTML = '';
}

async function initSkinViewer(skinUrl, resolvedModel) {
  destroySkinViewer();
  const container = document.getElementById('skin-3d-container');
  if (!container) return;
  try {
    if (typeof skinview3d === 'undefined') {
      container.innerHTML = '<div style="display:flex;flex-direction:column;align-items:center;justify-content:center;height:100%;color:var(--text-secondary);font-size:14px;gap:8px;"><span style="font-size:24px;">⏳</span><span>正在加载皮肤查看器...</span></div>';
      await _lazyLoadScript('js/skinview3d.bundle.js');
    }
    let skinModel = resolvedModel || ((_currentDetailAccount?.skinModel || '').toLowerCase() === 'slim') ? 'slim' : 'default';
    if (_currentDetailAccount) _currentDetailAccount._resolvedSkinModel = skinModel;
    await new Promise(r => setTimeout(r, 100));
    container.innerHTML = '';
    const cw = container.clientWidth || 360;
    const ch = container.clientHeight || 420;
    _skinViewer = new skinview3d.SkinViewer({
      width: cw,
      height: ch,
      skin: skinUrl || undefined,
      model: skinModel
    });
    container.appendChild(_skinViewer.canvas);
    _skinViewer.fov = 30;
    _skinViewer.zoom = 0.85;
    _skinViewer.autoRotate = true;
    _skinViewer.autoRotateSpeed = 0.5;
    _skinViewer.animation = new skinview3d.IdleAnimation();
    _skinViewer.animation.speed = 0.8;
    _skinViewer.cameraLight.intensity = 1.2;
    _skinViewer.globalLight.intensity = 2.5;
    _skinViewer.background = _currentSkinBg === 'black' ? 0x000000 : 0xffffff;
    _skinViewer.nameTag = _currentDetailAccount ? _currentDetailAccount.username : null;
    _skinResizeObserver = new ResizeObserver(() => {
      if (_skinViewer && container) {
        const w = container.clientWidth;
        const h = container.clientHeight;
        if (w > 0 && h > 0) _skinViewer.setSize(w, h);
      }
    });
    _skinResizeObserver.observe(container);
  } catch (e) {
    console.error('[SkinViewer] init error:', e);
    container.innerHTML = '<div style="display:flex;flex-direction:column;align-items:center;justify-content:center;height:100%;color:var(--text-secondary);font-size:14px;gap:8px;"><span style="font-size:32px;">👤</span><span>皮肤加载失败</span><span style="font-size:12px;color:var(--text-tertiary);">请检查网络连接或重新登录</span></div>';
  }
}

async function detailSelectAccount() {
  if (!_currentDetailAccount) return;
  await selectAccount(_currentDetailAccount.id);
  showAccountList();
}

async function detailDeleteAccount() {
  if (!_currentDetailAccount) return;
  await deleteAccount(_currentDetailAccount.id);
  showAccountList();
}

async function detailRefreshSkin() {
  if (!_currentDetailAccount || !_skinViewer) return;
  const acc = _currentDetailAccount;
  const accUuid = (acc.uuid || '').replace(/-/g, '');
  if (!accUuid) { showToast('无UUID', 'error'); return; }
  const skinUrl = `/api/skin-texture?uuid=${accUuid}${acc.serverUrl ? '&serverUrl=' + encodeURIComponent(acc.serverUrl) : ''}${acc.username ? '&username=' + encodeURIComponent(acc.username) : ''}&_=${Date.now()}`;
  try {
    let skinModel = ((_currentDetailAccount?.skinModel || '').toLowerCase() === 'slim') ? 'slim' : 'default';
    try {
      const probe = await fetch(skinUrl.replace(/&_=\d+/, ''), { method: 'HEAD' });
      const headerModel = probe.headers.get('X-Skin-Model');
      if (headerModel === 'slim' || headerModel === 'default') skinModel = headerModel;
    } catch (e) {}
    _currentDetailAccount._resolvedSkinModel = skinModel;
    await _skinViewer.loadSkin(skinUrl, { model: skinModel });
    _refreshAccountAvatars();
    showToast('皮肤已刷新', 'success');
  } catch (e) {
    showToast('皮肤刷新失败', 'error');
  }
}

function copyDetailUuid() {
  const uuidEl = document.getElementById('detail-uuid');
  if (!uuidEl) return;
  const text = uuidEl.textContent;
  if (!text || text === '-') return;
  if (navigator.clipboard) {
    navigator.clipboard.writeText(text).then(() => showToast('UUID已复制', 'success'));
  } else {
    const ta = document.createElement('textarea');
    ta.value = text;
    document.body.appendChild(ta);
    ta.select();
    document.execCommand('copy');
    document.body.removeChild(ta);
    showToast('UUID已复制', 'success');
  }
}

function setAnim(type) {
  if (!_skinViewer) return;
  const animMap = {
    idle: () => new skinview3d.IdleAnimation(),
    walk: () => new skinview3d.WalkingAnimation(),
    run: () => new skinview3d.RunningAnimation(),
    fly: () => new skinview3d.FlyingAnimation(),
    wave: () => new skinview3d.WaveAnimation(),
    crouch: () => new skinview3d.CrouchAnimation(),
    hit: () => new skinview3d.HitAnimation(),
    swim: () => new skinview3d.SwimAnimation()
  };
  const factory = animMap[type];
  if (!factory) return;
  _skinViewer.animation = factory();
  const speedMap = { idle: 0.6, walk: 0.8, run: 0.6, fly: 0.8, wave: 0.8, crouch: 0.5, hit: 0.9, swim: 0.7 };
  _skinViewer.animation.speed = speedMap[type] || 0.7;
  document.querySelectorAll('.acct-anim-btn').forEach(btn => {
    btn.classList.toggle('active', btn.dataset.anim === type);
  });
}

function setSkinBg(color) {
  _currentSkinBg = color;
  const left = document.getElementById('acct-detail-left');
  if (left) {
    left.classList.toggle('bg-black', color === 'black');
  }
  if (_skinViewer) {
    _skinViewer.background = color === 'black' ? 0x000000 : 0xffffff;
  }
  document.querySelectorAll('.acct-bg-btn').forEach(btn => {
    btn.classList.toggle('active', btn.dataset.bg === color);
  });
}

async function loadSkinSelector(acc) {
  const container = document.getElementById('acct-skin-grid');
  const section = document.getElementById('acct-detail-skins');
  if (!container || !section) return;
  if (acc.type !== 'offline' && acc.type !== 'microsoft') {
    section.style.display = 'none';
    return;
  }
  section.style.display = '';
  container.innerHTML = '';

  // 微软账户：只显示本地导入的皮肤库
  if (acc.type === 'microsoft') {
    try {
      const resp = await fetch(`/api/ms-skins/local?accountId=${encodeURIComponent(acc.id)}`);
      const data = await resp.json();
      if (data.success && data.skins && data.skins.length > 0) {
        data.skins.forEach(skin => {
          const div = document.createElement('div');
          div.className = 'acct-skin-item';
          div.title = `${skin.name}（点击应用到账户）`;
          div.onclick = () => applyMsSkin(skin.id);
          const canvas = document.createElement('canvas');
          canvas.width = 8;
          canvas.height = 8;
          canvas.style.width = '100%';
          canvas.style.height = '100%';
          canvas.style.imageRendering = 'pixelated';
          div.appendChild(canvas);
          const img = new Image();
          img.crossOrigin = 'anonymous';
          img.onload = function() {
            const ctx = canvas.getContext('2d');
            ctx.imageSmoothingEnabled = false;
            ctx.drawImage(img, 8, 8, 8, 8, 0, 0, 8, 8);
          };
          img.src = `/api/ms-skins/file?accountId=${encodeURIComponent(acc.id)}&skinId=${encodeURIComponent(skin.id)}&_=${Date.now()}`;
          // 长按或右键删除
          div.oncontextmenu = (e) => { e.preventDefault(); deleteMsSkin(skin.id, skin.name); };
          container.appendChild(div);
        });
      } else {
        const empty = document.createElement('div');
        empty.className = 'acct-skin-empty';
        empty.style.cssText = 'grid-column:1/-1;padding:12px;text-align:center;color:var(--text-muted);font-size:12px;';
        empty.textContent = '暂无本地皮肤，点击下方按钮导入';
        container.appendChild(empty);
      }
    } catch (e) {}
    return;
  }

  // 离线账户：显示默认皮肤 + 自定义皮肤
  try {
    const resp = await fetch('/api/default-skins');
    const data = await resp.json();
    if (!data.success || !data.skins) return;
    const currentSkinFile = acc.skinFile || 'steve_skin.png';
    const allSkins = data.skins.slice();
    if (currentSkinFile && currentSkinFile.startsWith('custom_') && !allSkins.some(s => s.file === currentSkinFile)) {
      allSkins.push({ id: 'custom', name: '自定义', file: currentSkinFile, model: acc.skinModel || 'default' });
    }
    allSkins.forEach(skin => {
      const div = document.createElement('div');
      div.className = 'acct-skin-item' + (skin.file === currentSkinFile ? ' active' : '');
      div.title = skin.name;
      div.onclick = () => selectSkin(skin.id, skin.file);
      const canvas = document.createElement('canvas');
      canvas.width = 8;
      canvas.height = 8;
      canvas.style.width = '100%';
      canvas.style.height = '100%';
      canvas.style.imageRendering = 'pixelated';
      div.appendChild(canvas);
      const img = new Image();
      img.crossOrigin = 'anonymous';
      img.onload = function() {
        const ctx = canvas.getContext('2d');
        ctx.imageSmoothingEnabled = false;
        ctx.drawImage(img, 8, 8, 8, 8, 0, 0, 8, 8);
      };
      if (skin.id === 'custom') {
        const accUuid = (acc.uuid || '').replace(/-/g, '');
        img.src = accUuid ? `/api/skin-head?uuid=${accUuid}&file=${encodeURIComponent(skin.file)}` : `/api/skin-head?id=steve`;
      } else {
        img.src = `/api/skin-head?id=${skin.id}`;
      }
      container.appendChild(div);
    });
  } catch (e) {}
}

async function selectSkin(skinId, skinFile) {
  if (!_currentDetailAccount) return;
  try {
    if (skinId === 'custom') {
      _currentDetailAccount.skinFile = skinFile;
      const accUuid = (_currentDetailAccount.uuid || '').replace(/-/g, '');
      const skinUrl = `/api/skin-texture?uuid=${accUuid}&_=${Date.now()}`;
      if (_skinViewer) await _skinViewer.loadSkin(skinUrl);
      loadSkinSelector(_currentDetailAccount);
      _refreshAccountAvatars();
      return;
    }
    const resp = await fetch('/api/set-account-skin', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ accountId: _currentDetailAccount.id, skinId })
    });
    const result = await resp.json();
    if (!result.success) { showToast('更换失败', 'error'); return; }
    _currentDetailAccount.skinFile = skinFile;
    const accUuid = (_currentDetailAccount.uuid || '').replace(/-/g, '');
    const skinUrl = `/api/skin-texture?uuid=${accUuid}&_=${Date.now()}`;
    if (_skinViewer) {
      await _skinViewer.loadSkin(skinUrl);
    }
    loadSkinSelector(_currentDetailAccount);
    _refreshAccountAvatars();
    showToast('皮肤已更换', 'success');
  } catch (e) {
    showToast('更换失败', 'error');
  }
}

async function handleSkinUpload(input) {
  if (!input.files || !input.files[0] || !_currentDetailAccount) return;
  const file = input.files[0];
  if (!file.name.toLowerCase().endsWith('.png')) {
    showToast('请选择 PNG 格式的皮肤文件', 'error');
    input.value = '';
    return;
  }
  showToast('正在导入…', 'info');
  try {
    const fileBase64 = await new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => {
        const dataUrl = reader.result;
        const base64 = dataUrl.split(',')[1];
        resolve(base64);
      };
      reader.onerror = reject;
      reader.readAsDataURL(file);
    });
    const modelSelect = document.getElementById('skin-model-select') || document.querySelector('input[name="skin-model"]:checked');
    let modelValue = 'default';
    if (modelSelect) {
      modelValue = modelSelect.value || modelSelect.getAttribute('data-model') || 'default';
    }

    // 微软账户：导入到本地皮肤库
    if (_currentDetailAccount.type === 'microsoft') {
      const resp = await fetch('/api/ms-skins/import', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          accountId: _currentDetailAccount.id,
          model: modelValue,
          fileBase64: fileBase64,
          name: file.name.replace(/\.png$/i, '')
        })
      });
      const result = await resp.json();
      if (result.success) {
        loadSkinSelector(_currentDetailAccount);
        showToast('皮肤已导入本地库，点击皮肤可应用到账户', 'success');
      } else {
        showToast(result.error || '导入失败', 'error');
      }
      input.value = '';
      return;
    }

    // 离线账户：直接应用
    const resp = await fetch('/api/upload-skin', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        accountId: _currentDetailAccount.id,
        model: modelValue,
        fileBase64: fileBase64
      })
    });
    const text = await resp.text();
    let result;
    try { result = JSON.parse(text); } catch (e) { showToast('上传失败: 服务器返回异常', 'error'); return; }
    if (result.success) {
      _currentDetailAccount.skinFile = result.fileName;
      const accUuid = (_currentDetailAccount.uuid || '').replace(/-/g, '');
      const skinUrl = `/api/skin-texture?uuid=${accUuid}&_=${Date.now()}`;
      if (_skinViewer) await _skinViewer.loadSkin(skinUrl);
      loadSkinSelector(_currentDetailAccount);
      _refreshAccountAvatars();
      showToast('皮肤已导入', 'success');
    } else {
      showToast(result.error || '上传失败', 'error');
    }
  } catch (e) {
    showToast('上传失败', 'error');
  }
  input.value = '';
}

// 微软账户：应用本地皮肤到 Minecraft 官方服务器
let _applyingMsSkin = false;
async function applyMsSkin(skinId) {
  if (!_currentDetailAccount || _currentDetailAccount.type !== 'microsoft') return;
  if (_applyingMsSkin) return; // 防止重复点击导致连续请求触发 Mojang 429
  _applyingMsSkin = true;
  showToast('正在上传到 Minecraft 官方…', 'info');
  try {
    const resp = await fetch('/api/ms-skins/apply', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        accountId: _currentDetailAccount.id,
        skinId: skinId
      })
    });
    const result = await resp.json();
    if (result.success) {
      const accUuid = (_currentDetailAccount.uuid || '').replace(/-/g, '');
      const skinUrl = `/api/skin-texture?uuid=${accUuid}&_=${Date.now()}`;
      if (_skinViewer) await _skinViewer.loadSkin(skinUrl);
      _refreshAccountAvatars();
      showToast('皮肤已应用到账户', 'success');
    } else if (result.needRelogin) {
      showToast('登录已过期，请重新登录微软账户', 'error');
    } else if (result.rateLimited) {
      showToast(result.error, 'error', 5000);
    } else {
      showToast(result.error || '应用失败', 'error');
    }
  } catch (e) {
    showToast('应用失败', 'error');
  } finally {
    _applyingMsSkin = false;
  }
}

// 微软账户：删除本地皮肤
async function deleteMsSkin(skinId, skinName) {
  if (!_currentDetailAccount || _currentDetailAccount.type !== 'microsoft') return;
  if (!confirm(`确定要删除皮肤「${skinName}」吗？`)) return;
  try {
    const resp = await fetch('/api/ms-skins/delete', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        accountId: _currentDetailAccount.id,
        skinId: skinId
      })
    });
    const result = await resp.json();
    if (result.success) {
      loadSkinSelector(_currentDetailAccount);
      showToast('已删除', 'success');
    } else {
      showToast(result.error || '删除失败', 'error');
    }
  } catch (e) {
    showToast('删除失败', 'error');
  }
}

async function startMsAuth() {
  showModal('msauth-modal');
  document.getElementById('msauth-status-text').textContent = '获取设备码中...';
  try {
    const result = await API.getMsDeviceCode();
    if (result.success) {
      const verifyUrl = result.verificationUriComplete || result.verificationUri;
      document.getElementById('msauth-url').href = verifyUrl;
      document.getElementById('msauth-url').textContent = verifyUrl;
      document.getElementById('msauth-code-text').textContent = result.userCode;
      document.getElementById('msauth-status-text').textContent = '等待登录...';

      try {
        await window.electronAPI?.clipboard?.writeText(result.userCode);
      } catch (e) {}

      setTimeout(async () => {
        try {
          await window.electronAPI?.openExternal?.(verifyUrl);
        } catch (e) {
          console.warn('[Auth] 自动打开浏览器失败:', e);
        }
      }, 500);

      if (msAuthPollInterval) clearInterval(msAuthPollInterval);
      let _msAuthRetryCount = 0;
      const _msAuthMaxRetry = 2;
      const _msAuthStartPoll = async (deviceCode, userCode) => {
        msAuthPollInterval = setInterval(async () => {
        try {
          const pollResult = await API.pollMsAuth(deviceCode);
          if (pollResult.success) {
            clearInterval(msAuthPollInterval);
            msAuthPollInterval = null;
            document.getElementById('msauth-status-text').textContent = '登录成功！';
            showToast(`欢迎，${pollResult.account.username}！`, 'success');
            setTimeout(() => closeMsAuthModal(), 1500);
            await loadAccounts();
          } else if (pollResult.pending) {
            document.getElementById('msauth-status-text').textContent = '等待验证...';
          } else {
            const isCodeUsed = pollResult.errorCode === 'invalid_grant' && pollResult.error && pollResult.error.includes('device_code');
            if (isCodeUsed && _msAuthRetryCount < _msAuthMaxRetry) {
              _msAuthRetryCount++;
              clearInterval(msAuthPollInterval);
              msAuthPollInterval = null;
              document.getElementById('msauth-status-text').textContent = '授权码已过期，正在重新获取...';
              try {
                const newResult = await API.getMsDeviceCode();
                if (newResult.success) {
                  const newVerifyUrl = newResult.verificationUriComplete || newResult.verificationUri;
                  document.getElementById('msauth-url').href = newVerifyUrl;
                  document.getElementById('msauth-url').textContent = newVerifyUrl;
                  document.getElementById('msauth-code-text').textContent = newResult.userCode;
                  document.getElementById('msauth-status-text').textContent = '新的授权码已获取，请重新登录...';
                  try { await window.electronAPI?.clipboard?.writeText(newResult.userCode); } catch (e) {}
                  try { await window.electronAPI?.openExternal?.(newVerifyUrl); } catch (e) {}
                  _msAuthStartPoll(newResult.deviceCode, newResult.userCode);
                  return;
                }
              } catch (retryErr) {
                console.warn('[Auth] 重新获取设备码失败:', retryErr);
              }
              document.getElementById('msauth-status-text').textContent = '获取新授权码失败，请点击重新登录';
              return;
            }
            let errMsg = pollResult.error || '验证失败';
            if (pollResult.needPurchase) errMsg = '❌ 该账号未购买Minecraft，请先购买游戏';
            else if (pollResult.needCreateProfile) errMsg = '❌ 未找到档案，请先在 Minecraft.net 创建角色名';
            else if (pollResult.isRateLimit) errMsg = `⏳ 请求过于频繁，请等待 ${pollResult.retryAfter || 5} 秒后重试`;
            else if (pollResult.xerr) errMsg = `❌ Xbox认证失败 (${pollResult.xerr})`;
            else if (isCodeUsed) errMsg = '授权码已过期或已被使用，请点击重新登录';
            document.getElementById('msauth-status-text').textContent = errMsg;
            if (pollResult.needPurchase || pollResult.needCreateProfile || pollResult.errorCode === 'invalid_grant') {
              clearInterval(msAuthPollInterval);
              msAuthPollInterval = null;
            }
          }
        } catch (e) {
          console.warn('[Auth] 微软登录轮询失败:', e);
        }
      }, (result.interval || 5) * 1000);
      };
      _msAuthStartPoll(result.deviceCode, result.userCode);
    } else {
      const errMsg = result.error || '获取设备码失败';
      document.getElementById('msauth-status-text').textContent = errMsg;
    }
  } catch (e) {
    const msg = e?.message || e?.error || '请求失败';
    document.getElementById('msauth-status-text').textContent = msg.includes('网络') || msg.includes('超时') ? '网络连接失败，请检查网络后重试' : msg;
  }
}

function closeMsAuthModal() {
  hideModal('msauth-modal');
  if (msAuthPollInterval) { clearInterval(msAuthPollInterval); msAuthPollInterval = null; }
}

function closeOfflineModal() {
  hideModal('offline-account-modal');
  document.getElementById('offline-username-input').value = '';
}

function copyMsCode() {
  const code = document.getElementById('msauth-code-text').textContent;
  window.electronAPI.clipboard.writeText(code).then(() => showToast('代码已复制', 'success'));
}

async function reopenMsAuthPage() {
  if (msAuthPollInterval) { clearInterval(msAuthPollInterval); msAuthPollInterval = null; }
  startMsAuth();
}

function closeThirdPartyModal() {
  hideModal('thirdparty-account-modal');
  document.getElementById('tp-username-input').value = '';
  document.getElementById('tp-password-input').value = '';
  document.getElementById('tp-server-info').style.display = 'none';
}

async function verifyThirdPartyServer(url) {
  const infoDiv = document.getElementById('tp-server-info');
  try {
    const result = await API.verifyThirdPartyServer(url);
    if (result.success) {
      document.getElementById('tp-server-name').textContent = result.meta?.serverName || '未知服务器';
      document.getElementById('tp-server-desc').textContent = result.meta?.implementationName || url;
      if (result.meta?.serverIcon) {
        document.getElementById('tp-server-icon').src = result.meta.serverIcon;
        document.getElementById('tp-server-icon').style.display = '';
      }
      infoDiv.style.display = '';
    } else {
      infoDiv.style.display = 'none';
    }
  } catch (e) {
    infoDiv.style.display = 'none';
  }
}

function cropSkinHeadCanvas(imgElement, outputSize = 64) {
  try {
    const canvas = document.createElement('canvas');
    const ctx = canvas.getContext('2d');
    const sw = imgElement.naturalWidth || imgElement.width;
    const sh = imgElement.naturalHeight || imgElement.height;
    if (sw < 64 || sh < 32) return null;
    const scale = sw / 64;
    canvas.width = outputSize;
    canvas.height = outputSize;
    ctx.imageSmoothingEnabled = false;
    const headX = Math.round(8 * scale), headY = Math.round(8 * scale), headDim = Math.round(8 * scale);
    ctx.drawImage(imgElement, headX, headY, headDim, headDim, 0, 0, outputSize, outputSize);
    if (sh >= 64) {
      const hatX = Math.round(40 * scale), hatY = Math.round(8 * scale);
      const hatCanvas = document.createElement('canvas');
      hatCanvas.width = outputSize;
      hatCanvas.height = outputSize;
      const hatCtx = hatCanvas.getContext('2d');
      hatCtx.imageSmoothingEnabled = false;
      hatCtx.drawImage(imgElement, hatX, hatY, headDim, headDim, 0, 0, outputSize, outputSize);
      const hatData = hatCtx.getImageData(0, 0, outputSize, outputSize);
      const faceData = ctx.getImageData(0, 0, outputSize, outputSize);
      for (let i = 0; i < hatData.data.length; i += 4) {
        const ha = hatData.data[i + 3] / 255;
        if (ha > 0) {
          const fa = faceData.data[i + 3] / 255;
          const outA = ha + fa * (1 - ha);
          if (outA > 0) {
            const invA = 1 / outA;
            faceData.data[i]     = Math.round((hatData.data[i] * ha + faceData.data[i] * fa * (1 - ha)) * invA);
            faceData.data[i + 1] = Math.round((hatData.data[i+1] * ha + faceData.data[i+1] * fa * (1 - ha)) * invA);
            faceData.data[i + 2] = Math.round((hatData.data[i+2] * ha + faceData.data[i+2] * fa * (1 - ha)) * invA);
            faceData.data[i + 3] = Math.round(outA * 255);
          }
        }
      }
      ctx.putImageData(faceData, 0, 0);
    }
    return canvas.toDataURL('image/png');
  } catch (e) {
    console.error('[cropSkinHeadCanvas] error:', e);
    return null;
  }
}

let tpPendingAuth = null;

function showProfileSelectModal(accessToken, clientToken, serverUrl, profiles) {
  tpPendingAuth = { accessToken, clientToken, serverUrl };
  const container = document.getElementById('tp-profile-list');
  container.innerHTML = profiles.map(p => {
    const pUuid = (p.id || '').replace(/-/g, '');
    const pServerParam = serverUrl ? `&serverUrl=${encodeURIComponent(serverUrl)}` : '';
    const pUsernameParam = p.name ? `&username=${encodeURIComponent(p.name)}` : '';
    const pSkinUrl = `/api/avatar?uuid=${pUuid}${pServerParam}${pUsernameParam}`;
    return `
    <div class="profile-select-item" onclick="selectThirdPartyProfile('${escapeOnclick(p.id)}', '${escapeOnclick(p.name)}')">
      <img src="${escapeHtml(pSkinUrl)}" alt="" class="profile-select-avatar" onerror="this.style.display='none'; this.nextElementSibling.style.display='flex';">
      <div class="profile-select-avatar-fallback" style="display:none;width:40px;height:40px;background:var(--bg-tertiary);border-radius:6px;align-items:center;justify-content:center;font-size:18px;color:var(--text-secondary);">${p.name.charAt(0).toUpperCase()}</div>
      <div class="profile-select-info">
        <div class="profile-select-name">${escapeHtml(p.name)}</div>
        <div class="profile-select-uuid">${p.id}</div>
      </div>
      <button class="btn btn-primary btn-sm">选择</button>
    </div>
  `;
  }).join('');
  container.querySelectorAll('.profile-select-avatar').forEach(img => {
    img.onload = function() {
      const w = this.naturalWidth || this.width;
      const h = this.naturalHeight || this.height;
      const isFullSkin = (w === 64 && h === 32) || w === 128 || w === 256;
      if (isFullSkin) {
        const cropped = cropSkinHeadCanvas(this, 64);
        if (cropped) {
          this.onload = null;
          this.src = cropped;
        }
      }
    };
  });
  showModal('tp-profile-select-modal');
}

function closeProfileSelectModal() {
  hideModal('tp-profile-select-modal');
  tpPendingAuth = null;
}

async function selectThirdPartyProfile(profileId, profileName) {
  if (!tpPendingAuth) return;
  showToast('正在选择角色...', 'info');
  try {
    const result = await API.selectThirdPartyProfile(
      tpPendingAuth.accessToken,
      tpPendingAuth.clientToken,
      tpPendingAuth.serverUrl,
      profileId,
      profileName
    );
    if (result.success) {
      showToast(`欢迎，${result.account.username}！`, 'success');
      closeProfileSelectModal();
      await loadAccounts();
    } else {
      showToast(result.error || '角色选择失败', 'error');
    }
  } catch (e) {
    showToast('角色选择失败', 'error');
  }
}

