/* page-mods.js - 模组页 Vue 组件（渐进式改造）
 * 原则：
 *   1. CSS 一行不动（class 名保留原样）
 *   2. HTML 结构原样搬运（标签、层级、id 全部不变）
 *   3. JS 函数全部复用（来自 js/app/*.js 的全局函数）
 */
const PageMods = {
  template: `
          <div class="page-header">
            <div class="tab-group" style="display:flex;gap:8px;margin-right:auto;">
              <button class="tab-btn active" data-tab="browse-mods">浏览模组</button>
            </div>
            <div class="page-actions">
              <button class="btn btn-secondary btn-sm" id="mod-multiselect-toggle" onclick="toggleModMultiSelect()">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:14px;height:14px"><polyline points="9 11 12 14 22 4"/><path d="M21 12v7a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2h11"/></svg>
                多选
              </button>
              <button class="btn btn-secondary" onclick="openFolder('mods')">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z"/></svg>
                打开文件夹
              </button>
            </div>
          </div>
          <div id="mod-multiselect-bar" class="mod-multiselect-bar" style="display:none">
            <label class="mod-select-all-label">
              <input type="checkbox" id="mod-select-all" onchange="toggleSelectAllMods(this.checked)">
              <span>全选</span>
            </label>
            <span class="mod-selected-count" id="mod-selected-count">已选 0 个</span>
            <span class="mod-filter-hint" id="mod-filter-hint"></span>
            <button class="btn btn-primary btn-sm" id="mod-batch-download-btn" onclick="batchDownloadMods()" disabled>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:14px;height:14px"><path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
              批量下载
            </button>
            <button class="btn btn-secondary btn-sm" onclick="toggleModMultiSelect()">取消多选</button>
          </div>
          <div id="browse-mods-panel" class="mod-panel">
            <div class="mod-filter-bar">
              <div class="search-bar">
                <input type="text" id="mod-search-input" placeholder="搜索模组名称..." class="search-input">
                <button id="mod-search-btn" class="btn btn-primary">搜索</button>
              </div>
              <div class="filter-row">
                <div class="filter-group">
                  <label class="filter-label">加载器</label>
                  <div class="custom-select custom-select-sm" id="mod-filter-loader-wrapper">
                    <div class="custom-select-trigger" id="mod-filter-loader-trigger">
                      <span class="custom-select-value" id="mod-filter-loader-value">全部</span>
                      <svg class="custom-select-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="6 9 12 15 18 9"></polyline></svg>
                    </div>
                    <div class="custom-select-dropdown" id="mod-filter-loader-dropdown">
                      <div class="custom-select-options" id="mod-filter-loader-options"></div>
                    </div>
                  </div>
                </div>
                <div class="filter-group">
                  <label class="filter-label">游戏版本</label>
                  <div class="custom-select custom-select-sm" id="mod-filter-version-wrapper">
                    <div class="custom-select-trigger" id="mod-filter-version-trigger">
                      <span class="custom-select-value" id="mod-filter-version-value">全部</span>
                      <svg class="custom-select-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="6 9 12 15 18 9"></polyline></svg>
                    </div>
                    <div class="custom-select-dropdown" id="mod-filter-version-dropdown">
                      <div class="custom-select-options" id="mod-filter-version-options"></div>
                    </div>
                  </div>
                </div>
                <div class="filter-group">
                  <label class="filter-label">分类</label>
                  <div class="custom-select custom-select-sm" id="mod-filter-category-wrapper">
                    <div class="custom-select-trigger" id="mod-filter-category-trigger">
                      <span class="custom-select-value" id="mod-filter-category-value">全部</span>
                      <svg class="custom-select-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="6 9 12 15 18 9"></polyline></svg>
                    </div>
                    <div class="custom-select-dropdown" id="mod-filter-category-dropdown">
                      <div class="custom-select-options" id="mod-filter-category-options"></div>
                    </div>
                  </div>
                </div>
                <div class="filter-group">
                  <label class="filter-label">排序</label>
                  <div class="custom-select custom-select-sm" id="mod-filter-sort-wrapper">
                    <div class="custom-select-trigger" id="mod-filter-sort-trigger">
                      <span class="custom-select-value" id="mod-filter-sort-value">相关度</span>
                      <svg class="custom-select-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="6 9 12 15 18 9"></polyline></svg>
                    </div>
                    <div class="custom-select-dropdown" id="mod-filter-sort-dropdown">
                      <div class="custom-select-options" id="mod-filter-sort-options"></div>
                    </div>
                  </div>
                </div>
                <div class="filter-group">
                  <label class="filter-label">来源</label>
                  <div class="custom-select custom-select-sm" id="mod-filter-source-wrapper">
                    <div class="custom-select-trigger" id="mod-filter-source-trigger">
                      <span class="custom-select-value" id="mod-filter-source-value">全部</span>
                      <svg class="custom-select-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="6 9 12 15 18 9"></polyline></svg>
                    </div>
                    <div class="custom-select-dropdown" id="mod-filter-source-dropdown">
                      <div class="custom-select-options" id="mod-filter-source-options"></div>
                    </div>
                  </div>
                </div>
                <button class="btn btn-secondary btn-sm" onclick="enterFavSubPage()" style="margin-left:auto;height:32px;display:inline-flex;align-items:center;gap:4px;">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:14px;height:14px"><path d="M20.84 4.61a5.5 5.5 0 00-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 00-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 000-7.78z"/></svg>
                  收藏
                </button>

              </div>
            </div>
            <div id="mod-browse-section" class="mod-browse-section">
              <h3 class="section-title" id="mod-browse-title">热门模组</h3>
              <div id="mod-browse-list" class="mod-list">
                <div class="loading-spinner"><div class="spinner"></div></div>
              </div>
              <div id="mod-pagination" class="pagination">
                <button id="mod-prev-btn" class="btn btn-secondary btn-sm">上一页</button>
                <span id="mod-page-info" class="page-info">1/1</span>
                <button id="mod-next-btn" class="btn btn-secondary btn-sm">下一页</button>
              </div>
            </div>
            <div id="mod-fav-section" class="mod-browse-section" style="display:none">
              <div style="display:flex;align-items:center;gap:12px;margin-bottom:16px">
                <button class="btn btn-icon" onclick="exitFavSubPage()" title="返回浏览">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:20px;height:20px"><path d="M15 18l-6-6 6-6"/></svg>
                </button>
                <h3 class="section-title" style="margin:0">我的收藏</h3>
                <select id="fav-sub-folder-select" style="padding:4px 10px;border-radius:6px;border:1px solid var(--border);background:var(--bg-card);color:var(--text-primary);font-size:13px;" onchange="onFavSubFolderChange(this.value)"></select>
                <div style="flex:1"></div>
                <button class="btn btn-secondary btn-sm" onclick="showFavManageMenu()">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:14px;height:14px"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 010 2.83 2 2 0 01-2.83 0l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-4 0v-.09A1.65 1.65 0 009 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 01-2.83 0 2 2 0 010-2.83l.06-.06A1.65 1.65 0 004.68 15a1.65 1.65 0 00-1.51-1H3a2 2 0 010-4h.09A1.65 1.65 0 004.6 9a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 012.83-2.83l.06.06a1.65 1.65 0 001.82.33H9a1.65 1.65 0 001-1.51V3a2 2 0 114 0v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 012.83 2.83l-.06.06a1.65 1.65 0 00-.33 1.82V9c.26.604.852.997 1.51 1H21a2 2 0 010 4h-.09a1.65 1.65 0 00-1.51 1z"/></svg>
                  管理
                </button>
                <button class="btn btn-secondary btn-sm" id="fav-sub-multi-toggle" onclick="toggleFavSubMultiSelect()">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:14px;height:14px"><polyline points="9 11 12 14 22 4"/><path d="M21 12v7a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2h11"/></svg>
                  多选
                </button>
                <button class="btn btn-secondary btn-sm" onclick="showFavImportModal()">导入</button>
                <button class="btn btn-secondary btn-sm" onclick="exportCurrentFav()">导出</button>
              </div>
              <div id="fav-sub-multi-bar" class="mod-multiselect-bar" style="display:none">
                <label class="mod-select-all-label">
                  <input type="checkbox" id="fav-sub-select-all" onchange="toggleFavSubSelectAll(this.checked)">
                  <span>全选</span>
                </label>
                <span class="mod-selected-count" id="fav-sub-selected-count">已选 0 个</span>
                <button class="btn btn-danger btn-sm" id="fav-sub-batch-remove" onclick="batchRemoveFavSub()" disabled>取消收藏</button>
                <button class="btn btn-primary btn-sm" id="fav-sub-batch-download" onclick="batchDownloadFavSub()" disabled>
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:14px;height:14px"><path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
                  批量下载
                </button>
                <button class="btn btn-secondary btn-sm" onclick="toggleFavSubMultiSelect()">取消多选</button>
              </div>
              <div id="fav-sub-search-bar" style="margin-bottom:16px">
                <input type="text" id="fav-sub-search-input" placeholder="搜索收藏的模组..." class="search-input" style="max-width:400px" oninput="onFavSubSearch(this.value)">
              </div>
              <div id="fav-sub-list" class="mod-list">
                <div class="loading-spinner"><div class="spinner"></div></div>
              </div>
              <div id="fav-sub-empty" style="display:none;text-align:center;padding:60px 20px;color:var(--text-secondary)">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" style="width:64px;height:64px;margin-bottom:16px;opacity:0.3"><path d="M20.84 4.61a5.5 5.5 0 00-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 00-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 000-7.78z"/></svg>
                <p style="font-size:15px;margin-bottom:8px">还没有收藏内容</p>
                <p style="font-size:13px">在模组搜索页面点击心形按钮收藏模组</p>
              </div>
            </div>
          </div>
  `
};

window.VersePC = window.VersePC || {};
window.VersePC.PageMods = PageMods;
