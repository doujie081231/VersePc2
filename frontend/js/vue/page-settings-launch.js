/* page-settings-launch.js - 启动设置页 Vue 组件（渐进式改造）
 * 原则：
 *   1. CSS 一行不动（class 名保留原样）
 *   2. HTML 结构原样搬运（标签、层级、id 全部不变）
 *   3. JS 函数全部复用（来自 js/app/*.js 的全局函数）
 */
const PageSettingsLaunch = {
  template: `
          <div class="page-header">
            <h2>启动设置</h2>
          </div>
          <div class="settings-container">
            <div class="card">
              <h3>启动选项</h3>
              <div class="form-group">
                <label>默认版本隔离</label>
                <select id="launch-version-isolation" class="select-input">
                  <option value="all">隔离所有版本</option>
                  <option value="none">不隔离</option>
                  <option value="custom">自定义隔离规则</option>
                </select>
              </div>
              <div class="form-group">
                <label>数据目录</label>
                <p class="hint" style="margin-bottom:8px;margin-top:0">启动器核心数据（版本、资源、库文件等）的存放位置，修改后需重启生效</p>
                <div style="display:flex;gap:8px;align-items:center">
                  <input type="text" id="setting-data-dir" class="text-input" style="flex:1" placeholder="加载中..." readonly>
                  <button type="button" class="btn btn-secondary btn-sm" onclick="browseDataDir()">浏览</button>
                  <button type="button" class="btn btn-secondary btn-sm" onclick="resetDataDir()">重置</button>
                </div>
              </div>
              <div class="form-group">
                <label>游戏窗口标题</label>
                <input type="text" id="launch-window-title" class="text-input" placeholder="默认">
              </div>
              <div class="form-group">
                <label>自定义信息</label>
                <input type="text" id="launch-custom-info" class="text-input" placeholder="VersePC2">
              </div>
              <div class="form-group">
                <label>启动器可见性</label>
                <select id="launcher-visibility" class="select-input">
                  <option value="keep">游戏启动后仍保持不变</option>
                  <option value="minimize">最小化到托盘</option>
                  <option value="hide">完全隐藏</option>
                  <option value="close">关闭启动器</option>
                </select>
              </div>
              <div class="form-group">
                <label class="checkbox-label">
                  <input type="checkbox" id="setting-minimize-on-game-run" checked>
                  <span>游戏运行时最小化启动器（低调模式）</span>
                </label>
                <p class="hint" style="margin-top:4px;margin-bottom:0">游戏启动 5 秒后自动最小化启动器窗口，游戏退出后自动恢复</p>
              </div>
              <div class="form-group">
                <label>进程优先级</label>
                <select id="process-priority" class="select-input">
                  <option value="low">低</option>
                  <option value="below-normal">低于正常</option>
                  <option value="normal" selected>中（平衡）</option>
                  <option value="above-normal">高于正常</option>
                  <option value="high">高</option>
                </select>
              </div>
              <div class="form-group">
                <label>游戏窗口大小</label>
                <div style="display:flex;gap:8px;align-items:center;">
                  <select id="window-size" class="select-input" style="flex:1;">
                    <option value="default">默认 (854 x 480)</option>
                    <option value="854x480">854 x 480</option>
                    <option value="1280x720">1280 x 720 (HD)</option>
                    <option value="1600x900">1600 x 900 (HD+)</option>
                    <option value="1920x1080">1920 x 1080 (Full HD)</option>
                    <option value="2560x1440">2560 x 1440 (QHD)</option>
                    <option value="3840x2160">3840 x 2160 (4K UHD)</option>
                    <option value="custom">自定义</option>
                  </select>
                </div>
                <div id="custom-window-size" style="display:none;margin-top:8px;gap:8px;align-items:center;">
                  <input type="number" id="custom-width" class="input-field" placeholder="宽度" min="320" max="7680" style="width:100px;">
                  <span style="color:var(--text-muted)">x</span>
                  <input type="number" id="custom-height" class="input-field" placeholder="高度" min="240" max="4320" style="width:100px;">
                  <span style="color:var(--text-muted);font-size:12px">(320~7680 x 240~4320)</span>
                </div>
              </div>
              <div class="form-group">
                <label class="checkbox-label">
                  <input type="checkbox" id="launch-fullscreen">
                  <span>全屏模式启动游戏</span>
                </label>
              </div>
              <div class="form-group">
                <label>游戏 Java</label>
                <div style="display: flex; gap: 8px; align-items: center;">
                  <select id="game-java-select" class="select-input" style="flex: 1;">
                    <option value="auto">自动选择合适的 Java</option>
                  </select>
                  <button class="btn btn-secondary btn-sm" onclick="detectJava()" title="自动搜索">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><circle cx="11" cy="11" r="8"/><path d="M21 21l-4.35-4.35"/></svg>
                    自动搜索
                  </button>
                  <button class="btn btn-secondary btn-sm" onclick="browseJavaPath()" title="手动导入">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
                    手动导入
                  </button>
                </div>
              </div>
            </div>

            <div class="card">
              <h3>内存分配</h3>
              <div style="margin-bottom: 12px; padding: 12px; background: var(--bg-secondary); border-radius: var(--radius);">
                <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 8px;">
                  <span style="font-size: 12px; color: var(--text-muted);">系统内存</span>
                  <span style="font-size: 14px; font-weight: 600;" id="sys-total-memory">-- GB</span>
                </div>
                <div style="width: 100%; height: 8px; background: var(--bg-tertiary); border-radius: 4px; overflow: hidden; margin-bottom: 8px;">
                  <div id="sys-memory-bar" style="height: 100%; border-radius: 4px; background: var(--accent); transition: width 0.3s; width: 0%"></div>
                </div>
                <div style="display: flex; justify-content: space-between; align-items: center;">
                  <span style="font-size: 12px; color: var(--text-muted);">已使用: <span id="sys-used-memory">--</span></span>
                  <span style="font-size: 12px; color: var(--text-muted);">可用: <span id="sys-free-memory">--</span></span>
                </div>
              </div>
              <div class="vset-radio-group">
                <label class="vset-radio">
                  <input type="radio" name="globalMemoryMode" value="auto" checked onchange="toggleMemoryMode()">
                  <span>自动配置</span>
                </label>
                <label class="vset-radio">
                  <input type="radio" name="globalMemoryMode" value="custom" onchange="toggleMemoryMode()">
                  <span>自定义</span>
                </label>
              </div>
              <div id="memory-auto-info" style="margin-top: 8px; padding: 10px 14px; background: var(--bg-secondary); border-radius: var(--radius); font-size: 13px;">
                自动分配: <span id="memory-auto-value" style="font-weight: 600; color: var(--accent);">-- GB</span>
                <span style="color: var(--text-muted); font-size: 12px; margin-left: 4px;">(系统内存 - 保留量)</span>
              </div>
              <div id="memory-custom-settings" style="display: none; margin-top: 8px;">
                <div class="vset-slider-wrap">
                  <div style="position: relative; margin-bottom: 8px;">
                    <div id="memory-slider-track" style="position: absolute; top: 50%; left: 0; right: 0; height: 6px; background: var(--bg-tertiary); border-radius: 3px; transform: translateY(-50%); pointer-events: none;"></div>
                    <div id="memory-slider-fill" style="position: absolute; top: 50%; left: 0; height: 6px; background: linear-gradient(90deg, #4ade80 0%, #fbbf24 70%, #ef4444 100%); border-radius: 3px; transform: translateY(-50%); pointer-events: none; width: 25%;"></div>
                    <div id="memory-used-marker" style="position: absolute; top: 50%; width: 2px; height: 14px; background: var(--text-primary); transform: translate(-50%, -50%); pointer-events: none; display: none; z-index: 2;"></div>
                    <div id="memory-used-label" style="position: absolute; top: -22px; transform: translateX(-50%); font-size: 11px; color: var(--text-muted); pointer-events: none; display: none; white-space: nowrap; background: var(--bg-secondary); padding: 2px 6px; border-radius: 4px;">已用</div>
                    <input type="range" id="memory-slider" min="512" max="16384" value="4096" step="256" class="vset-slider" oninput="updateMemoryDisplay()">
                  </div>
                  <div class="vset-slider-info">
                    <span style="font-size: 12px; color: var(--text-muted);">512 MB</span>
                    <span id="memory-value-display" style="font-weight: 600;">4096 MB (4.0 GB)</span>
                    <span style="font-size: 12px; color: var(--text-muted);" id="memory-slider-max">16384 MB</span>
                  </div>
                </div>
                <div id="memory-warning" style="display: none; margin-top: 6px; padding: 8px 12px; background: rgba(255,152,0,0.1); border: 1px solid rgba(255,152,0,0.2); border-radius: 6px; font-size: 12px; color: #ff9800;"></div>
              </div>
              <div style="margin-top: 12px; padding: 12px; background: var(--bg-secondary); border-radius: var(--radius); display: flex; justify-content: space-between; align-items: center;">
                <div>
                  <div style="font-size: 12px; color: var(--text-muted); margin-bottom: 4px;">游戏分配内存</div>
                  <div style="font-size: 20px; font-weight: 700; color: var(--accent);" id="allocated-memory-display">-- GB</div>
                </div>
                <div style="text-align: right;">
                  <div style="font-size: 12px; color: var(--text-muted); margin-bottom: 4px;">剩余可用</div>
                  <div style="font-size: 20px; font-weight: 700;" id="remaining-memory-display">-- GB</div>
                </div>
              </div>
            </div>

            <div class="card">
              <h3 style="display: flex; align-items: center; justify-content: space-between; cursor: pointer;" onclick="toggleAdvancedOptions()">
                <span>高级选项</span>
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="20" height="20" id="advanced-options-arrow" style="transition: transform 0.3s;"><polyline points="6 9 12 15 18 9"/></svg>
              </h3>
              <div id="advanced-options-content" style="display: none; margin-top: 16px;">
                <div class="form-group">
                  <label>Java 虚拟机参数</label>
                  <div style="display: flex; gap: 8px; align-items: flex-start;">
                    <textarea id="jvm-args" class="text-input textarea" rows="4" placeholder="-XX:OmitStackTraceInFastThrow -Djdk.lang.Process.allowAmbiguousCommands=True -Dfm.lignoresInvalidMinecraftCertificates=True -Dfm.lignorePatchDiscrepancies=True" style="flex: 1;"></textarea>
                    <button class="btn btn-secondary btn-sm" onclick="optimizeJvmArgs()" style="white-space: nowrap; margin-top: 4px;">智能优化</button>
                  </div>
                </div>
                <div class="form-group">
                  <label>游戏参数</label>
                  <input type="text" id="game-args" class="text-input" placeholder="--tweakclass...">
                </div>
                <div class="form-group">
                  <label>启动前执行命令</label>
                  <input type="text" id="pre-launch-command" class="text-input" placeholder="">
                </div>
                <div class="form-group">
                  <label>内存管理</label>
                  <select id="memory-management" class="select-input">
                    <option value="default">默认</option>
                    <option value="g1gc">调优 G1GC</option>
                    <option value="zgc">ZGC (低延迟)</option>
                    <option value="shenandoah">Shenandoah (平衡)</option>
                  </select>
                </div>
                <div class="form-group">
                  <label class="checkbox-label">
                    <input type="checkbox" id="disable-java-wrapper">
                    <span>禁用 Java Launch Wrapper</span>
                  </label>
                </div>
                <div class="form-group">
                  <label class="checkbox-label">
                    <input type="checkbox" id="disable-lwjgl-agent">
                    <span>禁用 LWJGL Unsafe Agent</span>
                  </label>
                </div>
                <div class="form-group">
                  <label class="checkbox-label">
                    <input type="checkbox" id="use-high-performance-gpu" checked>
                    <span>使用高性能显卡</span>
                  </label>
                </div>
                <div class="form-group">
                  <label class="checkbox-label">
                    <input type="checkbox" id="jvm-preheat">
                    <span>JVM 预热启动</span>
                  </label>
                  <span class="form-hint">空闲时预加载 JVM，加速游戏首次启动（占用约 100MB 内存）</span>
                </div>
                <div class="form-group">
                  <label class="checkbox-label">
                    <input type="checkbox" id="performance-boost" checked>
                    <span>性能增强模式</span>
                  </label>
                  <span class="form-hint">自动提升游戏进程优先级、绑定CPU大核、优化I/O调度（Intel 12代+ CPU效果显著）</span>
                </div>
                <div class="form-group">
                  <label class="checkbox-label">
                    <input type="checkbox" id="enable-cds" checked>
                    <span>CDS 类共享加速</span>
                  </label>
                  <span class="form-hint">使用类数据共享归档跳过类解析，加速游戏启动 30-50%</span>
                  <div style="margin-top: 6px;">
                    <button class="btn btn-secondary btn-sm" onclick="generateCdsArchive()">生成 CDS 归档</button>
                    <span id="cds-status-text" style="font-size: 11px; color: var(--text-tertiary); margin-left: 8px;"></span>
                  </div>
                </div>
                <div style="margin-top: 8px; padding: 10px; background: rgba(74, 158, 255, 0.08); border: 1px solid rgba(74, 158, 255, 0.2); border-radius: var(--radius); font-size: 12px; color: var(--accent);">
                  版本独立设置中还有更多高级选项可供调整。
                </div>
              </div>
            </div>

            <div class="form-actions">
              <button class="btn btn-primary" onclick="saveLaunchSettings()">保存设置</button>
              <button class="btn btn-secondary" onclick="resetLaunchSettings()">重置默认</button>
            </div>
          </div>
  `
};

window.VersePC = window.VersePC || {};
window.VersePC.PageSettingsLaunch = PageSettingsLaunch;
