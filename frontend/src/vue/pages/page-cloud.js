/* page-cloud - Verse云盘页 Vue 组件
 * 模板只负责结构，交互与数据全部在 features/misc/cloud.js 中实现
 */
import {
  cloudUpdateLoginState,
  cloudLogin,
  cloudLogout,
  cloudRefresh,
  cloudRedeemOpen,
  cloudUploadFlow,
  cloudBuy,
  cloudOpenSkin,
  cloudDialogClose
} from '../../features/misc/cloud.js';

const PageCloud = {
  setup() {
    return {
      cloudUpdateLoginState,
      cloudLogin,
      cloudLogout,
      cloudRefresh,
      cloudRedeemOpen,
      cloudUploadFlow,
      cloudBuy,
      cloudOpenSkin,
      cloudDialogClose
    };
  },
  template: `
          <div class="page-header">
            <h2>Verse云盘</h2>
            <div class="page-actions">
              <button class="btn btn-secondary btn-sm" @click="cloudBuy()" id="cloud-buy-btn" hidden>
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="fb-inline-icon"><circle cx="9" cy="21" r="1"/><circle cx="20" cy="21" r="1"/><path d="M1 1h4l2.68 13.39a2 2 0 0 0 2 1.61h9.72a2 2 0 0 0 2-1.61L23 6H6"/></svg>
                购买套餐
              </button>
              <button class="btn btn-secondary btn-sm" @click="cloudRedeemOpen()" id="cloud-redeem-btn" hidden>兑换</button>
              <button class="btn btn-secondary btn-sm" @click="cloudRefresh()" id="cloud-refresh-btn" hidden>刷新</button>
              <button class="btn btn-secondary btn-sm" @click="cloudLogout()" id="cloud-logout-btn" hidden>退出</button>
            </div>
          </div>

          <div class="cloud-container">

            <!-- 未登录：登录卡片 -->
            <div class="card cloud-login-card" id="cloud-login-card">
              <div class="cloud-login-hero">
                <div class="cloud-login-badge">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M17.5 19a4.5 4.5 0 0 0 .5-8.97A6 6 0 0 0 6.2 9.2 4 4 0 0 0 6 17.5"/><path d="M12 12v7"/><path d="M9 15l3-3 3 3"/></svg>
                </div>
                <div>
                  <h3>登录云盘</h3>
                  <p class="cloud-login-sub">存档一处备份，换机、重装都能取回</p>
                </div>
              </div>
              <div class="cloud-login-steps">
                <span class="cloud-login-step">账号 = 爱发电数字 UID</span>
                <span class="cloud-login-step">密码 = 皮肤站密码</span>
                <span class="cloud-login-step">没账号？先去 <a href="javascript:void(0)" @click="cloudOpenSkin()">皮肤站</a> 登录一次自动注册</span>
              </div>
              <div class="form-group">
                <label>UID <span class="fb-required">*</span></label>
                <input type="text" id="cloud-uid" class="text-input" placeholder="爱发电数字 UID，例如 546792942489" @input="cloudUpdateLoginState()">
              </div>
              <div class="form-group">
                <label>密码 <span class="fb-required">*</span></label>
                <input type="password" id="cloud-password" class="text-input" placeholder="统一账号密码" @input="cloudUpdateLoginState()" @keydown.enter="cloudLogin()">
              </div>
              <div class="fb-submit-bar">
                <span class="fb-submit-hint" id="cloud-login-hint"></span>
                <button class="btn btn-primary" id="cloud-login-btn" @click="cloudLogin()" disabled>登录</button>
              </div>
            </div>

            <!-- 已登录主界面 -->
            <div id="cloud-main" hidden>

              <!-- 容量概览卡 -->
              <div class="card cloud-cap-card">
                <div class="cloud-cap-head">
                  <div>
                    <span class="cloud-cap-title">云盘容量</span>
                    <div class="cloud-cap-nums" id="cloud-cap-nums">-- / --</div>
                  </div>
                  <div class="cloud-cap-days" id="cloud-cap-days"></div>
                </div>
                <div class="cloud-cap-track"><div class="cloud-cap-bar" id="cloud-cap-bar"></div></div>
                <div class="cloud-cap-foot">
                  <span class="cloud-cap-percent" id="cloud-cap-percent"></span>
                </div>
                <div class="cloud-cap-pool" id="cloud-cap-pool"></div>
              </div>

              <!-- 操作区 -->
              <div class="card">
                <div class="cloud-toolbar">
                  <button class="btn btn-primary cloud-upload-btn" @click="cloudUploadFlow()" id="cloud-upload-btn">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><path d="M17 8l-5-5-5 5"/><path d="M12 3v12"/></svg>
                    上传存档
                  </button>
                  <span class="form-hint">自动打包所选版本的 saves 目录，压缩为 zip 上传</span>
                </div>
                <div class="cloud-progress" id="cloud-progress" hidden>
                  <div class="cloud-progress-head">
                    <span class="cloud-progress-label" id="cloud-progress-label"></span>
                    <span class="cloud-progress-pct" id="cloud-progress-pct"></span>
                  </div>
                  <div class="cloud-cap-track"><div class="cloud-cap-bar cloud-bar-active" id="cloud-progress-bar"></div></div>
                </div>
              </div>

              <!-- 文件列表 -->
              <div class="card">
                <div class="cloud-files-head">
                  <h3>已保存内容</h3>
                  <span class="cloud-files-count" id="cloud-files-count"></span>
                </div>
                <div class="cloud-file-list" id="cloud-file-list"></div>
              </div>

            </div>
          </div>

          <!-- 通用选择弹层 -->
          <div class="cloud-overlay" id="cloud-overlay" hidden>
            <div class="cloud-dialog">
              <div class="cloud-dialog-title" id="cloud-dialog-title">选择</div>
              <div class="cloud-dialog-body" id="cloud-dialog-body"></div>
              <div class="cloud-dialog-foot" id="cloud-dialog-foot"></div>
            </div>
          </div>
  `
};

export default PageCloud;
