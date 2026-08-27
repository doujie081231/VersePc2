/**
 * rose-loader.js
 * 把页面中的 .spinner / .spinner-sm / .modal-spinner 替换为 Rose Curve 加载动画，
 * 完整还原 math-curve-loaders 的 Rose Curve 源码效果：
 *   - 路径随 "detailScale" 呼吸变化（0.52 ~ 1.0）
 *   - 粒子沿曲线运动并带拖影（trail）
 *   - 整体缓慢旋转
 * 通过 MutationObserver 自动生效，无需改动现有 HTML。
 */
(function () {
  var SVG_NS = 'http://www.w3.org/2000/svg';

  // 与源码一致的配置
  var CONFIG = {
    rotate: true,
    particleCount: 86,
    trailSpan: 0.12,
    durationMs: 5400,
    rotationDurationMs: 28000,
    pulseDurationMs: 4600,
    strokeWidth: 4.5,
    roseA: 9.2,
    roseABoost: 0.6,
    roseBreathBase: 0.72,
    roseBreathBoost: 0.28,
    roseK: 5,
    roseScale: 3.25
  };

  // 曲线点：与源码 point() 一致
  function point(progress, detailScale, config) {
    var t = progress * Math.PI * 2;
    var a = config.roseA + detailScale * config.roseABoost;
    var k = Math.round(config.roseK);
    var r = a * (config.roseBreathBase + detailScale * config.roseBreathBoost) * Math.cos(k * t);
    return {
      x: 50 + Math.cos(t) * r * config.roseScale,
      y: 50 + Math.sin(t) * r * config.roseScale
    };
  }

  function normalizeProgress(progress) {
    return ((progress % 1) + 1) % 1;
  }

  // 呼吸：与源码 getDetailScale() 一致
  function getDetailScale(time) {
    var pulseProgress = (time % CONFIG.pulseDurationMs) / CONFIG.pulseDurationMs;
    var pulseAngle = pulseProgress * Math.PI * 2;
    return 0.52 + ((Math.sin(pulseAngle + 0.55) + 1) / 2) * 0.48;
  }

  // 旋转：与源码 getRotation() 一致
  function getRotation(time) {
    if (!CONFIG.rotate) return 0;
    return -((time % CONFIG.rotationDurationMs) / CONFIG.rotationDurationMs) * 360;
  }

  // 动态路径
  function buildPath(detailScale, steps) {
    steps = steps || 480;
    var parts = [];
    for (var i = 0; i <= steps; i++) {
      var p = point(i / steps, detailScale, CONFIG);
      parts.push((i === 0 ? 'M' : 'L') + ' ' + p.x.toFixed(2) + ' ' + p.y.toFixed(2));
    }
    return parts.join(' ');
  }

  // 粒子：与源码 getParticle() 一致
  function getParticle(index, progress, detailScale) {
    var tailOffset = index / (CONFIG.particleCount - 1);
    var p = point(normalizeProgress(progress - tailOffset * CONFIG.trailSpan), detailScale, CONFIG);
    var fade = Math.pow(1 - tailOffset, 0.56);
    return {
      x: p.x,
      y: p.y,
      radius: 0.9 + fade * 2.7,
      opacity: 0.04 + fade * 0.96
    };
  }

  function createRoseAnimation(el) {
    var size = el.offsetWidth || 40;
    var stroke = size <= 24 ? 3 : CONFIG.strokeWidth;

    var svg = document.createElementNS(SVG_NS, 'svg');
    svg.setAttribute('viewBox', '0 0 100 100');
    svg.setAttribute('fill', 'none');
    svg.setAttribute('width', '100%');
    svg.setAttribute('height', '100%');
    svg.style.display = 'block';
    svg.style.overflow = 'visible';

    var group = document.createElementNS(SVG_NS, 'g');

    var path = document.createElementNS(SVG_NS, 'path');
    path.setAttribute('stroke', 'currentColor');
    path.setAttribute('stroke-linecap', 'round');
    path.setAttribute('stroke-linejoin', 'round');
    path.setAttribute('opacity', '0.1');
    path.setAttribute('stroke-width', String(stroke));
    group.appendChild(path);

    // 粒子圆（与源码一致）
    var particles = [];
    for (var n = 0; n < CONFIG.particleCount; n++) {
      var circle = document.createElementNS(SVG_NS, 'circle');
      circle.setAttribute('fill', 'currentColor');
      group.appendChild(circle);
      particles.push(circle);
    }

    svg.appendChild(group);
    el.appendChild(svg);

    var startedAt = performance.now();

    function render(now) {
      if (!el.isConnected) return;
      var time = now - startedAt;
      var progress = (time % CONFIG.durationMs) / CONFIG.durationMs;
      var detailScale = getDetailScale(time);

      group.setAttribute('transform', 'rotate(' + getRotation(time).toFixed(2) + ' 50 50)');

      path.setAttribute('d', buildPath(detailScale));

      for (var i = 0; i < particles.length; i++) {
        var particle = getParticle(i, progress, detailScale);
        particles[i].setAttribute('cx', particle.x.toFixed(2));
        particles[i].setAttribute('cy', particle.y.toFixed(2));
        particles[i].setAttribute('r', particle.radius.toFixed(2));
        particles[i].setAttribute('opacity', particle.opacity.toFixed(3));
      }
      requestAnimationFrame(render);
    }
    requestAnimationFrame(render);
  }

  function upgrade(el) {
    if (el.getAttribute('data-rose-init')) return;
    el.setAttribute('data-rose-init', '1');
    el.setAttribute('role', 'status');
    // 清掉纯 CSS 圆环样式，交给内联 SVG 展示
    el.style.border = 'none';
    el.style.background = 'transparent';
    el.style.display = 'inline-block';
    try { createRoseAnimation(el); } catch (e) {}
  }

  // 详情页等用"加载中..."文字占位，同样替换为玫瑰曲线动画
  var LOADING_RE = /加载(中|版本列表|中…|中\.\.\.)|正在(加载|获取)/;

  function upgradeTextLoader(el) {
    if (el.getAttribute('data-rose-pending') !== null) return;
    var text = (el.textContent || '').trim();
    if (!LOADING_RE.test(text)) return;
    el.setAttribute('data-rose-pending', '1');

    // 保留占位文字，前面插入玫瑰加载动画
    var holder = document.createElement('div');
    holder.className = 'rose-loader-holder';
    holder.style.cssText = 'display:flex;align-items:center;justify-content:center;gap:10px;width:100%;color:var(--accent,currentColor);';
    var spinner = document.createElement('div');
    spinner.className = 'rose-loader-inline';
    spinner.style.cssText = 'width:40px;height:40px;flex-shrink:0;color:inherit;';
    holder.appendChild(spinner);
    var label = document.createElement('span');
    label.style.color = 'var(--text-muted,inherit)';
    label.textContent = text;
    holder.appendChild(label);

    el.textContent = '';
    el.style.display = 'block';
    el.style.padding = '16px 0';
    el.appendChild(holder);
    try { createRoseAnimation(spinner); } catch (e) {}
  }

  function scan() {
    var list = document.querySelectorAll('.spinner, .spinner-sm, .modal-spinner');
    for (var i = 0; i < list.length; i++) upgrade(list[i]);

    // 文本型加载占位（详情页版本列表 / 依赖列表等）
    var textList = document.querySelectorAll('.empty-text');
    for (var j = 0; j < textList.length; j++) upgradeTextLoader(textList[j]);
  }

  // 初始扫描 + 监听动态插入
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', scan);
  } else {
    scan();
  }
  var mo = new MutationObserver(scan);
  mo.observe(document.documentElement, { childList: true, subtree: true });
})();