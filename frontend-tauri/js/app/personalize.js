async function selectWallpaper(element) {
    // 懒初始化壁纸引擎（若启动时因壁纸为"无"而跳过了初始化）
    if (typeof wallpaperEngine === 'undefined' || !wallpaperEngine) {
        if (typeof _lazyLoadScript === 'function') {
            try { await _lazyLoadScript('js/three.bundle.js'); } catch (e) {}
        }
        if (typeof initWallpaper === 'function') {
            try { initWallpaper(); } catch (e) { console.error('[Wallpaper] init error:', e); }
        }
    }

    document.querySelectorAll('.wallpaper-option').forEach(opt => opt.classList.remove('active'));
    element.classList.add('active');

    const mode = element.dataset.wallpaper;

    if (typeof switchWallpaperMode === 'function') {
        switchWallpaperMode(mode);
    }

    const isCustom = mode === 'customImage' || mode === 'customVideo';
    const isAurora = mode === 'auroraVideo';
    const isPanorama = mode === 'panorama';
    document.getElementById('custom-wallpaper-file-group').style.display = isCustom ? '' : 'none';
    document.getElementById('wallpaper-fit-group').style.display = (isCustom || isAurora) ? '' : 'none';
    document.getElementById('wallpaper-opacity-group').style.display = isCustom ? '' : 'none';
    document.getElementById('wallpaper-blur-group').style.display = isCustom ? '' : 'none';
    document.getElementById('panorama-theme-group').style.display = isPanorama ? '' : 'none';
    const speedRow = document.getElementById('panoramaSpeedRow');
    if (speedRow) speedRow.style.display = isPanorama ? '' : 'none';
    const mouseFollowRow = document.getElementById('panoramaMouseFollowRow');
    if (mouseFollowRow) mouseFollowRow.style.display = isPanorama ? '' : 'none';

    if (isAurora) {
        const fitSelect = document.getElementById('wallpaper-fit-select');
        if (fitSelect) { fitSelect.value = 'cover'; if (typeof setWallpaperFitMode === 'function') setWallpaperFitMode('cover'); }
        if (typeof setWallpaperBlur === 'function') setWallpaperBlur(0);
        if (typeof setWallpaperOpacity === 'function') setWallpaperOpacity(1);
        document.body.classList.add('aurora-theme');
        try {
            window.electronAPI.store.set('versepc_wallpaper_fit', 'cover');
        } catch (e) {}
    } else {
        document.body.classList.remove('aurora-theme');
    }

    if (isPanorama) {
        try {
            const [savedTheme, savedSpeed, savedFollow] = await Promise.all([
                window.electronAPI?.store?.get('versepc_panorama_theme'),
                window.electronAPI?.store?.get('versepc_panorama_speed'),
                window.electronAPI?.store?.get('versepc_panorama_mouse_follow'),
            ]);
            if (savedTheme) {
                const themeEl = document.querySelector(`.panorama-theme-option[data-theme="${savedTheme}"]`);
                if (themeEl) {
                    document.querySelectorAll('.panorama-theme-option').forEach(opt => opt.classList.remove('active'));
                    themeEl.classList.add('active');
                    if (typeof setPanoramaTheme === 'function') setPanoramaTheme(savedTheme);
                }
            }
            if (savedSpeed != null) {
                const slider = document.getElementById('panoramaSpeedSlider');
                if (slider) slider.value = savedSpeed;
                const label = document.getElementById('panoramaSpeedLabel');
                if (label) label.textContent = savedSpeed;
                if (typeof setPanoramaRotationSpeed === 'function') setPanoramaRotationSpeed(savedSpeed * 0.001);
            }
            if (savedFollow === true) {
                const toggle = document.getElementById('panoramaMouseFollowToggle');
                if (toggle) toggle.checked = true;
                if (typeof setPanoramaMouseFollow === 'function') setPanoramaMouseFollow(true);
            }
        } catch (e) {
            console.warn('[Settings] Failed to restore panorama settings:', e);
        }
    }

    if (isCustom) {
        const fileLabel = document.getElementById('custom-wallpaper-file-label');
        if (fileLabel) fileLabel.textContent = mode === 'customVideo' ? '选择视频文件' : '选择图片文件';
        const dropZone = document.getElementById('custom-wallpaper-drop-zone');
        if (dropZone) dropZone.textContent = mode === 'customVideo' ? '拖放视频到此处' : '拖放图片到此处';
    }

    try {
        await window.electronAPI.store.set('versepc_wallpaper', mode);
    } catch (e) {
        console.error('[Settings] Save wallpaper error:', e);
    }
}

async function pickCustomWallpaperFile() {
    const activeMode = document.querySelector('.wallpaper-option.active')?.dataset.wallpaper;
    const isVideo = activeMode === 'customVideo';

    const filters = isVideo
        ? [{ name: '视频文件', extensions: ['mp4', 'webm', 'mkv', 'avi'] }]
        : [{ name: '图片文件', extensions: ['jpg', 'jpeg', 'png', 'gif', 'bmp', 'webp'] }];

    try {
        const result = await window.electronAPI.selectFile({
            title: isVideo ? '选择视频壁纸' : '选择图片壁纸',
            filters
        });

        if (result.cancelled) return;

        const filePath = result.path;
        if (!filePath) {
            if (typeof showToast === 'function') showToast('未获取到文件路径，请重新选择', 'error');
            return;
        }
        await _applyCustomWallpaperFile(filePath, isVideo);
    } catch (e) {
        console.error('[Wallpaper] Pick file error:', e);
    }
}

async function _applyCustomWallpaperFile(filePath, isVideo) {
    if (!filePath) {
        if (typeof showToast === 'function') showToast('未获取到文件路径，请重新选择', 'error');
        return;
    }
    document.getElementById('custom-wallpaper-file-name').textContent = filePath.split(/[\\/]/).pop();

    if (isVideo) {
        if (typeof setCustomWallpaperVideo === 'function') {
            setCustomWallpaperVideo(filePath);
        }
        try { await window.electronAPI.store.set('versepc_custom_video', filePath); } catch (e) {}
    } else {
        if (typeof setCustomWallpaperImage === 'function') {
            setCustomWallpaperImage(filePath);
        }
        try { await window.electronAPI.store.set('versepc_custom_image', filePath); } catch (e) {}
        _updateCustomImagePreview(filePath);
    }
}

function _updateCustomImagePreview(filePath) {
    const preview = document.getElementById('wp-preview-custom-image');
    if (!preview) return;
    const icon = preview.querySelector('.wp-preview-icon');
    if (filePath) {
        if (icon) icon.style.display = 'none';
        let img = preview.querySelector('.wp-preview-thumb');
        if (!img) {
            img = document.createElement('img');
            img.className = 'wp-preview-thumb';
            img.style.cssText = 'width:100%;height:100%;object-fit:cover;position:absolute;inset:0;';
            preview.style.position = 'relative';
            preview.appendChild(img);
        }
        img.src = typeof wpfilePath === 'function' ? wpfilePath(filePath) : ('wpfile:///' + filePath.replace(/\\/g, '/').split('/').map(encodeURIComponent).join('/'));
    } else {
        if (icon) icon.style.display = '';
        const img = preview.querySelector('.wp-preview-thumb');
        if (img) img.remove();
    }
}

function initWallpaperDropZone() {
    const dropZone = document.getElementById('custom-wallpaper-drop-zone');
    if (!dropZone) return;

    dropZone.addEventListener('dragover', (e) => {
        e.preventDefault();
        e.stopPropagation();
        dropZone.classList.add('drag-over');
    });

    dropZone.addEventListener('dragleave', (e) => {
        e.preventDefault();
        e.stopPropagation();
        dropZone.classList.remove('drag-over');
    });

    dropZone.addEventListener('drop', async (e) => {
        e.preventDefault();
        e.stopPropagation();
        dropZone.classList.remove('drag-over');

        const activeMode = document.querySelector('.wallpaper-option.active')?.dataset.wallpaper;
        const isVideo = activeMode === 'customVideo';

        const file = e.dataTransfer.files[0];
        if (!file) return;

        const filePath = (window.electronAPI && window.electronAPI.getDroppedFilePath) ? window.electronAPI.getDroppedFilePath(file) : '';
        if (!filePath) return;

        const validImageExts = ['.jpg', '.jpeg', '.png', '.gif', '.bmp', '.webp'];
        const validVideoExts = ['.mp4', '.webm', '.mkv', '.avi'];
        const ext = '.' + file.name.split('.').pop().toLowerCase();

        if (isVideo && !validVideoExts.includes(ext)) {
            if (typeof showToast === 'function') showToast('请拖放视频文件', 'error');
            return;
        }
        if (!isVideo && !validImageExts.includes(ext)) {
            if (typeof showToast === 'function') showToast('请拖放图片文件', 'error');
            return;
        }

        await _applyCustomWallpaperFile(filePath, isVideo);
    });
}

function initWallpaperAutoAdapt() {
    if (typeof onWallpaperBrightnessChange !== 'function') return;

    onWallpaperBrightnessChange((brightness) => {
        const overlay = document.getElementById('wallpaper-overlay');
        if (!overlay) return;

        const app = document.getElementById('app');
        if (!app) return;

        const currentTheme = document.documentElement.getAttribute('data-theme') || 'dark';
        const isLightTheme = currentTheme === 'light' ||
            (currentTheme === 'custom' && document.documentElement.getAttribute('data-custom-theme-mode') === 'light');
        const isLight = brightness > 0.55;
        const isDark = brightness < 0.35;

        if (isLightTheme) {
            app.classList.remove('wp-light', 'wp-dark');
            overlay.style.background = 'transparent';
        } else if (isLight) {
            overlay.style.background = 'rgba(0, 0, 0, 0.15)';
            app.classList.add('wp-light');
            app.classList.remove('wp-dark');
        } else if (isDark) {
            overlay.style.background = 'transparent';
            app.classList.add('wp-dark');
            app.classList.remove('wp-light');
        } else {
            const alpha = (0.55 - brightness) * 0.3;
            overlay.style.background = `rgba(0, 0, 0, ${Math.max(0, alpha)})`;
            app.classList.remove('wp-light', 'wp-dark');
        }

        document.documentElement.style.setProperty('--wp-brightness', brightness);
    });
}

function onWallpaperOpacityChange(value) {
    const opacity = value / 100;
    document.getElementById('wallpaper-opacity-value').textContent = value + '%';
    if (typeof setWallpaperOpacity === 'function') setWallpaperOpacity(opacity);
    window.electronAPI?.store?.set('versepc_wallpaper_opacity', value).catch(() => {});
}

function onWallpaperBlurChange(value) {
    document.getElementById('wallpaper-blur-value').textContent = value + 'px';
    if (typeof setWallpaperBlur === 'function') setWallpaperBlur(parseInt(value));
    window.electronAPI?.store?.set('versepc_wallpaper_blur', value).catch(() => {});
}

function onWallpaperFitChange(value) {
    if (typeof setWallpaperFitMode === 'function') setWallpaperFitMode(value);
    window.electronAPI?.store?.set('versepc_wallpaper_fit', value).catch(() => {});
}

function selectPanoramaTheme(element) {
    document.querySelectorAll('.panorama-theme-option').forEach(opt => opt.classList.remove('active'));
    element.classList.add('active');
    const theme = element.dataset.theme;
    if (typeof setPanoramaTheme === 'function') setPanoramaTheme(theme);
    window.electronAPI?.store?.set('versepc_panorama_theme', theme).catch(() => {});
}

function onPanoramaSpeedChange(value) {
    const speed = value * 0.001;
    if (typeof setPanoramaRotationSpeed === 'function') setPanoramaRotationSpeed(speed);
    window.electronAPI?.store?.set('versepc_panorama_speed', parseInt(value));
    const label = document.getElementById('panoramaSpeedLabel');
    if (label) label.textContent = value;
}

function onPanoramaMouseFollowChange(enabled) {
    if (typeof setPanoramaMouseFollow === 'function') setPanoramaMouseFollow(enabled);
    window.electronAPI?.store?.set('versepc_panorama_mouse_follow', enabled);
}

function aiToggleApiKeyVisibility() {
    const input = document.getElementById('ai-api-key-input');
    if (!input) return;
    const btn = input.parentElement.querySelector('button');
    if (input.type === 'password') {
        input.type = 'text';
        if (btn) btn.textContent = '隐藏';
    } else {
        input.type = 'password';
        if (btn) btn.textContent = '显示';
    }
}

function applyThemeColors(themeName) {
    if (themeName === 'dark') {
        document.documentElement.style.setProperty('--accent', '#ffffff');
        document.documentElement.style.setProperty('--accent-hover', '#d0d0d0');
    } else {
        document.documentElement.style.setProperty('--accent', '#1a1a1a');
        document.documentElement.style.setProperty('--accent-hover', '#333333');
    }
}

async function updateCustomAccentColor(color) {
    const theme = document.documentElement.getAttribute('data-theme') || 'light';
    if (theme === 'dark') {
        document.documentElement.style.setProperty('--accent', '#ffffff');
        document.documentElement.style.setProperty('--accent-hover', '#d0d0d0');
        document.documentElement.style.setProperty('--accent-rgb', '255, 255, 255');
    } else {
        document.documentElement.style.setProperty('--accent', '#1a1a1a');
        document.documentElement.style.setProperty('--accent-hover', '#333333');
        document.documentElement.style.setProperty('--accent-rgb', '26, 26, 26');
    }
}

function toggleGlassEffect(enabled) {
    if (enabled) {
        document.documentElement.removeAttribute('data-no-glass');
    } else {
        document.documentElement.setAttribute('data-no-glass', '');
    }
    window.electronAPI.store.set('versepc_glass_effect', enabled ? '1' : '0').catch(() => {});
}

function toggleLiquidGlassEffect(enabled) {
    if (enabled) {
        document.documentElement.setAttribute('data-liquid-glass', '');
    } else {
        document.documentElement.removeAttribute('data-liquid-glass');
    }
    window.electronAPI.store.set('versepc_liquid_glass', enabled ? '1' : '0').catch(() => {});
}

async function savePersonalizeSettings() {
    const settings = {
        theme: document.querySelector('.theme-option.active')?.dataset.theme || 'light',
        wallpaper: document.querySelector('.wallpaper-option.active')?.dataset.wallpaper || 'none',
        glassEffect: document.getElementById('setting-glass-effect')?.checked ?? false,
        liquidGlass: document.getElementById('setting-liquid-glass')?.checked ?? false
    };

    try {
        await window.electronAPI.store.set('versepc_personalize_settings', JSON.stringify(settings));
        await window.electronAPI.store.set('versepc_wallpaper', settings.wallpaper);
        showToast('个性化设置已保存', 'success');
    } catch (e) {
        showToast('保存失败: ' + e.message, 'error');
    }
}

async function resetPersonalizeSettings() {
    const confirmed = await showConfirmDialog('重置设置', '确定要重置个性化设置为默认值吗?', '重置', '取消');
    if (!confirmed) return;

    document.querySelector('.theme-option[data-theme="light"]')?.click();

    document.querySelector('.wallpaper-option[data-wallpaper="none"]')?.click();

    const opacitySlider = document.getElementById('wallpaper-opacity-slider');
    if (opacitySlider) { opacitySlider.value = 100; onWallpaperOpacityChange(100); }
    const blurSlider = document.getElementById('wallpaper-blur-slider');
    if (blurSlider) { blurSlider.value = 0; onWallpaperBlurChange(0); }
    const fitSelect = document.getElementById('wallpaper-fit-select');
    if (fitSelect) { fitSelect.value = 'cover'; onWallpaperFitChange('cover'); }

    const glassCheckbox = document.getElementById('setting-glass-effect');
    if (glassCheckbox) { glassCheckbox.checked = false; toggleGlassEffect(false); }

    const liquidGlassCheckbox = document.getElementById('setting-liquid-glass');
    if (liquidGlassCheckbox) { liquidGlassCheckbox.checked = false; toggleLiquidGlassEffect(false); }

    if (typeof clearCustomThemeVars === 'function') clearCustomThemeVars();
    if (typeof syncCustomThemeColorUI === 'function') syncCustomThemeColorUI('#4c8dff');

    try {
        await window.electronAPI.store.set('versepc_personalize_settings', JSON.stringify({
            theme: 'light',
            wallpaper: 'none',
            glassEffect: false,
            liquidGlass: false
        }));
        await window.electronAPI.store.set('versepc_wallpaper', 'none');
        await window.electronAPI.store.delete('versepc_solid_color');
        await window.electronAPI.store.delete('versepc_custom_theme_color');
        await window.electronAPI.store.delete('versepc_custom_theme_light');
        await window.electronAPI.store.set('versepc_wallpaper_opacity', 100);
        await window.electronAPI.store.set('versepc_wallpaper_blur', 0);
        await window.electronAPI.store.set('versepc_wallpaper_fit', 'cover');
        await window.electronAPI.store.delete('versepc_custom_image');
        await window.electronAPI.store.delete('versepc_custom_video');
        await window.electronAPI.store.set('versepc_panorama_theme', 'overworld');
        await window.electronAPI.store.set('versepc_glass_effect', '0');
        await window.electronAPI.store.set('versepc_liquid_glass', '0');
        _updateCustomImagePreview(null);
        const nameEl = document.getElementById('custom-wallpaper-file-name');
        if (nameEl) nameEl.textContent = '未选择';
    } catch (e) {
        console.error('[Settings] Reset personalize settings save error:', e);
    }

    showToast('个性化设置已重置', 'success');
}

async function loadPersonalizeSettings() {
    try {
        const saved = await window.electronAPI.store.get('versepc_personalize_settings');
        if (saved) {
            const settings = JSON.parse(saved);
            if (settings.theme) {
                let themeName = settings.theme;
                const legacyThemes = ['blue', 'purple', 'green', 'orange', 'red', 'pink', 'teal', 'cyan', 'amber'];
                if (legacyThemes.includes(themeName)) themeName = 'light';
                const themeEl = document.querySelector(`.theme-option[data-theme="${themeName}"]`);
                if (themeEl) selectTheme(themeEl);
            }
            if (settings.wallpaper) {
                let wpName = settings.wallpaper;
                if (wpName === 'starry') wpName = 'panorama';
                const wpEl = document.querySelector(`.wallpaper-option[data-wallpaper="${wpName}"]`);
                if (wpEl) selectWallpaper(wpEl);
            }
            if (settings.glassEffect !== undefined) {
                const enabled = settings.glassEffect;
                const glassCheckbox = document.getElementById('setting-glass-effect');
                if (glassCheckbox) glassCheckbox.checked = enabled;
                toggleGlassEffect(enabled);
            }
            if (settings.liquidGlass !== undefined) {
                const enabled = settings.liquidGlass;
                const liquidGlassCheckbox = document.getElementById('setting-liquid-glass');
                if (liquidGlassCheckbox) liquidGlassCheckbox.checked = enabled;
                toggleLiquidGlassEffect(enabled);
            }
        } else {
            const savedTheme = await window.electronAPI.store.get('versepc_theme');
            if (savedTheme) {
                let themeName = savedTheme;
                const legacyThemes = ['blue', 'purple', 'green', 'orange', 'red', 'pink', 'teal', 'cyan', 'amber'];
                if (legacyThemes.includes(themeName)) themeName = 'light';
                const themeEl = document.querySelector(`.theme-option[data-theme="${themeName}"]`);
                if (themeEl) selectTheme(themeEl);
            } else {
                const defaultThemeEl = document.querySelector('.theme-option[data-theme="light"]');
                if (defaultThemeEl) selectTheme(defaultThemeEl);
            }
            const defaultWpEl = document.querySelector('.wallpaper-option[data-wallpaper="none"]');
            if (defaultWpEl) selectWallpaper(defaultWpEl);
        }

        const glassSaved = await window.electronAPI.store.get('versepc_glass_effect');
        if (glassSaved !== null && glassSaved !== undefined) {
            const enabled = glassSaved === '1';
            const glassCheckbox = document.getElementById('setting-glass-effect');
            if (glassCheckbox) glassCheckbox.checked = enabled;
            toggleGlassEffect(enabled);
        }

        const liquidGlassSaved = await window.electronAPI.store.get('versepc_liquid_glass');
        if (liquidGlassSaved !== null && liquidGlassSaved !== undefined) {
            const enabled = liquidGlassSaved === '1';
            const liquidGlassCheckbox = document.getElementById('setting-liquid-glass');
            if (liquidGlassCheckbox) liquidGlassCheckbox.checked = enabled;
            toggleLiquidGlassEffect(enabled);
        }
    } catch (e) {
        console.error('[Settings] Load personalize settings error:', e);
    }
}
