/**
 * @file favorites.js
 * @description 收藏夹管理模块 - 负责收藏夹的加载、渲染、多选、批量操作及子页面交互
 */

// 加载收藏夹数据，初始化当前选中项并渲染下拉选择
async function loadFavoritesData() {
  try {
    _favorites = await API.getFavorites();
    if (_favorites.length > 0 && !_currentFavId) {
      _currentFavId = _favorites[0].id;
    }
    renderFavFolderSelect();
  } catch (e) {
    console.error('[Fav] 加载收藏夹失败:', e);
    _favorites = [{ name: '默认', id: 'default', favs: [], notes: {} }];
  }
}

// 渲染收藏夹下拉选择框（主页和子页面共用）
function renderFavFolderSelect() {
  const sel = document.getElementById('fav-folder-select');
  if (sel) {
    sel.innerHTML = _favorites.map((f) => {
      return '<option value="' + f.id + '"' + (f.id === _currentFavId ? ' selected' : '') + '>' + escapeHtml(f.name) + ' (' + f.favs.length + ')</option>';
    }).join('');
    sel.onchange = () => {
      _currentFavId = sel.value;
      _favSelectedItems.clear();
      _favMultiSelectMode = false;
      renderFavPage();
    };
  }
  const subSel = document.getElementById('fav-sub-folder-select');
  if (subSel) {
    subSel.innerHTML = _favorites.map((f) => {
      return '<option value="' + escapeHtml(f.id) + '"' + (f.id === (_favSubCurrentFavId || _currentFavId) ? ' selected' : '') + '>' + escapeHtml(f.name) + ' (' + (f.favs ? f.favs.length : 0) + ')</option>';
    }).join('');
  }
}

// 渲染收藏夹主页面：按类型分组展示收藏项目，支持搜索、多选、备注
async function renderFavPage() {
  const content = document.getElementById('fav-content');
  const empty = document.getElementById('fav-empty');
  if (!content || !empty) return;

  const currentFav = _favorites.find((f) => f.id === _currentFavId);
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
    const filtered = _favSearchQuery
      ? projects.filter((p) => {
        return (p.title || '').toLowerCase().includes(_favSearchQuery.toLowerCase()) ||
          (p.description || '').toLowerCase().includes(_favSearchQuery.toLowerCase());
      })
      : projects;

    if (filtered.length === 0) {
      content.innerHTML = '<div style="text-align:center;padding:40px;color:var(--text-secondary)"><p>' + (_favSearchQuery ? '没有找到匹配的收藏' : '收藏夹为空') + '</p></div>';
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
        const isChecked = _favSelectedItems.has(p.id);
        const note = currentFav.notes && currentFav.notes[p.id] ? currentFav.notes[p.id] : '';
        html += '<div class="fav-item" data-id="' + escapeHtml(p.id) + '" onclick="openFavItemDetail(\'' + escapeOnclick(p.id) + '\', \'' + escapeOnclick(p.source || 'modrinth') + '\')">';
        if (_favMultiSelectMode) {
          html += '<input type="checkbox" class="fav-item-checkbox"' + (isChecked ? ' checked' : '') + ' onclick="event.stopPropagation(); toggleFavItemSelect(\'' + escapeOnclick(p.id) + '\')">';
        }
        if (p.icon) {
          html += '<img class="fav-item-icon" src="' + escapeHtml(p.icon) + '" onerror="this.style.display=\'none\';this.nextElementSibling.style.display=\'flex\'"><div class="fav-item-icon-placeholder" style="display:none">' + escapeHtml((p.title || '?')[0]) + '</div>';
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
        html += '<button class="btn-icon" title="编辑备注" onclick="event.stopPropagation(); editFavNote(\'' + escapeOnclick(p.id) + '\')"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:14px;height:14px"><path d="M11 4H4a2 2 0 00-2 2v14a2 2 0 002 2h14a2 2 0 002-2v-7"/><path d="M18.5 2.5a2.121 2.121 0 013 3L12 15l-4 1 1-4 9.5-9.5z"/></svg></button>';
        html += '<button class="btn-icon fav-remove" title="取消收藏" onclick="event.stopPropagation(); removeFavItem(\'' + escapeOnclick(p.id) + '\')"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:14px;height:14px"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg></button>';
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
        return { id: id, title: id, description: '加载失败', source: 'modrinth', projectType: 'mod' };
      }
    });
    const batchResults = await Promise.all(promises);
    results.push.apply(results, batchResults);
  }
  return results;
}

// 打开收藏项目详情（多选模式下切换选中状态）
function openFavItemDetail(projectId, source) {
  if (_favMultiSelectMode) {
    toggleFavItemSelect(projectId);
    return;
  }
  openModDetail(projectId, source);
}

let _favSubMultiSelect = false;
let _favSubSelected = new Set();
let _favSubSearchQuery = '';
let _favSubCurrentFavId = null;

// 进入收藏夹子页面（从模组浏览页切换过来）
function enterFavSubPage() {
  const browseSection = document.getElementById('mod-browse-section');
  const favSection = document.getElementById('mod-fav-section');
  if (!browseSection || !favSection) return;
  browseSection.style.display = 'none';
  favSection.style.display = 'block';
  _favSubCurrentFavId = _currentFavId;
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
  _favSubMultiSelect = false;
  _favSubSelected.clear();
  _favSubSearchQuery = '';
}

// 填充子页面收藏夹下拉选择
function populateFavSubFolderSelect() {
  const sel = document.getElementById('fav-sub-folder-select');
  if (!sel) return;
  sel.innerHTML = _favorites.map((f) => {
    return '<option value="' + escapeHtml(f.id) + '"' + (f.id === _favSubCurrentFavId ? ' selected' : '') + '>' + escapeHtml(f.name) + ' (' + (f.favs ? f.favs.length : 0) + ')</option>';
  }).join('');
}

// 子页面切换收藏夹时触发
function onFavSubFolderChange(favId) {
  _favSubCurrentFavId = favId;
  _currentFavId = favId;
  _favSubSelected.clear();
  renderFavSubFolderSelect();
  renderFavSubList();
}

function renderFavSubFolderSelect() {
  populateFavSubFolderSelect();
}

// 子页面搜索框输入回调
function onFavSubSearch(query) {
  _favSubSearchQuery = query;
  renderFavSubList();
}

// 渲染子页面收藏列表，支持搜索和多选模式
async function renderFavSubList() {
  const list = document.getElementById('fav-sub-list');
  const empty = document.getElementById('fav-sub-empty');
  if (!list || !empty) return;

  const currentFav = _favorites.find((f) => f.id === _favSubCurrentFavId);
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
    const filtered = _favSubSearchQuery
      ? projects.filter((p) => {
        return (p.title || '').toLowerCase().includes(_favSubSearchQuery.toLowerCase()) ||
          (p.description || '').toLowerCase().includes(_favSubSearchQuery.toLowerCase());
      })
      : projects;

    if (filtered.length === 0) {
      list.innerHTML = '<div style="text-align:center;padding:40px;color:var(--text-secondary)"><p>' + (_favSubSearchQuery ? '没有找到匹配的收藏' : '收藏夹为空') + '</p></div>';
      return;
    }

    list.innerHTML = filtered.map((p) => {
      const isFav = _favorites.some((f) => f.favs.includes(p.id));
      const isChecked = _favSubSelected.has(p.id);
      const source = p.source || 'modrinth';
      return '<div class="mod-item mod-item-clickable' + (_favSubMultiSelect ? ' mod-multiselect-active' : '') + '" onclick="openModDetail(\'' + escapeOnclick(p.id) + '\', \'' + escapeOnclick(source) + '\')">' +
        (_favSubMultiSelect ? '<div class="mod-checkbox' + (isChecked ? ' checked' : '') + '" data-mod-id="' + escapeHtml(p.id) + '" onclick="event.stopPropagation();toggleFavSubItemSelect(\'' + escapeOnclick(p.id) + '\')"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3"><polyline points="20 6 9 17 4 12"/></svg></div>' : '') +
        '<div class="mod-icon"><img src="' + escapeHtml(p.icon || '') + '" alt="" loading="lazy" onerror="this.style.display=\'none\';this.parentElement.classList.add(\'mod-icon--fallback\')"></div>' +
        '<div class="mod-info">' +
          '<div class="mod-name">' + escapeHtml(formatModNameWithChinese(p.slug || p.id, p.title)) + '</div>' +
          '<div class="mod-desc">' + escapeHtml(p.description || '') + '</div>' +
          '<div class="mod-meta">' +
            '<span>\u2B07 ' + formatNumber(p.downloads || 0) + '</span>' +
            '<span>\u2764 ' + escapeHtml(p.author || '') + '</span>' +
            '<span>' + (p.categories || []).slice(0, 3).join(', ') + '</span>' +
          '</div>' +
        '</div>' +
        '<div class="mod-actions" onclick="event.stopPropagation()">' +
          '<button class="fav-heart-btn active" data-project-id="' + escapeHtml(p.id) + '" onclick="event.stopPropagation(); showFavSelectDropdown(\'' + escapeOnclick(p.id) + '\', this)"><svg viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" stroke-width="2"><path d="M20.84 4.61a5.5 5.5 0 00-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 00-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 000-7.78z"/></svg></button>' +
          '<button class="btn btn-primary btn-sm" onclick="event.stopPropagation();openModDetail(\'' + escapeOnclick(p.id) + '\', \'' + escapeOnclick(source) + '\')">安装</button>' +
        '</div>' +
      '</div>';
    }).join('');
  } catch (e) {
    list.innerHTML = '<div style="text-align:center;padding:40px;color:var(--text-secondary)"><p>加载失败: ' + escapeHtml(e.message) + '</p></div>';
  }
}

// 切换子页面多选模式
function toggleFavSubMultiSelect() {
  _favSubMultiSelect = !_favSubMultiSelect;
  _favSubSelected.clear();
  const bar = document.getElementById('fav-sub-multi-bar');
  const toggle = document.getElementById('fav-sub-multi-toggle');
  if (bar) bar.style.display = _favSubMultiSelect ? 'flex' : 'none';
  if (toggle) toggle.textContent = _favSubMultiSelect ? '取消多选' : '多选';
  updateFavSubMultiBar();
  renderFavSubList();
}

// 切换子页面单个项目选中状态
function toggleFavSubItemSelect(projectId) {
  if (_favSubSelected.has(projectId)) {
    _favSubSelected.delete(projectId);
  } else {
    _favSubSelected.add(projectId);
  }
  updateFavSubMultiBar();
  const checkbox = document.querySelector('.mod-checkbox[data-mod-id="' + projectId + '"]');
  if (checkbox) checkbox.classList.toggle('checked', _favSubSelected.has(projectId));
}

// 子页面全选/取消全选
function toggleFavSubSelectAll(checked) {
  const currentFav = _favorites.find((f) => f.id === _favSubCurrentFavId);
  if (!currentFav) return;
  _favSubSelected.clear();
  if (checked) {
    currentFav.favs.forEach((id) => { _favSubSelected.add(id); });
  }
  updateFavSubMultiBar();
  document.querySelectorAll('#fav-sub-list .mod-checkbox').forEach((cb) => {
    cb.classList.toggle('checked', _favSubSelected.has(cb.getAttribute('data-mod-id')));
  });
}

// 更新子页面多选操作栏状态
function updateFavSubMultiBar() {
  const countEl = document.getElementById('fav-sub-selected-count');
  const removeBtn = document.getElementById('fav-sub-batch-remove');
  const downloadBtn = document.getElementById('fav-sub-batch-download');
  if (countEl) countEl.textContent = '已选 ' + _favSubSelected.size + ' 个';
  if (removeBtn) removeBtn.disabled = _favSubSelected.size === 0;
  if (downloadBtn) downloadBtn.disabled = _favSubSelected.size === 0;
}

// 批量取消子页面选中项目的收藏
async function batchRemoveFavSub() {
  if (_favSubSelected.size === 0) return;
  if (!confirm('确定取消收藏选中的 ' + _favSubSelected.size + ' 个项目？')) return;
  try {
    for (const projectId of _favSubSelected) {
      await API.removeFromFavorite(_favSubCurrentFavId, projectId);
      const fav = _favorites.find((f) => f.id === _favSubCurrentFavId);
      if (fav) fav.favs = fav.favs.filter((id) => id !== projectId);
    }
    _favSubSelected.clear();
    updateFavSubMultiBar();
    showToast('已取消收藏', 'success');
    renderFavSubList();
    renderFavFolderSelect();
  } catch (e) {
    showToast('操作失败', 'error');
  }
}

// 批量下载子页面选中项目
async function batchDownloadFavSub() {
  if (_favSubSelected.size === 0) return;
  try {
    const projects = await fetchFavProjects(Array.from(_favSubSelected));
    projects.forEach((p) => {
      if (p.id) quickInstallMod(p.id, p.source || 'modrinth', '', '');
    });
    showToast('已开始下载 ' + _favSubSelected.size + ' 个模组', 'success');
  } catch (e) {
    showToast('批量下载失败', 'error');
  }
}

// 切换单个项目选中状态
function toggleFavItemSelect(projectId) {
  if (_favSelectedItems.has(projectId)) {
    _favSelectedItems.delete(projectId);
  } else {
    _favSelectedItems.add(projectId);
  }
  updateFavSelectUI();
  const cb = document.querySelector('.fav-item[data-id="' + CSS.escape(projectId) + '"] .fav-item-checkbox');
  if (cb) cb.checked = _favSelectedItems.has(projectId);
}

// 切换主页多选模式
function toggleFavMultiSelect() {
  _favMultiSelectMode = !_favMultiSelectMode;
  _favSelectedItems.clear();
  const bar = document.getElementById('fav-multiselect-bar');
  if (bar) bar.style.display = _favMultiSelectMode ? 'flex' : 'none';
  const btn = document.getElementById('fav-multiselect-toggle');
  if (btn) btn.classList.toggle('active', _favMultiSelectMode);
  renderFavPage();
}

// 主页全选/取消全选
function toggleFavSelectAll(checked) {
  const currentFav = _favorites.find((f) => f.id === _currentFavId);
  if (!currentFav) return;
  _favSelectedItems.clear();
  if (checked) currentFav.favs.forEach((id) => { _favSelectedItems.add(id); });
  updateFavSelectUI();
  renderFavPage();
}

// 更新主页多选操作栏状态
function updateFavSelectUI() {
  const count = _favSelectedItems.size;
  const countEl = document.getElementById('fav-selected-count');
  if (countEl) countEl.textContent = '已选 ' + count + ' 个';
  const removeBtn = document.getElementById('fav-batch-remove-btn');
  if (removeBtn) removeBtn.disabled = count === 0;
  const downloadBtn = document.getElementById('fav-batch-download-btn');
  if (downloadBtn) downloadBtn.disabled = count === 0;
}

// 取消单个项目收藏
async function removeFavItem(projectId) {
  if (!_currentFavId) return;
  try {
    await API.removeFromFavorite(_currentFavId, projectId);
    const fav = _favorites.find((f) => f.id === _currentFavId);
    if (fav) fav.favs = fav.favs.filter((id) => id !== projectId);
    renderFavFolderSelect();
    renderFavPage();
    updateFavHeartButtons();
    showToast('已取消收藏', 'success');
  } catch (e) {
    showToast('操作失败', 'error');
  }
}

// 批量取消主页选中项目的收藏
async function batchRemoveFavorites() {
  if (_favSelectedItems.size === 0) return;
  const count = _favSelectedItems.size;
  if (!confirm('确定要取消收藏 ' + count + ' 个项目吗？')) return;
  try {
    const idsToRemove = Array.from(_favSelectedItems);
    for (let i = 0; i < idsToRemove.length; i++) {
      await API.removeFromFavorite(_currentFavId, idsToRemove[i]);
    }
    const fav = _favorites.find((f) => f.id === _currentFavId);
    if (fav) fav.favs = fav.favs.filter((id) => !_favSelectedItems.has(id));
    _favSelectedItems.clear();
    updateFavSelectUI();
    renderFavFolderSelect();
    renderFavPage();
    updateFavHeartButtons();
    showToast('已取消 ' + count + ' 个收藏', 'success');
  } catch (e) {
    showToast('操作失败', 'error');
  }
}

// 编辑收藏项目备注
async function editFavNote(projectId) {
  const currentFav = _favorites.find((f) => f.id === _currentFavId);
  if (!currentFav) return;
  const oldNote = currentFav.notes && currentFav.notes[projectId] ? currentFav.notes[projectId] : '';
  const note = prompt('编辑备注:', oldNote);
  if (note === null) return;
  try {
    await API.updateFavNote(_currentFavId, projectId, note);
    if (!currentFav.notes) currentFav.notes = {};
    if (note) currentFav.notes[projectId] = note;
    else delete currentFav.notes[projectId];
    renderFavPage();
    showToast('备注已更新', 'success');
  } catch (e) {
    showToast('操作失败', 'error');
  }
}

// 显示收藏夹管理菜单（新建/重命名/删除）
function showFavManageMenu() {
  closeFavMenus();
  const btn = event.currentTarget;
  const rect = btn.getBoundingClientRect();
  const menu = document.createElement('div');
  menu.className = 'fav-manage-menu';
  menu.id = 'fav-manage-menu-popup';
  menu.style.top = (rect.bottom + 4) + 'px';
  menu.style.right = (window.innerWidth - rect.right) + 'px';
  menu.innerHTML = '<div class="fav-manage-menu-item" onclick="createNewFavorite()"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:14px;height:14px"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>新建收藏夹</div>' +
    '<div class="fav-manage-menu-item" onclick="renameCurrentFavorite()"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:14px;height:14px"><path d="M11 4H4a2 2 0 00-2 2v14a2 2 0 002 2h14a2 2 0 002-2v-7"/><path d="M18.5 2.5a2.121 2.121 0 013 3L12 15l-4 1 1-4 9.5-9.5z"/></svg>重命名当前收藏夹</div>' +
    '<div class="fav-manage-menu-item danger" onclick="deleteCurrentFavorite()"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:14px;height:14px"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 01-2 2H7a2 2 0 01-2-2V6m3 0V4a2 2 0 012-2h4a2 2 0 012 2v2"/></svg>删除当前收藏夹</div>';
  document.body.appendChild(menu);
  setTimeout(() => { document.addEventListener('click', closeFavMenusHandler, { once: true }); }, 0);
}

// 关闭所有收藏夹相关弹出菜单
function closeFavMenus() {
  document.querySelectorAll('.fav-manage-menu, .fav-select-dropdown').forEach((el) => { el.remove(); });
}

// 点击菜单外部时关闭菜单
function closeFavMenusHandler(e) {
  if (!e.target.closest('.fav-manage-menu') && !e.target.closest('.fav-select-dropdown')) {
    closeFavMenus();
  }
}

// 新建收藏夹
async function createNewFavorite() {
  closeFavMenus();
  const name = prompt('请输入收藏夹名称:');
  if (!name) return;
  try {
    const result = await API.createFavorite(name);
    if (result && result.favorite) {
      _favorites.push(result.favorite);
      _currentFavId = result.favorite.id;
      renderFavFolderSelect();
      renderFavPage();
      showToast('收藏夹已创建', 'success');
    }
  } catch (e) {
    showToast('创建失败', 'error');
  }
}

// 重命名当前收藏夹
async function renameCurrentFavorite() {
  closeFavMenus();
  const fav = _favorites.find((f) => f.id === _currentFavId);
  if (!fav) return;
  const name = prompt('请输入新名称:', fav.name);
  if (!name || name === fav.name) return;
  try {
    await API.renameFavorite(_currentFavId, name);
    fav.name = name;
    renderFavFolderSelect();
    showToast('重命名成功', 'success');
  } catch (e) {
    showToast('重命名失败', 'error');
  }
}

// 删除当前收藏夹（至少保留一个）
async function deleteCurrentFavorite() {
  closeFavMenus();
  if (_favorites.length <= 1) {
    showToast('至少保留一个收藏夹', 'error');
    return;
  }
  const fav = _favorites.find((f) => f.id === _currentFavId);
  if (!fav) return;
  if (!confirm('确定要删除收藏夹"' + fav.name + '"吗？')) return;
  try {
    await API.deleteFavorite(_currentFavId);
    _favorites = _favorites.filter((f) => f.id !== _currentFavId);
    _currentFavId = _favorites.length > 0 ? _favorites[0].id : '';
    renderFavFolderSelect();
    renderFavPage();
    showToast('收藏夹已删除', 'success');
  } catch (e) {
    showToast('删除失败', 'error');
  }
}

// 导出当前收藏夹内容到剪贴板
function exportCurrentFav() {
  const fav = _favorites.find((f) => f.id === _currentFavId);
  if (!fav) return;
  const data = JSON.stringify(fav.favs);
  navigator.clipboard.writeText(data).then(() => {
    showToast('已复制到剪贴板', 'success');
  }).catch(() => {
    prompt('复制以下内容:', data);
  });
}

// 显示导入收藏夹对话框
function showFavImportModal() {
  const data = prompt('请粘贴收藏分享码:');
  if (!data) return;
  importFavData(data);
}

// 导入收藏数据
async function importFavData(data) {
  try {
    const result = await API.importFavorite(data, _currentFavId);
    if (result && result.success) {
      await loadFavoritesData();
      renderFavPage();
      showToast('已导入 ' + result.imported + ' 个项目', 'success');
    }
  } catch (e) {
    showToast('导入失败: ' + e.message, 'error');
  }
}

// 批量下载主页选中项目
async function batchDownloadFavorites() {
  if (_favSelectedItems.size === 0) return;
  const ids = Array.from(_favSelectedItems);
  showToast('正在准备下载 ' + ids.length + ' 个模组...', 'info');
  for (let i = 0; i < ids.length; i++) {
    try {
      const result = await API.downloadMod(ids[i], 'modrinth', '', '');
      if (result.success && result.sessionId) {
        showModDownloadModal(result.fileName, result.sessionId, result.path || '');
      }
    } catch (e) {
      console.error('下载失败:', ids[i], e);
    }
  }
  showToast('批量下载已启动', 'success');
}

// 更新所有收藏心形按钮的激活状态
function updateFavHeartButtons() {
  document.querySelectorAll('.fav-heart-btn').forEach((btn) => {
    const projectId = btn.dataset.projectId;
    if (!projectId) return;
    const isFav = _favorites.some((f) => f.favs.includes(projectId));
    btn.classList.toggle('active', isFav);
  });
}

// 显示收藏夹选择下拉菜单（选择收藏到哪个收藏夹）
function showFavSelectDropdown(projectId, anchorEl) {
  closeFavMenus();
  const rect = anchorEl.getBoundingClientRect();
  const dropdown = document.createElement('div');
  dropdown.className = 'fav-select-dropdown';
  dropdown.id = 'fav-select-dropdown-popup';
  dropdown.style.top = (rect.bottom + 4) + 'px';
  dropdown.style.left = Math.min(rect.left, window.innerWidth - 220) + 'px';

  const isFavInAny = _favorites.some((f) => f.favs.includes(projectId));
  let innerHtml = '';
  if (isFavInAny) {
    innerHtml += '<div class="fav-select-item" style="color:var(--red)" onclick="removeFromAllFavs(\'' + escapeOnclick(projectId) + '\')"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>取消所有收藏</div>';
  }

  _favorites.forEach((f) => {
    const has = f.favs.includes(projectId);
    innerHtml += '<div class="fav-select-item' + (has ? ' active' : '') + '" onclick="toggleFavForProject(\'' + escapeOnclick(f.id) + '\', \'' + escapeOnclick(projectId) + '\', ' + has + ')">';
    if (has) {
      innerHtml += '<svg viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" stroke-width="2"><path d="M20.84 4.61a5.5 5.5 0 00-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 00-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 000-7.78z"/></svg>';
    } else {
      innerHtml += '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M20.84 4.61a5.5 5.5 0 00-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 00-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 000-7.78z"/></svg>';
    }
    innerHtml += (has ? '取消收藏 ' : '收藏到 ') + escapeHtml(f.name) + '</div>';
  });
  dropdown.innerHTML = innerHtml;
  document.body.appendChild(dropdown);
  setTimeout(() => { document.addEventListener('click', closeFavMenusHandler, { once: true }); }, 0);
}

// 切换项目在某收藏夹中的收藏状态
async function toggleFavForProject(favId, projectId, isRemove) {
  closeFavMenus();
  try {
    if (isRemove) {
      await API.removeFromFavorite(favId, projectId);
      const fav = _favorites.find((f) => f.id === favId);
      if (fav) fav.favs = fav.favs.filter((id) => id !== projectId);
      showToast('已取消收藏', 'success');
    } else {
      await API.addToFavorite(favId, projectId);
      const fav2 = _favorites.find((f) => f.id === favId);
      if (fav2 && !fav2.favs.includes(projectId)) fav2.favs.push(projectId);
      showToast('已添加到收藏夹', 'success');
    }
    renderFavFolderSelect();
    updateFavHeartButtons();
    if (document.getElementById('page-mod-favorites') && document.getElementById('page-mod-favorites').classList.contains('active')) {
      renderFavPage();
    }
    if (document.getElementById('mod-fav-section') && document.getElementById('mod-fav-section').style.display !== 'none') {
      renderFavSubList();
    }
  } catch (e) {
    showToast('操作失败', 'error');
  }
}

// 从所有收藏夹中移除指定项目
async function removeFromAllFavs(projectId) {
  closeFavMenus();
  try {
    for (let i = 0; i < _favorites.length; i++) {
      const fav = _favorites[i];
      if (fav.favs.includes(projectId)) {
        await API.removeFromFavorite(fav.id, projectId);
        fav.favs = fav.favs.filter((id) => id !== projectId);
      }
    }
    renderFavFolderSelect();
    updateFavHeartButtons();
    if (document.getElementById('mod-fav-section') && document.getElementById('mod-fav-section').style.display !== 'none') {
      renderFavSubList();
    }
    showToast('已取消所有收藏', 'success');
  } catch (e) {
    showToast('操作失败', 'error');
  }
}

// 绑定收藏夹搜索按钮和输入框事件
function setupFavSearchListeners() {
  const searchBtn = document.getElementById('fav-search-btn');
  const searchInput = document.getElementById('fav-search-input');
  if (searchBtn && !searchBtn._favBound) {
    searchBtn._favBound = true;
    searchBtn.addEventListener('click', () => {
      _favSearchQuery = searchInput ? searchInput.value : '';
      renderFavPage();
    });
  }
  if (searchInput && !searchInput._favBound) {
    searchInput._favBound = true;
    searchInput.addEventListener('keyup', (e) => {
      if (e.key === 'Enter' && !e.isComposing) {
        _favSearchQuery = e.target.value;
        renderFavPage();
      }
    });
  }
}
