/* page-home.js - 首页 Vue 组件（页面结构层）
 * ============================================================================
 * 前端三层架构（详见 index.html 顶部注释）：
 *   - 本文件只负责 HTML 模板（Vue template 字符串）
 *   - 交互逻辑全部调用 js/app/*.js 里的函数
 *   - 全局状态变量在 js/app.js
 *
 * 改动原则：
 *   1. CSS 一行不动（class 名保留原样）
 *   2. HTML 结构原样搬运（标签、层级、id 全部不变）
 *   3. JS 函数全部复用（navigateToPage 等仍来自 js/app/*.js）
 *   4. 仅 onclick → @click 这类最小改动
 *   5. 新增页面交互逻辑请写到 js/app/ 对应文件，不要堆在本文件
 */
const PageHome = {
  template: `
    <div class="home-quick-actions">
      <div class="card home-launch-card">
        <div class="home-launch-left" @click="goAccounts" style="cursor:pointer" title="账户管理">
          <div class="account-avatar" id="home-avatar"><img src="img/icon.png" alt="" class="account-avatar-img"></div>
          <div class="account-details">
            <span class="account-name" id="home-player-name">未登录</span>
            <span class="account-type" id="home-account-type">离线模式</span>
          </div>
        </div>
        <div class="home-launch-right">
          <div class="quick-launch">
            <div class="home-current-version-card" id="home-current-version-card" title="点击切换版本">
              <!-- 由 JS 渲染：图标 + 版本名 + 加载器标签 + 右侧箭头 -->
            </div>
            <button id="home-launch-btn" class="btn btn-primary btn-lg">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polygon points="5 3 19 12 5 21 5 3"/></svg>
              启动游戏
            </button>
          </div>
        </div>
      </div>
    </div>
  `,
  methods: {
    goAccounts() {
      // 复用全局 navigateToPage 函数（来自 js/app/*.js）
      if (typeof navigateToPage === 'function') {
        navigateToPage('accounts');
      }
    }
  }
};

// 导出供主入口使用
window.VersePC = window.VersePC || {};
window.VersePC.PageHome = PageHome;
