/* page-shaders.js - 光影页 Vue 组件（渐进式改造）
 * 原则：
 *   1. CSS 一行不动（class 名保留原样）
 *   2. HTML 结构原样搬运（标签、层级、id 全部不变）
 *   3. JS 函数全部复用（来自 js/app/*.js 的全局函数）
 */
const PageShaders = {
  template: `
          <div class="page-header">
            <h2>光影包</h2>
            <div class="page-actions">
              <button class="btn btn-secondary" onclick="openFolder('shaderpacks')">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z"/></svg>
                打开文件夹
              </button>
            </div>
          </div>
          <div class="mod-filter-bar">
            <div class="search-bar">
              <input type="text" id="shader-search-input" placeholder="搜索光影包..." class="search-input">
              <button id="shader-search-btn" class="btn btn-primary">搜索</button>
            </div>
          </div>
          <div id="shader-browse-list" class="mod-list">
            <div class="loading-spinner"><div class="spinner"></div></div>
          </div>
          <div class="pagination">
            <button id="shader-prev-btn" class="btn btn-secondary btn-sm">上一页</button>
            <span id="shader-page-info" class="page-info">1/1</span>
            <button id="shader-next-btn" class="btn btn-secondary btn-sm">下一页</button>
          </div>
  `
};

window.VersePC = window.VersePC || {};
window.VersePC.PageShaders = PageShaders;
