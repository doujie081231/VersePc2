import { showToast as showToast } from '../misc/ui-components.js';
import { wpfilePath, wewpVideoPath } from './wallpaper-utils.js';

class CustomVideoRenderer {
    constructor(engine) {
        this.engine = engine;
        this.video = null;
        this.loaded = false;
        this._lastBrightness = -1;
        this._brightnessSampleCanvas = document.createElement('canvas');
        this._brightnessSampleCanvas.width = 32;
        this._brightnessSampleCanvas.height = 32;
        this._brightnessSampleCtx = this._brightnessSampleCanvas.getContext('2d', { willReadFrequently: true });
        this._brightnessCheckInterval = null;
        this._container = document.getElementById('wallpaper-video-container');
        const videoSrc = engine.currentMode === 'auroraVideo' ? engine.auroraVideoPath : engine.customVideoPath;
        if (videoSrc) {
            this.loadVideo(videoSrc);
        }
    }

    setTheme() {}
    onResize() {}

    async loadVideo(filePath) {
        this.loaded = false;
        this._lastBrightness = -1;
        // 清理旧视频
        if (this.video) {
            this.video.pause();
            this.video.removeAttribute('src');
            this.video.load();
            if (this.video.parentElement) {
                this.video.parentElement.removeChild(this.video);
            }
        }
        if (this._brightnessCheckInterval) {
            clearInterval(this._brightnessCheckInterval);
            this._brightnessCheckInterval = null;
        }

        // WE 视频壁纸：直接用 wewp 本地协议按 Range 流式读取原文件；
        // 普通自定义视频走 wpfile 协议，同样按 Range 流式播放。
        // 之前用 readFileBuffer 把整个视频经 IPC 读成字节数组，大文件一定失败，
        // 失败后回退的 wpfile 协议此前未注册，于是必然提示「视频壁纸加载失败」。
        const weVideo = this.engine.weVideo;
        const isWeStream = !!(weVideo && weVideo.id && weVideo.file);
        let videoUrl;
        if (isWeStream) {
            videoUrl = wewpVideoPath(weVideo.id, weVideo.file);
        } else if (filePath && !filePath.startsWith('blob:') && !filePath.startsWith('wpfile://')) {
            videoUrl = wpfilePath(filePath);
        } else {
            videoUrl = filePath;
        }

        // 创建 video DOM 元素，用 CSS filter 实现 GPU 加速 blur
        this.video = document.createElement('video');
        // WE 视频壁纸开启音频；普通自定义视频保持静音
        this.video.muted = !this.engine.weVideoAudio;
        this.video.loop = true;
        this.video.playsInline = true;
        this.video.preload = 'auto';
        this.video.style.position = 'absolute';
        this.video.style.top = '0';
        this.video.style.left = '0';
        this.video.style.width = '100%';
        this.video.style.height = '100%';
        this.video.style.objectFit = 'cover';
        this.video.style.imageRendering = 'auto';
        this.video.style.pointerEvents = 'none';

        this.video.oncanplay = () => {
            console.log('[Wallpaper] Video canplay triggered');
            this.loaded = true;
            this.video.play().catch((e) => {
                console.warn('[Wallpaper] Video autoplay blocked:', e);
            });
            this._startBrightnessSampling();
        };
        this.video.onerror = () => {
            console.error('[Wallpaper] Video load failed, errorCode:', this.video.error);
            this.loaded = false;
            if (typeof showToast === 'function') showToast('视频壁纸加载失败，请检查文件是否存在或更换其他位置', 'error');
        };
        this.video.src = videoUrl;

        // 添加到容器
        if (this._container) {
            this._container.appendChild(this.video);
        }
        this._updateStyle();
    }

    _updateStyle() {
        if (!this.video) return;
        // WE 视频壁纸没有模糊/透明度/适配控件：强制原始清晰度和 cover，
        // 避免自定义壁纸的 blur 设置叠加到 WE 视频上。
        const isWe = !!this.engine.weVideo;
        const opacity = isWe ? 1 : (this.engine.wallpaperOpacity != null ? this.engine.wallpaperOpacity : 1);
        const blur = isWe ? 0 : (this.engine.wallpaperBlur || 0);
        const fitMode = isWe ? 'cover' : (this.engine.wallpaperFitMode || 'cover');

        this.video.style.opacity = opacity;
        // CSS filter blur 由 GPU 加速，性能远优于 Canvas filter
        this.video.style.filter = blur > 0 ? `blur(${blur}px)` : 'none';
        // 固定放大 5% 避免 blur 边缘出现透明，不会随 blur 增大而过度放大
        this.video.style.transform = blur > 0 ? 'scale(1.05)' : 'none';
        // object-fit 映射
        const fitMap = {
            cover: 'cover',
            contain: 'contain',
            stretch: 'fill',
            center: 'none',
            topLeft: 'none',
            topRight: 'none',
            bottomLeft: 'none',
            bottomRight: 'none',
            tile: 'none',
            smart: 'cover'
        };
        this.video.style.objectFit = fitMap[fitMode] || 'cover';
    }

    _startBrightnessSampling() {
        if (this._brightnessCheckInterval) clearInterval(this._brightnessCheckInterval);
        this._brightnessCheckInterval = setInterval(() => {
            this._sampleBrightness();
        }, 2000);
        this._sampleBrightness();
    }

    _sampleBrightness() {
        if (!this.loaded || !this.video || this.video.paused) return;
        try {
            const sCtx = this._brightnessSampleCtx;
            sCtx.drawImage(this.video, 0, 0, 32, 32);
            const data = sCtx.getImageData(0, 0, 32, 32).data;
            let total = 0;
            const pixelCount = 32 * 32;
            for (let i = 0; i < data.length; i += 4) {
                total += (data[i] * 0.299 + data[i + 1] * 0.587 + data[i + 2] * 0.114);
            }
            const brightness = total / pixelCount / 255;
            if (Math.abs(brightness - this._lastBrightness) > 0.05) {
                this._lastBrightness = brightness;
                this.engine._notifyBrightness(brightness);
            }
        } catch (e) {}
    }

    render(_dt, _timestamp) {
        // 视频由 DOM 自动播放，无需 Canvas 绘制
        // 只需检查暂停状态并尝试恢复播放
        if (this.loaded && this.video && this.video.paused) {
            this.video.play().catch(() => {});
        }
        // 当 opacity/blur 变化时更新样式
        this._updateStyle();
    }

    destroy() {
        if (this._brightnessCheckInterval) {
            clearInterval(this._brightnessCheckInterval);
            this._brightnessCheckInterval = null;
        }
        if (this.video) {
            const src = this.video.src;
            this.video.pause();
            this.video.removeAttribute('src');
            this.video.load();
            if (this.video.parentElement) {
                this.video.parentElement.removeChild(this.video);
            }
            // 释放 blob URL
            if (src && src.startsWith('blob:')) {
                URL.revokeObjectURL(src);
            }
            this.video = null;
        }
        this.loaded = false;
    }
}

export { CustomVideoRenderer };
