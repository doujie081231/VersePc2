// 图片加载失败兜底：用 data-img-fallback 声明行为，避免内联 onerror（CSP 禁止内联事件处理器）。
// 支持的模式：
//   hide          隐藏图片
//   visibility    隐藏图片占位（保留布局）
//   sibling       隐藏图片并显示下一个兄弟节点（占位图标）
//   modicon       隐藏图片并给父节点加 mod-icon--fallback
//   modmgr        隐藏图片并给父节点加 modmgr-icon--fallback
//   remove-parent 移除父节点
export function installImageFallback() {
  if (typeof document === 'undefined') return;
  document.addEventListener(
    'error',
    (event) => {
      const img = event.target;
      if (!(img instanceof HTMLImageElement)) return;
      const mode = img.getAttribute('data-img-fallback');
      if (!mode) return;
      img.style.display = 'none';
      switch (mode) {
        case 'visibility':
          img.style.display = '';
          img.style.visibility = 'hidden';
          break;
        case 'sibling':
          if (img.nextElementSibling) img.nextElementSibling.style.display = 'flex';
          break;
        case 'modicon':
          if (img.parentElement) img.parentElement.classList.add('mod-icon--fallback');
          break;
        case 'modmgr':
          if (img.parentElement) img.parentElement.classList.add('modmgr-icon--fallback');
          break;
        case 'remove-parent':
          if (img.parentElement) img.parentElement.remove();
          break;
        default:
          break;
      }
    },
    true
  );
}
