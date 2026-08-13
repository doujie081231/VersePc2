/* page-downloads.js - 下载页 Vue 组件（渐进式改造）
 * 原则：
 *   1. CSS 一行不动（class 名保留原样）
 *   2. HTML 结构原样搬运（标签、层级、id 全部不变）
 *   3. JS 函数全部复用（来自 js/app/*.js 的全局函数）
 */
const PageDownloads = {
  template: `
          <div class="page-header">
            <h2>下载管理</h2>
            <div class="page-actions">
              <button class="btn btn-secondary btn-sm" onclick="clearCompletedDownloads()">清空已完成</button>
            </div>
          </div>
          <div id="download-queue-list" class="dl-queue-list">
            <p class="empty-text" id="dl-empty-hint">暂无下载任务</p>
          </div>
  `
};

window.VersePC = window.VersePC || {};
window.VersePC.PageDownloads = PageDownloads;
