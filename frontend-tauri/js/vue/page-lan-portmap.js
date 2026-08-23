/* page-lan-portmap.js - 红石联机页 Vue 组件
 * 改造自原端口映射页，对接红石联机（RedstoneOnline）服务端 API。
 *
 * UI 布局对齐红石联机模组游戏内 GUI（RedstoneScreen.java）：
 *   三级标签页：联机（主操作）/ 日志 / 服务器
 *   联机标签页居中紧凑布局：服务器循环选择 → 最大人数 → 开启/关闭隧道
 *   日志标签页：滚动日志窗口
 *   服务器标签页：API Key 管理 + 节点列表
 *
 * 所有网络操作通过主进程 IPC 完成（electronAPI.redstoneOnline.*）。
 */
const PageLanPortmap = {
  template: `
    <div class="page-header">
      <h2>
        <img src="img/RedstoneBlock.png" alt="" style="width:24px;height:24px;margin-right:8px;vertical-align:-5px;image-rendering:pixelated">
        红石联机
      </h2>
      <p class="page-subtitle">基于 frp 的内网穿透，一键开启外网联机</p>
    </div>

    <!-- 三级标签页 -->
    <div class="lan-tabs redstone-tabs" style="max-width:480px;margin:0 auto 20px">
      <button class="lan-tab active" data-redstone-tab="connect" onclick="redstoneSwitchTab('connect')">联机</button>
      <button class="lan-tab" data-redstone-tab="log" onclick="redstoneSwitchTab('log')">日志</button>
      <button class="lan-tab" data-redstone-tab="server" onclick="redstoneSwitchTab('server')">服务器</button>
    </div>

    <!-- ===== 联机标签页 ===== -->
    <div id="redstone-tab-connect" class="redstone-tab-content" style="display:block">
      <div style="max-width:560px;margin:0 auto;display:flex;gap:24px;align-items:flex-start">

        <!-- 左侧：主操作区 -->
        <div style="flex:1;min-width:0">
          <!-- 连接状态条 -->
          <div style="display:flex;align-items:center;gap:10px;margin-bottom:20px;padding:10px 16px;background:var(--bg-secondary);border:1px solid var(--border);border-radius:10px;">
            <span id="redstone-status-dot" class="lan-status-dot disconnected" style="margin:0!important;flex-shrink:0"></span>
            <span id="redstone-status-text" style="flex:1;font-size:14px;color:var(--text-primary);font-weight:500">未连接</span>
          </div>

          <!-- 控制卡片 -->
          <div style="background:var(--bg-secondary);border:1px solid var(--border);border-radius:12px;padding:20px;">
            <!-- 当前版本（自动读取） -->
            <div style="margin-bottom:14px">
              <div style="font-size:12px;font-weight:600;color:var(--text-muted);margin-bottom:6px;text-transform:uppercase;letter-spacing:0.05em">当前版本</div>
              <div id="redstone-current-version" style="width:100%;padding:10px 14px;border:1px solid var(--border);border-radius:8px;background:var(--bg-primary);color:var(--text-muted);font-size:14px;box-sizing:border-box">读取中...</div>
            </div>

            <!-- 服务器循环按钮 -->
            <div style="margin-bottom:14px">
              <div style="font-size:12px;font-weight:600;color:var(--text-muted);margin-bottom:6px;text-transform:uppercase;letter-spacing:0.05em">服务器节点</div>
              <button id="redstone-server-btn" onclick="redstoneCycleServer()" style="width:100%;padding:10px 14px;border:1px solid var(--border);border-radius:8px;background:var(--bg-primary);color:var(--text-primary);font-size:14px;text-align:center;cursor:default;transition:border-color 0.2s" onmouseover="this.style.borderColor='var(--accent)'" onmouseout="this.style.borderColor=''">
                服务器: 加载中...
              </button>
            </div>

            <!-- 最大人数 -->
            <div class="rs-form-row">
              <div class="rs-title-row" id="redstone-max-row">
                <div class="rs-switch-label">
                  <span>最大人数</span>
                </div>
                <div class="rs-title-input-wrap">
                  <input type="number" id="redstone-max-players" min="0" max="99" value="8" style="width:90px">
                  <span class="rs-title-count">人</span>
                </div>
              </div>
              <div class="rs-form-desc">本次联机允许的最大玩家数；0 表示不限制</div>
            </div>

            <!-- 开启/关闭隧道按钮 -->
            <button id="redstone-action-btn" onclick="redstoneToggle()" class="btn btn-primary" style="width:100%;justify-content:center;margin-top:16px;padding:12px 0;font-size:15px;font-weight:600;border-radius:10px">开启隧道</button>
          </div>

          <!-- 使用说明 -->
          <div style="margin-top:16px;padding:14px 16px;background:var(--bg-secondary);border:1px solid var(--border);border-radius:10px;">
            <div style="font-size:12px;font-weight:600;color:var(--text-muted);margin-bottom:8px;text-transform:uppercase;letter-spacing:0.05em">使用说明</div>
            <ol style="margin:0;padding-left:18px;font-size:13px;color:var(--text-secondary);line-height:1.8">
              <li>启动 Minecraft 并进入存档</li>
              <li>按 ESC → 对局域网开放（建议固定端口 25565）</li>
              <li>在本页选择服务器，点击<strong>"开启隧道"</strong></li>
              <li>联机地址<strong>自动复制到剪贴板</strong>，发给朋友即可加入</li>
              <li>单隧道带宽上限 4 Mbps，约支持 5-8 人游玩</li>
            </ol>
          </div>
        </div>

        <!-- 右侧：已连接信息区 -->
        <div id="redstone-connected-info" style="display:none;width:240px;flex-shrink:0">
          <div style="background:var(--bg-secondary);border:1px solid rgba(16,185,129,0.3);border-radius:12px;padding:20px;text-align:center;">
            <div style="width:48px;height:48px;border-radius:50%;background:rgba(16,185,129,0.15);display:flex;align-items:center;justify-content:center;margin:0 auto 12px">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:24px;height:24px;color:var(--green)"><polyline points="20 6 9 17 4 12"/></svg>
            </div>
            <div style="font-size:11px;color:var(--text-muted);margin-bottom:4px">联机地址</div>
            <div id="redstone-room-addr" style="font-family:monospace;font-size:18px;font-weight:700;color:var(--green);margin:4px 0 12px;word-break:break-all">--</div>
            <div style="font-size:11px;color:var(--text-muted);margin-bottom:12px;padding:4px 8px;background:rgba(16,185,129,0.08);border-radius:6px">已复制到剪贴板</div>
            <button class="btn btn-secondary btn-sm" onclick="redstoneCopyAddr()" style="width:100%;justify-content:center">重新复制</button>
          </div>
        </div>
      </div>
    </div>

    <!-- ===== 日志标签页 ===== -->
    <div id="redstone-tab-log" class="redstone-tab-content" style="display:none">
      <div style="max-width:640px;margin:0 auto;padding:0 16px">
        <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:12px">
          <div>
            <div style="font-size:14px;font-weight:600;color:var(--text-primary)">连接日志</div>
            <div style="font-size:12px;color:var(--text-muted);margin-top:2px">实时记录隧道连接状态</div>
          </div>
          <button class="btn btn-secondary btn-sm" onclick="document.getElementById('redstone-room-log').textContent='';addRedstoneLog('日志已清空')">清空日志</button>
        </div>
        <div style="background:var(--bg-secondary);border:1px solid var(--border);border-radius:10px;padding:16px;max-height:360px;min-height:160px;overflow-y:auto;font-family:monospace;font-size:12px;line-height:1.8;white-space:pre-wrap;color:var(--text-primary)" id="redstone-room-log"></div>
      </div>
    </div>

    <!-- ===== 服务器标签页 ===== -->
    <div id="redstone-tab-server" class="redstone-tab-content" style="display:none">
      <div style="max-width:520px;margin:0 auto;padding:0 16px">
        <!-- 服务器节点 -->
        <div style="background:var(--bg-secondary);border:1px solid var(--border);border-radius:12px;padding:20px">
          <div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:12px">
            <div>
              <div style="font-size:14px;font-weight:600;color:var(--text-primary)">服务器节点</div>
              <div style="font-size:12px;color:var(--text-muted);margin-top:2px" id="redstone-server-info">正在加载节点列表...</div>
            </div>
            <button class="btn btn-secondary btn-sm" onclick="redstoneRefreshServers()">刷新列表</button>
          </div>
          <div style="font-size:13px;color:var(--text-secondary);padding:8px 0" id="redstone-server-nodes">
            仅上海节点可用
          </div>
        </div>
      </div>
    </div>
  `
};

window.VersePC = window.VersePC || {};
window.VersePC.PageLanPortmap = PageLanPortmap;
