/* page-cloud - Verse云盘页 Vue 组件
 * 模板只负责结构，交互与数据全部在 js/app/cloud.js 中实现
 */
const PageCloud = {
  template: `
          <div class="page-header">
            <h2>Verse云盘</h2>
            <div class="page-actions">
              <button class="btn btn-secondary btn-sm" onclick="cloudBuy()" id="cloud-buy-btn" hidden>
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="fb-inline-icon"><circle cx="9" cy="21" r="1"/><circle cx="20" cy="21" r="1"/><path d="M1 1h4l2.68 13.39a2 2 0 0 0 2 1.61h9.72a2 2 0 0 0 2-1.61L23 6H6"/></svg>
                购买套餐
              </button>
              <button class="btn btn-secondary btn-sm" onclick="cloudRedeemOpen()" id="cloud-redeem-btn" hidden>兑换</button>
              <button class="btn btn-secondary btn-sm" onclick="cloudRefresh()" id="cloud-refresh-btn" hidden>刷新</button>
              <button class="btn btn-secondary btn-sm" onclick="cloudLogout()" id="cloud-logout-btn" hidden>退出</button>
            </div>
          </div>

          <div class="cloud-container">

            <!-- 未登录：登录卡片 -->
            <div class="card" id="cloud-login-card">
              <h3>登录云盘</h3>
              <div class="cloud-login-tip">
                使用统一的 Verse 账号登录：UID 为爱发电数字 UID，密码为皮肤站密码（即在启动器里外置登录用的那套账号密码）。
                还没有账号？先到 <a href="javascript:void(0)" onclick="cloudOpenSkin()">皮肤站</a> 用爱发电登录一次即可自动注册。
              </div>
              <div class="form-group">
                <label>UID <span class="fb-required">*</span></label>
                <input type="text" id="cloud-uid" class="text-input" placeholder="爱发电数字 UID，例如 546792942489" oninput="cloudUpdateLoginState()">
              </div>
              <div class="form-group">
                <label>密码 <span class="fb-required">*</span></label>
                <input type="password" id="cloud-password" class="text-input" placeholder="统一账号密码" onkeydown="if(event.key==='Enter')cloudLogin()" oninput="cloudUpdateLoginState()">
              </div>
              <div class="fb-submit-bar">
                <span class="fb-submit-hint" id="cloud-login-hint"></span>
                <button class="btn btn-primary" id="cloud-login-btn" onclick="cloudLogin()" disabled>登录</button>
              </div>
            </div>

            <!-- 已登录主界面 -->
            <div id="cloud-main" hidden>

              <!-- 容量概览卡 -->
              <div class="card cloud-cap-card">
                <div class="cloud-cap-top">
                  <div>
                    <div class="cloud-cap-title">云盘容量</div>
                    <div class="cloud-cap-nums" id="cloud-cap-nums">-- / --</div>
                  </div>
                  <div class="cloud-cap-days" id="cloud-cap-days"></div>
                </div>
                <div class="cloud-cap-track"><div class="cloud-cap-bar" id="cloud-cap-bar" style="width:0%"></div></div>
                <div class="cloud-cap-pool" id="cloud-cap-pool"></div>
              </div>

              <!-- 操作区 -->
              <div class="card">
                <div class="cloud-toolbar">
                  <button class="btn btn-primary" onclick="cloudUploadFlow()" id="cloud-upload-btn">上传存档</button>
                  <span class="form-hint">从已安装版本打包存档 zip 上传（仅支持 zip）</span>
                </div>
                <div class="cloud-progress" id="cloud-progress" hidden>
                  <div class="cloud-progress-label" id="cloud-progress-label"></div>
                  <div class="cloud-cap-track"><div class="cloud-cap-bar cloud-bar-active" id="cloud-progress-bar" style="width:0%"></div></div>
                </div>
              </div>

              <!-- 文件列表 -->
              <div class="card">
                <h3>已保存内容</h3>
                <div class="cloud-file-list" id="cloud-file-list"></div>
              </div>

            </div>
          </div>

          <!-- 通用选择弹层 -->
          <div class="cloud-overlay" id="cloud-overlay" hidden>
            <div class="cloud-dialog">
              <div class="cloud-dialog-title" id="cloud-dialog-title">选择</div>
              <div class="cloud-dialog-body" id="cloud-dialog-body"></div>
              <div class="cloud-dialog-foot">
                <button class="btn btn-secondary btn-sm" onclick="cloudDialogClose()">取消</button>
              </div>
            </div>
          </div>
  `
};

window.VersePC = window.VersePC || {};
window.VersePC.PageCloud = PageCloud;
