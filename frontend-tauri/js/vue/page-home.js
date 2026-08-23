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
 *
 * 「启动任务 / 运行中游戏」卡片：由 launch.js 的 updateGameInstanceList 写入
 * 共享响应式 store（window.VersePCGameStore），本组件读取并渲染。
 */

// 共享响应式 store（launch.js 写入，主页卡片读取）
if (!window.VersePCGameStore) {
  window.VersePCGameStore = Vue.reactive({ instances: [], running: false, now: Date.now() });
  // 每秒刷新一次，用于运行时长倒计时显示
  setInterval(function () { window.VersePCGameStore.now = Date.now(); }, 1000);
}

const PageHome = {
  template: `
    <div class="home-page">
      <!-- 标题：放在原本标题栏位置的中间（最顶部居中，不占内容空间） -->
      <div class="home-page-title topbar-title" data-tauri-drag-region @click="goHome">
        Verse<span class="home-page-title-accent">PC2</span>
      </div>

      <!-- 头像（居中放大，皮肤头部像素方块） -->
      <div class="home-avatar-box" id="home-avatar-box" @click="goAccounts" title="账户管理">
        <div class="home-avatar-img-wrap" id="home-avatar"><img src="img/icon.png" alt="" class="home-avatar-img"></div>
      </div>

      <!-- 账户信息（头像下方） -->
      <div class="home-account-bar" @click="goAccounts" style="cursor:pointer" title="账户管理">
        <span class="account-name" id="home-player-name">未登录</span>
        <span class="account-type" id="home-account-type">离线模式</span>
      </div>

      <!-- 版本列表卡片（头像下方） -->
      <div class="home-version-card-wrap">
        <div class="home-current-version-card" id="home-current-version-card" title="点击切换版本">
          <!-- 由 JS 渲染：图标 + 版本名 + 加载器标签 + 右侧箭头 -->
        </div>
      </div>

      <!-- 启动按钮（版本列表下方） -->
      <button id="home-launch-btn" class="btn btn-primary btn-lg home-launch-btn">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polygon points="5 3 19 12 5 21 5 3"/></svg>
        启动游戏
      </button>

      <!-- 启动任务 / 运行中游戏卡片（Vue 响应式，有运行实例时显示） -->
      <div class="home-running-card" v-if="gameInstances.length > 0">
        <div class="home-running-card-header">
          <span class="home-running-dot"></span>
          <span class="home-running-title">启动任务</span>
          <span class="home-running-count" v-if="gameInstances.length > 1">({{ gameInstances.length }})</span>
        </div>
        <div class="home-running-list">
          <div class="home-running-item" v-for="inst in gameInstances" :key="inst.sessionId">
            <div class="home-running-info">
              <div class="home-running-name">{{ inst.versionId }}</div>
              <div class="home-running-meta">PID: {{ inst.pid }} · {{ formatElapsed(inst) }}</div>
            </div>
            <button class="home-running-stop" @click="stopInstance(inst.sessionId)">停止</button>
          </div>
        </div>
      </div>
    </div>
  `,
  computed: {
    // 读取共享响应式 store，运行实例变化时主页卡片自动更新
    gameInstances() {
      return window.VersePCGameStore ? window.VersePCGameStore.instances : [];
    }
  },
  methods: {
    goAccounts() {
      if (typeof navigateToPage === 'function') {
        navigateToPage('accounts');
      }
    },
    goHome() {
      if (typeof navigateToPage === 'function') {
        navigateToPage('home');
      }
    },
    // 计算运行时长（每秒由 store.now 触发重新渲染）
    formatElapsed(inst) {
      const elapsed = Math.floor(((window.VersePCGameStore.now || Date.now()) - inst.startTime) / 1000);
      const mins = Math.floor(elapsed / 60);
      const secs = elapsed % 60;
      return mins > 0 ? `${mins}分${secs}秒` : `${secs}秒`;
    },
    stopInstance(sessionId) {
      if (typeof stopGameInstance === 'function') {
        stopGameInstance(sessionId);
      }
    }
  },
  mounted() {
    // Vue 已将首页 DOM 挂载到文档，此时才存在 home-avatar 等元素
    // 延迟一帧确保布局尺寸已计算完成，再补一次头像、版本卡片和启动按钮绑定
    // （因为 init-setup.js 的 setupLaunchBar 执行时机早于 Vue 挂载，会绑定不到 home-launch-btn）
    const init = () => {
      if (typeof loadAccounts !== 'function' || typeof loadVersions !== 'function' || typeof handleLaunch !== 'function') {
        setTimeout(init, 80);
        return;
      }
      // 补绑定主页启动按钮（setupLaunchBar 可能因 DOM 未挂载而绑定失败）
      const homeLaunchBtn = document.getElementById('home-launch-btn');
      if (homeLaunchBtn && !homeLaunchBtn._bound) {
        homeLaunchBtn._bound = true;
        homeLaunchBtn.addEventListener('click', handleLaunch);
      }
      // 刷新账户显示（含头像加载）和版本列表（含主页版本卡片渲染）
      loadAccounts().catch(e => console.error('[PageHome] loadAccounts error:', e));
      loadVersions().catch(e => console.error('[PageHome] loadVersions error:', e));
      // 拉取一次运行中游戏状态，让主页启动任务卡片尽快显示
      if (typeof updateGameStatus === 'function') {
        updateGameStatus().catch(function (e) { console.error('[PageHome] updateGameStatus error:', e); });
      }
    };
    requestAnimationFrame(() => setTimeout(init, 50));
  }
};

// 导出供主入口使用
window.VersePC = window.VersePC || {};
window.VersePC.PageHome = PageHome;