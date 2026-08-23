/* page-lan-terracotta.js - 青陶内网穿透页 Vue 组件（渐进式改造）
 * 原则：
 *   1. CSS 一行不动（class 名保留原样）
 *   2. HTML 结构原样搬运（标签、层级、id 全部不变）
 *   3. JS 函数全部复用（来自 js/app/*.js 的全局函数）
 */
const PageLanTerracotta = {
  template: `
          <div class="page-header">
            <h2>陶瓦联机</h2>
            <p class="page-subtitle">基于 Terracotta 的 P2P 虚拟组网，无需公网 IP 即可联机</p>
          </div>
          <div class="lan-container">
            <div class="lan-status-card" id="terracotta-status-card">
              <div class="lan-status-dot" id="terracotta-status-dot"></div>
              <div class="lan-status-info">
                <div class="lan-status-title" id="terracotta-status-title">未连接</div>
                <div class="lan-status-desc" id="terracotta-status-desc">创建房间或输入房间码加入</div>
              </div>
            </div>
            <div class="lan-tabs" id="terracotta-tabs">
              <button class="lan-tab active" data-panel="terracotta-host-panel" onclick="switchLanTab('terracotta', 'host', this)">创建房间</button>
              <button class="lan-tab" data-panel="terracotta-join-panel" onclick="switchLanTab('terracotta', 'join', this)">加入房间</button>
            </div>
            <div class="lan-room-panel" id="terracotta-host-panel">
              <div class="lan-room-steps">
                <div class="lan-room-steps-title">使用步骤</div>
                <ol>
                  <li>启动游戏并进入存档</li>
                  <li>按 Esc 打开菜单，点击"对局域网开放"</li>
                  <li>记下游戏界面的端口号，填入下方</li>
                </ol>
              </div>
              <div class="lan-create-form">
                <div class="lan-create-field">
                  <label>游戏端口</label>
                  <input type="number" id="terracotta-host-port" value="25565" class="lan-join-input" placeholder="局域网开放后显示的端口">
                </div>
                <button class="btn btn-primary btn-lg" onclick="terracottaStartHost()" style="width:100%;justify-content:center">开始创建房间</button>
              </div>
            </div>
            <div class="lan-room-panel" id="terracotta-join-panel" style="display:none">
              <div class="lan-create-form">
                <div class="lan-create-field">
                  <label>房间码</label>
                  <textarea id="terracotta-join-code" placeholder="粘贴房主发来的房间码..." class="lan-join-input lan-join-textarea"></textarea>
                </div>
                <button class="btn btn-primary btn-lg" onclick="terracottaJoinRoom()" style="width:100%;justify-content:center">加入房间</button>
              </div>
            </div>
            <div class="lan-room-panel" id="terracotta-connected" style="display:none">
              <div class="lan-room-header">
                <h3 id="terracotta-connected-title">连接中...</h3>
                <button class="btn btn-danger btn-sm" onclick="terracottaDisconnect()">断开连接</button>
              </div>
              <div class="lan-connected-body">
                <div class="lan-roomcode-display" id="terracotta-roomcode-field">
                  <label>房间码</label>
                  <div class="lan-roomcode-value" id="terracotta-roomcode">--</div>
                  <button class="btn btn-secondary btn-sm" onclick="terracottaCopyRoomCode()">复制</button>
                </div>
                <div class="lan-connected-fields">
                  <div class="lan-room-field" id="terracotta-addr-field" style="display:none">
                    <label>连接地址</label>
                    <div class="lan-room-value" id="terracotta-connect-addr">--</div>
                    <button class="btn btn-secondary btn-sm" onclick="terracottaCopyAddr()">复制</button>
                  </div>
                  <div class="lan-room-field">
                    <label>连接状态</label>
                    <div class="lan-room-value" id="terracotta-conn-status">等待连接...</div>
                  </div>
                </div>
              </div>
              <div class="lan-hint-bar" id="terracotta-hint">
                正在建立 P2P 连接，请稍候...
              </div>
            </div>
          </div>
  `
};

window.VersePC = window.VersePC || {};
window.VersePC.PageLanTerracotta = PageLanTerracotta;
