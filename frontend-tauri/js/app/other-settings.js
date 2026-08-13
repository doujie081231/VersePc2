// ─── 其他设置函数 ──────────────────────────────────────────

async function copyFeedbackEmail(btn) {
    const email = 'verselauncher@verselauncher.cn';
    try {
        if (window.electronAPI?.clipboard) {
            await window.electronAPI.clipboard.writeText(email);
        } else {
            await navigator.clipboard.writeText(email);
        }
        btn.textContent = '已复制';
        setTimeout(() => { btn.textContent = '复制'; }, 2000);
        showToast('邮箱已复制到剪贴板', 'success');
    } catch (e) {
        showToast('复制失败，请手动复制', 'error');
    }
}

function toggleDebugOptions() {
    const content = document.getElementById('debug-options-content');
    const arrow = document.getElementById('debug-options-arrow');
    if (content && arrow) {
        const isHidden = content.style.display === 'none';
        content.style.display = isHidden ? 'block' : 'none';
        arrow.style.transform = isHidden ? 'rotate(180deg)' : 'rotate(0)';
    }
}

async function saveOtherSettings() {
    const settings = {
        downloadSource: document.getElementById('setting-download-source')?.value,
        versionSource: document.getElementById('setting-version-source')?.value,
        maxThreads: document.getElementById('setting-max-threads')?.value,
        speedLimit: document.getElementById('setting-speed-limit')?.value,
        targetDir: document.getElementById('setting-target-dir')?.value,
        sslVerify: document.getElementById('setting-ssl-verify')?.checked,
        modSource: document.getElementById('setting-mod-source')?.value,
        filenameFormat: document.getElementById('setting-filename-format')?.value,
        modStyle: document.getElementById('setting-mod-style')?.value,
        ignoreQuilt: document.getElementById('setting-ignore-quilt')?.checked,
        notifyReleaseUpdates: document.getElementById('notify-release-updates')?.checked,
        notifySnapshotUpdates: document.getElementById('notify-snapshot-updates')?.checked,
        autoSetChinese: document.getElementById('auto-set-chinese')?.checked,
        launcherUpdateMode: document.getElementById('launcher-update-mode')?.value,
        launcherNoticeMode: document.getElementById('launcher-notice-mode')?.value,
        anonymousDataCollection: document.getElementById('anonymous-data-collection')?.checked,
        debugMode: document.getElementById('debug-mode')?.checked,
        verboseLogging: document.getElementById('verbose-logging')?.checked,
        consoleDebug: document.getElementById('enable-console-debug')?.checked,
        autoMemoryOptimize: document.getElementById('auto-memory-optimize')?.checked
    };

    try {
        await window.electronAPI.store.set('versepc_other_settings', JSON.stringify(settings));
        showToast('其他设置已保存', 'success');
    } catch (e) {
        showToast('保存失败: ' + e.message, 'error');
    }
}

async function resetOtherSettings() {
    const confirmed = await showConfirmDialog('重置设置', '确定要重置其他设置为默认值吗?', '重置', '取消');
    if (!confirmed) return;

    document.getElementById('setting-download-source').value = 'auto';
    document.getElementById('setting-version-source').value = 'auto';
    document.getElementById('setting-max-threads').value = 32;
    document.getElementById('thread-count-value').textContent = '32';
    document.getElementById('setting-speed-limit').value = 0;
    document.getElementById('speed-limit-value').textContent = '无限制';
    document.getElementById('setting-target-dir').value = '';
    document.getElementById('setting-ssl-verify').checked = false;
    document.getElementById('setting-mod-source').value = 'modrinth';
    document.getElementById('setting-filename-format').value = 'default';
    document.getElementById('setting-mod-style').value = 'title';
    document.getElementById('setting-ignore-quilt').checked = false;
    document.getElementById('notify-release-updates').checked = false;
    document.getElementById('notify-snapshot-updates').checked = false;
    document.getElementById('auto-set-chinese').checked = true;
    document.getElementById('launcher-update-mode').value = 'auto';
    document.getElementById('launcher-notice-mode').value = 'show-all';
    document.getElementById('anonymous-data-collection').checked = false;
    document.getElementById('debug-mode').checked = false;
    document.getElementById('verbose-logging').checked = false;
    document.getElementById('enable-console-debug').checked = false;
    document.getElementById('auto-memory-optimize').checked = true;

    API.saveSetting('autoSetChinese', true).catch(() => {});
    showToast('其他设置已重置', 'success');
}

async function loadOtherSettings() {
    try {
        const saved = await window.electronAPI.store.get('versepc_other_settings');
        if (saved) {
            const settings = JSON.parse(saved);
            if (settings.downloadSource) document.getElementById('setting-download-source').value = settings.downloadSource;
            if (settings.versionSource) document.getElementById('setting-version-source').value = settings.versionSource;
            if (settings.maxThreads) {
                document.getElementById('setting-max-threads').value = settings.maxThreads;
                document.getElementById('thread-count-value').textContent = settings.maxThreads;
            }
            if (settings.speedLimit !== undefined) {
                document.getElementById('setting-speed-limit').value = settings.speedLimit;
                updateSpeedLimitLabel(settings.speedLimit);
            }
            if (settings.targetDir) document.getElementById('setting-target-dir').value = settings.targetDir;
            if (settings.sslVerify !== undefined) document.getElementById('setting-ssl-verify').checked = settings.sslVerify;
            if (settings.modSource) document.getElementById('setting-mod-source').value = settings.modSource;
            if (settings.filenameFormat) document.getElementById('setting-filename-format').value = settings.filenameFormat;
            if (settings.modStyle) document.getElementById('setting-mod-style').value = settings.modStyle;
            if (settings.ignoreQuilt !== undefined) document.getElementById('setting-ignore-quilt').checked = settings.ignoreQuilt;
            if (settings.notifyReleaseUpdates !== undefined) document.getElementById('notify-release-updates').checked = settings.notifyReleaseUpdates;
            if (settings.notifySnapshotUpdates !== undefined) document.getElementById('notify-snapshot-updates').checked = settings.notifySnapshotUpdates;
            if (settings.autoSetChinese !== undefined) document.getElementById('auto-set-chinese').checked = settings.autoSetChinese;
            if (settings.debugMode !== undefined) document.getElementById('debug-mode').checked = settings.debugMode;
            if (settings.verboseLogging !== undefined) document.getElementById('verbose-logging').checked = settings.verboseLogging;
            if (settings.consoleDebug !== undefined) document.getElementById('enable-console-debug').checked = settings.consoleDebug;
            if (settings.autoMemoryOptimize !== undefined) document.getElementById('auto-memory-optimize').checked = settings.autoMemoryOptimize;
        }
    } catch (e) {
        console.error('[Settings] Load other settings error:', e);
    }
}

function updateSpeedLimitLabel(value) {
    const label = document.getElementById('speed-limit-value');
    if (label) {
        label.textContent = value == 0 ? '无限制' : `${value} MB/s`;
    }
}

