/* page-mod-detail.js - 模组详情页 Vue 组件（渐进式改造）
 * 原则：
 *   1. CSS 一行不动（class 名保留原样）
 *   2. HTML 结构原样搬运（标签、层级、id 全部不变）
 *   3. JS 函数全部复用（来自 js/app/*.js 的全局函数）
 */
const PageModDetail = {
  template: `
          <div class="moddetail-page-header">
            <button class="btn btn-icon" onclick="goBackFromDetail()">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:20px;height:20px"><path d="M15 18l-6-6 6-6"/></svg>
            </button>
            <div class="moddetail-top-info">
              <div class="moddetail-icon-wrap">
                <img id="md-icon-img" src="" alt="" onerror="this.style.display='none'">
                <span id="md-icon-fallback" class="icon-fallback" style="display:none"></span>
              </div>
              <div class="moddetail-text-info">
                <h2 id="md-name"></h2>
                <p id="md-desc" class="md-desc"></p>
                <div class="md-stats-row">
                  <span id="md-downloads" class="md-stat">⬇ 0</span>
                  <span id="md-followers" class="md-stat">❤ 0</span>
                  <span id="md-updated" class="md-stat">🕐 更新于</span>
                  <span id="md-source-badge" class="md-source-tag">Modrinth</span>
                </div>
              </div>
            </div>
          </div>
          <div class="md-action-bar">
            <button class="btn btn-secondary md-action-btn" onclick="openModSourceUrl()">转到 Modrinth</button>
            <button class="btn btn-secondary md-action-btn" onclick="copyModName()">复制名称</button>
            <button class="btn btn-secondary md-action-btn" id="md-fav-btn" onclick="showFavSelectDropdown(currentModDetailId, this)">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:14px;height:14px"><path d="M20.84 4.61a5.5 5.5 0 00-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 00-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 000-7.78z"/></svg>
              收藏
            </button>
          </div>
          <div id="md-deps-section" class="md-deps-section" style="display:none">
            <div class="md-deps-header">
              <div style="display:flex;align-items:center;gap:8px;flex:1;cursor:pointer" onclick="toggleMdDepsSection()">
                <svg viewBox="0 0 24 24" fill="none" stroke="var(--warning)" stroke-width="2" style="width:18px;height:18px"><path d="M10.29 3.86L1.82 18a2 2 0 001.71 3h16.94a2 2 0 001.71-3L13.71 3.86a2 2 0 00-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
                <span style="font-size:14px;font-weight:600;color:var(--text-primary)">前置模组</span>
                <span id="md-deps-count" style="font-size:12px;color:var(--text-muted)"></span>
              </div>
              <div style="display:flex;align-items:center;gap:8px">
                <button id="md-deps-download-all-btn" class="md-deps-download-all" onclick="event.stopPropagation();downloadAllDeps()" title="一键下载所有前置模组">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" style="width:14px;height:14px"><path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
                  <span>一键下载</span>
                </button>
                <svg id="md-deps-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:16px;height:16px;transition:transform 0.2s;cursor:pointer" onclick="event.stopPropagation();toggleMdDepsSection()"><path d="M6 9l6 6 6-6"/></svg>
              </div>
            </div>
            <div id="md-deps-list" class="md-deps-list"></div>
          </div>
          <div class="md-body">
            <div class="md-version-tabs" id="md-version-tabs">
              <button class="md-vtab active" data-ver="" onclick="switchMdVersionTab('')">全部</button>
            </div>
            <div class="md-version-list" id="md-version-list">
              <p class="empty-text" style="padding:30px 0;text-align:center;color:var(--text-muted)">选择版本标签查看文件列表</p>
            </div>
          </div>
  `
};

window.VersePC = window.VersePC || {};
window.VersePC.PageModDetail = PageModDetail;
