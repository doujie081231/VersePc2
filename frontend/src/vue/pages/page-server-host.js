/* page-server-host.js - 开服页 Vue 组件 */
import { initServerHostPage, _shDetach, _shClearConsole } from '../../features/misc/server-host.js';

import { serverHostOpenFolder, serverHostCreate, serverHostCancelCreate, serverHostRefreshVersions, serverHostRefreshList, serverHostStart, serverHostStop, serverHostSyncMods, _shClearConsole as serverHostClearConsole, serverHostDelete, serverHostSendCommand } from '../../features/misc/server-host.js';
import VButton from '@/components/verse/VButton.vue';
import VInput from '@/components/verse/VInput.vue';

const PageServerHost = {
  components: { VButton, VInput },
  setup() {
    return { serverHostOpenFolder, serverHostCreate, serverHostCancelCreate, serverHostRefreshVersions, serverHostRefreshList, serverHostStart, serverHostStop, serverHostSyncMods, serverHostClearConsole, serverHostDelete, serverHostSendCommand };
  },
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
        <VButton variant="ghost" size="sm" @click="serverHostOpenFolder()">打开目录</VButton>
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
              <VInput type="text" id="server-host-name" placeholder="MyServer" default-value="MyServer" />
            </div>
            <div class="form-group">
              <label class="filter-label">端口</label>
              <VInput type="number" id="server-host-port" min="1" max="65535" default-value="25565" />
            </div>
            <div class="form-group">
              <label class="filter-label">最大内存 (MB)</label>
              <VInput type="number" id="server-host-mem" min="512" max="32768" step="256" default-value="2048" />
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
          <VButton variant="accent" id="server-host-create-btn" @click="serverHostCreate()">创建并准备服务端</VButton>
          <VButton variant="ghost" id="server-host-cancel-btn" @click="serverHostCancelCreate()">取消创建</VButton>
          <VButton variant="ghost" @click="serverHostRefreshVersions(true)">刷新版本</VButton>
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
            <VButton variant="ghost" size="sm" @click="serverHostRefreshList()">刷新</VButton>
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
            <VButton variant="accent" size="sm" id="server-host-start-btn" @click="serverHostStart()">启动</VButton>
            <VButton variant="ghost" size="sm" id="server-host-stop-btn" @click="serverHostStop()" disabled>停止</VButton>
            <VButton variant="ghost" size="sm" id="server-host-sync-btn" @click="serverHostSyncMods()">同步模组</VButton>
            <VButton variant="ghost" size="sm" @click="serverHostClearConsole()">清屏</VButton>
            <VButton variant="ghost" size="sm" class="sh-btn-delete" @click="serverHostDelete()">删除</VButton>
          </div>

          <div id="server-host-console" class="sh-console">
            <div class="sh-console-line" style="opacity:.6">等待服务器输出…</div>
          </div>

          <div class="sh-cmd-row">
            <VInput id="server-host-cmd" class="font-mono" placeholder="输入指令，例如 list / op Steve / stop，按回车发送" autocomplete="off" />
            <VButton variant="accent" @click="serverHostSendCommand()">发送</VButton>
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

export default PageServerHost;
