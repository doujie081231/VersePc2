/* page-version-settings.js - 版本设置页 Vue 组件（渐进式改造）
 * 原则：
 *   1. CSS 一行不动（class 名保留原样）
 *   2. HTML 结构原样搬运（标签、层级、id 全部不变）
 *   3. JS 函数全部复用（来自 js/app/*.js 的全局函数）
 */
const PageVersionSettings = {
  template: `
        <div class="vset-header">
          <button class="btn btn-icon" onclick="closeVersionSettings()">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:20px;height:20px"><path d="M15 18l-6-6 6-6"/></svg>
          </button>
          <span class="vset-title" id="vset-title">版本设置</span>
          <button class="btn btn-icon" onclick="closeVersionSettings()">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:20px;height:20px"><path d="M6 18L18 6M6 6l12 12"/></svg>
          </button>
        </div>
        <div class="vset-body">
          <div class="vset-sidebar">
            <button class="vset-nav-item active" data-tab="overview" onclick="switchVSetTab('overview')">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="square" stroke-linejoin="miter"><rect x="3" y="3" width="8" height="8"/><rect x="13" y="3" width="8" height="8"/><rect x="3" y="13" width="8" height="8"/><rect x="13" y="13" width="8" height="8"/></svg>
              <span>概览</span>
            </button>
            <button class="vset-nav-item" data-tab="settings" onclick="switchVSetTab('settings')">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="square" stroke-linejoin="miter"><circle cx="12" cy="12" r="3"/><path d="M12 2V5"/><path d="M12 19V22"/><path d="M2 12H5"/><path d="M19 12H22"/><path d="M4.93 4.93L7.05 7.05"/><path d="M16.95 16.95L19.07 19.07"/><path d="M4.93 19.07L7.05 16.95"/><path d="M16.95 7.05L19.07 4.93"/></svg>
              <span>设置</span>
            </button>
            <button class="vset-nav-item" data-tab="modmgr" onclick="switchVSetTab('modmgr')">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="square" stroke-linejoin="miter"><path d="M12 2L21 7V17L12 22L3 17V7L12 2Z"/><path d="M12 8L16 10.5V15.5L12 18L8 15.5V10.5L12 8Z"/></svg>
              <span>模组</span>
            </button>
            <button class="vset-nav-item" data-tab="export" onclick="switchVSetTab('export')">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="square" stroke-linejoin="miter"><rect x="4" y="3" width="16" height="3" rx="0"/><rect x="4" y="18" width="16" height="3" rx="0"/><path d="M12 6V15"/><path d="M8 12L12 16L16 12"/></svg>
              <span>导出</span>
            </button>
          </div>
          <div class="vset-content">
            <!-- 概览 -->
            <div class="vset-panel active" id="vset-panel-overview">
              <div id="vset-external-info" style="display:none;margin-bottom:16px"></div>
              <div class="vset-section">
                <div class="vset-section-title">快捷方式</div>
                <div class="vset-btn-group">
                  <button class="btn btn-secondary btn-sm" onclick="openVersionFolder()">版本文件夹</button>
                  <button class="btn btn-secondary btn-sm" onclick="openSavesFolder()">存档文件夹</button>
                  <button class="btn btn-secondary btn-sm" onclick="openModsFolder()">Mod文件夹</button>
                </div>
              </div>
              <div class="vset-section">
                <div class="vset-section-title">高级管理</div>
                <div class="vset-btn-group">
                  <button class="btn btn-secondary btn-sm win-only" onclick="exportLaunchScript()">导出启动脚本</button>
                  <button class="btn btn-secondary btn-sm" onclick="repairFiles()">补全文件</button>
                  <button class="btn btn-danger btn-sm" onclick="deleteCurrentVersion()">删除版本</button>
                </div>
              </div>
            </div>
            <!-- 设置 -->
            <div class="vset-panel" id="vset-panel-settings">
              <div class="vset-section">
                <div class="vset-section-title">版本信息</div>
                <div class="vset-form-row">
                  <label class="vset-label">自定义版本名</label>
                  <input type="text" class="vset-input" id="vset-custom-name" placeholder="留空使用默认名称" oninput="saveCurrentVersionSetting('customName', this.value);refreshVersionDisplayName()">
                </div>
                <div class="vset-form-row">
                  <label class="vset-label">版本描述</label>
                  <input type="text" class="vset-input" id="vset-description" placeholder="留空使用默认描述" oninput="saveCurrentVersionSetting('description', this.value)">
                </div>
              </div>
              <div class="vset-section">
                <div class="vset-section-title">启动选项</div>
                <div class="vset-form-row">
                  <label class="vset-label">版本隔离</label>
                  <div class="custom-select custom-select-sm" id="vset-isolation-wrapper">
                    <div class="custom-select-trigger" id="vset-isolation-trigger">
                      <span class="custom-select-value" id="vset-isolation-value">跟随全局设置</span>
                      <svg class="custom-select-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="6 9 12 15 18 9"></polyline></svg>
                    </div>
                    <div class="custom-select-dropdown" id="vset-isolation-dropdown">
                      <div class="custom-select-options" id="vset-isolation-options"></div>
                    </div>
                  </div>
                </div>
                <div class="vset-form-row">
                  <label class="vset-label">游戏窗口标题</label>
                  <input type="text" class="vset-input" id="vset-window-title" placeholder="跟随全局设置">
                </div>
                <div class="vset-form-row">
                  <label class="vset-label">自定义信息</label>
                  <input type="text" class="vset-input" id="vset-custom-info" placeholder="跟随全局设置">
                </div>
                <div class="vset-form-row">
                  <label class="vset-label">游戏 Java</label>
                  <div style="display:flex;gap:8px;align-items:center;width:100%;">
                    <div class="custom-select custom-select-sm" id="vset-java-wrapper" style="flex:1;">
                      <div class="custom-select-trigger" id="vset-java-trigger">
                        <span class="custom-select-value" id="vset-java-value">跟随全局设置</span>
                        <svg class="custom-select-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="6 9 12 15 18 9"></polyline></svg>
                      </div>
                      <div class="custom-select-dropdown" id="vset-java-dropdown">
                        <div class="custom-select-options" id="vset-java-options"></div>
                      </div>
                    </div>
                    <button class="btn btn-secondary btn-sm" onclick="vsetDetectJava()" title="自动搜索">
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><circle cx="11" cy="11" r="8"/><path d="M21 21l-4.35-4.35"/></svg>
                      自动搜索
                    </button>
                    <button class="btn btn-secondary btn-sm" onclick="vsetBrowseJava()" title="手动导入">
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
                      手动导入
                    </button>
                  </div>
                </div>
              </div>
              <div class="vset-section">
                <div class="vset-section-title">内存分配</div>
                <div class="vset-radio-group" id="vset-memory-mode">
                  <label class="vset-radio"><input type="radio" name="vsetMemoryMode" value="global" checked><span>跟随全局设置</span></label>
                  <label class="vset-radio"><input type="radio" name="vsetMemoryMode" value="auto"><span>自动配置</span></label>
                  <label class="vset-radio"><input type="radio" name="vsetMemoryMode" value="custom"><span>自定义</span></label>
                </div>
                <div class="vset-slider-wrap" id="vset-memory-custom" style="display:none">
                  <input type="range" class="vset-slider" id="vset-memory-value" min="512" max="16384" step="256" value="4096">
                  <div class="vset-slider-info">
                    <span id="vset-memory-display">4096 MB</span>
                    <span style="color:var(--text-muted);font-size:12px">已用内存: <span id="vset-used-memory">--</span></span>
                    <span style="color:var(--text-muted);font-size:12px">游戏分配: <span id="vset-game-memory">--</span></span>
                  </div>
                </div>
                <div class="vset-form-row" style="margin-top:10px">
                  <label class="vset-label">启动游戏前进行内存优化</label>
                  <div class="custom-select custom-select-sm" id="vset-mem-optimize-wrapper">
                    <div class="custom-select-trigger" id="vset-mem-optimize-trigger">
                      <span class="custom-select-value" id="vset-mem-optimize-value">跟随全局设置</span>
                      <svg class="custom-select-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="6 9 12 15 18 9"></polyline></svg>
                    </div>
                    <div class="custom-select-dropdown" id="vset-mem-optimize-dropdown">
                      <div class="custom-select-options" id="vset-mem-optimize-options"></div>
                    </div>
                  </div>
                </div>
              </div>
              <div class="vset-section">
                <div class="vset-section-title">高级启动选项</div>
                <div class="vset-form-row">
                  <label class="vset-label">JVM 参数</label>
                  <input type="text" class="vset-input" id="vset-jvm-args" placeholder="跟随全局设置，如 -XX:+UseG1GC">
                </div>
                <div class="vset-form-row">
                  <label class="vset-label">游戏参数</label>
                  <input type="text" class="vset-input" id="vset-game-args" placeholder="跟随全局设置，如 --demo">
                </div>
              </div>
              <div class="vset-info-bar">这些设置只对该游戏版本生效，不影响其他版本。</div>
            </div>
            <!-- 模组 -->
            <div class="vset-panel" id="vset-panel-modmgr">
              <div class="modmgr-header-row">
                <div class="modmgr-search-box">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="modmgr-search-icon"><circle cx="11" cy="11" r="8"/><path d="M21 21l-4.35-4.35"/></svg>
                  <input type="text" class="modmgr-search-input" placeholder="搜索已安装模组..." id="modmgr-search" oninput="filterInstalledMods()">
                </div>
                <button class="btn btn-primary btn-sm" onclick="goDownloadMods()">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:14px;height:14px"><path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
                  下载模组
                </button>
              </div>
              <div class="modmgr-actions">
                <button class="btn btn-secondary btn-sm" onclick="openModsFolder()">打开文件夹</button>
                <button class="btn btn-secondary btn-sm" onclick="checkModUpdatesForVersion()">检查更新</button>
              </div>
              <div class="modmgr-list" id="modmgr-mod-list">
                <p class="empty-text" style="padding:30px 0;text-align:center;color:var(--text-muted)">暂无模组</p>
              </div>
            </div>
            <!-- 导出 -->
            <div class="vset-panel" id="vset-panel-export">
              <div class="export-header-row">
                <div class="export-field">
                  <label class="vset-label">整合包名称</label>
                  <input type="text" class="vset-input" id="export-name" placeholder="输入整合包名称">
                </div>
                <div class="export-field">
                  <label class="vset-label">整合包版本</label>
                  <input type="text" class="vset-input" id="export-version" placeholder="1.0.0">
                </div>
              </div>
              <div class="export-header-row" style="margin-top:12px">
                <div class="export-field" style="flex:1">
                  <label class="vset-label">作者</label>
                  <input type="text" class="vset-input" id="export-author" placeholder="输入作者名称">
                </div>
              </div>
              <div class="export-field" style="margin-top:12px">
                <label class="vset-label">描述</label>
                <textarea class="vset-textarea" id="export-description" placeholder="输入整合包描述..." style="width:100%;min-height:60px;resize:vertical;padding:8px 12px;border-radius:8px;border:1px solid var(--border-color);background:var(--bg-secondary);color:var(--text-primary);font-size:13px"></textarea>
              </div>
              <div class="export-section-title">导出内容列表</div>
              <div class="export-tree" id="export-tree">
                <div class="export-tree-item expanded" onclick="toggleExportTree(this)">
                  <input type="checkbox" checked class="export-cb" data-key="game">
                  <span class="export-toggle">▾</span>
                  <span class="export-label">游戏本体</span>
                  <span class="export-desc" id="export-game-desc">正式版</span>
                  <div class="export-children">
                    <div class="export-tree-item">
                      <input type="checkbox" checked class="export-cb" data-key="game_settings">
                      <span class="export-label">游戏设置</span>
                      <span class="export-desc">options.txt</span>
                    </div>
                    <div class="export-tree-item">
                      <input type="checkbox" checked class="export-cb" data-key="servers">
                      <span class="export-label">服务器列表</span>
                      <span class="export-desc">servers.dat</span>
                    </div>
                  </div>
                </div>
                <div class="export-tree-item expanded" onclick="toggleExportTree(this)">
                  <input type="checkbox" checked class="export-cb" data-key="mods">
                  <span class="export-toggle">▾</span>
                  <span class="export-label">Mod</span>
                  <span class="export-desc" id="export-mods-desc">导组</span>
                  <div class="export-children">
                    <div class="export-tree-item">
                      <input type="checkbox" checked class="export-cb" data-key="mod_files">
                      <span class="export-label">Mod 文件</span>
                      <span class="export-desc" id="export-mod-count">0 个</span>
                    </div>
                    <div class="export-tree-item">
                      <input type="checkbox" checked class="export-cb" data-key="mod_configs">
                      <span class="export-label">Mod 配置</span>
                      <span class="export-desc">config 文件夹</span>
                    </div>
                  </div>
                </div>
                <div class="export-tree-item expanded" onclick="toggleExportTree(this)">
                  <input type="checkbox" class="export-cb" data-key="resourcepacks">
                  <span class="export-toggle">▾</span>
                  <span class="export-label">资源包</span>
                  <span class="export-desc" id="export-rp-desc">纹理包/材质包</span>
                  <div class="export-children" id="export-rp-list"></div>
                </div>
                <div class="export-tree-item" onclick="toggleExportTree(this)">
                  <input type="checkbox" class="export-cb" data-key="shaderpacks">
                  <span class="export-toggle" style="visibility:hidden">▾</span>
                  <span class="export-label">光影包</span>
                  <span class="export-desc">shaderpacks 文件夹</span>
                </div>
                <div class="export-tree-item expanded" onclick="toggleExportTree(this)">
                  <input type="checkbox" class="export-cb" data-key="saves">
                  <span class="export-toggle">▾</span>
                  <span class="export-label">存档</span>
                  <span class="export-desc" id="export-saves-desc">游戏存档</span>
                  <div class="export-children" id="export-saves-list"></div>
                </div>
                <div class="export-tree-item" onclick="toggleExportTree(this)">
                  <input type="checkbox" class="export-cb" data-key="screenshots">
                  <span class="export-toggle" style="visibility:hidden">▾</span>
                  <span class="export-label">截图</span>
                  <span class="export-desc">screenshots 文件夹</span>
                </div>
                <div class="export-tree-item expanded" onclick="toggleExportTree(this)">
                  <span class="export-toggle">▾</span>
                  <span class="export-label" style="color:var(--text-muted)">更多选项</span>
                  <div class="export-children">
                    <div class="export-tree-item">
                      <input type="checkbox" class="export-cb" data-key="defaultconfigs">
                      <span class="export-label">默认配置</span>
                      <span class="export-desc">defaultconfigs</span>
                    </div>
                    <div class="export-tree-item">
                      <input type="checkbox" class="export-cb" data-key="kubejs">
                      <span class="export-label">KubeJS</span>
                      <span class="export-desc">kubejs 脚本</span>
                    </div>
                    <div class="export-tree-item">
                      <input type="checkbox" class="export-cb" data-key="journeymap">
                      <span class="export-label">JourneyMap</span>
                      <span class="export-desc">地图数据</span>
                    </div>
                    <div class="export-tree-item">
                      <input type="checkbox" class="export-cb" data-key="waystones">
                      <span class="export-label">Waystones</span>
                      <span class="export-desc">传送点数据</span>
                    </div>
                  </div>
                </div>
              </div>
              <div class="export-footer">
                <button class="btn btn-primary btn-lg" onclick="startExport()">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:16px;height:16px;margin-right:6px"><path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>
                  开始导出
                </button>
              </div>
            </div>
          </div>
        </div>
  `
};

window.VersePC = window.VersePC || {};
window.VersePC.PageVersionSettings = PageVersionSettings;
