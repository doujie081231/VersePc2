/* page-server-host.js - 开服页 Vue 组件 */
const PageServerHost = {
  template: `
    <div class="page-header">
      <div class="page-header-text">
        <h2>
          本地开服
          <span class="sh-page-badge">实验功能</span>
        </h2>
        <p class="page-subtitle">基于已安装版本创建 Minecraft 服务端，支持原版 / Forge / Fabric / NeoForge</p>
      </div>
      <div class="page-actions">
        <button class="btn btn-ghost btn-sm" onclick="serverHostOpenFolder()">打开目录</button>
      </div>
    </div>

    <div class="settings-container" id="server-host-container">
      <!-- 创建服务端 -->
      <div class="card sh-create-card">
        <div class="sh-create-header">
          <h3>创建服务端</h3>
          <p class="form-hint">选择本地版本并填写基础信息，即可生成独立服务端实例</p>
        </div>

        <div class="sh-create-form">
          <div class="form-group sh-version-group">
            <label class="filter-label">游戏版本</label>
            <div class="custom-select" id="server-host-version-wrapper">
              <div class="custom-select-trigger" id="server-host-version-trigger">
                <span class="custom-select-value placeholder" id="server-host-version-value">加载中...</span>
                <svg class="custom-select-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="6 9 12 15 18 9"></polyline></svg>
              </div>
              <div class="custom-select-dropdown" id="server-host-version-dropdown">
                <div class="custom-select-options" id="server-host-version-options"></div>
              </div>
            </div>
            <span class="form-hint" id="server-host-loader-hint">选择本地版本后将自动识别加载器类型。</span>
          </div>

          <div class="sh-form-row">
            <div class="form-group">
              <label class="filter-label">服务器名称</label>
              <input type="text" id="server-host-name" class="text-input" placeholder="MyServer" value="MyServer" />
            </div>
            <div class="form-group">
              <label class="filter-label">端口</label>
              <input type="number" id="server-host-port" class="text-input" min="1" max="65535" value="25565" />
            </div>
            <div class="form-group">
              <label class="filter-label">最大内存 (MB)</label>
              <input type="number" id="server-host-mem" class="text-input" min="512" max="32768" step="256" value="2048" />
            </div>
          </div>

          <div class="sh-options-row">
            <label class="checkbox-label">
              <input type="checkbox" id="server-host-online" checked />
              <span>正版验证 (online-mode)</span>
            </label>
            <label class="checkbox-label">
              <input type="checkbox" id="server-host-sync-mods" checked />
              <span>一键同步客户端模组</span>
            </label>
          </div>
        </div>

        <div class="sh-create-actions">
          <button class="btn btn-primary" id="server-host-create-btn" onclick="serverHostCreate()">创建并准备服务端</button>
          <button class="btn btn-ghost" id="server-host-cancel-btn" onclick="serverHostCancelCreate()">取消创建</button>
          <button class="btn btn-ghost" onclick="serverHostRefreshVersions()">刷新版本</button>
        </div>

        <div class="sh-progress" id="server-host-progress" aria-hidden="true">
          <div class="sh-progress-bar" id="server-host-progress-bar"></div>
        </div>
        <div class="sh-progress-meta" id="server-host-progress-meta">
          <span id="server-host-progress-text"></span>
          <span id="server-host-progress-pct"></span>
        </div>
        <p class="form-hint" id="server-host-status">进入页面后会自动加载已安装版本。</p>
      </div>

      <!-- 服务器列表 + 控制台 -->
      <div class="sh-main-grid">
        <div class="card sh-list-card">
          <div class="sh-list-header">
            <h3>我的服务器</h3>
            <button class="btn btn-ghost btn-sm" onclick="serverHostRefreshList()">刷新</button>
          </div>
          <div id="server-host-list" class="sh-list"></div>
        </div>

        <div class="card sh-console-card">
          <div class="sh-console-header">
            <div>
              <div class="sh-console-title">
                <span id="server-host-active-title">未选择</span>
                <span id="server-host-active-loader"></span>
              </div>
              <p class="form-hint" id="server-host-active-msg">选择服务器后可启动并查看控制台</p>
            </div>
            <div id="server-host-active-badge"></div>
          </div>

          <div class="sh-console-toolbar">
            <button class="btn btn-primary btn-sm" id="server-host-start-btn" onclick="serverHostStart()">启动</button>
            <button class="btn btn-ghost btn-sm" id="server-host-stop-btn" onclick="serverHostStop()" disabled>停止</button>
            <button class="btn btn-ghost btn-sm" id="server-host-sync-btn" onclick="serverHostSyncMods()">同步模组</button>
            <button class="btn btn-ghost btn-sm" onclick="serverHostClearConsole()">清屏</button>
            <button class="btn btn-ghost btn-sm sh-btn-delete" onclick="serverHostDelete()">删除</button>
          </div>

          <div id="server-host-console" class="sh-console">
            <div class="sh-console-line" style="opacity:.6">等待服务器输出…</div>
          </div>

          <div class="sh-cmd-row">
            <input id="server-host-cmd" class="text-input" placeholder="输入指令，例如 list / op Steve / stop，按回车发送" autocomplete="off" />
            <button class="btn btn-primary" onclick="serverHostSendCommand()">发送</button>
          </div>
        </div>
      </div>

      <!-- 底部提示 -->
      <div class="sh-tips">
        <p class="form-hint">原版会下载官方 server.jar；Forge / NeoForge / Fabric 会自动安装对应服务端并同步模组。数据保存在 <code>servers/&lt;id&gt;/</code> 目录，启动完成后控制台会显示 “Done”。</p>
      </div>
    </div>
  `,
  mounted() {
    if (typeof initServerHostPage === 'function') {
      initServerHostPage();
    }
  },
  beforeUnmount() {
    if (typeof _shDetach === 'function') {
      _shDetach();
    }
  }
};

window.VersePC = window.VersePC || {};
window.VersePC.PageServerHost = PageServerHost;
