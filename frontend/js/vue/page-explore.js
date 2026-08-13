/* page-explore.js - 探索页 Vue 组件（渐进式改造）
 * 原则：
 *   1. CSS 一行不动（class 名保留原样）
 *   2. HTML 结构原样搬运（标签、层级、id 全部不变）
 *   3. JS 函数全部复用（来自 js/app/*.js 的全局函数）
 */
const PageExplore = {
  template: `
          <div class="page-header">
            <h2>实验性</h2>
            <p class="page-subtitle">V 岛 · 顶部灵动岛助手</p>
          </div>
          <div class="settings-container">
            <div class="card">
              <h3>V 岛</h3>
              <div class="form-group">
                <label class="checkbox-label">
                  <input type="checkbox" id="vIsland">
                  <span>启用 V 岛</span>
                </label>
                <span class="form-hint">开启后下载任务以顶部灵动岛形式展示</span>
              </div>
            </div>
            <div class="card">
              <h3>引导</h3>
              <p class="form-hint" style="margin-bottom:12px">首次开启 V 岛时显示引导页，也可手动重新播放</p>
              <button class="btn btn-ghost btn-sm" id="v-island-replay-onboarding">重新播放引导</button>
            </div>
          </div>
  `
};

window.VersePC = window.VersePC || {};
window.VersePC.PageExplore = PageExplore;
