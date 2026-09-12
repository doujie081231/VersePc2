/* page-lan-enderlink.js - EnderLink联机页 Vue 组件
 * 与陶瓦/红石联机页面形式一致：
 *   1. 引导：EnderLink 图标 → 欢迎语 → 开始（转圈）→ 祝你使用愉快
 *   2. 主界面：运行中实例卡片 + 创建房间 / 加入房间 按钮
 *   3. 创建流程：启动游戏提示(端口25565) → 下一步(需游戏运行) → 开启联机中(转圈) → 联机地址 + 关闭
 *   4. 加入流程：输入联机地址 → 下一步 → 连接中(转圈) → 连接成功 + 退出
 * 后端对接 enderlinkOnline（lytapi 大厅 + frp 内网穿透）。
 */
const PageLanEnderlink = {
  name: 'PageLanEnderlink',
  data() {
    return {};
  },
  computed: {
    // 运行中的游戏实例（与首页共用共享响应式 store）
    gameInstances() {
      return window.VersePCGameStore ? window.VersePCGameStore.instances : [];
    }
  },
  methods: {
    formatElapsed(inst) {
      const elapsed = Math.floor(((window.VersePCGameStore.now || Date.now()) - inst.startTime) / 1000);
      const mins = Math.floor(elapsed / 60);
      const secs = elapsed % 60;
      return mins > 0 ? `${mins}分${secs}秒` : `${secs}秒`;
    },
    host() {
      if (typeof enderlinkHostStep1 === 'function') enderlinkHostStep1();
    }
  },
  template: `
          <!-- 首次使用引导 -->
          <div id="enderlink-onboarding" class="terracotta-ob" style="display:none">
            <div class="terracotta-ob-icon">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M14 3l8 8-8 8-2-2 6-6-6-6z"/><path d="M10 21l-8-8 8-8 2 2-6 6 6 6z"/></svg>
            </div>
            <h2 class="terracotta-ob-title" id="enderlink-ob-title">你好！欢迎使用 EnderLink 联机</h2>
            <p class="terracotta-ob-desc" id="enderlink-ob-desc">基于 lytapi 大厅 + frp 内网穿透，一键开启外网联机</p>
            <button class="terracotta-ob-dl" id="enderlink-ob-dl" onclick="enderlinkStartOnboarding()" title="开始使用 EnderLink 联机">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
            </button>
            <div class="terracotta-ob-spinner" id="enderlink-ob-spinner" style="display:none">
              <div class="spinner" role="status"></div>
              <span class="terracotta-ob-progress-text" id="enderlink-ob-progress-text">正在初始化...</span>
            </div>
            <div class="terracotta-ob-done" id="enderlink-ob-done" style="display:none">
              <svg class="terracotta-ob-check" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>
              <h2>祝你使用愉快！</h2>
            </div>
          </div>

          <div id="enderlink-main">
          <div class="page-header">
            <h2>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" style="width:24px;height:24px;margin-right:8px;vertical-align:-5px" stroke-linecap="round" stroke-linejoin="round"><path d="M14 3l8 8-8 8-2-2 6-6-6-6z"/><path d="M10 21l-8-8 8-8 2 2-6 6 6 6z"/></svg>
              EnderLink联机
            </h2>
            <p class="page-subtitle">基于 lytapi 大厅 + frp 内网穿透，一键开启外网联机</p>
          </div>
          <div class="lan-container">

            <!-- 主界面：运行中实例 + 操作按钮 -->
            <div id="enderlink-home" class="terracotta-home">
              <div v-if="gameInstances.length > 0" class="terracotta-instance-list">
                <div class="terracotta-instance-card" v-for="inst in gameInstances" :key="inst.sessionId">
                  <div class="terracotta-instance-icon">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"><path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z"/><polyline points="3.27 6.96 12 12.01 20.73 6.96"/><line x1="12" y1="22.08" x2="12" y2="12"/></svg>
                  </div>
                  <div class="terracotta-instance-info">
                    <div class="terracotta-instance-name">{{ inst.versionId }}</div>
                    <div class="terracotta-instance-meta">PID {{ inst.pid }} · 已运行 {{ formatElapsed(inst) }}</div>
                  </div>
                  <span class="terracotta-instance-dot"></span>
                </div>
              </div>
              <div v-else class="terracotta-empty">
                <p>当前没有正在运行中的实例，请你启动游戏后再继续</p>
              </div>

              <div class="terracotta-actions">
                <button class="btn btn-primary btn-lg" @click="host()" :disabled="gameInstances.length === 0" title="创建房间">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:16px;height:16px"><path d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z"/><polyline points="14 3 14 9 20 9"/></svg>
                  创建房间
                </button>
                <p class="terracotta-join-hint">朋友在游戏内直接输入你分享的联机地址即可加入</p>
              </div>
            </div>

            <!-- 创建房间流程 -->
            <div id="enderlink-host-panel" class="terracotta-flow" style="display:none">
              <div id="enderlink-host-step1" class="terracotta-step">
                <h3>启动游戏并开放局域网</h3>
                <p class="terracotta-step-desc">1. 请先启动游戏并进入存档<br>2. 按 Esc 打开菜单，点击「对局域网开放」<br>3. 将端口设置为 <b>25565</b></p>
                <div class="terracotta-port-hint">端口号：25565</div>
                <button class="terracotta-next-btn" id="enderlink-host-next" onclick="enderlinkHostNext()" disabled title="请先启动游戏并开放局域网">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
                </button>
              </div>
              <div id="enderlink-host-step2" class="terracotta-step" style="display:none">
                <div class="spinner" role="status"></div>
                <p class="terracotta-step-text">正在开启联机中...</p>
              </div>
              <div id="enderlink-host-step3" class="terracotta-step" style="display:none">
                <div class="terracotta-step-bigicon">
                  <svg class="terracotta-ob-check" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>
                </div>
                <h3>联机已开启</h3>
                <div class="terracotta-info-row">
                  <span class="terracotta-info-label">联机地址</span>
                  <span class="terracotta-info-value" id="enderlink-room-addr">--</span>
                  <button class="terracotta-copy-btn" onclick="enderlinkCopyAddr()" title="复制联机地址">复制</button>
                </div>
                <p class="terracotta-step-desc">将联机地址发送给朋友，朋友在 Minecraft 多人游戏中添加该地址即可加入</p>
                <button class="terracotta-close-btn" onclick="enderlinkDisconnect()" title="关闭联机">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
                </button>
              </div>
            </div>

            <div class="lan-hint-bar" id="enderlink-hint" style="display:none"></div>
          </div>
          </div>
  `
};

window.VersePC = window.VersePC || {};
window.VersePC.PageLanEnderlink = PageLanEnderlink;
