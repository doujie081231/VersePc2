/* page-installed-versions.js - 已安装版本页 Vue 组件（渐进式改造第三步）
 * 原则：
 *   1. CSS 一行不动（class 名保留原样）
 *   2. HTML 结构原样搬运（标签、层级、id 全部不变）
 *   3. JS 函数全部复用（navigateToPage / addExternalFolder / refreshInstalledVersions
 *      / onFolderSelectorChange 仍来自 js/app/*.js）
 */
const PageInstalledVersions = {
  template: `
    <div class="page-header" style="gap:12px;padding:16px 24px;flex-wrap:wrap">
      <button class="btn btn-icon" onclick="navigateToPage('home')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:20px;height:20px"><path d="M15 18l-6-6 6-6"/></svg>
      </button>
      <h2>已安装版本</h2>
      <div id="folder-selector-wrapper" style="display:flex;align-items:center;gap:6px;">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:16px;height:16px;opacity:0.6"><path d="M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z"/></svg>
        <select id="folder-selector" class="folder-selector-select" onchange="onFolderSelectorChange()">
          <option value="__internal">游戏文件夹</option>
        </select>
      </div>
      <div class="page-actions" style="margin-left:auto;">
        <button class="btn btn-secondary" onclick="addExternalFolder()">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z"/><line x1="12" y1="11" x2="12" y2="17"/><line x1="9" y1="14" x2="15" y2="14"/></svg>
          添加已有文件夹
        </button>
        <button class="btn btn-secondary" onclick="refreshInstalledVersions()">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="23 4 23 10 17 10"/><path d="M20.49 15a9 9 0 11-2.12-9.36L23 10"/></svg>
          刷新
        </button>
      </div>
    </div>
    <div id="installed-versions-list" class="version-list"></div>
  `
};

window.VersePC = window.VersePC || {};
window.VersePC.PageInstalledVersions = PageInstalledVersions;
