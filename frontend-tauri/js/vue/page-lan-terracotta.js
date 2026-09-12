/* page-lan-terracotta.js - 陶瓦联机页 Vue 组件
 * 流程：
 *   1. 首次引导：陶瓦图标 → 欢迎语 → 下载核心（转圈动画）→ 祝你使用愉快
 *   2. 主界面：显示运行中实例卡片（无实例则提示）→ 下方 创建房间 / 加入房间 按钮
 *   3. 创建流程：启动游戏提示(端口25565) → 下一步(需游戏运行) → 创建隧道中(转圈) → 隧道信息 + 关闭
 *   4. 加入流程：输入房间码 → 下一步 → 连接中(转圈) → 连接成功 + 退出房间
 */
const PageLanTerracotta = {
  name: 'PageLanTerracotta',
  data() {
    return {};
  },
  computed: {
    // 运行中的游戏实例（与首页共用共享响应式 store，由 launch.js 写入）
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
      if (typeof terracottaHostStep1 === 'function') terracottaHostStep1();
    },
    join() {
      if (typeof terracottaJoinStep1 === 'function') terracottaJoinStep1();
    }
  },
  template: `
          <!-- 首次使用引导（核心未安装时显示，微软风格引导动画） -->
          <div id="terracotta-onboarding" class="terracotta-ob" style="display:none">
            <div class="terracotta-ob-icon">
              <img src="img/terracotta.ico" class="terracotta-ob-logo" alt="陶瓦联机">
            </div>
            <h2 class="terracotta-ob-title" id="terracotta-ob-title">你好！欢迎使用陶瓦联机</h2>
            <p class="terracotta-ob-desc" id="terracotta-ob-desc">在首次使用前，请先下载核心</p>
            <button class="terracotta-ob-dl" id="terracotta-ob-dl" onclick="terracottaDownloadCore()" title="下载陶瓦联机核心">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
            </button>
            <div class="terracotta-ob-spinner" id="terracotta-ob-spinner" style="display:none">
              <div class="spinner" role="status"></div>
              <span class="terracotta-ob-progress-text" id="terracotta-ob-progress-text">正在下载核心...</span>
            </div>
            <div class="terracotta-ob-done" id="terracotta-ob-done" style="display:none">
              <svg class="terracotta-ob-check" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>
              <h2>祝你使用愉快！</h2>
            </div>
          </div>

          <div id="terracotta-main">
          <div class="page-header">
            <h2>陶瓦联机</h2>
            <p class="page-subtitle">基于 Terracotta 的 P2P 虚拟组网，无需公网 IP 即可联机</p>
          </div>
          <div class="lan-container">

            <!-- 主界面：运行中实例 + 操作按钮 -->
            <div id="terracotta-home" class="terracotta-home">
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
                <button class="btn btn-secondary btn-lg" @click="join()" title="加入房间">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:16px;height:16px"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>
                  加入房间
                </button>
              </div>
            </div>

            <!-- 创建房间流程 -->
            <div id="terracotta-host-panel" class="terracotta-flow" style="display:none">
              <div id="terracotta-host-step1" class="terracotta-step">
                <h3>启动游戏并开放局域网</h3>
                <p class="terracotta-step-desc">1. 请先启动游戏并进入存档<br>2. 按 Esc 打开菜单，点击「对局域网开放」<br>3. 将端口设置为 <b>25565</b></p>
                <div class="terracotta-port-hint">端口号：25565</div>
                <button class="terracotta-next-btn" id="terracotta-host-next" onclick="terracottaHostNext()" disabled title="请先启动游戏并开放局域网">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
                </button>
              </div>
              <div id="terracotta-host-step2" class="terracotta-step" style="display:none">
                <div class="spinner" role="status"></div>
                <p class="terracotta-step-text">正在创建隧道连接中...</p>
              </div>
              <div id="terracotta-host-step3" class="terracotta-step" style="display:none">
                <div class="terracotta-step-bigicon">
                  <svg class="terracotta-ob-check" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>
                </div>
                <h3>隧道连接已建立</h3>
                <div class="terracotta-info-row">
                  <span class="terracotta-info-label">房间码</span>
                  <span class="terracotta-info-value" id="terracotta-roomcode">--</span>
                  <button class="terracotta-copy-btn" onclick="terracottaCopyRoomCode()" title="复制房间码">复制</button>
                </div>
                <div class="terracotta-info-row">
                  <span class="terracotta-info-label">连接地址</span>
                  <span class="terracotta-info-value" id="terracotta-connect-addr">--</span>
                  <button class="terracotta-copy-btn" onclick="terracottaCopyAddr()" title="复制连接地址">复制</button>
                </div>
                <button class="terracotta-close-btn" onclick="terracottaDisconnect()" title="关闭隧道">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
                </button>
              </div>
            </div>

            <!-- 加入房间流程 -->
            <div id="terracotta-join-panel" class="terracotta-flow" style="display:none">
              <div id="terracotta-join-step1" class="terracotta-step">
                <h3>加入房间</h3>
                <p class="terracotta-step-desc">请粘贴房主发给你的房间码</p>
                <textarea id="terracotta-join-code" class="terracotta-join-textarea" placeholder="粘贴房主发来的房间码..." oninput="terracottaJoinCodeInput(this)"></textarea>
                <button class="terracotta-next-btn" id="terracotta-join-next" onclick="terracottaJoinNext()" disabled title="请先输入房间码">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
                </button>
              </div>
              <div id="terracotta-join-step2" class="terracotta-step" style="display:none">
                <div class="spinner" role="status"></div>
                <p class="terracotta-step-text">正在连接房间...</p>
              </div>
              <div id="terracotta-join-step3" class="terracotta-step" style="display:none">
                <div class="terracotta-step-bigicon">
                  <svg class="terracotta-ob-check" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>
                </div>
                <h3>连接成功</h3>
                <p class="terracotta-step-desc">启动游戏后，在多人游戏的「局域网联机」板块找到房间即可进入</p>
                <button class="terracotta-close-btn" onclick="terracottaDisconnect()" title="退出房间">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"/><polyline points="16 17 21 12 16 7"/><line x1="21" y1="12" x2="9" y2="12"/></svg>
                </button>
              </div>
            </div>

            <div class="lan-hint-bar" id="terracotta-hint" style="display:none"></div>
          </div>
          </div>
  `
};

window.VersePC = window.VersePC || {};
window.VersePC.PageLanTerracotta = PageLanTerracotta;
