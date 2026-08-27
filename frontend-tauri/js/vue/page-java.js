/* page-java.js - Java页 Vue 组件（渐进式改造）
 * 原则：
 *   1. CSS 一行不动（class 名保留原样）
 *   2. HTML 结构原样搬运（标签、层级、id 全部不变）
 *   3. JS 函数全部复用（来自 js/app/*.js 的全局函数）
 */
const PageJava = {
  template: `
          <div class="page-header">
            <h2>Java 管理</h2>
            <p class="page-desc">管理和下载Java运行时环境</p>
          </div>
          
          <div class="card">
            <h3>已安装的Java</h3>
            <div id="installed-java-list" class="java-list">
              <div class="loading">正在检测Java...</div>
            </div>
            <div style="display: flex; gap: 8px; flex-wrap: wrap; margin-top: 12px;">
              <button id="refresh-java-btn" class="btn btn-secondary">刷新列表</button>
              <button id="add-manual-java-btn" class="btn btn-secondary">手动添加 Java</button>
              <button id="import-archive-btn" class="btn btn-secondary">导入压缩包</button>
              <button id="import-directory-btn" class="btn btn-secondary">导入文件夹</button>
            </div>
            <p class="hint" style="margin-top: 8px; font-size: 12px;">
              「手动添加」：选择电脑上已有的 java.exe 加入列表（原位引用，不复制文件）<br>
              「导入压缩包」：导入 .zip 格式的 Java 压缩包到启动器目录<br>
              「导入文件夹」：导入已解压好的 Java 文件夹到启动器目录
            </p>
          </div>

          <div id="java-import-progress" class="card" style="margin-top: 20px; display: none;">
            <h3>导入进度</h3>
            <div class="progress-container">
              <div class="progress-bar">
                <div id="java-import-fill" class="progress-fill" style="width: 0%"></div>
              </div>
              <div id="java-import-text" class="progress-text">0%</div>
            </div>
            <p id="java-import-message" class="hint" style="margin-top: 8px;">准备导入...</p>
          </div>
          
          <div class="card" style="margin-top: 20px;">
            <h3>下载Java</h3>
            <p class="hint">选择需要的Java版本进行下载，系统将自动尝试多个下载源以获得最佳速度</p>
            
            <div id="java-download-list" class="java-download-list">
              <div class="loading">正在获取Java版本列表...</div>
            </div>

            <div class="card" style="margin-top: 20px;">
              <h3>Minecraft JE 版本与 JDK 版本对应关系</h3>
              <div style="font-size:13px;color:var(--text-secondary);line-height:1.8;">
                <div>1.16.4 或更早 : JDK 8 (Java 8)</div>   
                <div>1.16.5 : JDK 8 或 11 (Java 8/11)</div>
                <div>1.17 - 1.17.1 : JDK 16 (Java 16)</div>   
                <div>1.18 - 1.20.6 : JDK 17 (Java 17)</div>   
                <div>1.21.x : JDK 21 (Java 21)</div>   
                <div>26.x 及更高版本 : JDK 25 (Java 25)</div>
              </div>
            </div>
            
            <div class="java-download-tip" style="margin-top: 16px; padding: 12px 16px; background: var(--bg-tertiary); border-radius: var(--radius); border-left: 3px solid var(--orange); font-size: 13px; color: var(--text-secondary); line-height: 1.6;">
              <strong style="color: var(--orange);">提示：</strong>下载将自动尝试国内镜像源（中科大/清华/中科院），如全部失败会回退到 Adoptium 官方。你也可以前往 <a href="https://adoptium.net/zh-CN/temurin/releases/" target="_blank" style="color: var(--accent); text-decoration: underline;">Adoptium 官网</a> 手动下载，安装后在上方「已安装的Java」中点击刷新即可自动检测。
            </div>
          </div>
          
          <div id="java-download-progress" class="card" style="margin-top: 20px; display: none;">
            <h3>下载进度</h3>
            <div class="progress-container">
              <div class="progress-bar">
                <div id="java-progress-fill" class="progress-fill" style="width: 0%"></div>
              </div>
              <div id="java-progress-text" class="progress-text">0%</div>
            </div>
            <p id="java-progress-message" class="hint" style="margin-top: 8px;">准备下载...</p>
            <div style="margin-top: 12px;">
              <button class="btn btn-danger btn-sm" id="java-cancel-btn" onclick="cancelJavaDownload()" style="display: none;">取消下载</button>
            </div>
          </div>
  `
};

window.VersePC = window.VersePC || {};
window.VersePC.PageJava = PageJava;
