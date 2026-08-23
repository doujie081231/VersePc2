/* page-private-server.js - 私人服务器页面 Vue 组件（渐进式改造）
 * 原则：
 *   1. CSS 一行不动（class 名保留原样）
 *   2. HTML 结构原样搬运（标签、层级、id 全部不变）
 *   3. JS 函数全部复用（来自 js/app/private-server.js 的全局函数）
 */
const PagePrivateServer = {
  template: `
          <div class="page-header">
            <h2>私人服务器</h2>
            <p class="page-subtitle">管理你的专属 Minecraft 服务器</p>
          </div>
          <div class="ps-page-container" id="private-server-container">
            <!-- 内容由 js/app/private-server.js 的 initPrivateServerPage 填充 -->
          </div>
  `,
  mounted() {
    if (typeof initPrivateServerPage === 'function') {
      initPrivateServerPage();
    }
  }
};

window.VersePC = window.VersePC || {};
window.VersePC.PagePrivateServer = PagePrivateServer;
