/* page-console - 控制台页 Vue 组件 */
const PageConsole = {
  template: `
          <div class="page-header">
            <h2>游戏日志</h2>
            <div class="page-actions">
              <button id="export-log-btn" class="btn btn-accent btn-sm" onclick="exportGameLog()">导出日志</button>
              <button id="clear-log-btn" class="btn btn-secondary btn-sm">清空</button>
            </div>
          </div>
          <div id="console-output" class="console-output">
            <p class="console-wait">等待游戏启动...</p>
          </div>
  `
};

window.VersePC = window.VersePC || {};
window.VersePC.PageConsole = PageConsole;
