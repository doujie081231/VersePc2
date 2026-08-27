/**
 * js/app/private-server.js - 私人服务器页面业务逻辑
 *
 * 功能：
 *   1. 加载/保存服务器列表
 *   2. 渲染服务器卡片
 *   3. 添加/编辑/删除服务器
 *   4. 一键复制地址
 *   5. 在线状态检测
 */

let privateServerList = [];
let privateServerCheckCache = {}; // id -> { online, error, checkedAt }
let _privateServerModal = null;

// 页面状态：搜索、分类、翻页
let psSearchKeyword = '';
let psActiveCategory = 'all'; // all | online | offline | modpack
let psCurrentPage = 1;
const PS_PAGE_SIZE = 10;

// 顶级视图：my-servers（我的服务器）| lobby（联机大厅）
let psActiveView = 'my-servers';

// 联机大厅状态
let lobbyRooms = [];                 // 当前展示的公开房间列表
let lobbyLoading = false;            // 是否正在加载
let lobbyOffset = 0;                 // 分页起点（对应接口 offset）
const LOBBY_PAGE_SIZE = 10;          // 每页 10 条（接口固定返回 10 条）
let lobbyHasMore = false;            // 是否还有更多
let lobbyServerList = [];            // 红石联机服务器节点列表
let lobbyServerIdx = 0;              // 当前选中的节点索引
let lobbyServerAddress = '';         // 当前节点的 address
let lobbyLoaded = false;             // 是否已首次加载过
let lobbyError = '';                 // 最近一次错误信息

// 分类定义
const PS_CATEGORIES = [
  { id: 'all', label: '全部' },
  { id: 'online', label: '在线' },
  { id: 'offline', label: '离线' },
  { id: 'modpack', label: '有整合包' },
];

async function loadPrivateServers() {
  try {
    const result = await window.electronAPI.privateServer.list();
    if (result && result.ok && Array.isArray(result.servers)) {
      privateServerList = result.servers;
    } else {
      privateServerList = [];
    }
  } catch (e) {
    console.error('[PrivateServer] load failed:', e);
    privateServerList = [];
  }
}

async function savePrivateServers() {
  try {
    const result = await window.electronAPI.privateServer.save(privateServerList);
    return result && result.ok;
  } catch (e) {
    console.error('[PrivateServer] save failed:', e);
    return false;
  }
}

function getPrivateServerById(id) {
  return privateServerList.find(s => s.id === id);
}

async function deletePrivateServer(id) {
  if (!confirm('确定要删除这个服务器卡片吗？')) return;
  const idx = privateServerList.findIndex(s => s.id === id);
  if (idx === -1) return;
  privateServerList.splice(idx, 1);
  if (await savePrivateServers()) {
    renderPrivateServerPage();
    showToast('已删除', 'success');
  } else {
    showToast('删除失败', 'error');
  }
}

async function copyPrivateServerAddress(address) {
  try {
    const result = await window.electronAPI.privateServer.copyAddress(address);
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

/** 图标加载失败时，通过后端把本地路径或网络 URL 转成 data URL 显示 */
async function loadPrivateServerIcon(img, icon) {
  img.onerror = null; // 防止加载失败后无限循环
  if (!icon) { img.src = 'img/icon.svg'; return; }
  try {
    const r = await window.electronAPI.privateServer.getIcon(icon);
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
  privateServerCheckCache[id] = { online: null, error: null, checkedAt: Date.now() };
  renderPrivateServerPage();
  try {
    const result = await window.electronAPI.privateServer.check(server.address);
    privateServerCheckCache[id] = {
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
    privateServerCheckCache[id] = { online: false, error: e.message, checkedAt: Date.now() };
  }
  renderPrivateServerPage();
}

function openPrivateServerModal(id) {
  const isEdit = !!id;
  const server = isEdit ? getPrivateServerById(id) : null;

  const modal = document.createElement('div');
  modal.className = 'modal-overlay';
  modal.id = 'private-server-modal';
  modal.innerHTML = `
    <div class="modal" style="max-width:480px">
      <div class="modal-header">
        <h3>${isEdit ? '编辑服务器' : '添加服务器'}</h3>
        <button class="modal-close" onclick="closePrivateServerModal()">&times;</button>
      </div>
      <div class="modal-body">
        <div class="form-group">
          <label>服务器名称 <span style="color:#ff4d4f">*</span></label>
          <input type="text" id="ps-modal-name" class="text-input" placeholder="例如：豆杰的小服" value="${escapeHtml(server?.name || '')}">
        </div>
        <div class="form-group">
          <label>服务器地址 <span style="color:#ff4d4f">*</span></label>
          <input type="text" id="ps-modal-address" class="text-input" placeholder="例如：mc.example.com:25565" value="${escapeHtml(server?.address || '')}">
          <span class="form-hint">支持域名或 IP，不带端口默认 25565</span>
        </div>
        <div class="form-group">
          <label>简介</label>
          <input type="text" id="ps-modal-desc" class="text-input" placeholder="一句话介绍服务器" value="${escapeHtml(server?.description || '')}">
        </div>
        <div class="form-group">
          <label>图标</label>
          <input type="text" id="ps-modal-icon" class="text-input" placeholder="本地图片路径或网络 URL（留空使用默认图标）" value="${escapeHtml(server?.icon || '')}">
          <span class="form-hint">支持本地文件路径或 http/https 图片链接</span>
        </div>
        <div class="form-group">
          <label>整合包下载地址</label>
          <input type="text" id="ps-modal-modpack" class="text-input" placeholder="可选：填写后卡片显示“下载整合包”按钮" value="${escapeHtml(server?.modpackUrl || '')}">
        </div>
      </div>
      <div class="modal-footer">
        <button class="btn btn-secondary" onclick="closePrivateServerModal()">取消</button>
        <button class="btn btn-primary" id="ps-modal-confirm">保存</button>
      </div>
    </div>
  `;
  document.body.appendChild(modal);
  _privateServerModal = modal;

  // 点击遮罩关闭
  modal.addEventListener('click', (e) => {
    if (e.target === modal) closePrivateServerModal();
  });

  // 确认保存
  const confirmBtn = document.getElementById('ps-modal-confirm');
  confirmBtn.onclick = async () => {
    const name = document.getElementById('ps-modal-name').value.trim();
    const address = document.getElementById('ps-modal-address').value.trim();
    const description = document.getElementById('ps-modal-desc').value.trim();
    const icon = document.getElementById('ps-modal-icon').value.trim();
    const modpackUrl = document.getElementById('ps-modal-modpack').value.trim();

    if (!name || !address) {
      showToast('名称和地址不能为空', 'error');
      return;
    }

    let ok = false;
    if (isEdit) {
      const idx = privateServerList.findIndex(s => s.id === id);
      if (idx !== -1) {
        privateServerList[idx] = {
          ...privateServerList[idx],
          name, address, description, icon, modpackUrl,
        };
        ok = await savePrivateServers();
      }
    } else {
      try {
        const result = await window.electronAPI.privateServer.add({
          name, address, description, icon, modpackUrl,
        });
        if (result && result.ok && result.server) {
          privateServerList.push(result.server);
          ok = true;
        }
      } catch (e) {
        console.error('[PrivateServer] add failed:', e);
      }
    }

    if (ok) {
      closePrivateServerModal();
      renderPrivateServerPage();
      showToast(isEdit ? '已保存' : '已添加', 'success');
    } else {
      showToast('保存失败', 'error');
    }
  };
}

function closePrivateServerModal() {
  if (_privateServerModal) {
    _privateServerModal.remove();
    _privateServerModal = null;
  }
}

function getPrivateServerStatusHtml(server) {
  const cached = privateServerCheckCache[server.id];
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
      <a href="javascript:void(0)" onclick="window.electronAPI?.openExternal('https://afdian.com/a/versejava')">点击这里</a>
    </div>
  `;
}

function renderPrivateServerPage() {
  const container = document.getElementById('private-server-container');
  if (!container) return;

  // 顶级 tab：我的服务器 / 联机大厅
  const topTabsHtml = `
    <div class="ps-top-tabs">
      <button class="ps-top-tab${psActiveView === 'my-servers' ? ' active' : ''}" data-view="my-servers">私人服务器</button>
      <button class="ps-top-tab${psActiveView === 'lobby' ? ' active' : ''}" data-view="lobby">红石联机大厅</button>
    </div>
  `;

  let bodyHtml = '';
  if (psActiveView === 'lobby') {
    bodyHtml = renderLobbyInner();
  } else {
    bodyHtml = renderMyServersInner();
  }

  container.innerHTML = topTabsHtml + bodyHtml;

  // 绑定顶级 tab 切换
  container.querySelectorAll('.ps-top-tab').forEach(btn => {
    btn.addEventListener('click', () => {
      const v = btn.dataset.view;
      if (v === psActiveView) return;
      psActiveView = v;
      renderPrivateServerPage();
      if (psActiveView === 'lobby' && !lobbyLoaded) {
        loadLobbyRooms();
      }
    });
  });

  // 绑定子视图事件
  if (psActiveView === 'lobby') {
    bindLobbyEvents(container);
  } else {
    bindPrivateServerToolbarEvents(container);
  }
}

/** "我的服务器"视图内容（不含顶级 tab） */
function renderMyServersInner() {
  if (privateServerList.length === 0) {
    return `
      <div class="ps-empty">
        <p>还没有私人服务器</p>
      </div>
      ${getJoinBannerHtml()}
    `;
  }

  // 过滤
  const filtered = privateServerList.filter(server => {
    // 分类
    if (psActiveCategory === 'online') {
      const cached = privateServerCheckCache[server.id];
      if (!cached || !cached.online) return false;
    } else if (psActiveCategory === 'offline') {
      const cached = privateServerCheckCache[server.id];
      if (!cached || cached.online) return false;
    } else if (psActiveCategory === 'modpack') {
      if (!server.modpackUrl) return false;
    }
    // 搜索
    if (psSearchKeyword) {
      const kw = psSearchKeyword.toLowerCase();
      const hay = `${server.name || ''} ${server.address || ''} ${server.description || ''}`.toLowerCase();
      if (!hay.includes(kw)) return false;
    }
    return true;
  });

  // 翻页
  const totalPages = Math.max(1, Math.ceil(filtered.length / PS_PAGE_SIZE));
  if (psCurrentPage > totalPages) psCurrentPage = totalPages;
  const start = (psCurrentPage - 1) * PS_PAGE_SIZE;
  const pageItems = filtered.slice(start, start + PS_PAGE_SIZE);

  // 工具栏：搜索 + 分类
  const categoriesHtml = PS_CATEGORIES.map(cat => {
    const isActive = cat.id === psActiveCategory;
    return `<button class="ps-tab${isActive ? ' active' : ''}" data-cat="${cat.id}">${cat.label}</button>`;
  }).join('');

  // 列表
  const cardsHtml = pageItems.map(server => {
    const iconSrc = server.icon ? escapeHtml(server.icon) : 'img/icon.svg';
    const iconEsc = String(server.icon || '').replace(/\\/g, '\\\\').replace(/'/g, "\\'");
    const iconHtml = `<img src="${iconSrc}" alt="" class="ps-card-icon" onerror="loadPrivateServerIcon(this,'${iconEsc}')">`;
    const modpackBtn = server.modpackUrl
      ? `<button class="btn btn-ghost btn-sm" onclick="window.open('${escapeHtml(server.modpackUrl)}', '_blank')">下载整合包</button>`
      : '';
    const descParts = [server.description, server.maxPlayers ? `人数限制：${server.maxPlayers}` : ''].filter(Boolean);
    const descHtml = descParts.length ? `<div class="ps-card-desc">${escapeHtml(descParts.join(' ｜ '))}</div>` : '';
    const cached = privateServerCheckCache[server.id];
    const versionHtml = (cached && cached.version) ? `<div class="ps-card-version">版本 ${escapeHtml(cached.version)}</div>` : '';
    return `
      <div class="ps-card" data-id="${escapeHtml(server.id)}">
        ${iconHtml}
        <div class="ps-card-info">
          <div class="ps-card-title">${escapeHtml(server.name)}</div>
          <div class="ps-card-address" onclick="copyPrivateServerAddress('${escapeHtml(server.address)}')" title="点击复制">
            ${escapeHtml(server.address)}
          </div>
          ${descHtml}
          ${versionHtml}
        </div>
        <div class="ps-card-status" onclick="checkPrivateServerStatus('${escapeHtml(server.id)}');event.stopPropagation();" title="点击检测">
          ${getPrivateServerStatusHtml(server)}
        </div>
        <div class="ps-card-actions">
          <button class="btn btn-primary btn-sm" onclick="copyPrivateServerAddress('${escapeHtml(server.address)}')">复制地址</button>
          ${modpackBtn}
        </div>
      </div>
    `;
  }).join('');

  // 翻页器
  const paginationHtml = totalPages > 1 ? `
    <div class="ps-pagination">
      <button class="ps-page-btn" data-page="prev" ${psCurrentPage <= 1 ? 'disabled' : ''}>上一页</button>
      <span class="ps-page-info">${psCurrentPage} / ${totalPages}</span>
      <button class="ps-page-btn" data-page="next" ${psCurrentPage >= totalPages ? 'disabled' : ''}>下一页</button>
    </div>
  ` : '';

  const emptyHint = filtered.length === 0 ? `<div class="ps-empty"><p>没有符合条件的服务器</p></div>` : '';

  return `
    <div class="ps-toolbar">
      <div class="ps-tabs">${categoriesHtml}</div>
      <div class="ps-toolbar-right">
        <div class="ps-search">
          <svg class="ps-search-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>
          <input type="text" id="ps-search-input" class="ps-search-input" placeholder="搜索服务器名称、地址或简介" value="${escapeHtml(psSearchKeyword)}">
        </div>
        <button class="ps-refresh-btn" onclick="refreshPrivateServerList()" title="刷新列表">
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
        psSearchKeyword = e.target.value.trim();
        psCurrentPage = 1;
        renderPrivateServerPage();
      }, 200);
    });
  }

  // 分类标签
  container.querySelectorAll('.ps-tab').forEach(btn => {
    btn.addEventListener('click', () => {
      psActiveCategory = btn.dataset.cat;
      psCurrentPage = 1;
      renderPrivateServerPage();
    });
  });

  // 翻页
  container.querySelectorAll('.ps-page-btn').forEach(btn => {
    btn.addEventListener('click', () => {
      if (btn.disabled) return;
      if (btn.dataset.page === 'prev') {
        psCurrentPage = Math.max(1, psCurrentPage - 1);
      } else if (btn.dataset.page === 'next') {
        psCurrentPage++;
      }
      renderPrivateServerPage();
    });
  });
}

async function initPrivateServerPage() {
  await loadPrivateServers();
  renderPrivateServerPage();
  // 自动检测所有服务器在线状态
  for (const server of privateServerList) {
    checkPrivateServerStatus(server.id);
  }
}

// 刷新服务器列表
async function refreshPrivateServerList() {
  privateServerCheckCache = {};
  await initPrivateServerPage();
}

// ===================================================================
// 联机大厅（公开房间列表）
// ===================================================================

/** 确保服务器节点列表已加载，并设置当前选中节点 */
async function ensureLobbyServers() {
  if (lobbyServerList.length === 0) {
    try {
      const result = await window.electronAPI.redstoneOnline.getServers();
      if (result && result.ok && Array.isArray(result.servers) && result.servers.length > 0) {
        lobbyServerList = result.servers;
      } else {
        lobbyServerList = [];
      }
    } catch (e) {
      console.error('[Lobby] load servers failed:', e);
      lobbyServerList = [];
    }
  }
  if (lobbyServerIdx >= lobbyServerList.length) lobbyServerIdx = 0;
  if (lobbyServerList.length > 0) {
    lobbyServerAddress = lobbyServerList[lobbyServerIdx].address;
  } else {
    lobbyServerAddress = '';
  }
}

/** 循环切换服务器节点 */
async function lobbyCycleServer() {
  if (lobbyServerList.length === 0) {
    await ensureLobbyServers();
  }
  if (lobbyServerList.length === 0) {
    showToast('未获取到服务器节点', 'error');
    return;
  }
  lobbyServerIdx = (lobbyServerIdx + 1) % lobbyServerList.length;
  lobbyServerAddress = lobbyServerList[lobbyServerIdx].address;
  lobbyOffset = 0;
  await loadLobbyRooms();
}

/** 拉取公开房间列表 */
async function loadLobbyRooms(resetOffset = false) {
  if (resetOffset) lobbyOffset = 0;
  await ensureLobbyServers();
  if (!lobbyServerAddress) {
    lobbyError = '未获取到服务器节点，无法加载公开房间';
    lobbyRooms = [];
    lobbyLoaded = true;
    renderPrivateServerPage();
    return;
  }
  lobbyLoading = true;
  lobbyError = '';
  renderPrivateServerPage();
  try {
    const result = await window.electronAPI.redstoneOnline.listPublicTunnels({
      serverAddress: lobbyServerAddress,
      offset: lobbyOffset,
    });
    if (result && result.ok) {
      lobbyRooms = Array.isArray(result.tunnels) ? result.tunnels : [];
      // 接口固定返回最多 10 条；等于页大小则可能还有更多
      lobbyHasMore = lobbyRooms.length >= LOBBY_PAGE_SIZE;
      lobbyError = '';
    } else {
      lobbyRooms = [];
      lobbyHasMore = false;
      lobbyError = (result && result.error) || '加载公开房间失败';
    }
  } catch (e) {
    console.error('[Lobby] load rooms failed:', e);
    lobbyRooms = [];
    lobbyHasMore = false;
    lobbyError = e.message || '加载公开房间失败';
  } finally {
    lobbyLoading = false;
    lobbyLoaded = true;
    renderPrivateServerPage();
  }
}

/** 联机大厅视图内容（不含顶级 tab） */
function renderLobbyInner() {
  // 节点信息
  const serverLabel = lobbyServerList[lobbyServerIdx]
    ? `${lobbyServerList[lobbyServerIdx].name || lobbyServerList[lobbyServerIdx].address}`
    : '未选择节点';

  // 工具栏：节点切换 + 刷新
  const toolbarHtml = `
    <div class="ps-toolbar">
      <div class="ps-tabs">
        <button class="ps-tab active" id="lobby-server-btn" title="点击切换节点">
          节点：${escapeHtml(serverLabel)}
        </button>
      </div>
      <div class="ps-toolbar-right">
        <button class="ps-refresh-btn" id="lobby-refresh-btn" title="刷新列表">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="23 4 23 10 17 10"/><path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10"/></svg>
        </button>
      </div>
    </div>
  `;

  // 加载中
  if (lobbyLoading) {
    return toolbarHtml + `
      <div class="ps-empty"><p>正在加载公开房间...</p></div>
      ${getLobbyTipHtml()}
    `;
  }

  // 错误
  if (lobbyError) {
    return toolbarHtml + `
      <div class="ps-empty">
        <p>加载失败：${escapeHtml(lobbyError)}</p>
        <button class="btn btn-secondary btn-sm" style="margin-top:8px" onclick="loadLobbyRooms(true)">重试</button>
      </div>
      ${getLobbyTipHtml()}
    `;
  }

  // 未加载或空
  if (!lobbyLoaded) {
    return toolbarHtml + `
      <div class="ps-empty"><p>点击下方刷新按钮加载公开房间</p></div>
      ${getLobbyTipHtml()}
    `;
  }

  if (lobbyRooms.length === 0) {
    return toolbarHtml + `
      <div class="ps-empty"><p>当前节点暂无公开房间</p></div>
      ${getLobbyTipHtml()}
    `;
  }

  // 房间卡片
  const cardsHtml = lobbyRooms.map(room => {
    const title = room.title ? escapeHtml(room.title) : '未命名房间';
    const desc = room.description ? escapeHtml(room.description) : '';
    const port = parseInt(room.port, 10) || 0;
    const fullAddr = port ? `${lobbyServerAddress}:${port}` : '';
    const onlineBadge = room.online
      ? `<span class="lobby-badge online" title="仅允许正版账户加入">仅正版</span>`
      : `<span class="lobby-badge offline" title="允许离线账户加入">允许离线</span>`;
    // 版本信息：description 即为房主上传的版本描述（如 "1.20.1 / Fabric 0.15.7" 或 "1.21 / 原版"）
    const descHtml = desc ? `<div class="ps-card-desc">${desc}</div>` : '';
    // 模组加载器标签：从 description 判断是否原版
    let loaderBadge = '';
    if (desc) {
      if (/原版/.test(desc)) {
        loaderBadge = `<span class="lobby-badge offline" title="原版房间，无模组加载器">原版</span>`;
      } else {
        loaderBadge = `<span class="lobby-badge online" title="该版本有模组加载器">有模组加载器</span>`;
      }
    }
    const addrHtml = fullAddr
      ? `<div class="ps-card-address" data-addr="${escapeHtml(fullAddr)}" title="点击复制">${escapeHtml(fullAddr)}</div>`
      : `<div class="ps-card-address" style="color:var(--text-muted)">端口信息缺失</div>`;
    const copyBtn = fullAddr
      ? `<button class="btn btn-primary btn-sm" data-copy="${escapeHtml(fullAddr)}">复制地址</button>`
      : '';
    return `
      <div class="ps-card lobby-card">
        <div class="ps-card-info">
          <div class="ps-card-title">${title} ${onlineBadge} ${loaderBadge}</div>
          ${addrHtml}
          ${descHtml}
        </div>
        <div class="ps-card-actions">
          ${copyBtn}
        </div>
      </div>
    `;
  }).join('');

  // 翻页
  const currentPage = Math.floor(lobbyOffset / LOBBY_PAGE_SIZE) + 1;
  const paginationHtml = `
    <div class="ps-pagination">
      <button class="ps-page-btn" data-page="prev" ${lobbyOffset <= 0 ? 'disabled' : ''}>上一页</button>
      <span class="ps-page-info">第 ${currentPage} 页</span>
      <button class="ps-page-btn" data-page="next" ${!lobbyHasMore ? 'disabled' : ''}>下一页</button>
    </div>
  `;

  return toolbarHtml + `
    <div class="ps-list">${cardsHtml}</div>
    ${paginationHtml}
    ${getLobbyTipHtml()}
  `;
}

/** 联机大厅底部说明 */
function getLobbyTipHtml() {
  return `
    <div class="ps-join-banner">
      <span>联机大厅展示当前节点下所有公开房间，复制地址后在游戏内"多人游戏 → 直接连接"粘贴即可加入</span>
    </div>
  `;
}

/** 绑定联机大厅事件 */
function bindLobbyEvents(container) {
  // 节点切换
  const serverBtn = container.querySelector('#lobby-server-btn');
  if (serverBtn) {
    serverBtn.addEventListener('click', lobbyCycleServer);
  }
  // 刷新
  const refreshBtn = container.querySelector('#lobby-refresh-btn');
  if (refreshBtn) {
    refreshBtn.addEventListener('click', () => loadLobbyRooms(true));
  }
  // 翻页
  container.querySelectorAll('.ps-page-btn').forEach(btn => {
    btn.addEventListener('click', () => {
      if (btn.disabled) return;
      if (btn.dataset.page === 'prev') {
        lobbyOffset = Math.max(0, lobbyOffset - LOBBY_PAGE_SIZE);
      } else if (btn.dataset.page === 'next') {
        if (!lobbyHasMore) return;
        lobbyOffset += LOBBY_PAGE_SIZE;
      }
      loadLobbyRooms();
    });
  });
  // 复制地址（卡片地址文本点击）
  container.querySelectorAll('.lobby-card .ps-card-address[data-addr]').forEach(el => {
    el.addEventListener('click', () => copyLobbyAddress(el.dataset.addr));
  });
  // 复制地址按钮
  container.querySelectorAll('.lobby-card [data-copy]').forEach(btn => {
    btn.addEventListener('click', () => copyLobbyAddress(btn.dataset.copy));
  });
}

/** 复制联机大厅房间地址 */
async function copyLobbyAddress(address) {
  if (!address) return;
  try {
    const result = await window.electronAPI.privateServer.copyAddress(address);
    if (result && result.ok) {
      showToast('联机地址已复制，去游戏内直接连接粘贴即可', 'success');
    } else {
      showToast('复制失败', 'error');
    }
  } catch (e) {
    console.error('[Lobby] copy failed:', e);
    showToast('复制失败', 'error');
  }
}

/** 查看房间模组列表弹窗 */
async function showLobbyModsModal(serverAddress, tunnelId, title) {
  // 先弹出加载中弹窗
  const modal = document.createElement('div');
  modal.className = 'modal-overlay';
  modal.id = 'lobby-mods-modal';
  modal.innerHTML = `
    <div class="modal" style="max-width:420px">
      <div class="modal-header">
        <h3>房间模组 - ${escapeHtml(title)}</h3>
        <button class="modal-close" onclick="closeLobbyModsModal()">&times;</button>
      </div>
      <div class="modal-body">
        <div id="lobby-mods-content" style="text-align:center;padding:20px;color:var(--text-muted)">正在加载模组列表...</div>
      </div>
      <div class="modal-footer">
        <button class="btn btn-secondary" onclick="closeLobbyModsModal()">关闭</button>
      </div>
    </div>
  `;
  document.body.appendChild(modal);
  modal.addEventListener('click', (e) => { if (e.target === modal) closeLobbyModsModal(); });

  const content = document.getElementById('lobby-mods-content');
  try {
    const result = await window.electronAPI.redstoneOnline.getTunnelMods({
      serverAddress,
      tunnelId,
    });
    if (result && result.ok && Array.isArray(result.mods)) {
      if (result.mods.length === 0) {
        content.innerHTML = `<div style="text-align:center;padding:20px;color:var(--text-muted)">该房间没有模组，是原版房间</div>`;
      } else {
        const list = result.mods.map(m => `<li>${escapeHtml(m)}</li>`).join('');
        content.innerHTML = `
          <div style="font-size:13px;color:var(--text-muted);margin-bottom:8px">共 ${result.mods.length} 个模组</div>
          <ul style="margin:0;padding-left:20px;font-size:14px;line-height:1.9;max-height:300px;overflow-y:auto">${list}</ul>
        `;
      }
    } else {
      content.innerHTML = `<div style="text-align:center;padding:20px;color:var(--text-muted)">加载失败：${escapeHtml((result && result.error) || '未知错误')}</div>`;
    }
  } catch (e) {
    content.innerHTML = `<div style="text-align:center;padding:20px;color:var(--text-muted)">加载失败：${escapeHtml(e.message)}</div>`;
  }
}

function closeLobbyModsModal() {
  const m = document.getElementById('lobby-mods-modal');
  if (m) m.remove();
}
