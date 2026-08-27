/* page-modpacks.js - 整合包页 Vue 组件（渐进式改造）
 * 原则：
 *   1. CSS 一行不动（class 名保留原样）
 *   2. HTML 结构原样搬运（标签、层级、id 全部不变）
 *   3. JS 函数全部复用（来自 js/app/*.js 的全局函数）
 */
const PageModpacks = {
  template: `
          <div class="page-header">
            <h2>整合包</h2>
          </div>
          <div class="mod-filter-bar">
            <div class="search-bar">
              <input type="text" id="modpack-search-input" placeholder="搜索整合包..." class="search-input">
              <button id="modpack-search-btn" class="btn btn-primary">搜索</button>
            </div>
            <div class="filter-row">
              <div class="filter-group">
                <label class="filter-label">加载器</label>
                <div class="custom-select custom-select-sm" id="modpack-filter-loader-wrapper">
                  <div class="custom-select-trigger" id="modpack-filter-loader-trigger">
                    <span class="custom-select-value" id="modpack-filter-loader-value">全部</span>
                    <svg class="custom-select-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="6 9 12 15 18 9"></polyline></svg>
                  </div>
                  <div class="custom-select-dropdown" id="modpack-filter-loader-dropdown">
                    <div class="custom-select-options" id="modpack-filter-loader-options"></div>
                  </div>
                </div>
              </div>
              <div class="filter-group">
                <label class="filter-label">游戏版本</label>
                <div class="custom-select custom-select-sm" id="modpack-filter-version-wrapper">
                  <div class="custom-select-trigger" id="modpack-filter-version-trigger">
                    <span class="custom-select-value" id="modpack-filter-version-value">全部</span>
                    <svg class="custom-select-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="6 9 12 15 18 9"></polyline></svg>
                  </div>
                  <div class="custom-select-dropdown" id="modpack-filter-version-dropdown">
                    <div class="custom-select-options" id="modpack-filter-version-options"></div>
                  </div>
                </div>
              </div>
              <div class="filter-group">
                <label class="filter-label">下载源</label>
                <div class="custom-select custom-select-sm" id="modpack-filter-source-wrapper">
                  <div class="custom-select-trigger" id="modpack-filter-source-trigger">
                    <span class="custom-select-value" id="modpack-filter-source-value">全部</span>
                    <svg class="custom-select-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="6 9 12 15 18 9"></polyline></svg>
                  </div>
                  <div class="custom-select-dropdown" id="modpack-filter-source-dropdown">
                    <div class="custom-select-options" id="modpack-filter-source-options"></div>
                  </div>
                </div>
              </div>
            </div>
          <div id="modpack-browse-list" class="mod-list">
            <div class="loading-spinner"><div class="spinner"></div></div>
          </div>
          <div class="pagination">
            <button id="modpack-prev-btn" class="btn btn-secondary btn-sm">上一页</button>
            <span id="modpack-page-info" class="page-info">1/1</span>
            <button id="modpack-next-btn" class="btn btn-secondary btn-sm">下一页</button>
          </div>
  `
};

window.VersePC = window.VersePC || {};
window.VersePC.PageModpacks = PageModpacks;
