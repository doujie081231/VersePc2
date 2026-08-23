/**
 * @file dino-game.js - 赞助者彩蛋：恐龙跳跃小游戏
 * @description 在「赞助者」卡片输入订单号，验证通过后可进入全屏恐龙小游戏。
 *
 * 架构说明：
 *   - 游戏本身直接复用开源代码（dino/index.html、index.js、index.css、assets），
 *     dino 文件夹里的文件原样保留，想在游戏里改内容/皮肤/难度，直接改 dino 里的文件即可。
 *   - 本文件只负责「外壳」：一个覆盖整个窗口的全屏覆盖层，顶部提供「返回」按钮回到原页面。
 *   - 外壳与游戏用 iframe 隔离，互不干扰，不会和启动器现有界面发生样式/按键冲突。
 *
 * 订单号验证：
 *   - 由 verifySponsorOrder(orderId) 处理，调用统一 API 层 apiPost('/api/sponsor/verify')，
 *     在 Tauri 与 Electron 环境下均可用（Tauri 走 invoke('api_proxy')，Electron 走 fetch）。
 */

const DinoGame = {
  overlay: null,

  /** 打开全屏覆盖层，内嵌恐龙游戏 */
  open() {
    if (this.overlay) this.close();

    const ov = document.createElement('div');
    ov.id = 'dino-overlay';
    ov.style.cssText =
      'position:fixed;inset:0;z-index:99999;background:#fff;display:flex;flex-direction:column;' +
      'font-family:system-ui,-apple-system,"Segoe UI",sans-serif;';
    ov.innerHTML = `
      <div class="dino-topbar" style="display:flex;align-items:center;justify-content:space-between;padding:8px 14px;border-bottom:1px solid #e0e0e0;background:#fafafa;position:relative;z-index:9999;flex-shrink:0;">
        <button class="btn btn-ghost btn-sm dino-back-btn" type="button" style="position:relative;z-index:10000;pointer-events:auto;cursor:pointer;" onclick="window.closeDinoGame && window.closeDinoGame()">← 返回</button>
        <span style="font-size:13px;color:#888;">赞助者彩蛋 · 恐龙快跑</span>
        <span style="width:60px;"></span>
      </div>
      <div style="flex:1;position:relative;overflow:hidden;background:#fff;z-index:1;min-height:0;">
        <iframe id="dino-frame" src="dino/index.html"
          style="position:absolute;inset:0;width:100%;height:100%;border:0;display:block;"></iframe>
      </div>
    `;
    document.body.appendChild(ov);
    this.overlay = ov;

    // 返回按钮：事件监听绑定 + 内联 onclick 双保险，确保一定能关闭
    const backBtn = ov.querySelector('.dino-back-btn');
    if (backBtn) {
      const closeHandler = function () { DinoGame.close(); };
      backBtn.addEventListener('click', closeHandler);
      backBtn.addEventListener('pointerup', closeHandler);
    }

    // Esc 键也能关闭覆盖层回到原页面。
    // 注意：游戏在 iframe 里运行时会抢占键盘焦点，父窗口收不到 keydown，
    // 所以同时监听 iframe 内部（同源），保证游戏进行中按 Esc 也能返回。
    const escHandler = function (e) {
      if (e.key === 'Escape') DinoGame.close();
    };
    window.addEventListener('keydown', escHandler);
    this._escHandler = escHandler;

    const frame = ov.querySelector('#dino-frame');
    if (frame) {
      const bindFrameEsc = function () {
        try {
          frame.contentWindow.addEventListener('keydown', escHandler);
        } catch (err) { /* 跨域时忽略，父窗口监听仍可用 */ }
      };
      if (frame.contentWindow) {
        bindFrameEsc();
      } else {
        frame.addEventListener('load', bindFrameEsc);
      }
      this._frameEscHandler = escHandler;
    }
  },

  /** 关闭覆盖层，回到原页面 */
  close() {
    if (this.overlay) {
      const frame = this.overlay.querySelector('#dino-frame');
      if (frame && frame.contentWindow) {
        try { frame.contentWindow.removeEventListener('keydown', this._frameEscHandler); } catch (err) {}
      }
    }
    if (this._escHandler) {
      window.removeEventListener('keydown', this._escHandler);
      this._escHandler = null;
    }
    this._frameEscHandler = null;
    if (this.overlay) {
      this.overlay.remove();
      this.overlay = null;
    }
  }
};

/** 全局兜底关闭函数，供内联 onclick 使用 */
window.closeDinoGame = function () {
  DinoGame.close();
};

/**
 * 订单号验证。
 * 提交订单号到后端校验是否在真实赞助订单白名单中。
 * @param {string} orderId 用户输入的赞助订单号
 * @returns {Promise<{ok: boolean, msg?: string}>}
 */
async function verifySponsorOrder(orderId) {
  try {
    const data = await apiPost('/api/sponsor/verify', { orderId });
    return data.ok ? { ok: true } : { ok: false, msg: data.msg || '订单号无效' };
  } catch (e) {
    console.error('订单号验证请求失败:', e);
    return { ok: false, msg: '验证服务不可用，请稍后重试' };
  }
}

/** 赞助者卡片「进入小游戏」点击处理 */
async function openSponsorDino() {
  const input = document.getElementById('sponsor-order-input');
  const orderId = input ? input.value.trim() : '';
  if (!orderId) {
    showToast('请输入赞助订单号', 'info');
    return;
  }
  const result = await verifySponsorOrder(orderId);
  if (result.ok) {
    window.dinoGame.open();
  } else {
    showToast(result.msg || '订单号验证失败', 'error');
  }
}

window.dinoGame = DinoGame;
window.verifySponsorOrder = verifySponsorOrder;
window.openSponsorDino = openSponsorDino;