/* page-downloads - 下载页 Vue 组件 */
const PageDownloads = {
  template: `
          <div class="page-header dl-page-header">
            <h2>下载管理</h2>
            <div class="page-actions dl-page-actions">
              <div class="dl-search">
                <svg class="dl-search-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="8"/><path d="M21 21l-4.35-4.35"/></svg>
                <input type="text" id="dl-search-input" class="dl-search-input" placeholder="搜索下载任务..." oninput="dlManager.applySearch(this.value)">
              </div>
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
