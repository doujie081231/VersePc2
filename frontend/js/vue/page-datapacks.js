/* page-datapacks.js - 数据包页 Vue 组件（渐进式改造）
 * 原则：
 *   1. CSS 一行不动（class 名保留原样）
 *   2. HTML 结构原样搬运（标签、层级、id 全部不变）
 *   3. JS 函数全部复用（来自 js/app/*.js 的全局函数）
 */
const PageDatapacks = {
  template: `
          <div class="page-header">
            <h2>数据包</h2>
          </div>
          <div class="mod-filter-bar">
            <div class="search-bar">
              <input type="text" id="datapack-search-input" placeholder="搜索数据包..." class="search-input">
              <button id="datapack-search-btn" class="btn btn-primary">搜索</button>
            </div>
            <div class="filter-row">
              <div class="filter-group">
                <label class="filter-label">游戏版本</label>
                <div class="custom-select custom-select-sm" id="datapack-filter-version-wrapper">
                  <div class="custom-select-trigger" id="datapack-filter-version-trigger">
                    <span class="custom-select-value" id="datapack-filter-version-value">全部</span>
                    <svg class="custom-select-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="6 9 12 15 18 9"></polyline></svg>
                  </div>
                  <div class="custom-select-dropdown" id="datapack-filter-version-dropdown">
                    <div class="custom-select-options" id="datapack-filter-version-options"></div>
                  </div>
                </div>
              </div>
            </div>
          </div>
          <div id="datapack-browse-list" class="mod-list">
            <div class="loading-spinner"><div class="spinner"></div></div>
          </div>
          <div class="pagination">
            <button id="datapack-prev-btn" class="btn btn-secondary btn-sm">上一页</button>
            <span id="datapack-page-info" class="page-info">1/1</span>
            <button id="datapack-next-btn" class="btn btn-secondary btn-sm">下一页</button>
          </div>
  `
};

window.VersePC = window.VersePC || {};
window.VersePC.PageDatapacks = PageDatapacks;
