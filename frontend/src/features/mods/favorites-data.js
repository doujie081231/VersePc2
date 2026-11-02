// 收藏夹数据与渲染：加载收藏夹、主页与子页面列表渲染、项目详情拉取、搜索监听。
import { API } from '../../core/api.js';
import { escapeHtml, normalizeIconUrl } from '../../core/utils.js';
import { formatNumber } from '../misc/ui-components.js';
import { appState } from '../../core/app-state.js';
import { formatModNameWithChinese } from './chinese-names.js';
import { actionAttrs, actionAttrsStop } from '../../core/actions.js';
import { favSubState } from './favorites-state.js';

// 加载收藏夹数据，初始化当前选中项并渲染下拉选择
async function loadFavoritesData() {
  try {
    appState._favorites = await API.getFavorites();
    if (appState._favorites.length > 0 && !appState._currentFavId) {
      appState._currentFavId = appState._favorites[0].id;
    }
    renderFavFolderSelect();
  } catch (e) {
    console.error('[Fav] 加载收藏夹失败:', e);
    appState._favorites = [{ name: '默认', id: 'default', favs: [], notes: {} }];
  }
}

// 渲染收藏夹下拉选择框（主页和子页面共用）
function renderFavFolderSelect() {
  const sel = document.getElementById('fav-folder-select');
  if (sel) {
    sel.innerHTML = appState._favorites.map((f) => {
      return '<option value="' + f.id + '"' + (f.id === appState._currentFavId ? ' selected' : '') + '>' + escapeHtml(f.name) + ' (' + f.favs.length + ')</option>';
    }).join('');
    sel.onchange = () => {
      appState._currentFavId = sel.value;
      appState._favSelectedItems.clear();
      appState._favMultiSelectMode = false;
      renderFavPage();
    };
  }
  const subSel = document.getElementById('fav-sub-folder-select');
  if (subSel) {
    subSel.innerHTML = appState._favorites.map((f) => {
      return '<option value="' + escapeHtml(f.id) + '"' + (f.id === (favSubState.currentFavId || appState._currentFavId) ? ' selected' : '') + '>' + escapeHtml(f.name) + ' (' + (f.favs ? f.favs.length : 0) + ')</option>';
    }).join('');
  }
}

// 渲染收藏夹主页面：按类型分组展示收藏项目，支持搜索、多选、备注
async function renderFavPage() {
  const content = document.getElementById('fav-content');
  const empty = document.getElementById('fav-empty');
  if (!content || !empty) return;

  const currentFav = appState._favorites.find((f) => f.id === appState._currentFavId);
  if (!currentFav || !currentFav.favs || currentFav.favs.length === 0) {
    content.style.display = 'none';
    empty.style.display = 'block';
    return;
  }
  content.style.display = 'block';
  empty.style.display = 'none';

  content.innerHTML = '<div class="loading-spinner"><div class="spinner"></div></div>';

  try {
    const projectIds = currentFav.favs;
    const projects = await fetchFavProjects(projectIds);
    const filtered = appState._favSearchQuery
      ? projects.filter((p) => {
        return (p.title || '').toLowerCase().includes(appState._favSearchQuery.toLowerCase()) ||
          (p.description || '').toLowerCase().includes(appState._favSearchQuery.toLowerCase());
      })
      : projects;

    if (filtered.length === 0) {
      content.innerHTML = '<div style="text-align:center;padding:40px;color:var(--text-secondary)"><p>' + (appState._favSearchQuery ? '没有找到匹配的收藏' : '收藏夹为空') + '</p></div>';
      return;
    }

    // 按项目类型分组展示
    const grouped = {};
    filtered.forEach((p) => {
      const type = p.projectType || p.source || 'mod';
      if (!grouped[type]) grouped[type] = [];
      grouped[type].push(p);
    });

    const typeLabels = { mod: 'Mod', modpack: '整合包', resourcepack: '资源包', shader: '光影', datapack: '数据包' };
    let html = '';
    Object.keys(grouped).forEach((type) => {
      const items = grouped[type];
      html += '<div class="fav-category-title">' + (typeLabels[type] || type) + ' (' + items.length + ')</div>';
      items.forEach((p) => {
        const isChecked = appState._favSelectedItems.has(p.id);
        const note = currentFav.notes && currentFav.notes[p.id] ? currentFav.notes[p.id] : '';
        html += '<div class="fav-item" data-id="' + escapeHtml(p.id) + '"' + actionAttrs('openFavItemDetail', p.id, p.source || 'modrinth') + '>';
        if (appState._favMultiSelectMode) {
          html += '<input type="checkbox" class="fav-item-checkbox"' + (isChecked ? ' checked' : '') + actionAttrsStop('toggleFavItemSelect', p.id) + '>';
        }
        if (p.icon) {
          html += '<img class="fav-item-icon" src="' + escapeHtml(normalizeIconUrl(p.icon)) + '" data-img-fallback="sibling"><div class="fav-item-icon-placeholder" style="display:none">' + escapeHtml((p.title || '?')[0]) + '</div>';
        } else {
          html += '<div class="fav-item-icon-placeholder">' + escapeHtml((p.title || '?')[0]) + '</div>';
        }
        html += '<div class="fav-item-info"><div class="fav-item-name">' + escapeHtml(p.title || p.id) + '</div><div class="fav-item-desc">' + escapeHtml(p.description || '') + '</div>';
        if (note) {
          html += '<div class="fav-item-note">' + escapeHtml(note) + '</div>';
        }
        html += '</div>';
        html += '<span class="fav-item-type">' + (typeLabels[type] || type) + '</span>';
        html += '<div class="fav-item-actions">';
        html += '<button class="btn-icon" title="编辑备注"' + actionAttrsStop('editFavNote', p.id) + '><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:14px;height:14px"><path d="M11 4H4a2 2 0 00-2 2v14a2 2 0 002 2h14a2 2 0 002-2v-7"/><path d="M18.5 2.5a2.121 2.121 0 013 3L12 15l-4 1 1-4 9.5-9.5z"/></svg></button>';
        html += '<button class="btn-icon fav-remove" title="取消收藏"' + actionAttrsStop('removeFavItem', p.id) + '><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:14px;height:14px"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg></button>';
        html += '</div></div>';
      });
    });
    content.innerHTML = html;
  } catch (e) {
    content.innerHTML = '<div style="text-align:center;padding:40px;color:var(--text-secondary)"><p>加载失败: ' + escapeHtml(e.message) + '</p></div>';
  }
}

// 分批获取收藏项目的详细信息（每批 10 个，避免单次请求过多）
async function fetchFavProjects(projectIds) {
  const results = [];
  const batchSize = 10;
  for (let i = 0; i < projectIds.length; i += batchSize) {
    const batch = projectIds.slice(i, i + batchSize);
    const promises = batch.map(async (id) => {
      try {
        const detail = await API.getModDetail(id, 'modrinth');
        return Object.assign({}, detail, { source: 'modrinth' });
      } catch (e) {
        try {
          const detail = await API.getModDetail(id, 'curseforge');
          return Object.assign({}, detail, { source: 'curseforge' });
        } catch (e2) {
          return { id: id, title: id, description: '加载失败', source: 'modrinth', projectType: 'mod' };
        }
      }
    });
    const batchResults = await Promise.all(promises);
    results.push.apply(results, batchResults);
  }
  return results;
}

// 进入收藏夹子页面（从模组浏览页切换过来）
function enterFavSubPage() {
  const browseSection = document.getElementById('mod-browse-section');
  const favSection = document.getElementById('mod-fav-section');
  if (!browseSection || !favSection) return;
  browseSection.style.display = 'none';
  favSection.style.display = 'block';
  favSubState.currentFavId = appState._currentFavId;
  populateFavSubFolderSelect();
  renderFavSubList();
}

// 退出收藏夹子页面，重置多选状态
function exitFavSubPage() {
  const browseSection = document.getElementById('mod-browse-section');
  const favSection = document.getElementById('mod-fav-section');
  if (!browseSection || !favSection) return;
  favSection.style.display = 'none';
  browseSection.style.display = 'block';
  favSubState.multiSelect = false;
  favSubState.selected.clear();
  favSubState.searchQuery = '';
}

// 填充子页面收藏夹下拉选择
function populateFavSubFolderSelect() {
  const sel = document.getElementById('fav-sub-folder-select');
  if (!sel) return;
  sel.innerHTML = appState._favorites.map((f) => {
    return '<option value="' + escapeHtml(f.id) + '"' + (f.id === favSubState.currentFavId ? ' selected' : '') + '>' + escapeHtml(f.name) + ' (' + (f.favs ? f.favs.length : 0) + ')</option>';
  }).join('');
}

// 子页面切换收藏夹时触发
function onFavSubFolderChange(favId) {
  favSubState.currentFavId = favId;
  appState._currentFavId = favId;
  favSubState.selected.clear();
  renderFavSubFolderSelect();
  renderFavSubList();
}

function renderFavSubFolderSelect() {
  populateFavSubFolderSelect();
}

// 子页面搜索框输入回调
function onFavSubSearch(query) {
  favSubState.searchQuery = query;
  renderFavSubList();
}

// 渲染子页面收藏列表，支持搜索和多选模式
async function renderFavSubList() {
  const list = document.getElementById('fav-sub-list');
  const empty = document.getElementById('fav-sub-empty');
  if (!list || !empty) return;

  const currentFav = appState._favorites.find((f) => f.id === favSubState.currentFavId);
  if (!currentFav || !currentFav.favs || currentFav.favs.length === 0) {
    list.style.display = 'none';
    empty.style.display = 'block';
    return;
  }
  list.style.display = '';
  empty.style.display = 'none';
  list.innerHTML = '<div class="loading-spinner"><div class="spinner"></div></div>';

  try {
    const projects = await fetchFavProjects(currentFav.favs);
    const filtered = favSubState.searchQuery
      ? projects.filter((p) => {
        return (p.title || '').toLowerCase().includes(favSubState.searchQuery.toLowerCase()) ||
          (p.description || '').toLowerCase().includes(favSubState.searchQuery.toLowerCase());
      })
      : projects;

    if (filtered.length === 0) {
      list.innerHTML = '<div style="text-align:center;padding:40px;color:var(--text-secondary)"><p>' + (favSubState.searchQuery ? '没有找到匹配的收藏' : '收藏夹为空') + '</p></div>';
      return;
    }

    list.innerHTML = filtered.map((p) => {
      const isChecked = favSubState.selected.has(p.id);
      const source = p.source || 'modrinth';
      return '<div class="mod-item mod-item-clickable' + (favSubState.multiSelect ? ' mod-multiselect-active' : '') + '"' + actionAttrs('openModDetail', p.id, source) + '>' +
        (favSubState.multiSelect ? '<div class="mod-checkbox' + (isChecked ? ' checked' : '') + '" data-mod-id="' + escapeHtml(p.id) + '"' + actionAttrsStop('toggleFavSubItemSelect', p.id) + '><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3"><polyline points="20 6 9 17 4 12"/></svg></div>' : '') +
        '<div class="mod-icon"><img src="' + escapeHtml(normalizeIconUrl(p.icon || '')) + '" alt="" loading="lazy" data-img-fallback="modicon"></div>' +
        '<div class="mod-info">' +
          '<div class="mod-name">' + escapeHtml(formatModNameWithChinese(p.slug || p.id, p.title)) + '</div>' +
          '<div class="mod-desc">' + escapeHtml(p.description || '') + '</div>' +
          '<div class="mod-meta">' +
            '<span>\u2B07 ' + formatNumber(p.downloads || 0) + '</span>' +
            '<span>\u2764 ' + escapeHtml(p.author || '') + '</span>' +
            '<span>' + (p.categories || []).slice(0, 3).join(', ') + '</span>' +
          '</div>' +
        '</div>' +
        '<div class="mod-actions" data-stop="1">' +
          '<button class="fav-heart-btn active" data-project-id="' + escapeHtml(p.id) + '"' + actionAttrsStop('showFavSelectDropdown', p.id) + '><svg viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" stroke-width="2"><path d="M20.84 4.61a5.5 5.5 0 00-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 00-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 000-7.78z"/></svg></button>' +
          '<button class="btn btn-primary btn-sm"' + actionAttrsStop('openModDetail', p.id, source) + '>安装</button>' +
        '</div>' +
      '</div>';
    }).join('');
  } catch (e) {
    list.innerHTML = '<div style="text-align:center;padding:40px;color:var(--text-secondary)"><p>加载失败: ' + escapeHtml(e.message) + '</p></div>';
  }
}

// 绑定收藏夹搜索按钮和输入框事件
function setupFavSearchListeners() {
  const searchBtn = document.getElementById('fav-search-btn');
  const searchInput = document.getElementById('fav-search-input');
  if (searchBtn && !searchBtn._favBound) {
    searchBtn._favBound = true;
    searchBtn.addEventListener('click', () => {
      appState._favSearchQuery = searchInput ? searchInput.value : '';
      renderFavPage();
    });
  }
  if (searchInput && !searchInput._favBound) {
    searchInput._favBound = true;
    searchInput.addEventListener('keyup', (e) => {
      if (e.key === 'Enter' && !e.isComposing) {
        appState._favSearchQuery = e.target.value;
        renderFavPage();
      }
    });
  }
}

export { loadFavoritesData, renderFavFolderSelect, renderFavPage, fetchFavProjects, enterFavSubPage, exitFavSubPage, populateFavSubFolderSelect, onFavSubFolderChange, renderFavSubFolderSelect, onFavSubSearch, renderFavSubList, setupFavSearchListeners };
