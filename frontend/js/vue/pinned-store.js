/* pinned-store.js - 收藏置顶面板共享 store
 * 数据存 localStorage（键 versepc_pinned），跨页面共享响应式 store。
 * 置顶项结构：{ type:'save'|'version'|'shader'|'mod', id(唯一键), name, extra:{} }
 *   save    id=folder,  extra={ folder, versionId, icon, difficulty }
 *   version id=versionId, extra={ loaderText }
 *   shader/mod id=projectId, extra={ projectId, source, icon }
 */
window.VersePC = window.VersePC || {};

// 内联 SVG 常量（面板与原生 innerHTML 按钮共用）
window.VersePC.PIN_ICONS = {
  pin: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 17v5"/><path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z"/></svg>',
  star: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"/></svg>',
  trash: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>',
  folder: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>',
  cube: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z"/><polyline points="3.27 6.96 12 12.01 20.73 6.96"/><line x1="12" y1="22.08" x2="12" y2="12"/></svg>',
  sun: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="5"/><line x1="12" y1="1" x2="12" y2="3"/><line x1="12" y1="21" x2="12" y2="23"/><line x1="4.22" y1="4.22" x2="5.64" y2="5.64"/><line x1="18.36" y1="18.36" x2="19.78" y2="19.78"/><line x1="1" y1="12" x2="3" y2="12"/><line x1="21" y1="12" x2="23" y2="12"/><line x1="4.22" y1="19.78" x2="5.64" y2="18.36"/><line x1="18.36" y1="5.64" x2="19.78" y2="4.22"/></svg>',
  puzzle: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="3" width="7" height="7"/><rect x="14" y="3" width="7" height="7"/><rect x="14" y="14" width="7" height="7"/><rect x="3" y="14" width="7" height="7"/></svg>'
};

const STORAGE_KEY = 'versepc_pinned';
const TYPE_LABELS = { save: '存档', version: '版本', shader: '光影', mod: '模组' };

if (!window.VersePC.pinnedStore) {
  let items = [];
  try {
    const arr = JSON.parse(localStorage.getItem(STORAGE_KEY) || '[]');
    if (Array.isArray(arr)) items = arr;
  } catch (e) {}
  window.VersePC.pinnedStore = Vue.reactive({ items, activeTab: 'save' });
}
const store = window.VersePC.pinnedStore;

function savePinned() {
  try { localStorage.setItem(STORAGE_KEY, JSON.stringify(store.items)); } catch (e) {}
}

function findPinIndex(type, id) {
  return store.items.findIndex(p => p.type === type && p.id === id);
}

function isPinned(type, id) {
  return findPinIndex(type, id) >= 0;
}

// toggle：已存在→移除，否则插到最前；返回新状态（true=已置顶）
function togglePin(type, id, name, extra) {
  const idx = findPinIndex(type, id);
  if (idx >= 0) {
    store.items.splice(idx, 1);
    savePinned();
    return false;
  }
  store.items.unshift({ type, id, name: String(name || id), extra: extra || {} });
  savePinned();
  return true;
}

function removePinned(type, id) {
  const idx = findPinIndex(type, id);
  if (idx >= 0) {
    store.items.splice(idx, 1);
    savePinned();
    return true;
  }
  return false;
}

// 带 toast 的包装（原生 onclick 与 Vue @click 共用）
function togglePinWithToast(type, id, name, extra) {
  const now = togglePin(type, id, name, extra);
  if (typeof showToast === 'function') {
    showToast(now ? `已置顶${TYPE_LABELS[type] || ''}` : `已取消置顶${TYPE_LABELS[type] || ''}`, now ? 'success' : 'info');
  }
  return now;
}

// 供原生 innerHTML onclick 用的参数化包装（全字符串参数，配合 escapeOnclick 安全）
window.togglePinSave = (folder, name, versionId, iconPath, difficulty) =>
  togglePinWithToast('save', folder, name, { folder, versionId, icon: iconPath, difficulty });
window.togglePinVersion = (versionId, name, loaderText) =>
  togglePinWithToast('version', versionId, name, { versionId, loaderText });
window.togglePinProject = (type, projectId, name, source, iconUrl) =>
  togglePinWithToast(type, projectId, name, { projectId, source, icon: iconUrl });

window.isPinned = isPinned;
window.removePinned = removePinned;

// 供原生 innerHTML 按钮使用：toggle 后同步切换按钮 active 状态与 title
window.pinBtnToggle = function (el, type, id, name, extra, activeTitle, inactiveTitle) {
  const now = togglePinWithToast(type, id, name, extra);
  if (el) {
    el.classList.toggle('active', now);
    el.title = now ? (activeTitle || '取消置顶') : (inactiveTitle || '置顶');
  }
  return now;
};

// 旧文本便利贴数据不再使用，顺带清理
try { localStorage.removeItem('versepc_stickynotes'); } catch (e) {}
