/* page-explore.js - 实验性页面 Vue 组件
 * 说明：V 岛模块设置已迁移到「设置 → 其他」；实验性页面现由
 * PageServerHost（开服）组件渲染，见 index.html 挂载配置。
 */
const PageExplore = {
  template: `
          <div class="page-header">
            <h2>实验性</h2>
            <p class="page-subtitle">本地离线开服功能正在重新优化中 敬请期待</p>
          </div>
  `
};

window.VersePC = window.VersePC || {};
window.VersePC.PageExplore = PageExplore;