import { bridge as bridge } from '../../core/tauri-bridge.js';
import { escapeHtml as escapeHtml } from '../../core/utils.js';
import { showToast as showToast } from './ui-components.js';
import { actionAttrs as actionAttrs, actionAttrsStop as actionAttrsStop } from '../../core/actions.js';
import { S } from './private-server-state.js';
import { loadLobbyRooms, renderLobbyInner, bindLobbyEvents } from './private-server-lobby.js';
import { registerLazyLoader } from '../../core/lazy-loaders.js';

async function loadPrivateServers() {
  try {
    const result = await bridge.privateServer.list();
    if (result && result.ok && Array.isArray(result.servers)) {
      S.privateServerList = result.servers;
    } else {
      S.privateServerList = [];
    }
  } catch (e) {
    console.error('[PrivateServer] load failed:', e);
    S.privateServerList = [];
  }
}

async function savePrivateServers() {
  try {
    const result = await bridge.privateServer.save(S.privateServerList);
    return result && result.ok;
  } catch (e) {
    console.error('[PrivateServer] save failed:', e);
    return false;
  }
}

function getPrivateServerById(id) {
  return S.privateServerList.find(s => s.id === id);
}

async function deletePrivateServer(id) {
  if (!confirm('确定要删除这个服务器卡片吗？')) return;
  const idx = S.privateServerList.findIndex(s => s.id === id);
  if (idx === -1) return;
  S.privateServerList.splice(idx, 1);
  if (await savePrivateServers()) {
    renderPrivateServerPage();
    showToast('已删除', 'success');
  } else {
    showToast('删除失败', 'error');
  }
}

async function copyPrivateServerAddress(address) {
  try {
    const result = await bridge.privateServer.copyAddress(address);
    if (result && result.ok) {
      showToast('地址已复制到剪贴板', 'success');
    } else {
      showToast('复制失败', 'error');
    }
  } catch (e) {
    console.error('[PrivateServer] copy failed:', e);
    showToast('复制失败', 'error');
  }
}

async function loadPrivateServerIcon(img, icon) {
  img.onerror = null; // 防止加载失败后无限循环
  if (!icon) { img.src = 'img/icon.svg'; return; }
  try {
    const r = await bridge.privateServer.getIcon(icon);
    if (r && r.ok && r.dataUrl) {
      img.src = r.dataUrl;
      return;
    }
  } catch (e) {
    console.error('[PrivateServer] load icon failed:', e);
  }
  img.src = 'img/icon.svg';
}

async function checkPrivateServerStatus(id) {
  const server = getPrivateServerById(id);
  if (!server) return;
  S.privateServerCheckCache[id] = { online: null, error: null, checkedAt: Date.now() };
  renderPrivateServerPage();
  try {
    const result = await bridge.privateServer.check(server.address);
    S.privateServerCheckCache[id] = {
      online: result && result.online,
      error: result && result.error,
      latency: result ? result.latency : null,
      version: result ? result.version : '',
      motd: result ? result.motd : '',
      playersOnline: result ? result.playersOnline : 0,
      playersMax: result ? result.playersMax : 0,
      checkedAt: Date.now(),
    };
  } catch (e) {
    S.privateServerCheckCache[id] = { online: false, error: e.message, checkedAt: Date.now() };
  }
  renderPrivateServerPage();
}

function getPrivateServerStatusHtml(server) {
  const cached = S.privateServerCheckCache[server.id];
  if (!cached || cached.online === null) {
    return `<span class="ps-status-dot"></span><span class="ps-status-text">检测中</span>`;
  }
  if (cached.online) {
    const players = (cached.playersOnline != null && cached.playersMax != null)
      ? `${cached.playersOnline}/${cached.playersMax}` : '';
    const latency = cached.latency ? `${cached.latency}ms` : '';
    const parts = ['在线', players, latency].filter(Boolean);
    return `<span class="ps-status-dot online"></span><span class="ps-status-text online">${escapeHtml(parts.join(' · '))}</span>`;
  }
  return `<span class="ps-status-dot offline"></span><span class="ps-status-text offline">离线</span>`;
}

function getJoinBannerHtml() {
  return `
    <div class="ps-join-banner">
      <span>如果你想入驻 Verse，</span>
      <a href="javascript:void(0)" data-action="openExternal" data-args="[&quot;https://afdian.com/a/versejava&quot;]">点击这里</a>
    </div>
  `;
}

function renderPrivateServerPage() {
  const container = document.getElementById('private-server-container');
  if (!container) return;

  // 顶级 tab：我的服务器 / 联机大厅
  const topTabsHtml = `
    <div class="ps-top-tabs">
      <button class="ps-top-tab${S.psActiveView === 'my-servers' ? ' active' : ''}" data-view="my-servers">私人服务器</button>
      <button class="ps-top-tab${S.psActiveView === 'lobby' ? ' active' : ''}" data-view="lobby">红石联机大厅</button>
    </div>
  `;

  let bodyHtml = '';
  if (S.psActiveView === 'lobby') {
    bodyHtml = renderLobbyInner();
  } else {
    bodyHtml = renderMyServersInner();
  }

  container.innerHTML = topTabsHtml + bodyHtml;

  // 图片失败兜底：CSP 禁止内联 onerror，渲染后统一绑定
  container.querySelectorAll('.ps-card-icon[data-img-icon]').forEach((img) => {
    const icon = img.getAttribute('data-img-icon') || '';
    img.onerror = () => loadPrivateServerIcon(img, icon);
  });

  // 绑定顶级 tab 切换
  container.querySelectorAll('.ps-top-tab').forEach(btn => {
    btn.addEventListener('click', () => {
      const v = btn.dataset.view;
      if (v === S.psActiveView) return;
      S.psActiveView = v;
      renderPrivateServerPage();
      if (S.psActiveView === 'lobby' && !S.lobbyLoaded) {
        loadLobbyRooms();
      }
    });
  });

  // 绑定子视图事件
  if (S.psActiveView === 'lobby') {
    bindLobbyEvents(container);
  } else {
    bindPrivateServerToolbarEvents(container);
  }
}

function renderMyServersInner() {
  if (S.privateServerList.length === 0) {
    return `
      <div class="ps-empty">
        <p>还没有私人服务器</p>
      </div>
      ${getJoinBannerHtml()}
    `;
  }

  // 过滤
  const filtered = S.privateServerList.filter(server => {
    // 分类
    if (S.psActiveCategory === 'online') {
      const cached = S.privateServerCheckCache[server.id];
      if (!cached || !cached.online) return false;
    } else if (S.psActiveCategory === 'offline') {
      const cached = S.privateServerCheckCache[server.id];
      if (!cached || cached.online) return false;
    } else if (S.psActiveCategory === 'modpack') {
      if (!server.modpackUrl) return false;
    }
    // 搜索
    if (S.psSearchKeyword) {
      const kw = S.psSearchKeyword.toLowerCase();
      const hay = `${server.name || ''} ${server.address || ''} ${server.description || ''}`.toLowerCase();
      if (!hay.includes(kw)) return false;
    }
    return true;
  });

  // 翻页
  const totalPages = Math.max(1, Math.ceil(filtered.length / S.PS_PAGE_SIZE));
  if (S.psCurrentPage > totalPages) S.psCurrentPage = totalPages;
  const start = (S.psCurrentPage - 1) * S.PS_PAGE_SIZE;
  const pageItems = filtered.slice(start, start + S.PS_PAGE_SIZE);

  // 工具栏：搜索 + 分类
  const categoriesHtml = S.PS_CATEGORIES.map(cat => {
    const isActive = cat.id === S.psActiveCategory;
    return `<button class="ps-tab${isActive ? ' active' : ''}" data-cat="${cat.id}">${cat.label}</button>`;
  }).join('');

  // 列表
  const cardsHtml = pageItems.map(server => {
    const iconSrc = server.icon ? escapeHtml(server.icon) : 'img/icon.svg';
    const iconHtml = `<img src="${iconSrc}" alt="" class="ps-card-icon" data-img-icon="${escapeHtml(server.icon || '')}">`;
    const modpackBtn = server.modpackUrl
      ? `<button class="btn btn-ghost btn-sm"${actionAttrs('openWindow', server.modpackUrl)}>下载整合包</button>`
      : '';
    const descParts = [server.description, server.maxPlayers ? `人数限制：${server.maxPlayers}` : ''].filter(Boolean);
    const descHtml = descParts.length ? `<div class="ps-card-desc">${escapeHtml(descParts.join(' ｜ '))}</div>` : '';
    const cached = S.privateServerCheckCache[server.id];
    const versionHtml = (cached && cached.version) ? `<div class="ps-card-version">版本 ${escapeHtml(cached.version)}</div>` : '';
    return `
      <div class="ps-card" data-id="${escapeHtml(server.id)}">
        ${iconHtml}
        <div class="ps-card-info">
          <div class="ps-card-title">${escapeHtml(server.name)}</div>
          <div class="ps-card-address"${actionAttrs('copyPrivateServerAddress', server.address)} title="点击复制">
            ${escapeHtml(server.address)}
          </div>
          ${descHtml}
          ${versionHtml}
        </div>
        <div class="ps-card-status"${actionAttrsStop('checkPrivateServerStatus', server.id)} title="点击检测">
          ${getPrivateServerStatusHtml(server)}
        </div>
        <div class="ps-card-actions">
          <button class="btn btn-primary btn-sm"${actionAttrs('copyPrivateServerAddress', server.address)}>复制地址</button>
          ${modpackBtn}
        </div>
      </div>
    `;
  }).join('');

  // 翻页器
  const paginationHtml = totalPages > 1 ? `
    <div class="ps-pagination">
      <button class="ps-page-btn" data-page="prev" ${S.psCurrentPage <= 1 ? 'disabled' : ''}>上一页</button>
      <span class="ps-page-info">${S.psCurrentPage} / ${totalPages}</span>
      <button class="ps-page-btn" data-page="next" ${S.psCurrentPage >= totalPages ? 'disabled' : ''}>下一页</button>
    </div>
  ` : '';

  const emptyHint = filtered.length === 0 ? `<div class="ps-empty"><p>没有符合条件的服务器</p></div>` : '';

  return `
    <div class="ps-toolbar">
      <div class="ps-tabs">${categoriesHtml}</div>
      <div class="ps-toolbar-right">
        <div class="ps-search">
          <svg class="ps-search-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>
          <input type="text" id="ps-search-input" class="ps-search-input" placeholder="搜索服务器名称、地址或简介" value="${escapeHtml(S.psSearchKeyword)}">
        </div>
        <button class="ps-refresh-btn"${actionAttrs('refreshPrivateServerList')} title="刷新列表">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="23 4 23 10 17 10"/><path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10"/></svg>
        </button>
      </div>
    </div>
    <div class="ps-list">
      ${cardsHtml}
      ${emptyHint}
    </div>
    ${paginationHtml}
    ${getJoinBannerHtml()}
  `;
}

function bindPrivateServerToolbarEvents(container) {
  // 搜索框
  const searchInput = container.querySelector('#ps-search-input');
  if (searchInput) {
    let timer = null;
    searchInput.addEventListener('input', (e) => {
      clearTimeout(timer);
      timer = setTimeout(() => {
        S.psSearchKeyword = e.target.value.trim();
        S.psCurrentPage = 1;
        renderPrivateServerPage();
      }, 200);
    });
  }

  // 分类标签
  container.querySelectorAll('.ps-tab').forEach(btn => {
    btn.addEventListener('click', () => {
      S.psActiveCategory = btn.dataset.cat;
      S.psCurrentPage = 1;
      renderPrivateServerPage();
    });
  });

  // 翻页
  container.querySelectorAll('.ps-page-btn').forEach(btn => {
    btn.addEventListener('click', () => {
      if (btn.disabled) return;
      if (btn.dataset.page === 'prev') {
        S.psCurrentPage = Math.max(1, S.psCurrentPage - 1);
      } else if (btn.dataset.page === 'next') {
        S.psCurrentPage++;
      }
      renderPrivateServerPage();
    });
  });
}

async function initPrivateServerPage() {
  await loadPrivateServers();
  renderPrivateServerPage();
  // 自动检测所有服务器在线状态
  for (const server of S.privateServerList) {
    checkPrivateServerStatus(server.id);
  }
}

async function refreshPrivateServerList() {
  S.privateServerCheckCache = {};
  await initPrivateServerPage();
}

export { loadPrivateServers, savePrivateServers, getPrivateServerById, deletePrivateServer, copyPrivateServerAddress, loadPrivateServerIcon, checkPrivateServerStatus, getPrivateServerStatusHtml, getJoinBannerHtml, renderPrivateServerPage, renderMyServersInner, bindPrivateServerToolbarEvents, initPrivateServerPage, refreshPrivateServerList };

// 房间操作完成后由大厅模块触发整页刷新
registerLazyLoader('private-server:render', renderPrivateServerPage);
