/**
 * liquid-glass.js - 苹果 Liquid Glass 折射引擎
 * ----------------------------------------------------------------------------
 * 文件职责：为指定元素加上"苹果液态玻璃"折射效果。
 *
 * 实现思路参考开源项目 shuding/liquid-glass：
 *   玻璃的折射不靠模糊，而靠位移贴图。贴图由圆角矩形有向距离场（SDF）生成 ——
 *   只在玻璃边缘形成一圈折射带，中心保持通透清晰，与真实弧形玻璃一致。
 *   贴图经 feImage 送入 feDisplacementMap，再由 backdrop-filter: url(#filter)
 *   作用到元素后方的背景上，使背景被真正弯折（而非随机噪声扭曲）。
 *
 * 另含色散（chromatic aberration）：位移按 R/G/B 三个不同比例各跑一遍，
 * 分通道取出后叠加，在玻璃边缘形成极细的红蓝彩边。
 *
 * 用法：
 *   window.LiquidGlass.apply(document.querySelector('.sidebar'), { strength: 0.35 });
 *   window.LiquidGlass.remove(element);
 */

(function () {
    'use strict';

    const SVG_NS = 'http://www.w3.org/2000/svg';
    const XLINK_NS = 'http://www.w3.org/1999/xlink';
    const STYLE_ID = 'liquid-glass-style';
    const MAX_MAP_EDGE = 256;   // 贴图最长边，降低分辨率以压低生成开销
    const MIN_EDGE = 8;

    const DEFAULTS = {
        depth: 0.45,        // 折射带厚度（相对圆角半径）
        strength: 0.35,     // 折射强度（最大位移 = strength × 半径）
        chroma: 0.06,       // 色散强度（R/B 位移比例的差异）
        blur: 1.5,          // 玻璃本体的雾度
        saturate: 1.8,
        brightness: 1.04
    };

    const surfaces = new Map();
    let sequence = 0;

    function clamp(value, min, max) {
        return value < min ? min : (value > max ? max : value);
    }

    /** 圆角矩形有向距离场：内部为负、边界为 0、外部为正 */
    function roundedRectSDF(px, py, halfW, halfH, radius) {
        const qx = Math.abs(px) - (halfW - radius);
        const qy = Math.abs(py) - (halfH - radius);
        return Math.min(Math.max(qx, qy), 0) +
            Math.hypot(Math.max(qx, 0), Math.max(qy, 0)) - radius;
    }

    /**
     * 生成位移贴图：R/G 通道编码 x/y 方向的位移（127.5 表示不偏移）。
     * 位移方向取 SDF 梯度（即表面法线），强度沿边缘向内衰减，形成一圈均匀的折射带。
     */
    function buildDisplacementMap(width, height, options) {
        const shrink = Math.min(1, MAX_MAP_EDGE / Math.max(width, height));
        const mapW = Math.max(MIN_EDGE, Math.round(width * shrink));
        const mapH = Math.max(MIN_EDGE, Math.round(height * shrink));

        const canvas = document.createElement('canvas');
        canvas.width = mapW;
        canvas.height = mapH;
        const context = canvas.getContext('2d');
        const image = context.createImageData(mapW, mapH);
        const pixels = image.data;

        const halfW = width / 2;
        const halfH = height / 2;
        const radius = Math.min(halfW, halfH);
        const depth = Math.max(2, radius * options.depth);
        const reach = options.strength * radius;

        const offsets = new Float32Array(mapW * mapH * 2);
        let peak = 0;

        for (let y = 0; y < mapH; y++) {
            const py = (y + 0.5) / shrink - halfH;
            for (let x = 0; x < mapW; x++) {
                const px = (x + 0.5) / shrink - halfW;

                const distance = roundedRectSDF(px, py, halfW, halfH, radius);
                // 边界处折射最强（1），向内 depth 像素衰减到 0，中心完全通透
                const band = clamp(1 + distance / depth, 0, 1);
                const amount = band * reach;

                // SDF 梯度即表面法线，位移沿法线指向玻璃内部
                const gradX = roundedRectSDF(px + 0.5, py, halfW, halfH, radius) -
                    roundedRectSDF(px - 0.5, py, halfW, halfH, radius);
                const gradY = roundedRectSDF(px, py + 0.5, halfW, halfH, radius) -
                    roundedRectSDF(px, py - 0.5, halfW, halfH, radius);
                const norm = Math.hypot(gradX, gradY) || 1;

                const dx = -(gradX / norm) * amount;
                const dy = -(gradY / norm) * amount;

                const index = (y * mapW + x) * 2;
                offsets[index] = dx;
                offsets[index + 1] = dy;
                const magnitude = Math.max(Math.abs(dx), Math.abs(dy));
                if (magnitude > peak) peak = magnitude;
            }
        }

        if (peak < 1e-4) peak = 1;

        for (let i = 0, p = 0; p < mapW * mapH; p++, i += 4) {
            const k = p * 2;
            pixels[i] = 127.5 + (offsets[k] / peak) * 127.5;
            pixels[i + 1] = 127.5 + (offsets[k + 1] / peak) * 127.5;
            pixels[i + 2] = 0;
            pixels[i + 3] = 255;
        }
        context.putImageData(image, 0, 0);

        return { href: canvas.toDataURL('image/png'), scale: 2 * peak };
    }

    function styleHost() {
        let host = document.getElementById(STYLE_ID);
        if (!host) {
            host = document.createElement('style');
            host.id = STYLE_ID;
            document.head.appendChild(host);
        }
        return host;
    }

    /** 依据当前生效的玻璃表面重建样式表；未启用时整表清空 */
    function syncStyles() {
        const rules = [];
        surfaces.forEach((surface) => {
            const value = 'url(#' + surface.filterId + ') blur(' + surface.options.blur +
                'px) saturate(' + surface.options.saturate + ') brightness(' + surface.options.brightness + ')';
            const selector = '[data-liquid-glass-ref="' + surface.filterId + '"]';
            rules.push(selector + '{backdrop-filter:' + value + ' !important;-webkit-backdrop-filter:' + value + ' !important;}');
        });
        styleHost().textContent = rules.join('\n');
    }

    class LiquidGlassSurface {
        constructor(element, options) {
            this.element = element;
            this.options = Object.assign({}, DEFAULTS, options || {});
            this.filterId = 'liquid-glass-' + (++sequence);
            this.width = 0;
            this.height = 0;
            this.frame = 0;
            this.build();
            this.measure(true);
            this.observer = new ResizeObserver(() => this.schedule());
            this.observer.observe(element);
        }

        build() {
            const svg = document.createElementNS(SVG_NS, 'svg');
            svg.setAttribute('aria-hidden', 'true');
            svg.style.cssText = 'position:fixed;top:0;left:0;width:0;height:0;overflow:hidden;pointer-events:none';

            const filter = document.createElementNS(SVG_NS, 'filter');
            filter.setAttribute('id', this.filterId);
            filter.setAttribute('filterUnits', 'userSpaceOnUse');
            filter.setAttribute('colorInterpolationFilters', 'sRGB');
            filter.setAttribute('x', '0');
            filter.setAttribute('y', '0');

            this.image = document.createElementNS(SVG_NS, 'feImage');
            this.image.setAttribute('result', 'map');
            this.image.setAttribute('preserveAspectRatio', 'none');

            // 三个通道用略有差异的位移比例，分通道取色后叠加形成边缘彩边
            this.displacements = ['r', 'g', 'b'].map((channel) => {
                const node = document.createElementNS(SVG_NS, 'feDisplacementMap');
                node.setAttribute('in', 'SourceGraphic');
                node.setAttribute('in2', 'map');
                node.setAttribute('xChannelSelector', 'R');
                node.setAttribute('yChannelSelector', 'G');
                node.setAttribute('result', 'shift-' + channel);
                return node;
            });

            const channels = [
                ['r', '1 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 1 0'],
                ['g', '0 0 0 0 0  0 1 0 0 0  0 0 0 0 0  0 0 0 1 0'],
                ['b', '0 0 0 0 0  0 0 0 0 0  0 0 1 0 0  0 0 0 1 0']
            ].map(([channel, matrix]) => {
                const node = document.createElementNS(SVG_NS, 'feColorMatrix');
                node.setAttribute('in', 'shift-' + channel);
                node.setAttribute('type', 'matrix');
                node.setAttribute('values', matrix);
                node.setAttribute('result', 'tint-' + channel);
                return node;
            });

            const mergeRG = document.createElementNS(SVG_NS, 'feBlend');
            mergeRG.setAttribute('in', 'tint-r');
            mergeRG.setAttribute('in2', 'tint-g');
            mergeRG.setAttribute('mode', 'screen');
            mergeRG.setAttribute('result', 'merge-rg');

            const mergeRGB = document.createElementNS(SVG_NS, 'feBlend');
            mergeRGB.setAttribute('in', 'merge-rg');
            mergeRGB.setAttribute('in2', 'tint-b');
            mergeRGB.setAttribute('mode', 'screen');

            const defs = document.createElementNS(SVG_NS, 'defs');
            [this.image].concat(this.displacements, channels, [mergeRG, mergeRGB])
                .forEach((node) => filter.appendChild(node));
            defs.appendChild(filter);
            svg.appendChild(defs);
            document.body.appendChild(svg);

            this.svg = svg;
            this.filter = filter;
            this.merges = [mergeRG, mergeRGB];
        }

        measure(force) {
            const rect = this.element.getBoundingClientRect();
            const width = Math.max(MIN_EDGE, Math.round(rect.width));
            const height = Math.max(MIN_EDGE, Math.round(rect.height));
            if (!force && width === this.width && height === this.height) return;

            this.width = width;
            this.height = height;

            const map = buildDisplacementMap(width, height, this.options);
            this.filter.setAttribute('width', width);
            this.filter.setAttribute('height', height);
            this.image.setAttribute('width', width);
            this.image.setAttribute('height', height);
            this.image.setAttribute('href', map.href);
            this.image.setAttributeNS(XLINK_NS, 'xlink:href', map.href);

            const chroma = this.options.chroma;
            this.displacements[0].setAttribute('scale', map.scale * (1 - chroma));
            this.displacements[1].setAttribute('scale', map.scale);
            this.displacements[2].setAttribute('scale', map.scale * (1 + chroma));
        }

        schedule() {
            if (this.frame) return;
            this.frame = requestAnimationFrame(() => {
                this.frame = 0;
                this.measure(false);
            });
        }

        destroy() {
            if (this.frame) cancelAnimationFrame(this.frame);
            this.observer.disconnect();
            this.svg.remove();
            delete this.element.dataset.liquidGlassRef;
        }
    }

    // backdrop-filter 里的 url() 只有 Chromium 内核会真正绘制，其余内核解析通过但不作画，
    // 因此这里额外校验内核，避免把元素原本的模糊替换成"什么都不画"的无效滤镜。
    const supported = (function () {
        const engine = navigator.userAgent;
        const isChromium = /\b(Chrome|Chromium|Edg|OPR)\b/.test(engine) && !/Firefox/.test(engine);
        if (!isChromium || typeof ResizeObserver === 'undefined') return false;
        try {
            return CSS.supports('backdrop-filter', 'url(#liquid-glass-probe)') ||
                CSS.supports('-webkit-backdrop-filter', 'url(#liquid-glass-probe)');
        } catch (error) {
            return false;
        }
    })();

    window.LiquidGlass = {
        get supported() {
            return supported;
        },

        apply(element, options) {
            if (!element || !supported) return null;
            if (surfaces.has(element)) return surfaces.get(element);
            const surface = new LiquidGlassSurface(element, options);
            surfaces.set(element, surface);
            element.dataset.liquidGlassRef = surface.filterId;
            syncStyles();
            return surface;
        },

        remove(element) {
            const surface = surfaces.get(element);
            if (!surface) return;
            surface.destroy();
            surfaces.delete(element);
            syncStyles();
        },

        refresh(element) {
            const surface = surfaces.get(element);
            if (surface) surface.schedule();
        },

        removeAll() {
            surfaces.forEach((surface) => surface.destroy());
            surfaces.clear();
            syncStyles();
        }
    };
})();