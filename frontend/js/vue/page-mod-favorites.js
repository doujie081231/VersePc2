/* page-mod-favorites.js - 模组收藏页 Vue 组件（渐进式改造）
 * 原则：
 *   1. CSS 一行不动（class 名保留原样）
 *   2. HTML 结构原样搬运（标签、层级、id 全部不变）
 *   3. JS 函数全部复用（来自 js/app/*.js 的全局函数）
 */
const PageModFavorites = {
  template: `
          <div class="page-header">
            <h2>收藏夹</h2>
            <div class="page-actions">
              <select id="fav-folder-select" style="min-width:150px;padding:6px 12px;border-radius:8px;border:1px solid var(--border);background:var(--bg-card);color:var(--text-primary);font-size:13px;">
              </select>
              <button class="btn btn-secondary btn-sm" onclick="showFavManageMenu()">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:14px;height:14px"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 010 2.83 2 2 0 01-2.83 0l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-4 0v-.09A1.65 1.65 0 009 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 01-2.83 0 2 2 0 010-2.83l.06-.06A1.65 1.65 0 004.68 15a1.65 1.65 0 00-1.51-1H3a2 2 0 010-4h.09A1.65 1.65 0 004.6 9a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 012.83-2.83l.06.06a1.65 1.65 0 001.82.33H9a1.65 1.65 0 001-1.51V3a2 2 0 114 0v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 012.83 2.83l-.06.06a1.65 1.65 0 00-.33 1.82V9c.26.604.852.997 1.51 1H21a2 2 0 010 4h-.09a1.65 1.65 0 00-1.51 1z"/></svg>
                管理
              </button>
              <button class="btn btn-secondary btn-sm" id="fav-multiselect-toggle" onclick="toggleFavMultiSelect()">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:14px;height:14px"><polyline points="9 11 12 14 22 4"/><path d="M21 12v7a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2h11"/></svg>
                多选
              </button>
              <button class="btn btn-secondary btn-sm" onclick="showFavImportModal()">导入</button>
              <button class="btn btn-secondary btn-sm" onclick="exportCurrentFav()">导出</button>
            </div>
          </div>
          <div id="fav-multiselect-bar" class="mod-multiselect-bar" style="display:none">
            <label class="mod-select-all-label">
              <input type="checkbox" id="fav-select-all" onchange="toggleFavSelectAll(this.checked)">
              <span>全选</span>
            </label>
            <span class="mod-selected-count" id="fav-selected-count">已选 0 个</span>
            <button class="btn btn-danger btn-sm" id="fav-batch-remove-btn" onclick="batchRemoveFavorites()" disabled>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:14px;height:14px"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 01-2 2H7a2 2 0 01-2-2V6m3 0V4a2 2 0 012-2h4a2 2 0 012 2v2"/></svg>
              取消收藏
            </button>
            <button class="btn btn-primary btn-sm" id="fav-batch-download-btn" onclick="batchDownloadFavorites()" disabled>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:14px;height:14px"><path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
              批量下载
            </button>
            <button class="btn btn-secondary btn-sm" onclick="toggleFavMultiSelect()">取消多选</button>
          </div>
          <div class="mod-filter-bar" style="margin-bottom:16px">
            <div class="search-bar">
              <input type="text" id="fav-search-input" placeholder="搜索收藏的模组..." class="search-input">
              <button id="fav-search-btn" class="btn btn-primary">搜索</button>
            </div>
          </div>
          <div id="fav-content">
            <div class="loading-spinner"><div class="spinner"></div></div>
          </div>
          <div id="fav-empty" style="display:none;text-align:center;padding:60px 20px;color:var(--text-secondary)">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" style="width:64px;height:64px;margin-bottom:16px;opacity:0.3"><path d="M20.84 4.61a5.5 5.5 0 00-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 00-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 000-7.78z"/></svg>
            <p style="font-size:15px;margin-bottom:8px">还没有收藏内容</p>
            <p style="font-size:13px">在模组搜索页面点击心形按钮收藏模组</p>
          </div>
  `
};

window.VersePC = window.VersePC || {};
window.VersePC.PageModFavorites = PageModFavorites;
