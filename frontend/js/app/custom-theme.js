/**
 * @file custom-theme.js
 * @description 自定义主题色 - 根据用户选择的颜色通过 HSL 算法动态生成整套 CSS 主题变量，
 *              使界面（背景、强调色、边框、进度条、玻璃质感等）全部自适应主题色。
 *              支持深色基底与浅色基底两种模式。
 */

const CUSTOM_THEME_DEFAULT_COLOR = '#4c8dff';
const CUSTOM_THEME_STORE_KEY = 'versepc_custom_theme_color';
const CUSTOM_THEME_LIGHT_STORE_KEY = 'versepc_custom_theme_light';

/** 记录所有被 inline 覆盖的 CSS 变量名，便于切回黑/白主题时清除 */
const _CUSTOM_THEME_VARS = [
  '--bg-primary', '--bg-secondary', '--bg-tertiary', '--bg-card', '--bg-hover', '--bg-active',
  '--accent', '--accent-hover', '--accent-glow', '--accent-text', '--accent-rgb',
  '--bg-glass', '--bg-card-glass',
  '--border', '--hover-overlay', '--dot-border',
  '--splash-bg-start', '--splash-bg-end',
  '--launch-bg', '--launch-track-bg', '--launch-progress-bg', '--launch-status-color',
  '--hero-gradient-start', '--hero-gradient-end',
  '--sidebar-bg-solid'
];

// ─── 颜色空间转换工具 ─────────────────────────────────────

function _hexToRgb(hex) {
  const m = /^#?([0-9a-fA-F]{6})$/.exec(String(hex).trim());
  if (!m) return null;
  const int = parseInt(m[1], 16);
  return { r: (int >> 16) & 255, g: (int >> 8) & 255, b: int & 255 };
}

function _rgbToHsl(r, g, b) {
  r /= 255; g /= 255; b /= 255;
  const max = Math.max(r, g, b), min = Math.min(r, g, b);
  let h = 0, s = 0;
  const l = (max + min) / 2;
  const d = max - min;
  if (d !== 0) {
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
    switch (max) {
      case r: h = (g - b) / d + (g < b ? 6 : 0); break;
      case g: h = (b - r) / d + 2; break;
      default: h = (r - g) / d + 4;
    }
    h *= 60;
  }
  return { h, s: s * 100, l: l * 100 };
}

function _hslToHex(h, s, l) {
  h = ((h % 360) + 360) % 360;
  s = Math.max(0, Math.min(100, s)) / 100;
  l = Math.max(0, Math.min(100, l)) / 100;
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = l - c / 2;
  let r = 0, g = 0, b = 0;
  if (h < 60) { r = c; g = x; }
  else if (h < 120) { r = x; g = c; }
  else if (h < 180) { g = c; b = x; }
  else if (h < 240) { g = x; b = c; }
  else if (h < 300) { r = x; b = c; }
  else { r = c; b = x; }
  const to = (v) => Math.round((v + m) * 255).toString(16).padStart(2, '0');
  return `#${to(r)}${to(g)}${to(b)}`;
}

/** 根据背景色亮度返回具有足够对比度的文字色（黑或白） */
function _contrastTextColor(hex) {
  const rgb = _hexToRgb(hex);
  if (!rgb) return '#ffffff';
  // W3C 相对亮度公式
  const luminance = (0.299 * rgb.r + 0.587 * rgb.g + 0.114 * rgb.b) / 255;
  return luminance > 0.55 ? '#0a0a0a' : '#ffffff';
}

// ─── 主题变量生成与应用 ───────────────────────────────────

/**
 * 根据主题色生成整套 CSS 变量
 * @param {string} hex 主题色，如 '#4c8dff'
 * @param {boolean} [isLight=false] 是否使用浅色基底
 * @returns {Object<string,string>} 变量名 → 值
 */
function generateCustomThemeVars(hex, isLight = false) {
  const rgb = _hexToRgb(hex);
  if (!rgb) return null;
  const { h, s, l } = _rgbToHsl(rgb.r, rgb.g, rgb.b);

  // 无彩色（黑/白/灰）饱和度极低时，保持灰阶，避免色相退化为红色
  const isAchromatic = s < 10;
  // 背景饱和度：降低，避免大面积刺眼
  const bgSat = isAchromatic ? 0 : Math.min(s * 0.45, 45);
  // 强调色饱和度：保证鲜明
  const accS = isAchromatic ? 0 : Math.min(Math.max(s, 55), 95);
  const accL = Math.min(Math.max(l, 52), 68);
  const accent = _hslToHex(h, accS, accL);
  const accentHover = _hslToHex(h, accS, Math.min(accL + 12, 80));
  const aRgb = _hexToRgb(accent);

  if (isLight) {
    return {
      '--bg-primary': _hslToHex(h, bgSat, 98),
      '--bg-secondary': _hslToHex(h, bgSat, 96),
      '--bg-tertiary': _hslToHex(h, bgSat, 93),
      '--bg-card': _hslToHex(h, bgSat, 97),
      '--bg-hover': _hslToHex(h, bgSat, 91),
      '--bg-active': _hslToHex(h, bgSat, 87),

      '--accent': accent,
      '--accent-hover': accentHover,
      '--accent-rgb': `${aRgb.r}, ${aRgb.g}, ${aRgb.b}`,
      '--accent-glow': `rgba(${aRgb.r}, ${aRgb.g}, ${aRgb.b}, 0.25)`,
      '--accent-text': _contrastTextColor(accent),

      '--bg-glass': `hsla(${Math.round(h)}, ${Math.round(bgSat)}%, 96%, 0.85)`,
      '--bg-card-glass': `hsla(${Math.round(h)}, ${Math.round(bgSat)}%, 97%, 0.7)`,

      '--border': `hsla(${Math.round(h)}, ${Math.round(accS)}%, ${Math.round(accL)}%, 0.22)`,
      '--hover-overlay': `hsla(${Math.round(h)}, ${Math.round(accS)}%, ${Math.round(accL)}%, 0.06)`,
      '--dot-border': `hsla(${Math.round(h)}, ${Math.round(accS)}%, ${Math.round(accL)}%, 0.45)`,

      '--splash-bg-start': _hslToHex(h, bgSat, 98),
      '--splash-bg-end': _hslToHex(h, bgSat, 95),

      '--launch-bg': _hslToHex(h, bgSat, 98),
      '--launch-track-bg': `hsla(${Math.round(h)}, ${Math.round(accS)}%, ${Math.round(accL)}%, 0.18)`,
      '--launch-progress-bg': `linear-gradient(90deg, ${accentHover}, ${accent})`,
      '--launch-status-color': `hsla(${Math.round(h)}, ${Math.round(isAchromatic ? 0 : Math.max(s, 30))}%, 45%, 0.6)`,

      '--hero-gradient-start': _hslToHex(h, accS, Math.max(accL, 60)),
      '--hero-gradient-end': _hslToHex(h, accS, Math.max(accL - 22, 36)),

      '--sidebar-bg-solid': `hsla(${Math.round(h)}, ${Math.round(bgSat)}%, 96%, 0.95)`
    };
  }

  return {
    '--bg-primary': _hslToHex(h, bgSat, 7),
    '--bg-secondary': _hslToHex(h, bgSat, 10.5),
    '--bg-tertiary': _hslToHex(h, bgSat, 13.5),
    '--bg-card': _hslToHex(h, bgSat, 12),
    '--bg-hover': _hslToHex(h, bgSat, 17),
    '--bg-active': _hslToHex(h, bgSat, 21),

    '--accent': accent,
    '--accent-hover': accentHover,
    '--accent-rgb': `${aRgb.r}, ${aRgb.g}, ${aRgb.b}`,
    '--accent-glow': `rgba(${aRgb.r}, ${aRgb.g}, ${aRgb.b}, 0.25)`,
    '--accent-text': _contrastTextColor(accent),

    '--bg-glass': `hsla(${Math.round(h)}, ${Math.round(bgSat)}%, 8%, 0.85)`,
    '--bg-card-glass': `hsla(${Math.round(h)}, ${Math.round(bgSat)}%, 12%, 0.7)`,

    '--border': `hsla(${Math.round(h)}, ${Math.round(accS)}%, ${Math.round(accL)}%, 0.16)`,
    '--hover-overlay': `hsla(${Math.round(h)}, ${Math.round(accS)}%, ${Math.round(accL)}%, 0.07)`,
    '--dot-border': `hsla(${Math.round(h)}, ${Math.round(accS)}%, ${Math.round(accL)}%, 0.35)`,

    '--splash-bg-start': _hslToHex(h, bgSat, 8),
    '--splash-bg-end': _hslToHex(h, bgSat, 5),

    '--launch-bg': _hslToHex(h, bgSat, 8),
    '--launch-track-bg': `hsla(${Math.round(h)}, ${Math.round(accS)}%, ${Math.round(accL)}%, 0.14)`,
    '--launch-progress-bg': `linear-gradient(90deg, ${accentHover}, ${accent})`,
    '--launch-status-color': `hsla(${Math.round(h)}, ${Math.round(isAchromatic ? 0 : Math.max(s, 30))}%, 75%, 0.5)`,

    '--hero-gradient-start': _hslToHex(h, accS, Math.max(accL, 60)),
    '--hero-gradient-end': _hslToHex(h, accS, Math.max(accL - 22, 36)),

    '--sidebar-bg-solid': `hsla(${Math.round(h)}, ${Math.round(bgSat)}%, 10%, 0.95)`
  };
}

/**
 * 应用自定义主题色：生成变量并 inline 设置到根元素
 * @param {string} color 主题色 hex
 * @param {Object} [opts]
 * @param {boolean} [opts.save=true] 是否持久化颜色
 * @param {boolean} [opts.isLight] 是否浅色模式（不指定则读取保存值）
 */
async function applyCustomThemeColor(color, opts = {}) {
  let { save = true, isLight } = opts;
  if (typeof isLight !== 'boolean') {
    isLight = await getSavedCustomThemeLight();
  }
  const vars = generateCustomThemeVars(color, isLight);
  if (!vars) return;

  const root = document.documentElement;
  for (const [name, value] of Object.entries(vars)) {
    root.style.setProperty(name, value);
  }

  syncCustomThemeColorUI(color);
  setCustomThemeMode(isLight, { save: false });

  if (save) {
    try {
      await window.electronAPI?.store?.set(CUSTOM_THEME_STORE_KEY, color);
    } catch (e) {
      console.error('[Theme] Save custom theme color error:', e);
    }
  }
}

/** 设置自定义主题深浅模式 */
async function setCustomThemeMode(isLight, opts = {}) {
  const { save = true } = opts;
  const root = document.documentElement;
  if (isLight) {
    root.setAttribute('data-custom-theme-mode', 'light');
    document.documentElement.classList.remove('dark-theme');
    document.documentElement.classList.add('light-theme');
  } else {
    root.removeAttribute('data-custom-theme-mode');
    document.documentElement.classList.add('dark-theme');
    document.documentElement.classList.remove('light-theme');
  }
  if (save) {
    try {
      await window.electronAPI?.store?.set(CUSTOM_THEME_LIGHT_STORE_KEY, isLight);
    } catch (e) {
      console.error('[Theme] Save custom theme light mode error:', e);
    }
  }
}

/** 从 store 读取保存的浅色模式状态 */
async function getSavedCustomThemeLight() {
  try {
    const saved = await window.electronAPI?.store?.get(CUSTOM_THEME_LIGHT_STORE_KEY);
    return saved === true || saved === 'true' || saved === '1';
  } catch (e) { /* ignore */ }
  return false;
}

/** 清除所有自定义主题 inline 变量（切回黑/白主题时调用） */
function clearCustomThemeVars() {
  const root = document.documentElement;
  for (const name of _CUSTOM_THEME_VARS) {
    root.style.removeProperty(name);
  }
  root.removeAttribute('data-custom-theme-mode');
}

/** 从 store 读取保存的自定义主题色 */
async function getSavedCustomThemeColor() {
  try {
    const saved = await window.electronAPI?.store?.get(CUSTOM_THEME_STORE_KEY);
    if (saved && _hexToRgb(saved)) return saved;
  } catch (e) { /* ignore */ }
  return CUSTOM_THEME_DEFAULT_COLOR;
}

// ─── UI 交互 ─────────────────────────────────────────────

/** 同步颜色选择器控件状态（颜色值、文本、预设色块、主题卡色点、深浅开关） */
function syncCustomThemeColorUI(color) {
  const colorInput = document.getElementById('custom-theme-color-input');
  if (colorInput && colorInput.value !== color) colorInput.value = color;

  const valueEl = document.getElementById('custom-theme-color-value');
  if (valueEl) valueEl.textContent = color;

  const swatchDot = document.getElementById('custom-theme-swatch-dot');
  if (swatchDot) swatchDot.style.background = color;

  document.querySelectorAll('.custom-theme-preset').forEach(preset => {
    preset.classList.toggle('active', preset.dataset.color?.toLowerCase() === color.toLowerCase());
  });
}

/** 同步浅色模式开关状态 */
function syncCustomThemeLightModeUI(isLight) {
  const toggle = document.getElementById('custom-theme-light-mode');
  if (toggle) toggle.checked = !!isLight;
}

/** 显示/隐藏自定义颜色选择器区域 */
function toggleCustomThemeColorGroup(show) {
  const group = document.getElementById('custom-theme-color-group');
  if (group) group.style.display = show ? '' : 'none';
}

/** 颜色选择器 input 事件（实时预览并保存） */
function onCustomThemeColorInput(color) {
  if (document.documentElement.getAttribute('data-theme') !== 'custom') return;
  applyCustomThemeColor(color);
}

/** 浅色模式开关变化 */
async function onCustomThemeLightModeChange(isLight) {
  if (document.documentElement.getAttribute('data-theme') !== 'custom') return;
  const color = document.getElementById('custom-theme-color-input')?.value || CUSTOM_THEME_DEFAULT_COLOR;
  await setCustomThemeMode(isLight);
  await applyCustomThemeColor(color);
}

/** 预设色块点击：同步到 color input 并应用 */
function pickCustomThemePreset(color) {
  if (document.documentElement.getAttribute('data-theme') !== 'custom') return;
  applyCustomThemeColor(color);
}
