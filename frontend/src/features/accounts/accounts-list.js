// accounts-list.js — 账户列表、轮播与切换
import { API as API } from '../../core/api.js';
import { escapeHtml as escapeHtml, _lazyLoadScript as _lazyLoadScript } from '../../core/utils.js';
import { showToast as showToast, showModal as showModal, showConfirmDialog as showConfirmDialog } from '../misc/ui-components.js';
import { AVATAR_CACHE_VERSION as AVATAR_CACHE_VERSION } from '../../core/constants.js';
import { actionAttrs as actionAttrs, actionAttrsStop as actionAttrsStop } from '../../core/actions.js';
import { accountsState as S } from './accounts-state.js';
import { _loadAvatarImg, _loadAvatarImgInto } from './accounts-avatar.js';
import { cropSkinHeadCanvas, setSkinBg } from './accounts-skin-utils.js';
import { loadSkinSelector } from './accounts-skins.js';
import { loadCapeSelector } from './accounts-capes.js';
import { registerLazyLoader } from '../../core/lazy-loaders.js';

async function loadAccounts() {
  try {
    const [accounts, settings] = await Promise.all([
      API.getAccounts(),
      API.getSettings(),
    ]);
    // 首次加载不触发滑入动画，仅切换账户时触发
    S._accountsListInitialized = true;
    const container = document.getElementById('accounts-list');

    // 账户列表 DOM 只有在"账户"页面挂载后才存在（是另一个 Vue 组件），
    // 用户在主页时这里是 null，需要跳过账户列表渲染，只做主页/启动栏的显示更新
    if (container) {
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
        return `<div class="account-item ${isSelected ? 'selected' : ''}"${actionAttrs('showAccountDetail', acc.id)}>
          <div class="account-avatar">${avatarHtml}</div>
          <div class="account-item-info">
            <div class="account-item-name">${escapeHtml(acc.username)}</div>
            <div class="account-item-uuid">${acc.uuid}</div>
            <div class="account-item-type ${typeClass}">${typeLabel}</div>
          </div>
          <div class="mod-actions">
            ${!isSelected ? `<button class="btn btn-primary btn-sm"${actionAttrsStop('selectAccount', acc.id)}>选择</button>` : '<span style="color: var(--accent-ink); font-size: 12px; padding: 4px 10px; display: inline-flex; align-items: center;">当前使用</span>'}
      <button class="btn btn-danger btn-sm"${actionAttrsStop('deleteAccount', acc.id)}>删除</button>
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
    } // end of if (container)

    const selectedAccount = accounts.find(a => a.id === settings.selectedAccount) || accounts[0];

    /* ================= 新版账户页：可拖动账号轮播轨道 ================= */
    const trackEl = document.getElementById('accts-track');
    if (trackEl) {
      renderAccountCarousel(trackEl, accounts, selectedAccount, settings);
    }

    if (selectedAccount) {
      const homeAvatarBox = document.getElementById('home-avatar-box');
      if (homeAvatarBox) homeAvatarBox.classList.remove('home-avatar-add');
      const accUuid = (selectedAccount.uuid || '').replace(/-/g, '');
      
      document.getElementById('home-player-name').textContent = selectedAccount.username;
      const accountTypeText = selectedAccount.type === 'microsoft' ? '微软账户' : selectedAccount.type === 'thirdparty' ? '外置登录' : '离线模式';
      document.getElementById('home-account-type').textContent = accountTypeText;
      try { localStorage.setItem('cachedPlayerName', selectedAccount.username); localStorage.setItem('cachedAccountType', accountTypeText); } catch(e) {}
      
      // 主页使用皮肤头部头像（由皮肤贴图裁出的头部像素方块，带像素纹理）
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
      // 无账户：显示"添加账号"虚线方框 + 十字（点击可进入账户页添加）
      const homeAvatar = document.getElementById('home-avatar');
      if (homeAvatar) {
        homeAvatar.innerHTML =
          '<div class="home-avatar-add">' +
            '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="home-avatar-add-icon">' +
              '<line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/>' +
            '</svg>' +
            '<div class="home-avatar-add-title">添加账号</div>' +
          '</div>';
        const homeAvatarBox = document.getElementById('home-avatar-box');
        if (homeAvatarBox) homeAvatarBox.classList.add('home-avatar-add');
      }
      document.getElementById('home-player-name').textContent = '未登录';
      document.getElementById('home-account-type').textContent = '离线模式';
      const launchAvatar = document.getElementById('launch-avatar');
      launchAvatar.innerHTML = '<img src="img/icon.png" alt="" class="account-avatar-img">';
      document.getElementById('launch-player-name').textContent = 'Player';
    }
  } catch (e) { console.error('[Accounts] Failed to update account display:', e); }
}

function renderAccountCarousel(trackEl, accounts, selectedAccount, _settings) {
  trackEl.innerHTML = '';
  // 第一张：添加账号卡片（虚拟账号 id）
  S._carouselCards = [S.ADD_CARD_ID].concat(accounts.map(a => a.id));
  let selIdx = selectedAccount ? accounts.findIndex(a => a.id === selectedAccount.id) : -1;
  if (selIdx < 0) selIdx = (accounts.length > 0) ? 0 : -1;
  // 若完全没有账号，把当前选中放在"添加账号"那张，方便用户第一眼看到
  S._carouselCurrentIndex = (selIdx >= 0) ? (selIdx + 1) : 0;

  // 第一张：添加账号卡片（虚线方形 + 十字）
  const addCard = document.createElement('div');
  addCard.className = 'accts-card accts-add-card' + (S._carouselCurrentIndex === 0 ? ' is-selected' : '');
  addCard.dataset.accountId = S.ADD_CARD_ID;
  addCard.title = '添加账号';
  addCard.innerHTML = `
    <div class="accts-add-card-inner">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="accts-add-card-icon">
        <line x1="12" y1="5" x2="12" y2="19"/>
        <line x1="5" y1="12" x2="19" y2="12"/>
      </svg>
      <div class="accts-add-card-title">添加账号</div>
      <div class="accts-add-card-sub">微软 / 离线 / 外置登录</div>
    </div>`;
  addCard.addEventListener('click', () => {
    // 点添加卡片 → 直接弹窗
    S._carouselCurrentIndex = 0;
    _carouselResetPosition(true);
    try { showModal('add-account-modal'); } catch(e) {}
  });
  trackEl.appendChild(addCard);

  const typeMap = { microsoft: '微软账户', thirdparty: '外置登录', offline: '离线账户' };
  accounts.forEach((acc, i) => {
    const idx = i + 1;
    const isSel = idx === S._carouselCurrentIndex;
    const card = document.createElement('div');
    card.className = 'accts-card ' + (isSel ? 'is-selected' : '');
    card.dataset.accountId = acc.id;

    const avatar = document.createElement('div');
    avatar.className = 'accts-card-avatar';
    const img = document.createElement('img');
    if (!(acc.uuid || '').replace(/-/g, '')) img.src = 'img/icon.png';
    avatar.appendChild(img);
    card.appendChild(avatar);

    const info = document.createElement('div');
    info.className = 'accts-card-info';
    const name = document.createElement('div');
    name.className = 'accts-card-name';
    name.textContent = acc.username;
    info.appendChild(name);
    if (isSel) {
      const tag = document.createElement('div');
      tag.className = 'accts-card-type';
      tag.textContent = typeMap[acc.type] || '离线账户';
      info.appendChild(tag);
    }
    card.appendChild(info);

    // 点击卡片 → 直接进入账号详情
    card.addEventListener('click', () => {
      showAccountDetail(acc.id);
    });

    trackEl.appendChild(card);
    const accUuid = (acc.uuid || '').replace(/-/g, '');
    _loadAvatarImgInto(img, accUuid, acc.serverUrl, acc.username, acc.type);
  });

  // 定位到当前选中并绑定拖动
  _carouselBindDrag(trackEl);
  // 尺寸就绪后定位到中间（带重试，等待容器/卡片有一定宽度）
  resetCarouselPosition();
}

function _carouselResetPosition(animate) {
  const trackEl = document.getElementById('accts-track');
  if (!trackEl) return;
  const cards = trackEl.querySelectorAll('.accts-card');
  if (!cards.length) return;
  const card = cards[0];
  const cardW = card.offsetWidth || 180;
  const cardMargin = 24;
  const ctx = document.getElementById('accts-carousel');
  const containerW = ctx ? ctx.clientWidth : 500;
  // 轨道无 padding：第 i 张卡片的中心 = translateX + i*(cardW+gap) + cardW/2
  // 令其 = containerW/2 → translateX = containerW/2 - cardW/2 - i*(cardW+gap)
  const target = containerW / 2 - (cardW / 2) - S._carouselCurrentIndex * (cardW + cardMargin);
  trackEl.style.transition = animate ? 'transform 0.45s var(--ease-out-expo, cubic-bezier(0.16,1,0.3,1)), opacity 0.28s ease' : 'none';
  trackEl.style.transform = `translateX(${target}px)`;

  cards.forEach((c, i) => {
    const isSel = i === S._carouselCurrentIndex;
    c.classList.toggle('is-selected', isSel);
  });
}

function _carouselBindDrag(trackEl) {
  const carousel = document.getElementById('accts-carousel');
  if (!carousel) return;
  if (carousel.dataset.dragBound) return;
  carousel.dataset.dragBound = '1';

  trackEl.style.cursor = 'grab';
  let startX = 0, startY = 0, baseX = 0, dragging = false, moved = 0;
  let suppressClick = false;   // 拖动后抑制卡片 click，避免误触发详情/切换
  let ejecting = false;        // 正在执行上滑删除（z 轴飞出）

  carousel.addEventListener('pointerdown', (e) => {
    if (e.button !== 0 && e.pointerType === 'mouse') return;
    dragging = true;
    moved = 0;
    suppressClick = false;
    ejecting = false;
    startX = e.clientX;
    startY = e.clientY;
    const m = trackEl.style.transform.match(/translateX\(([-\d.]+)px\)/);
    baseX = m ? parseFloat(m[1]) : 0;
    trackEl.style.transition = 'none';
    trackEl.style.cursor = 'grabbing';
  });

  // 用 document 跟踪 move/up，避免 setPointerCapture 把 click 重定向到容器，
  // 导致点击卡片无法进入详情
  const onMove = (e) => {
    if (!dragging) return;
    const dx = e.clientX - startX;
    const dy = e.clientY - startY;
    // 上滑删除：在选中卡片上向上划，卡片沿 z 轴飞出
    if (!ejecting && dy < -40 && Math.abs(dy) > Math.abs(dx) * 1.1) {
      ejecting = true;
      suppressClick = true;
      dragging = false;
      trackEl.style.cursor = 'grab';
      triggerEjectDelete();
      return;
    }
    moved = dx;
    if (Math.abs(moved) > 6) suppressClick = true;  // 判定为拖动而非点击
    const cardW = (trackEl.querySelector('.accts-card') || { offsetWidth: 180 }).offsetWidth || 180;
    const step = cardW + 24;
    const containerW = document.getElementById('accts-carousel').clientWidth;
    // 允许的最大/最小 translateX（轨道从 x=0 开始）
    const maxX = containerW / 2 - cardW / 2;                       // 第 0 张居中
    const minX = containerW / 2 - cardW / 2 - (S._carouselCards.length - 1) * step; // 最后一张居中
    let next = baseX + moved;
    // 两端阻尼
    if (next > maxX + step * 0.35) next = maxX + step * 0.35 + (next - maxX - step * 0.35) * 0.25;
    if (next < minX - step * 0.35) next = minX - step * 0.35 + (next - minX + step * 0.35) * 0.25;
    trackEl.style.transform = `translateX(${next}px)`;
  };

  // 上滑删除：选中卡片沿 z 轴翻飞消失，再删除账号
  const triggerEjectDelete = () => {
    const idx = S._carouselCurrentIndex;
    const id = S._carouselCards[idx];
    if (!id || id === S.ADD_CARD_ID) {
      _carouselResetPosition(true);
      return;
    }
    const cards = trackEl.querySelectorAll('.accts-card');
    const card = cards[idx];
    if (card) card.classList.add('accts-card-eject');
    setTimeout(() => {
      if (card) card.classList.remove('accts-card-eject');
      deleteAccount(id);
    }, 420);
  };

  const endDrag = () => {
    if (!dragging) return;
    dragging = false;
    trackEl.style.cursor = 'grab';

    const cardW = (trackEl.querySelector('.accts-card') || { offsetWidth: 180 }).offsetWidth || 180;
    const step = cardW + 24;
    // 当前卡片中心相对容器中心偏移多少格（向右拖 moved>0 → 露出左边前面的卡片 → 向前翻）
    const offsetFromCenter = -moved / step;
    let target = Math.round(S._carouselCurrentIndex + offsetFromCenter);
    if (target < 0) target = 0;
    if (target > S._carouselCards.length - 1) target = S._carouselCards.length - 1;

    if (target !== S._carouselCurrentIndex) {
      S._carouselCurrentIndex = target;
      _carouselResetPosition(true);
      const id = S._carouselCards[target];
      if (id && id !== S.ADD_CARD_ID) {
        selectAccount(id);
      } else if (id === S.ADD_CARD_ID) {
        try { showModal('add-account-modal'); } catch(e) {}
      }
    } else {
      _carouselResetPosition(true);
    }
  };

  document.addEventListener('pointermove', onMove);
  document.addEventListener('pointerup', endDrag);
  document.addEventListener('pointercancel', endDrag);

  // 拖动结束后抑制一次 click（避免和拖动冲突）
  carousel.addEventListener('click', (e) => {
    if (suppressClick) {
      e.preventDefault();
      e.stopPropagation();
      suppressClick = false;
    }
  }, true);
}

function resetCarouselPosition() {
  const trackEl = document.getElementById('accts-track');
  if (!trackEl || !trackEl.querySelector('.accts-card')) return;
  const pending = document.getElementById('accts-carousel');
  if (pending) pending.classList.add('accts-carousel-pending');
  let retries = 0;
  const tryCenter = () => {
    const ctx = document.getElementById('accts-carousel');
    const first = trackEl.querySelector('.accts-card');
    if (ctx && first && ctx.clientWidth > 0 && first.offsetWidth > 0) {
      _carouselResetPosition(false);
      setTimeout(() => { trackEl.style.transition = ''; }, 0);
      if (ctx) ctx.classList.remove('accts-carousel-pending');
    } else if (retries < 60) {
      retries += 1;
      setTimeout(tryCenter, 20);
    } else {
      _carouselResetPosition(false);
      setTimeout(() => { trackEl.style.transition = ''; }, 0);
      if (ctx) ctx.classList.remove('accts-carousel-pending');
    }
  };
  requestAnimationFrame(tryCenter);
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

function showAccountDetail(accountId) {
  API.getAccounts().then(async accounts => {
    const acc = accounts.find(a => a.id === accountId);
    if (!acc) return;
    S._currentDetailAccount = acc;
    const accUuid = (acc.uuid || '').replace(/-/g, '');
    document.getElementById('detail-username').textContent = acc.username;
    document.getElementById('detail-uuid').textContent = acc.uuid || '-';
    const typeMap = { microsoft: '正版', thirdparty: '外置登录', offline: '离线' };
    const badgeLabel = typeMap[acc.type] || '离线';
    document.getElementById('detail-skin-type').textContent = badgeLabel;
    const typeEl = document.getElementById('detail-account-type');
    if (typeEl) typeEl.textContent = badgeLabel;
    document.getElementById('page-account-detail').style.display = '';
    setSkinBg(S._currentSkinBg);

    // 新版账户页：隐藏轮播主舞台（accounts-redesign 是详情页的父容器，不能整体隐藏），让详情显示
    const stage = document.querySelector('#page-accounts .accts-stage');
    if (stage) stage.style.display = 'none';
    const oldHeader = document.querySelector('#page-accounts .page-header');
    if (oldHeader) oldHeader.style.display = 'none';
    const pageAccounts = document.getElementById('page-accounts');
    const activePage = pageAccounts.closest('.page.active') || pageAccounts.parentElement;
    if (activePage) {
      pageAccounts.style.height = activePage.clientHeight + 'px';
    }
    pageAccounts.style.overflow = 'hidden';
    document.getElementById('page-account-detail').style.display = '';
    setSkinBg(S._currentSkinBg);

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
    if (S._currentDetailAccount) S._currentDetailAccount._resolvedSkinModel = skinModel;

    await initSkinViewer(skinDataUrl, skinModel);
    loadSkinSelector(acc);
    loadCapeSelector(acc);
  });
}

function showAccountList() {
  document.getElementById('page-account-detail').style.display = 'none';
  const accountsList = document.getElementById('accounts-list');
  if (accountsList) accountsList.style.display = '';
  const stage = document.querySelector('#page-accounts .accts-stage');
  if (stage) stage.style.display = '';
  const oldHeader = document.querySelector('#page-accounts .page-header');
  if (oldHeader) oldHeader.style.display = '';
  const pageAccounts = document.getElementById('page-accounts');
  pageAccounts.style.height = '';
  pageAccounts.style.overflow = '';
  destroySkinViewer();
  S._currentDetailAccount = null;
  // 返回列表后重新定位轮播到中间
  if (typeof resetCarouselPosition === 'function') {
    setTimeout(() => resetCarouselPosition(), 80);
  }
}

function destroySkinViewer() {
  if (S._skinResizeTimer) {
    clearTimeout(S._skinResizeTimer);
    S._skinResizeTimer = null;
  }
  if (S._skinResizeObserver) {
    try { S._skinResizeObserver.disconnect(); } catch (e) {}
    S._skinResizeObserver = null;
  }
  if (S._skinViewer) {
    try { S._skinViewer.dispose(); } catch (e) {}
    S._skinViewer = null;
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
    let skinModel = resolvedModel ||
      (((S._currentDetailAccount?.skinModel || '').toLowerCase() === 'slim') ? 'slim' : 'default');
    if (S._currentDetailAccount) S._currentDetailAccount._resolvedSkinModel = skinModel;
    await new Promise(r => setTimeout(r, 100));
    container.innerHTML = '';
    const cw = container.clientWidth || 360;
    const ch = container.clientHeight || 420;
    // 拿不到皮肤时用本地内置皮肤兜底，保证模型一定显示（离线账户无皮肤等）
    const finalSkinUrl = skinUrl || (skinModel === 'slim' ? 'img/skin_alex.png' : 'img/steve_skin.png');
    // 模型交给 skinview3d 按皮肤纹理自动识别（与 PCL 的皮肤预览一致）：
    // 外置登录/第三方皮肤的 account.skinModel 经常缺失或标成 default，
    // 直接传 'default' 会把 Alex 细手臂皮肤渲染成 Steve 粗手臂。
    S._skinViewer = new skinview3d.SkinViewer({
      width: cw,
      height: ch,
      skin: finalSkinUrl,
      model: 'auto-detect'
    });
    container.appendChild(S._skinViewer.canvas);
    S._skinViewer.fov = 30;
    S._skinViewer.zoom = 0.85;
    S._skinViewer.autoRotate = false; // 默认不旋转，呈现 Idle 呼吸效果
    S._skinViewer.animation = new skinview3d.IdleAnimation();
    S._skinViewer.animation.speed = 0.8;
    S._skinViewer.cameraLight.intensity = 1.2;
    S._skinViewer.globalLight.intensity = 2.5;
    S._skinViewer.background = S._currentSkinBg === 'black' ? 0x000000 : 0xffffff;
    S._skinViewer.nameTag = S._currentDetailAccount ? S._currentDetailAccount.username : null;
    // 导航栏收起/展开时，容器宽度会在 0.45s 过渡中逐帧变化；每帧 setSize 都会重建
    // WebGL 画布并短暂清空，表现为人物预览闪烁。这里改为停止变化后再统一重设一次尺寸。
    let lastSkinW = cw, lastSkinH = ch;
    S._skinResizeObserver = new ResizeObserver(() => {
      if (!S._skinViewer || !container) return;
      if (S._skinResizeTimer) clearTimeout(S._skinResizeTimer);
      S._skinResizeTimer = setTimeout(() => {
        S._skinResizeTimer = null;
        if (!S._skinViewer || !container) return;
        const w = container.clientWidth;
        const h = container.clientHeight;
        if (w <= 0 || h <= 0) return;
        if (w === lastSkinW && h === lastSkinH) return;
        lastSkinW = w;
        lastSkinH = h;
        S._skinViewer.setSize(w, h);
        // setSize 会清空画布；立刻补渲染一帧，避免尺寸更新当帧出现空白闪烁
        S._skinViewer.render();
      }, 150);
    });
    S._skinResizeObserver.observe(container);
  } catch (e) {
    console.error('[SkinViewer] init error:', e);
    container.innerHTML = '<div style="display:flex;flex-direction:column;align-items:center;justify-content:center;height:100%;color:var(--text-secondary);font-size:14px;gap:8px;"><span style="font-size:32px;">👤</span><span>皮肤加载失败</span><span style="font-size:12px;color:var(--text-tertiary);">请检查网络连接或重新登录</span></div>';
  }
}

export { loadAccounts, renderAccountCarousel, _carouselResetPosition, _carouselBindDrag, resetCarouselPosition, selectAccount, deleteAccount, showAccountDetail, showAccountList, destroySkinViewer, initSkinViewer };

// 皮肤模块完成换肤后触发列表刷新
registerLazyLoader('accounts:reload', loadAccounts);
// 详情模块通过懒加载中转调用列表动作
registerLazyLoader('accounts:select', selectAccount);
registerLazyLoader('accounts:show-list', showAccountList);
registerLazyLoader('accounts:delete', deleteAccount);
