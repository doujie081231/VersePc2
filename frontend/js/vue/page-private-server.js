/* page-private-server - 私人服务器页 Vue 组件 */
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
