async function loadSettings() {
    try {
        const settings = await API.getSettings();
        const sv = (id, fallback) => { const el = document.getElementById(id); if (el) return el; return { value: fallback, checked: !!fallback, textContent: String(fallback) }; };

        sv('setting-java-path').value = settings.javaPath || '';
        sv('setting-max-memory').value = settings.maxMemory || 4096;
        sv('setting-min-memory').value = settings.minMemory || 1024;
        sv('setting-version-isolation').checked = settings.versionIsolation !== false;
        sv('setting-fullscreen').checked = !!settings.fullscreen;
        sv('setting-resolution').value = settings.resolution || '1920x1080';
        sv('setting-java-args').value = settings.javaArgs || '';
        sv('setting-close-on-launch').checked = !!settings.closeOnLaunch;
        sv('setting-auto-update').checked = settings.autoUpdate !== false;

        let downloadSourceValue = settings.downloadSource || 'china-first';
        if (downloadSourceValue === 'bmclapi') downloadSourceValue = 'china-first';
        sv('setting-download-source').value = downloadSourceValue;
        sv('setting-version-source').value = settings.versionSource || 'auto';
        const maxThreads = settings.maxThreads || 64;
        sv('setting-max-threads').value = maxThreads;
        const threadCountEl = document.getElementById('thread-count-value');
        if (threadCountEl) threadCountEl.textContent = maxThreads;
        const enableChunkEl = document.getElementById('setting-enable-chunk-download');
        if (enableChunkEl) enableChunkEl.checked = settings.enableChunkDownload !== false;
        const maxChunksEl = document.getElementById('setting-max-chunks-per-file');
        if (maxChunksEl) {
            const maxChunks = settings.maxChunksPerFile || 64;
            maxChunksEl.value = maxChunks;
            const chunkLabel = document.getElementById('chunk-count-value');
            if (chunkLabel) chunkLabel.textContent = maxChunks;
        }
        const speedLimit = settings.speedLimit || 0;
        sv('setting-speed-limit').value = speedLimit;
        updateSpeedLimitLabel(speedLimit);
        sv('setting-target-dir').value = settings.targetDir || '';
        sv('setting-ssl-verify').checked = !!settings.sslVerify;

        sv('setting-mod-source').value = settings.modSource || 'modrinth';
        sv('setting-filename-format').value = settings.filenameFormat || 'default';
        sv('setting-mod-style').value = settings.modStyle || 'title';
        sv('setting-ignore-quilt').checked = !!settings.ignoreQuilt;

        const accentColor = settings.accentColor || '#ffffff';
        const accentColorInput = document.getElementById('custom-accent-color');
        if (accentColorInput) accentColorInput.value = accentColor;
        const accentColorValueEl = document.getElementById('custom-color-value');
        if (accentColorValueEl) accentColorValueEl.textContent = accentColor;

        let savedTheme = settings.theme || 'light';
        const legacyThemes = ['blue', 'purple', 'green', 'orange', 'red', 'pink', 'teal', 'cyan', 'amber'];
        if (legacyThemes.includes(savedTheme)) savedTheme = 'light';
        document.documentElement.setAttribute('data-theme', savedTheme);
        document.querySelectorAll('.theme-option').forEach(btn => {
            btn.classList.toggle('active', btn.getAttribute('data-theme') === savedTheme);
        });
        if (savedTheme === 'custom') {
            // 自定义主题色：恢复保存的颜色/深浅模式并显示颜色选择器
            if (typeof toggleCustomThemeColorGroup === 'function') toggleCustomThemeColorGroup(true);
            if (typeof getSavedCustomThemeColor === 'function' && typeof applyCustomThemeColor === 'function') {
                const customColor = await getSavedCustomThemeColor();
                const isLight = typeof getSavedCustomThemeLight === 'function'
                    ? await getSavedCustomThemeLight()
                    : false;
                await applyCustomThemeColor(customColor, { save: false, isLight });
            }
            if (typeof syncCustomThemeLightModeUI === 'function') {
                const isLight = typeof getSavedCustomThemeLight === 'function'
                    ? await getSavedCustomThemeLight()
                    : false;
                syncCustomThemeLightModeUI(isLight);
            }
        } else {
            if (typeof toggleCustomThemeColorGroup === 'function') toggleCustomThemeColorGroup(false);
            const defaultAccent = savedTheme === 'light' ? '#1a1a1a' : '#ffffff';
            const effectiveAccent = settings.accentColor || defaultAccent;
            if (accentColorInput) accentColorInput.value = effectiveAccent;
            if (accentColorValueEl) accentColorValueEl.textContent = effectiveAccent;
            const colorPreviewDot = document.getElementById('color-preview-dot');
            if (colorPreviewDot) colorPreviewDot.style.background = effectiveAccent;
            if (settings.accentColor && settings.accentColor !== defaultAccent) {
                applyAccentColor(settings.accentColor);
            }
        }
    } catch (e) { console.error('[Settings] Failed to load settings:', e); }
}

function updateSpeedLimitLabel(value) {
    const el = document.getElementById('speed-limit-value');
    if (el) {
        el.textContent = value === 0 ? '无限制' : value + ' MB/s';
    }
}

async function saveCurrentSettings() {
    const g = (id) => document.getElementById(id);
    const settings = {
        javaPath: g('setting-java-path')?.value || '',
        maxMemory: parseInt(g('setting-max-memory')?.value || '2048', 10),
        minMemory: parseInt(g('setting-min-memory')?.value || '256', 10),
        versionIsolation: g('setting-version-isolation')?.checked || false,
        fullscreen: g('setting-fullscreen')?.checked || false,
        resolution: g('setting-resolution')?.value || '',
        javaArgs: g('setting-java-args')?.value || '',
        closeOnLaunch: g('setting-close-on-launch')?.checked || false,
        autoUpdate: g('setting-auto-update')?.checked || false,

        downloadSource: g('setting-download-source')?.value || 'china-first',
        versionSource: g('setting-version-source')?.value || 'mojang',
        maxThreads: parseInt(g('setting-max-threads')?.value || '64', 10),
        enableChunkDownload: g('setting-enable-chunk-download') ? g('setting-enable-chunk-download').checked : true,
        maxChunksPerFile: g('setting-max-chunks-per-file') ? parseInt(g('setting-max-chunks-per-file').value, 10) : 64,
        speedLimit: parseInt(g('setting-speed-limit')?.value || '0', 10),
        targetDir: g('setting-target-dir')?.value || '',
        sslVerify: g('setting-ssl-verify')?.checked || false,

        modSource: g('setting-mod-source')?.value || 'modrinth',
        filenameFormat: g('setting-filename-format')?.value || '',
        modStyle: g('setting-mod-style')?.value || '',
        ignoreQuilt: g('setting-ignore-quilt')?.checked || false,

        accentColor: g('custom-accent-color')?.value || '#ffffff'
    };
    try {
        await API.saveSettings(settings);
        showToast('设置已保存', 'success');
    } catch (e) {
        showToast('保存设置失败', 'error');
    }
}
