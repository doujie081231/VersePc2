/* page-accounts.js - 账户页 Vue 组件（新版堆叠布局） */
import { handleSkinUpload, handleCapeUpload } from '../../features/accounts/accounts.js';

import { closeMsAuthModal, copyMsCode, reopenMsAuthPage, closeOfflineModal, closeThirdPartyModal, closeProfileSelectModal, setSkinBg, showAccountList, copyDetailUuid, setAnim, fetchSessionCape, unselectCape, detailSelectAccount, detailRefreshSkin, detailDeleteAccount, openMsAuthModal, openOfflineAccountModal, openThirdPartyAccountModal, submitThirdPartyLogin } from '../../features/accounts/accounts.js';
import { accountsState } from '../../features/accounts/accounts-state.js';
import { showToast } from '../../features/misc/ui-components.js';
import { switchPage } from '../../features/settings/window-theme.js';
import VButton from '@/components/verse/VButton.vue';
import VInput from '@/components/verse/VInput.vue';

function openSkinLibrary() {
  const acc = accountsState._currentDetailAccount;
  if (!acc) {
    showToast('请先打开一个账户的详情', 'warning');
    return;
  }
  // switchPage 离开账户页时会清空 _currentDetailAccount，这里先留一份快照
  accountsState._skinLibraryAccount = {
    id: acc.id,
    name: acc.username || acc.name || acc.id,
    type: acc.type
  };
  switchPage('skin-library');
}

function pickSkinFile() {
  document.getElementById('skin-file-input')?.click();
}

function pickCapeFile() {
  document.getElementById('cape-file-input')?.click();
}

const PageAccounts = {
  components: { VButton, VInput },
  setup() {
    // handleSkinUpload / handleCapeUpload 原为内联 onchange 调用的全局函数，改为模板事件后必须暴露到作用域
    return { closeMsAuthModal, copyMsCode, reopenMsAuthPage, closeOfflineModal, closeThirdPartyModal, closeProfileSelectModal, setSkinBg, showAccountList, copyDetailUuid, setAnim, fetchSessionCape, unselectCape, detailSelectAccount, detailRefreshSkin, detailDeleteAccount, openMsAuthModal, openOfflineAccountModal, openThirdPartyAccountModal, submitThirdPartyLogin, pickSkinFile, pickCapeFile, handleSkinUpload, handleCapeUpload, openSkinLibrary };
  },
  template: `
          <div class="accounts-redesign">
            <!-- 顶部标题（原顶部简单一行） -->
            <div class="accts-header-row">
              <h2 class="accts-header-title">账户</h2>
            </div>

            <!-- 主体：可拖动的账号轮播轨道（第一张为添加账号卡片，之后为账号卡片） -->
            <div class="accts-stage">
              <div class="accts-carousel" id="accts-carousel">
                <div class="accts-track" id="accts-track">
                  <!-- 由 accounts.js 动态渲染：第一张是"添加账号"卡片，后面是账号卡片 -->
                </div>
                <div class="accts-carousel-hint">← 左右拖动切换账号，最左侧可添加 →</div>
              </div>
            </div>

            <!-- 占位：原账户列表（为保持 accounts.js 旧逻辑可用仍保留，但不显示） -->
            <div id="accounts-list" class="account-list account-list-hidden">
              <p class="empty-text">暂无账户，请添加账户</p>
            </div>

            <!-- ==========================================
                 添加账号弹窗（新版卡片式三选项）
                 ========================================== -->
            <div id="add-account-modal" class="modal" style="display:none">
              <div class="modal-content add-acct-modal">
                <div class="modal-header">
                  <h3>添加账号</h3>
                  <button class="modal-close" id="close-add-account-modal">&times;</button>
                </div>
                <div class="add-acct-cards">
                  <!-- 微软 -->
                  <button class="add-acct-card" id="add-acct-ms" data-type="microsoft" @click="openMsAuthModal()">
                    <div class="add-acct-card-icon add-acct-card-icon-ms">
                      <svg viewBox="0 0 23 23" width="34" height="34"><path fill="#f25022" d="M0 0h11v11H0z"/><path fill="#7fba00" d="M12 0h11v11H12z"/><path fill="#00a4ef" d="M0 12h11v11H0z"/><path fill="#ffb900" d="M12 12h11v11H12z"/></svg>
                    </div>
                    <div class="add-acct-card-text">
                      <div class="add-acct-card-title">微软账户</div>
                      <div class="add-acct-card-sub">Minecraft 正版登录</div>
                    </div>
                  </button>

                  <!-- 离线 -->
                  <button class="add-acct-card" id="add-acct-offline" data-type="offline" @click="openOfflineAccountModal()">
                    <div class="add-acct-card-icon add-acct-card-icon-offline">
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" width="34" height="34"><path d="M16 21v-2a4 4 0 00-4-4H5a4 4 0 00-4 4v2"/><circle cx="8.5" cy="7" r="4"/></svg>
                    </div>
                    <div class="add-acct-card-text">
                      <div class="add-acct-card-title">离线账户</div>
                      <div class="add-acct-card-sub">本地玩家 ID 登录</div>
                    </div>
                  </button>

                  <!-- 第三方 -->
                  <button class="add-acct-card" id="add-acct-thirdparty" data-type="thirdparty" @click="openThirdPartyAccountModal()">
                    <div class="add-acct-card-icon add-acct-card-icon-thirdparty">
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" width="34" height="34"><circle cx="12" cy="12" r="10"/><line x1="2" y1="12" x2="22" y2="12"/><path d="M12 2a15.3 15.3 0 014 10 15.3 15.3 0 01-4 10 15.3 15.3 0 01-4-10 15.3 15.3 0 014-10z"/></svg>
                    </div>
                    <div class="add-acct-card-text">
                      <div class="add-acct-card-title">外置登录</div>
                      <div class="add-acct-card-sub">第三方 / authlib-injector</div>
                    </div>
                  </button>
                </div>
              </div>
            </div>

            <!-- 下方保留原微软/离线/第三方业务弹窗，仅隐藏顶部原按钮 -->
            <div id="msauth-modal" class="modal" style="display:none">
              <div class="modal-content">
                <div class="modal-header">
                  <h3>微软登录</h3>
                  <button class="modal-close" @click="closeMsAuthModal()">&times;</button>
                </div>
                <div class="modal-body">
                  <div class="msauth-steps">
                    <p style="color:var(--text-secondary);font-size:13px;margin-bottom:12px;">浏览器将自动打开登录页面，授权码已复制到剪贴板。</p>
                    <p>验证链接：</p>
                    <a id="msauth-url" href="#" target="_blank" class="msauth-link"></a>
                    <p>授权码：</p>
                    <div class="msauth-code">
                      <span id="msauth-code-text"></span>
                      <VButton variant="secondary" size="sm" @click="copyMsCode()">复制</VButton>
                    </div>
                  </div>
                  <div class="msauth-status">
                    <div class="spinner-sm"></div>
                    <span id="msauth-status-text">等待登录...</span>
                  </div>
                </div>
                <div class="modal-footer">
                  <VButton variant="secondary" @click="reopenMsAuthPage()">重新打开网页</VButton>
                  <VButton variant="secondary" @click="closeMsAuthModal()">取消</VButton>
                </div>
              </div>
            </div>
            <div id="offline-account-modal" class="modal" style="display:none">
              <div class="modal-content">
                <div class="modal-header">
                  <h3>添加离线账户</h3>
                  <button class="modal-close" @click="closeOfflineModal()">&times;</button>
                </div>
                <div class="modal-body">
                  <div class="form-group">
                    <label>玩家 ID</label>
                    <VInput type="text" id="offline-username-input" placeholder="3 - 16 位，只可包含英文字母、数字与下划线" maxlength="16" />
                    <div class="form-hint" style="font-size:12px;color:var(--text-secondary);margin-top:4px;">UUID 将按照行业规范自动生成（与 HMCL、BakaXL 等一致）</div>
                  </div>
                </div>
                <div class="modal-footer">
                  <VButton variant="secondary" @click="closeOfflineModal()">取消</VButton>
                  <VButton id="create-offline-btn" variant="accent">创建</VButton>
                </div>
              </div>
            </div>
            <div id="thirdparty-account-modal" class="modal" style="display:none">
              <div class="modal-content">
                <div class="modal-header">
                  <h3>外置登录（authlib-injector）</h3>
                  <button class="modal-close" @click="closeThirdPartyModal()">&times;</button>
                </div>
                <div class="modal-body">
                  <div class="form-group">
                    <label>认证服务器</label>
                    <select id="tp-server-preset" class="select-input" style="margin-bottom:8px;">
                      <option value="">选择预设服务器...</option>
                      <option value="https://littleskin.cn/api/yggdrasil">LittleSkin Yggdrasil</option>
                      <option value="https://skin2.mcdb.cn/api/yggdrasil">MCBBS 皮肤站</option>
                      <option value="https://auth.mc-user.com:233/api/yggdrasil">皮肤站2</option>
                      <option value="custom">自定义服务器</option>
                    </select>
                    <VInput type="text" id="tp-server-url" placeholder="https://littleskin.cn/api/yggdrasil" />
                    <div style="font-size:11px;color:var(--text-muted);margin-top:4px;">输入Yggdrasil认证服务器API地址</div>
                    <div style="font-size:11px;color:var(--text-muted);margin-top:6px;">更多信息参考 Yggdrasil 手册： <a href="https://manual.littlesk.in/yggdrasil/" target="_blank">manual.littlesk.in/yggdrasil/</a></div>
                  </div>
                  <div class="form-group">
                    <label>邮箱 / 用户名</label>
                    <VInput type="text" id="tp-username-input" placeholder="输入邮箱或用户名" />
                  </div>
                  <div class="form-group">
                    <label>密码</label>
                    <VInput type="password" id="tp-password-input" placeholder="输入密码" />
                  </div>
                  <div id="tp-server-info" style="display:none;padding:12px;background:var(--bg-tertiary);border-radius:8px;margin-top:8px;">
                    <div style="display:flex;align-items:center;gap:8px;margin-bottom:8px;">
                      <img id="tp-server-icon" src="" style="width:32px;height:32px;border-radius:4px;display:none;">
                      <div>
                        <div id="tp-server-name" style="font-weight:600;color:var(--text-primary);font-size:14px;"></div>
                        <div id="tp-server-desc" style="font-size:11px;color:var(--text-muted);"></div>
                      </div>
                    </div>
                  </div>
                </div>
                <div class="modal-footer">
                  <VButton variant="secondary" @click="closeThirdPartyModal()">取消</VButton>
                  <VButton id="tp-login-btn" variant="accent" @click="submitThirdPartyLogin()">登录</VButton>
                </div>
              </div>
            </div>
            <div id="tp-profile-select-modal" class="modal" style="display:none">
              <div class="modal-content">
                <div class="modal-header">
                  <h3>选择角色</h3>
                  <button class="modal-close" @click="closeProfileSelectModal()">&times;</button>
                </div>
                <div class="modal-body">
                  <p style="color:var(--text-secondary);margin-bottom:16px;font-size:13px;">该账户有多个角色，请选择要使用的角色：</p>
                  <div id="tp-profile-list" class="profile-select-list"></div>
                </div>
              </div>
            </div>

            <!-- 账户详情页 -->
            <div id="page-account-detail" class="acct-detail" style="display:none">
              <div class="acct-detail-left" id="acct-detail-left">
                <div class="acct-bg-toggle">
                  <button class="acct-bg-btn active" data-bg="white" @click="setSkinBg('white')" title="白色背景" aria-label="白色背景"></button>
                  <button class="acct-bg-btn" data-bg="black" @click="setSkinBg('black')" title="黑色背景" aria-label="黑色背景"></button>
                </div>
                <div id="skin-3d-container"></div>
              </div>
              <div class="acct-detail-right">
                <div class="acct-detail-header-area">
                  <button class="acct-detail-back-btn" @click="showAccountList()" title="返回账户列表" aria-label="返回账户列表">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" width="18" height="18"><polyline points="15 18 9 12 15 6"/></svg>
                  </button>
                  <h2 class="acct-detail-title" id="detail-username">-</h2>
                  <span class="acct-detail-badge" id="detail-skin-type">经典</span>
                  <VButton variant="accent" size="sm" class="ml-auto" @click="openSkinLibrary()">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" width="14" height="14"><path d="M20.38 3.46 16 2a4 4 0 0 1-8 0L3.62 3.46a2 2 0 0 0-1.58 2.23l.42 4.1A2 2 0 0 0 4.7 11.5l1.3-.8V21a2 2 0 0 0 2 2h8a2 2 0 0 0 2-2v-10.3l1.3.8a2 2 0 0 0 2.24-1.71l.42-4.1a2 2 0 0 0-1.58-2.23z"/></svg>
                    皮肤库
                  </VButton>
                </div>
                <div class="acct-detail-fields">
                  <div class="acct-detail-field">
                    <span class="acct-detail-label">UUID</span>
                    <div class="acct-detail-copy-row">
                      <code class="acct-detail-uuid" id="detail-uuid">-</code>
                      <button class="acct-copy-btn" @click="copyDetailUuid()" aria-label="复制UUID" title="复制UUID">
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" width="14" height="14"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1"/></svg>
                      </button>
                    </div>
                  </div>
                  <div class="acct-detail-field">
                    <span class="acct-detail-label">账户类型</span>
                    <span class="acct-detail-val" id="detail-account-type">离线</span>
                  </div>
                </div>
                <div class="acct-detail-animations">
                  <span class="acct-detail-label">动作</span>
                  <div class="acct-anim-btns">
                    <button class="acct-anim-btn active" data-anim="idle" @click="setAnim('idle')" title="站立"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18"><circle cx="12" cy="5" r="2"/><path d="M10 22V17H7l5-9 5 9h-3v5"/></svg></button>
                    <button class="acct-anim-btn" data-anim="walk" @click="setAnim('walk')" title="行走"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18"><circle cx="14" cy="4" r="2"/><path d="M6 22l3-7 3 2 4-5 2 4"/><path d="M10 12l-2 6"/></svg></button>
                    <button class="acct-anim-btn" data-anim="run" @click="setAnim('run')" title="跑步"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18"><circle cx="15" cy="4" r="2"/><path d="M4 22l4-6 3 1 5-6 3 3"/><path d="M13 11l-3 5"/></svg></button>
                    <button class="acct-anim-btn" data-anim="fly" @click="setAnim('fly')" title="飞行"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18"><circle cx="12" cy="5" r="2"/><path d="M5 14l7-3 7 3"/><path d="M12 8v14"/><path d="M8 14l4 2 4-2"/></svg></button>
                    <button class="acct-anim-btn" data-anim="wave" @click="setAnim('wave')" title="挥手"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18"><circle cx="12" cy="5" r="2"/><path d="M7 22v-5l-2-3"/><path d="M17 22v-5l2-3"/><path d="M9 14l3-4 3 4"/></svg></button>
                    <button class="acct-anim-btn" data-anim="crouch" @click="setAnim('crouch')" title="蹲下"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18"><circle cx="12" cy="8" r="2"/><path d="M8 22v-4l2-4h4l2 4v4"/><path d="M10 14l-2 2"/><path d="M14 14l2 2"/></svg></button>
                    <button class="acct-anim-btn" data-anim="hit" @click="setAnim('hit')" title="攻击"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18"><path d="M14 2l8 8-4 4-8-8z"/><path d="M10 10L2 22"/><circle cx="12" cy="5" r="1"/></svg></button>
                    <button class="acct-anim-btn" data-anim="swim" @click="setAnim('swim')" title="游泳"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18"><circle cx="12" cy="5" r="2"/><path d="M2 17c2-2 4-2 6 0s4 2 6 0 4-2 6 0"/><path d="M6 12l6-2 6 2"/></svg></button>
                  </div>
                </div>
                <div class="acct-detail-skins" id="acct-detail-skins" style="display:none">
                  <span class="acct-detail-label">皮肤</span>
                  <div class="acct-skin-grid" id="acct-skin-grid"></div>
                  <input type="file" id="skin-file-input" accept=".png" style="display:none" @change="handleSkinUpload($event.target)">
                  <button class="acct-skin-import-btn" @click="pickSkinFile()">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>
                    <span>导入自定义皮肤</span>
                  </button>
                </div>
                <div class="acct-detail-skins" id="acct-detail-capes" style="display:none">
                  <span class="acct-detail-label">披风</span>
                  <div class="acct-skin-grid" id="acct-cape-grid"></div>
                  <input type="file" id="cape-file-input" accept=".png" style="display:none" @change="handleCapeUpload($event.target)">
                  <div class="acct-cape-btns">
                    <button class="acct-skin-import-btn" @click="pickCapeFile()">
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>
                      <span>导入披风</span>
                    </button>
                    <button class="acct-skin-import-btn" id="cape-fetch-session-btn" style="display:none" @click="fetchSessionCape()">
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" width="14" height="14"><polyline points="21 15 21 19 15 21"/><polyline points="3 9 3 5 9 3"/><line x1="12" y1="3" x2="12" y2="15"/><polyline points="8 7 12 3 16 7"/></svg>
                      <span>获取官方披风</span>
                    </button>
                    <button class="acct-skin-import-btn" @click="unselectCape()">
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><line x1="5" y1="12" x2="19" y2="12"/></svg>
                      <span>卸下披风</span>
                    </button>
                  </div>
                </div>
                <div class="acct-detail-actions">
                  <button class="acct-btn acct-btn-primary" @click="detailSelectAccount()">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" width="16" height="16"><polyline points="20 6 9 17 4 12"/></svg>
                    <span>切换到此账户</span>
                  </button>
                  <div class="acct-detail-actions-row">
                    <button class="acct-btn acct-btn-ghost" @click="detailRefreshSkin()">
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" width="15" height="15"><polyline points="23 4 23 10 17 10"/><polyline points="1 20 1 14 7 14"/><path d="M3.51 9a9 9 0 0114.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0020.49 15"/></svg>
                      <span>刷新皮肤</span>
                    </button>
                    <button class="acct-btn acct-btn-danger" @click="detailDeleteAccount()">
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" width="15" height="15"><path d="M9 21H5a2 2 0 01-2-2V5a2 2 0 012-2h4"/><polyline points="16 17 21 12 16 7"/><line x1="21" y1="12" x2="9" y2="12"/></svg>
                      <span>登出</span>
                    </button>
                  </div>
                </div>
              </div>
            </div>
          </div>
  `
};

export default PageAccounts;
