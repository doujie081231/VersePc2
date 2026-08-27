/* page-lan-enderlink.js - EnderLink联机页 Vue 组件
 * 对接 lytapi.asia 联机大厅与 frp 内网穿透(frpc)
 * 三级标签页：联机 / 大厅 / 节点
 */
const PageLanEnderlink = {
  template: `
    <div class="page-header">
      <h2>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" style="width:24px;height:24px;margin-right:8px;vertical-align:-5px" stroke-linecap="round" stroke-linejoin="round"><path d="M14 3l8 8-8 8-2-2 6-6-6-6z"/><path d="M10 21l-8-8 8-8 2 2-6 6 6 6z"/></svg>
        EnderLink联机
      </h2>
      <p class="page-subtitle">基于 lytapi 大厅 + frp 内网穿透，一键开启外网联机</p>
    </div>

    <div class="lan-tabs enderlink-tabs" style="max-width:480px;margin:0 auto 20px">
      <button class="lan-tab active" data-enderlink-tab="connect" onclick="enderlinkSwitchTab('connect')">联机</button>
      <button class="lan-tab" data-enderlink-tab="hall" onclick="enderlinkSwitchTab('hall')">大厅</button>
      <button class="lan-tab" data-enderlink-tab="log" onclick="enderlinkSwitchTab('log')">日志</button>
      <button class="lan-tab" data-enderlink-tab="node" onclick="enderlinkSwitchTab('node')">节点</button>
    </div>

    <div id="enderlink-tab-connect" class="enderlink-tab-content" style="display:block">
      <div style="max-width:560px;margin:0 auto;display:flex;gap:24px;align-items:flex-start">
        <div style="flex:1;min-width:0">
          <div style="display:flex;align-items:center;gap:10px;margin-bottom:20px;padding:10px 16px;background:var(--bg-secondary);border:1px solid var(--border);border-radius:10px;">
            <span id="enderlink-status-dot" class="lan-status-dot disconnected" style="margin:0!important;flex-shrink:0"></span>
            <span id="enderlink-status-text" style="flex:1;font-size:14px;color:var(--text-primary);font-weight:500">未连接</span>
          </div>

          <div style="background:var(--bg-secondary);border:1px solid var(--border);border-radius:12px;padding:20px;">
            <div style="margin-bottom:14px">
              <div style="font-size:12px;font-weight:600;color:var(--text-muted);margin-bottom:6px;text-transform:uppercase;letter-spacing:0.05em">联机节点</div>
              <button id="enderlink-node-btn" onclick="enderlinkCycleNode()" style="width:100%;padding:10px 14px;border:1px solid var(--border);border-radius:8px;background:var(--bg-primary);color:var(--text-primary);font-size:14px;text-align:center;cursor:default;transition:border-color 0.2s" onmouseover="this.style.borderColor='var(--accent)'" onmouseout="this.style.borderColor=''">
                节点: 加载中...
              </button>
            </div>

            <div class="rs-form-row">
              <div class="rs-title-row">
                <div class="rs-switch-label"><span>房间名</span></div>
                <div class="rs-title-input-wrap">
                  <input type="text" id="enderlink-room-name" maxlength="16" placeholder="可选" style="width:140px">
                </div>
              </div>
            </div>

            <div class="rs-form-row">
              <div class="rs-title-row">
                <div class="rs-switch-label"><span>公开大厅</span></div>
                <div class="rs-title-input-wrap" style="justify-content:center">
                  <span id="enderlink-public" class="toggle-switch active" onclick="this.classList.toggle('active')"></span>
                </div>
              </div>
              <div class="rs-form-desc">勾选后房间会出现在联机大厅，其他人可加入</div>
            </div>

            <button id="enderlink-action-btn" onclick="enderlinkToggle()" class="btn btn-primary" style="width:100%;justify-content:center;margin-top:16px;padding:12px 0;font-size:15px;font-weight:600;border-radius:10px">开启联机</button>
          </div>

          <div style="margin-top:16px;padding:14px 16px;background:var(--bg-secondary);border:1px solid var(--border);border-radius:10px;">
            <div style="font-size:12px;font-weight:600;color:var(--text-muted);margin-bottom:8px;text-transform:uppercase;letter-spacing:0.05em">使用说明</div>
            <ol style="margin:0;padding-left:18px;font-size:13px;color:var(--text-secondary);line-height:1.8">
              <li>启动 Minecraft 并进入存档</li>
              <li>按 ESC → 对局域网开放（建议端口 25565）</li>
              <li>选择节点，勾选公开后点击<strong>"开启联机"</strong></li>
              <li>联机地址自动复制到剪贴板，发给朋友即可加入</li>
            </ol>
          </div>
        </div>

        <div id="enderlink-connected-info" style="display:none;width:240px;flex-shrink:0">
          <div style="background:var(--bg-secondary);border:1px solid rgba(16,185,129,0.3);border-radius:12px;padding:20px;text-align:center;">
            <div style="width:48px;height:48px;border-radius:50%;background:rgba(16,185,129,0.15);display:flex;align-items:center;justify-content:center;margin:0 auto 12px">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:24px;height:24px;color:var(--green)"><polyline points="20 6 9 17 4 12"/></svg>
            </div>
            <div style="font-size:11px;color:var(--text-muted);margin-bottom:4px">联机地址</div>
            <div id="enderlink-room-addr" style="font-family:monospace;font-size:18px;font-weight:700;color:var(--green);margin:4px 0 12px;word-break:break-all">--</div>
            <div style="font-size:11px;color:var(--text-muted);margin-bottom:12px;padding:4px 8px;background:rgba(16,185,129,0.08);border-radius:6px">已复制到剪贴板</div>
            <button class="btn btn-secondary btn-sm" onclick="enderlinkCopyAddr()" style="width:100%;justify-content:center">重新复制</button>
          </div>
        </div>
      </div>
    </div>

    <div id="enderlink-tab-hall" class="enderlink-tab-content" style="display:none">
      <div style="max-width:640px;margin:0 auto;padding:0 16px">
        <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:12px">
          <div>
            <div style="font-size:14px;font-weight:600;color:var(--text-primary)">联机大厅</div>
            <div style="font-size:12px;color:var(--text-muted);margin-top:2px">公开房间列表</div>
          </div>
          <button class="btn btn-secondary btn-sm" onclick="enderlinkRefreshRooms()">刷新</button>
        </div>
        <div style="background:var(--bg-secondary);border:1px solid var(--border);border-radius:12px;overflow:hidden;">
          <div id="enderlink-hall-list" style="min-height:160px;color:var(--text-muted);font-size:13px;padding:16px">加载中...</div>
        </div>
      </div>
    </div>

    <div id="enderlink-tab-log" class="enderlink-tab-content" style="display:none">
      <div style="max-width:640px;margin:0 auto;padding:0 16px">
        <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:12px">
          <div>
            <div style="font-size:14px;font-weight:600;color:var(--text-primary)">连接日志</div>
            <div style="font-size:12px;color:var(--text-muted);margin-top:2px">实时记录联机连接状态</div>
          </div>
          <button class="btn btn-secondary btn-sm" onclick="document.getElementById('enderlink-room-log').textContent='';addEnderlinkLog('日志已清空')">清空日志</button>
        </div>
        <div style="background:var(--bg-secondary);border:1px solid var(--border);border-radius:10px;padding:16px;max-height:360px;min-height:160px;overflow-y:auto;font-family:monospace;font-size:12px;line-height:1.8;white-space:pre-wrap;color:var(--text-primary)" id="enderlink-room-log"></div>
      </div>
    </div>

    <div id="enderlink-tab-node" class="enderlink-tab-content" style="display:none">
      <div style="max-width:560px;margin:0 auto;padding:0 16px">
        <div style="background:var(--bg-secondary);border:1px solid var(--border);border-radius:12px;padding:20px">
          <div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:12px">
            <div>
              <div style="font-size:14px;font-weight:600;color:var(--text-primary)">frp 节点</div>
              <div style="font-size:12px;color:var(--text-muted);margin-top:2px" id="enderlink-node-info">正在加载节点列表...</div>
            </div>
            <button class="btn btn-secondary btn-sm" onclick="enderlinkRefreshNodes()">刷新列表</button>
          </div>
          <div style="font-size:13px;color:var(--text-secondary);padding:8px 0;line-height:1.8" id="enderlink-node-list">
            加载中...
          </div>
        </div>
      </div>
    </div>
  `
};

window.VersePC = window.VersePC || {};
window.VersePC.PageLanEnderlink = PageLanEnderlink;