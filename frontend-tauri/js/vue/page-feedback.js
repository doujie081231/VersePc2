/* page-feedback - 问题反馈页 Vue 组件
 * 模板只负责结构，交互与数据全部在 js/app/feedback.js 中实现
 */
const PageFeedback = {
  template: `
          <div class="page-header">
            <h2>问题反馈</h2>
            <div class="page-actions">
              <button class="btn btn-secondary btn-sm" onclick="openFeedbackSite()">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="fb-inline-icon"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/></svg>
                访问官网
              </button>
            </div>
          </div>

          <div class="feedback-container">

            <div class="card">
              <h3>反馈类型</h3>
              <div class="fb-type-group" id="fb-type-group">
                <button class="fb-type-btn active" data-type="bug" onclick="selectFeedbackType(this)">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 6a3 3 0 0 1 6 0"/><rect x="8" y="6" width="8" height="12" rx="4"/><path d="M8 10H4M8 14H4M8 18l-3 2M16 10h4M16 14h4M16 18l3 2"/></svg>
                  <span class="fb-type-title">问题反馈</span>
                  <span class="fb-type-desc">崩溃、报错、功能异常</span>
                </button>
                <button class="fb-type-btn" data-type="feature" onclick="selectFeedbackType(this)">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2a7 7 0 0 0-4 12.7V17h8v-2.3A7 7 0 0 0 12 2z"/><line x1="9.5" y1="20" x2="14.5" y2="20"/><line x1="10.5" y1="23" x2="13.5" y2="23"/></svg>
                  <span class="fb-type-title">功能建议</span>
                  <span class="fb-type-desc">希望增加或改进的功能</span>
                </button>
                <button class="fb-type-btn" data-type="other" onclick="selectFeedbackType(this)">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 11.5a8.5 8.5 0 0 1-8.5 8.5 8.6 8.6 0 0 1-3.6-.8L3 21l1.8-5.5A8.5 8.5 0 0 1 12.5 3 8.5 8.5 0 0 1 21 11.5z"/></svg>
                  <span class="fb-type-title">其他</span>
                  <span class="fb-type-desc">使用体验、咨询等</span>
                </button>
              </div>
            </div>

            <div class="card">
              <h3>问题分类</h3>
              <div class="fb-category-group" id="fb-category-group">
                <button class="fb-category-btn" data-cat="launch" onclick="selectFeedbackCategory(this)">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/></svg>
                  <span class="fb-category-title">启动问题</span>
                  <span class="fb-category-desc">启动失败、崩溃、闪退</span>
                </button>
                <button class="fb-category-btn" data-cat="download" onclick="selectFeedbackCategory(this)">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
                  <span class="fb-category-title">下载问题</span>
                  <span class="fb-category-desc">资源下载慢、卡住、校验失败</span>
                </button>
                <button class="fb-category-btn active" data-cat="other" onclick="selectFeedbackCategory(this)">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9"/><path d="M12 8v4M12 16h.01"/></svg>
                  <span class="fb-category-title">其他</span>
                  <span class="fb-category-desc">功能建议、体验优化等</span>
                </button>
              </div>
              <span class="form-hint">选择分类后将自动附带对应日志，请先复现问题再上传</span>
            </div>

            <div class="card">
              <h3>反馈内容</h3>
              <div class="form-group">
                <label>标题 <span class="fb-required">*</span></label>
                <input type="text" id="fb-title" class="text-input" maxlength="50" placeholder="一句话概括你遇到的问题" oninput="updateFeedbackSubmitState()">
                <span class="form-hint">例如：启动 1.20.1 Forge 时崩溃并提示找不到主类</span>
              </div>
              <div class="form-group">
                <label>详细描述 <span class="fb-required">*</span></label>
                <textarea id="fb-detail" class="text-input fb-textarea" maxlength="2000" placeholder="请描述复现步骤、期望结果与实际结果，越具体越容易定位" oninput="onFeedbackDetailInput()"></textarea>
                <span class="form-hint fb-counter" id="fb-detail-counter">0 / 2000</span>
              </div>
              <div class="form-group">
                <label>联系方式（选填）</label>
                <input type="text" id="fb-contact" class="text-input" maxlength="80" placeholder="邮箱 / QQ，便于我们回复你">
                <span class="form-hint">不填也能提交，但可能无法收到处理结果</span>
              </div>
            </div>

            <div class="card">
              <h3>日志附件<span class="fb-auto-log-badge" id="fb-auto-log-badge" hidden>自动收集</span></h3>
              <div class="fb-auto-log-tip" id="fb-auto-log-tip">选择「启动问题」或「下载问题」后自动收集 data/logs 下的相关日志</div>
              <div class="fb-file-list" id="fb-auto-log-list"></div>
              <span class="form-hint fb-warn">提交前请一定先复现问题，确保日志已记录到本次异常，再上传</span>
            </div>

            <div class="card">
              <h3>截图或视频</h3>
              <div class="fb-drop" id="fb-drop" onclick="pickFeedbackFiles()">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="fb-drop-icon"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 9 12 4 17 9"/><line x1="12" y1="4" x2="12" y2="16"/></svg>
                <div class="fb-drop-title">点击选择文件，或把文件拖到这里</div>
                <div class="fb-drop-desc">支持截图、视频，最多 10 个，单个不超过 20 MB</div>
              </div>
              <input type="file" id="fb-file-input" multiple accept="image/*,video/*" hidden onchange="onFeedbackFilesPicked(this.files)">
              <div class="fb-file-list" id="fb-file-list"></div>
            </div>

            <div class="card">
              <h3>环境信息</h3>
              <div class="form-group">
                <label class="checkbox-label">
                  <input type="checkbox" id="fb-include-env" checked>
                  <span>随反馈一并提交（推荐）</span>
                </label>
                <span class="form-hint">包含启动器版本、系统与 Java 信息，有助于快速定位问题</span>
              </div>
              <div class="fb-env-list" id="fb-env-list"></div>
            </div>

            <div class="card">
              <!-- 人机验证：接入站点验证码接口后在此渲染，未接入时保持隐藏 -->
              <div class="form-group fb-captcha-group" id="fb-captcha-group" hidden>
                <label>人机验证 <span class="fb-required">*</span></label>
                <div class="fb-captcha-row">
                  <input type="text" id="fb-captcha-input" class="text-input fb-captcha-input" maxlength="4" placeholder="输入图中 4 位字符" oninput="updateFeedbackSubmitState()">
                  <div class="fb-captcha-image" id="fb-captcha-image" onclick="refreshFeedbackCaptcha()" title="看不清？点击换一张"></div>
                </div>
                <span class="form-hint">点击图片可更换</span>
              </div>
              <div class="fb-submit-bar">
                <span class="fb-submit-hint" id="fb-submit-hint"></span>
                <button class="btn btn-secondary btn-sm" id="fb-reset-btn" onclick="resetFeedbackForm()">重置</button>
                <button class="btn btn-primary" id="fb-submit-btn" onclick="submitFeedback()" disabled>提交反馈</button>
              </div>
            </div>

          </div>
  `
};

window.VersePC = window.VersePC || {};
window.VersePC.PageFeedback = PageFeedback;