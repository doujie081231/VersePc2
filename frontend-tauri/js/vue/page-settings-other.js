/* page-settings-other.js - 其他设置页 Vue 组件（渐进式改造）
 * 原则：
 *   1. CSS 一行不动（class 名保留原样）
 *   2. HTML 结构原样搬运（标签、层级、id 全部不变）
 *   3. JS 函数全部复用（来自 js/app/*.js 的全局函数）
 */
const PageSettingsOther = {
  template: `
          <div class="page-header">
            <h2>其他设置</h2>
          </div>
          <div class="settings-container">
            <details class="card sponsor-card" id="sponsor-card" style="position:relative;">
              <summary class="sponsor-header" style="cursor:pointer;list-style:none;display:flex;align-items:center;gap:8px;">
                <svg class="chevron-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:16px;height:16px;"><polyline points="6 9 12 15 18 9"/></svg>
                <h3 style="margin:0;">赞助者</h3>
                <span class="sponsor-count" id="sponsor-count"></span>
              </summary>
              <span class="nav-btn-experimental-badge sponsor-experimental-badge">实验</span>
              <div class="sponsor-dino-entry">
                <div class="sponsor-dino-head">
                  <div>
                    <div class="sponsor-dino-title">赞助者专属彩蛋</div>
                    <div class="sponsor-dino-desc">输入赞助订单号，解锁谷歌原版恐龙小游戏</div>
                  </div>
                </div>
                <div class="sponsor-dino-row">
                  <input type="text" id="sponsor-order-input" class="text-input sponsor-dino-input" placeholder="输入赞助订单号..." onkeydown="if(event.key==='Enter')openSponsorDino()">
                  <button class="btn btn-primary btn-sm sponsor-dino-btn" onclick="openSponsorDino()">进入小游戏</button>
                </div>
              </div>
              <div class="sponsor-divider"></div>
              <div class="sponsor-search">
                <svg class="sponsor-search-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="7"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>
                <input type="text" id="sponsor-search-input" class="sponsor-search-input" placeholder="搜索赞助者..." oninput="filterSponsors(this.value)">
              </div>
              <div class="sponsor-grid" id="sponsor-list">
              </div>
              <button class="btn btn-ghost btn-sm sponsor-more-btn" id="sponsor-more-btn" onclick="toggleShowMoreSponsors()">
                展开更多
                <svg class="chevron-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:12px;height:12px;margin-left:4px"><polyline points="6 9 12 15 18 9"/></svg>
              </button>
            </details>
            <div class="card">
              <h3>反馈</h3>
              <p style="font-size: 13px; color: var(--text-secondary); margin-bottom: 12px;">如果你遇到了问题或有建议，欢迎通过邮件反馈</p>
              <div class="form-group">
                <label>反馈邮箱</label>
                <div class="input-with-btn">
                  <input type="text" class="text-input" value="verselauncher@verselauncher.cn" readonly>
                  <button class="btn btn-primary btn-sm" onclick="copyFeedbackEmail(this)">复制</button>
                </div>
              </div>
            </div>

            <div class="card">
              <h3>下载</h3>
              <div class="form-group">
                <label>文件下载源</label>
                <select id="setting-download-source" class="select-input">
                  <option value="china-first">国内优先（默认）</option>
                  <option value="auto">智能混合：优先官方源，在加载慢时换用镜像源</option>
                  <option value="official-first">官方优先（失败后回退镜像）</option>
                  <option value="mojang">直连 Mojang 官方源</option>
                </select>
              </div>
              <div class="form-group">
                <label>版本列表源</label>
                <select id="setting-version-source" class="select-input">
                  <option value="auto">优先使用官方源，在加载缓慢时换用镜像源</option>
                  <option value="bmclapi">BMCLAPI 镜像源 (bangbang93)</option>
                  <option value="mojang">Mojang 官方源</option>
                </select>
              </div>
              <div class="form-group">
                <label>最大线程数: <span id="thread-count-value">32</span></label>
                <input type="range" id="setting-max-threads" min="1" max="128" value="64" class="slider-input" oninput="document.getElementById('thread-count-value').textContent=this.value">
                <span class="form-hint">文件级并发下载数，建议 32-64</span>
              </div>
              <div class="form-group">
                <label class="checkbox-label">
                  <input type="checkbox" id="setting-enable-chunk-download" checked>
                  <span>启用分块下载（大文件多线程分段）</span>
                </label>
              </div>
              <div class="form-group">
                <label>每文件最大分块数: <span id="chunk-count-value">8</span></label>
                <input type="range" id="setting-max-chunks-per-file" min="1" max="64" value="64" class="slider-input" oninput="document.getElementById('chunk-count-value').textContent=this.value">
                <span class="form-hint">单个大文件的最大并行下载分块数</span>
              </div>
              <div class="form-group">
                <label>速度限制: <span id="speed-limit-value">无限制</span></label>
                <input type="range" id="setting-speed-limit" min="0" max="100" value="0" class="slider-input" oninput="updateSpeedLimitLabel(this.value)">
                <span class="form-hint">0 = 无限制, 1-100 = MB/s</span>
              </div>
              <div class="form-group">
                <label>目标文件夹</label>
                <div class="input-with-btn">
                  <input type="text" id="setting-target-dir" class="text-input" placeholder="默认目录">
                  <button class="btn btn-secondary btn-sm" onclick="browseFolder('target')">浏览...</button>
                </div>
                <span class="form-hint">请在「启动 → 版本选择 → 文件夹列表」中更改下载目标文件夹。</span>
              </div>
              <div class="form-group">
                <label class="checkbox-label">
                  <input type="checkbox" id="setting-ssl-verify">
                  <span>在正版登录时验证 SSL 证书</span>
                </label>
              </div>
            </div>

            <div class="card">
              <h3>社区资源</h3>
              <div class="form-group">
                <label>文件名格式</label>
                <select id="setting-filename-format" class="select-input">
                  <option value="default">[机械动力] create-1.21.1-6.0.4</option>
                  <option value="simple">create-1.21.1-6.0.4.jar</option>
                  <option value="full">[机械动力] create-1.21.1-6.0.4 [Fabric]</option>
                </select>
              </div>
              <div class="form-group">
                <label>Mod 管理样式</label>
                <select id="setting-mod-style" class="select-input">
                  <option value="title">标题显示译名，详情显示文件名</option>
                  <option value="filename">始终显示文件名</option>
                  <option value="compact">紧凑模式</option>
                </select>
              </div>
              <div class="form-group">
                <label class="checkbox-label">
                  <input type="checkbox" id="setting-ignore-quilt">
                  <span>在显示 Mod 加载器时忽略 Quilt</span>
                </label>
              </div>
            </div>

            <div class="card">
              <h3>辅助功能</h3>
              <div class="form-group">
                <label>游戏更新提示</label>
                <div style="display: flex; gap: 12px; margin-top: 6px;">
                  <label class="checkbox-label">
                    <input type="checkbox" id="notify-release-updates">
                    <span>正式版更新提示</span>
                  </label>
                  <label class="checkbox-label">
                    <input type="checkbox" id="notify-snapshot-updates">
                    <span>测试版更新提示</span>
                  </label>
                </div>
              </div>
              <div class="form-group">
                <label>游戏语言</label>
                <label class="checkbox-label">
                  <input type="checkbox" id="auto-set-chinese" checked>
                  <span>自动设置为中文</span>
                </label>
              </div>
            </div>

            <div class="card">
              <h3>内存优化</h3>
              <p class="form-hint" style="margin-bottom: 12px;">将物理内存占用降低约 1/3，不仅限于 Minecraft。如果使用机械硬盘，可能会导致一小段时间的卡顿。</p>
              <div class="form-group">
                <div style="display: flex; align-items: center; gap: 16px;">
                  <div id="memory-info-display" style="flex: 1;">
                    <div style="font-size: 13px; color: var(--text-secondary);">内存使用率</div>
                    <div style="display: flex; align-items: center; gap: 8px; margin-top: 4px;">
                      <div style="flex: 1; height: 8px; background: var(--bg-active); border-radius: 4px; overflow: hidden;">
                        <div id="memory-usage-bar" style="width: 0%; height: 100%; background: var(--accent); border-radius: 4px; transition: width 0.5s ease;"></div>
                      </div>
                      <span id="memory-usage-text" style="font-size: 13px; color: var(--text-secondary); min-width: 36px; text-align: right;">0%</span>
                    </div>
                    <div id="memory-detail-text" style="font-size: 12px; color: var(--text-tertiary); margin-top: 4px;">正在获取...</div>
                  </div>
                  <button class="btn btn-primary win-only" id="memory-optimize-btn" onclick="doMemoryOptimize()">内存优化</button>
                </div>
              </div>
              <div class="form-group" style="margin-top: 12px; padding-top: 12px; border-top: 1px solid var(--border);">
                <label class="checkbox-label">
                  <input type="checkbox" id="auto-memory-optimize" checked>
                  <span>启动游戏前优化一次</span>
                </label>
                <span class="form-hint">每次启动游戏前自动清理系统待机内存和所有进程的工作集，释放约 1/3 物理内存</span>
              </div>
            </div>

            <div class="card">
              <h3>数据管理</h3>
              <div class="btn-group">
                <button class="btn btn-secondary" onclick="openFolder('data')">打开数据目录</button>
                <button class="btn btn-secondary" onclick="openFolder('versions')">打开版本目录</button>
                <button class="btn btn-secondary" onclick="openFolder('assets')">打开资源目录</button>
                <button class="btn btn-secondary" onclick="openFolder('mods')">打开模组目录</button>
              </div>
              <div style="margin-top: 12px; padding-top: 12px; border-top: 1px solid var(--border);">
                <div style="display: flex; align-items: center; justify-content: space-between; margin-bottom: 8px;">
                  <div>
                    <div style="font-size: 13px; font-weight: 500;">清理垃圾文件</div>
                    <div style="font-size: 12px; color: var(--text-muted);">清理游戏日志、缓存、临时文件等</div>
                  </div>
                  <span id="cleanup-size-info" style="font-size: 12px; color: var(--text-muted);"></span>
                </div>
                <div id="cleanup-details" style="display: none; margin-bottom: 10px; font-size: 12px; color: var(--text-secondary);"></div>
                <div class="btn-group">
                  <button class="btn btn-secondary" id="cleanup-scan-btn" onclick="cleanupScan()">扫描</button>
                  <button class="btn btn-danger" id="cleanup-run-btn" onclick="cleanupRun()" disabled>一键清理</button>
                </div>
              </div>
            </div>

            <div class="card">
              <h3>退出</h3>
              <div class="form-group">
                <button class="btn btn-danger btn-lg btn-block" id="exit-launcher-btn">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="20" height="20"><path d="M9 21H5a2 2 0 01-2-2V5a2 2 0 012-2h4"/><polyline points="16 17 21 12 16 7"/><line x1="21" y1="12" x2="9" y2="12"/></svg>
                  退出启动器
                </button>
              </div>
            </div>

            <div class="card">
              <h3>关于 VersePC2</h3>
              <p style="font-size: 13px; color: var(--text-secondary); margin-bottom: 12px;" id="updater-version-info">当前版本：加载中...</p>
              <div style="margin-bottom:12px;padding:10px 14px;background:var(--bg-secondary,rgba(255,255,255,0.03));border:1px solid var(--border-color,rgba(255,255,255,0.06));border-radius:8px;">
                <div style="display:flex;align-items:center;gap:8px;margin-bottom:6px;">
                  <span style="font-size:13px;color:var(--text-primary);">更新加速</span>
                  <span style="font-size:11px;padding:1px 8px;background:rgba(76,175,80,0.15);color:#4caf50;border-radius:10px;">多源自动</span>
                </div>
                <div style="font-size:12px;color:var(--text-secondary);line-height:1.6;">GitHub Raw → ghproxy → jsDelivr，按顺序自动尝试，无需手动配置</div>
              </div>
              <div id="updater-status-area" style="margin-bottom:12px;"></div>
              <div id="updater-release-notes" style="margin-bottom:12px;display:none;">
                <details style="background:var(--bg-secondary,rgba(255,255,255,0.05));border-radius:8px;padding:10px;border:1px solid var(--border-color,rgba(255,255,255,0.08));">
                  <summary style="cursor:pointer;font-size:13px;color:var(--accent);font-weight:500;">更新日志</summary>
                  <pre id="updater-release-notes-content" style="white-space:pre-wrap;word-break:break-word;font-size:12px;color:var(--text-secondary);margin-top:8px;line-height:1.6;"></pre>
                </details>
              </div>
              <div class="btn-group" id="updater-btn-group">
                <button class="btn btn-primary" id="updater-check-btn" onclick="handleCheckUpdate()">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:16px;height:16px;margin-right:6px;">
                    <path d="M21 2l-2 2m-7.61 7.61a5.5 5.5 0 11-7.778 7.778 5.5 5.5 0 017.777-7.777zm0 0L15.5 7.5m0 0l3 3L22 7l-3-3m-3.5 3.5L19 4"/>
                  </svg>
                  检查更新
                </button>
                <button class="btn btn-secondary" id="updater-skip-btn" style="display:none" onclick="handleSkipVersion()">
                  跳过此版本
                </button>
                <button class="btn btn-outline" id="updater-release-btn" style="display:none" onclick="api.updater.openReleasePage()">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:16px;height:16px;margin-right:6px;"><path d="M18 13v6a2 2 0 01-2 2H5a2 2 0 01-2-2V8a2 2 0 012-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/></svg>
                  查看详情
                </button>
                <button class="btn btn-accent" id="updater-download-btn" style="display:none" onclick="handleDownloadUpdate()">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:16px;height:16px;margin-right:6px;">
                    <path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/>
                  </svg>
                  下载更新
                </button>
                <button class="btn btn-success" id="updater-install-btn" style="display:none" onclick="handleInstallUpdate()">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:16px;height:16px;margin-right:6px;">
                    <polyline points="20 6 9 17 4 12"/>
                  </svg>
                  安装并重启
                </button>
                <button class="btn btn-secondary" onclick="showUpdateAnnouncement()">查看更新公告</button>
                <button class="btn btn-secondary win-only" onclick="createDesktopShortcut()">创建桌面快捷方式</button>
              </div>
            </div>

            <div class="card">
              <h3 style="margin-bottom: 16px;">致谢</h3>
              <p style="font-size: 13px; color: var(--text-secondary); margin-bottom: 16px; line-height: 1.6;">
                VersePC 的开发离不开以下开源项目的启发与技术支持，在此表示衷心感谢！
              </p>
              <div style="display: flex; gap: 16px; flex-wrap: wrap;">
                <div class="mod-item" style="flex: 1; min-width: 280px; padding: 16px; cursor: pointer;" onclick="window.open('https://github.com/Meloong-Git/PCL', '_blank')">
                  <img src="img/pcl.png" alt="PCL" style="width: 48px; height: 48px; border-radius: 12px; object-fit: cover; flex-shrink: 0; box-shadow: 0 4px 12px rgba(0, 0, 0, 0.2);">
                  <div style="flex: 1; min-width: 0;">
                    <div style="font-size: 15px; font-weight: 600; color: var(--text-primary); margin-bottom: 4px;">Plain Craft Launcher 2</div>
                    <div style="font-size: 12px; color: var(--text-secondary); line-height: 1.5;">借鉴了其优秀的多线程下载技术，大幅提升了文件下载速度与稳定性</div>
                  </div>
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width: 16px; height: 16px; color: var(--text-muted); flex-shrink: 0;"><path d="M18 13v6a2 2 0 01-2 2H5a2 2 0 01-2-2V8a2 2 0 012-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/></svg>
                </div>
                <div class="mod-item" style="flex: 1; min-width: 280px; padding: 16px; cursor: pointer;" onclick="window.open('https://github.com/HMCL-dev/HMCL', '_blank')">
                  <img src="img/hmcl.png" alt="HMCL" style="width: 48px; height: 48px; border-radius: 12px; object-fit: cover; flex-shrink: 0; box-shadow: 0 4px 12px rgba(0, 0, 0, 0.2);">
                  <div style="flex: 1; min-width: 0;">
                    <div style="font-size: 15px; font-weight: 600; color: var(--text-primary); margin-bottom: 4px;">Hello Minecraft! Launcher</div>
                    <div style="font-size: 12px; color: var(--text-secondary); line-height: 1.5;">参考了其设计理念与部分功能实现，为启动器开发提供了宝贵思路</div>
                  </div>
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width: 16px; height: 16px; color: var(--text-muted); flex-shrink: 0;"><path d="M18 13v6a2 2 0 01-2 2H5a2 2 0 01-2-2V8a2 2 0 012-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/></svg>
                </div>
                <div class="mod-item" style="flex: 1; min-width: 280px; padding: 16px; cursor: pointer;" onclick="window.open('https://www.ifdian.net/a/RedstoneOnline?utm_source=copylink&utm_medium=link', '_blank')">
                  <img src="img/chrono-avatar.jpg" alt="Chrono" style="width: 48px; height: 48px; border-radius: 50%; object-fit: cover; flex-shrink: 0; box-shadow: 0 4px 12px rgba(0, 0, 0, 0.2);">
                  <div style="flex: 1; min-width: 0;">
                    <div style="font-size: 15px; font-weight: 600; color: var(--text-primary); margin-bottom: 4px;">Chrono · 红石联机</div>
                    <div style="font-size: 12px; color: var(--text-secondary); line-height: 1.5;">提供内网穿透服务与 TCP 隧道技术支持，让外网联机变得简单</div>
                  </div>
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width: 16px; height: 16px; color: var(--text-muted); flex-shrink: 0;"><path d="M18 13v6a2 2 0 01-2 2H5a2 2 0 01-2-2V8a2 2 0 012-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/></svg>
                </div>
                <div class="mod-item" style="flex: 1; min-width: 280px; padding: 16px; cursor: pointer;" onclick="window.open('https://github.com/modrinth/theseus', '_blank')">
                  <img src="img/theseus.png" alt="Theseus" style="width: 48px; height: 48px; border-radius: 12px; object-fit: cover; flex-shrink: 0; box-shadow: 0 4px 12px rgba(0, 0, 0, 0.2);">
                  <div style="flex: 1; min-width: 0;">
                    <div style="font-size: 15px; font-weight: 600; color: var(--text-primary); margin-bottom: 4px;">Theseus · modrinth/theseus</div>
                    <div style="font-size: 12px; color: var(--text-secondary); line-height: 1.5;">其整合包下载引擎为 VersePC 的整合包下载与文件并发处理提供了重要参考</div>
                  </div>
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width: 16px; height: 16px; color: var(--text-muted); flex-shrink: 0;"><path d="M18 13v6a2 2 0 01-2 2H5a2 2 0 01-2-2V8a2 2 0 012-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/></svg>
                </div>
              </div>
            </div>

            <div class="form-actions">
              <button class="btn btn-primary" onclick="saveOtherSettings()">保存设置</button>
              <button class="btn btn-secondary" onclick="resetOtherSettings()">重置默认</button>
            </div>
          </div>
  `
};

window.VersePC = window.VersePC || {};
window.VersePC.PageSettingsOther = PageSettingsOther;
