/* page-resourcepacks.js - 资源包页 Vue 组件（渐进式改造）
 * 原则：
 *   1. CSS 一行不动（class 名保留原样）
 *   2. HTML 结构原样搬运（标签、层级、id 全部不变）
 *   3. JS 函数全部复用（来自 js/app/*.js 的全局函数）
 */
const PageResourcepacks = {
  template: `
          <div class="page-header">
            <h2>材质包</h2>
            <div class="page-actions">
              <button class="btn btn-secondary" onclick="openFolder('resourcepacks')">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z"/></svg>
                打开文件夹
              </button>
            </div>
          </div>
          <div class="mod-filter-bar">
            <div class="search-bar">
              <input type="text" id="resourcepack-search-input" placeholder="搜索材质包..." class="search-input">
              <button id="resourcepack-search-btn" class="btn btn-primary">搜索</button>
            </div>
            <div class="filter-row">
              <div class="filter-group">
                <label class="filter-label">游戏版本</label>
                <div class="custom-select custom-select-sm" id="resourcepack-filter-version-wrapper">
                  <div class="custom-select-trigger" id="resourcepack-filter-version-trigger">
                    <span class="custom-select-value" id="resourcepack-filter-version-value">全部</span>
                    <svg class="custom-select-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="6 9 12 15 18 9"></polyline></svg>
                  </div>
                  <div class="custom-select-dropdown" id="resourcepack-filter-version-dropdown">
                    <div class="custom-select-options" id="resourcepack-filter-version-options"></div>
                  </div>
                </div>
              </div>
              <div class="filter-group">
                <label class="filter-label">分辨率</label>
                <div class="custom-select custom-select-sm" id="resourcepack-filter-resolution-wrapper">
                  <div class="custom-select-trigger" id="resourcepack-filter-resolution-trigger">
                    <span class="custom-select-value" id="resourcepack-filter-resolution-value">全部</span>
                    <svg class="custom-select-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="6 9 12 15 18 9"></polyline></svg>
                  </div>
                  <div class="custom-select-dropdown" id="resourcepack-filter-resolution-dropdown">
                    <div class="custom-select-options" id="resourcepack-filter-resolution-options"></div>
                  </div>
                </div>
              </div>
            </div>
          </div>
          <div id="resourcepack-browse-list" class="mod-list">
            <div class="loading-spinner"><div class="spinner"></div></div>
          </div>
          <div class="pagination">
            <button id="resourcepack-prev-btn" class="btn btn-secondary btn-sm">上一页</button>
            <span id="resourcepack-page-info" class="page-info">1/1</span>
            <button id="resourcepack-next-btn" class="btn btn-secondary btn-sm">下一页</button>
          </div>
  `
};

window.VersePC = window.VersePC || {};
window.VersePC.PageResourcepacks = PageResourcepacks;
