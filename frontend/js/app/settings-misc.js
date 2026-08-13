function checkForUpdates() {
    showToast('正在检查更新...', 'info');
    handleCheckUpdate();
}

let _memoryOptimizing = false;


async function refreshMemoryInfo() {
    try {
        const info = await API.getMemoryInfo();
        if (!info || info.error) return;
        const bar = document.getElementById('memory-usage-bar');
        const text = document.getElementById('memory-usage-text');
        const detail = document.getElementById('memory-detail-text');
        if (bar) bar.style.width = info.loadPercent + '%';
        if (text) text.textContent = info.loadPercent + '%';
        if (detail) detail.textContent = `${formatBytes(info.used)} / ${formatBytes(info.total)}`;
        if (bar) {
            if (info.loadPercent > 85) bar.style.background = '#ef4444';
            else if (info.loadPercent > 70) bar.style.background = '#f59e0b';
            else bar.style.background = 'var(--accent)';
        }
    } catch (e) {}
}

async function doMemoryOptimize() {
    if (_memoryOptimizing) {
        showToast('内存优化正在进行中，请稍候', 'info');
        return;
    }
    _memoryOptimizing = true;
    const btn = document.getElementById('memory-optimize-btn');
    if (btn) { btn.disabled = true; btn.textContent = '优化中...'; }
    showToast('正在执行内存优化...', 'info');
    try {
        const result = await API.memoryOptimize();
        if (result.success) {
            const freedStr = result.freedMB > 0 ? `释放了 ${result.freedMB} MB` : '内存已优化';
            showToast(`内存优化完成，${freedStr}，当前可用 ${result.afterMB} MB`, 'success');
        } else {
            showToast('内存优化失败: ' + (result.error || '未知错误'), 'error');
        }
    } catch (e) {
        showToast('内存优化失败: ' + e.message, 'error');
    } finally {
        _memoryOptimizing = false;
        if (btn) { btn.disabled = false; btn.textContent = '内存优化'; }
        refreshMemoryInfo();
    }
}

async function exportSettings() {
    try {
        const allSettings = {
            launch: await window.electronAPI.store.get('versepc_launch_settings'),
            personalize: await window.electronAPI.store.get('versepc_personalize_settings'),
            other: await window.electronAPI.store.get('versepc_other_settings'),
            exportTime: new Date().toISOString()
        };

        const blob = new Blob([JSON.stringify(allSettings, null, 2)], { type: 'application/json' });
        const url = URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url;
        a.download = `versepc-settings-${new Date().toISOString().slice(0,10)}.json`;
        a.click();
        URL.revokeObjectURL(url);

        showToast('设置已导出', 'success');
    } catch (e) {
        showToast('导出失败: ' + e.message, 'error');
    }
}

function importSettings() {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = '.json';

    input.onchange = async (e) => {
        const file = e.target.files[0];
        if (!file) return;

        try {
            const text = await file.text();
            const settings = JSON.parse(text);

            if (settings.launch) {
                await window.electronAPI.store.set('versepc_launch_settings', settings.launch);
                loadLaunchSettings();
            }
            if (settings.personalize) {
                await window.electronAPI.store.set('versepc_personalize_settings', settings.personalize);
                loadPersonalizeSettings();
            }
            if (settings.other) {
                await window.electronAPI.store.set('versepc_other_settings', settings.other);
                loadOtherSettings();
            }

            showToast('设置已导入，请刷新页面查看效果', 'success');
        } catch (err) {
            showToast('导入失败: 无效的设置文件', 'error');
        }
    };

    input.click();
}

async function createDesktopShortcut() {
    try {
        const result = await API.createShortcut('desktop');
        if (result.success) showToast('桌面快捷方式已创建', 'success');
        else showToast('创建失败', 'error');
    } catch (e) {
        showToast('创建失败: ' + e.message, 'error');
    }
}

async function openScreenshots(versionId) {
    const modal = document.getElementById('screenshot-modal');
    const grid = document.getElementById('screenshot-grid');
    grid.innerHTML = '<div style="text-align:center;color:var(--text-secondary);padding:20px;">加载中...</div>';
    modal.style.display = 'flex';
    modal.classList.add('modal-visible');

    try {
        const result = await API.getScreenshots(versionId);
        if (result.screenshots && result.screenshots.length > 0) {
            grid.innerHTML = result.screenshots.map(ss => `
                <div style="position:relative;border-radius:6px;overflow:hidden;cursor:pointer;background:var(--bg-active);" onclick="window.open('${ss.url}','_blank')">
                    <img src="${ss.url}" style="width:100%;height:120px;object-fit:cover;display:block;">
                    <div style="padding:4px 6px;font-size:11px;color:var(--text-secondary);white-space:nowrap;overflow:hidden;text-overflow:ellipsis;">${ss.name}</div>
                </div>
            `).join('');
        } else {
            grid.innerHTML = '<div style="text-align:center;color:var(--text-secondary);padding:20px;">暂无截图</div>';
        }
    } catch (e) {
        grid.innerHTML = '<div style="text-align:center;color:var(--text-secondary);padding:20px;">加载失败</div>';
    }
}

function closeScreenshotModal() {
    const modal = document.getElementById('screenshot-modal');
    if (modal) {
        modal.classList.remove('modal-visible');
        modal.style.display = 'none';
    }
}

// ─── 初始化设置页面 ──────────────────────────────────────

async function initSettingsPages() {
    setupSettingsSubmenu();
    loadLaunchSettings();
    await loadPersonalizeSettings();
    loadOtherSettings();
    fetch('/api/settings/data-dir').then(r => r.json()).then(d => {
        const el = document.getElementById('setting-data-dir');
        if (el && d.dataDir) {
            el.value = d.dataDir;
            el.title = d.dataDir;
        }
    }).catch(() => {});
}

function uploadImage(type) {
    const inputId = type === 'background' ? 'bg-image-input' : 'avatar-input';
    const input = document.getElementById(inputId);
    if (input) {
        input.click();
    }
}

function handleImageUpload(input, type) {
    const file = input.files[0];
    if (!file) return;

    const reader = new FileReader();
    reader.onload = async (e) => {
        const dataUrl = e.target.result;
        try {
            if (type === 'background') {
                await API.saveBackgroundImage(dataUrl);
                const preview = document.getElementById('bg-image-preview');
                const placeholder = document.getElementById('bg-image-placeholder');
                if (preview) {
                    preview.style.backgroundImage = `url(${dataUrl})`;
                    preview.style.display = 'block';
                }
                if (placeholder) placeholder.style.display = 'none';
                document.body.style.setProperty('--bg-image', `url(${dataUrl})`);
                showToast('背景图片已更新', 'success');
            } else if (type === 'avatar') {
                await API.saveAvatarImage(dataUrl);
                const preview = document.getElementById('avatar-preview');
                const placeholder = document.getElementById('avatar-placeholder');
                if (preview) {
                    preview.style.backgroundImage = `url(${dataUrl})`;
                    preview.style.display = 'block';
                }
                if (placeholder) placeholder.style.display = 'none';
                const homeAvatar = document.getElementById('home-avatar');
                const launchAvatar = document.getElementById('launch-avatar');
                if (homeAvatar) {
                    homeAvatar.style.backgroundImage = '';
                    homeAvatar.innerHTML = `<img src="${dataUrl}" class="account-avatar-img" width="64" height="64">`;
                }
                if (launchAvatar) {
                    launchAvatar.style.backgroundImage = '';
                    launchAvatar.innerHTML = `<img src="${dataUrl}" class="account-avatar-img">`;
                }
                showToast('头像已更新', 'success');
            }
        } catch (err) {
            showToast('图片保存失败: ' + (err.message || ''), 'error');
        }
    };
    reader.readAsDataURL(file);
}

function clearImage(type) {
    if (type === 'background') {
        API.clearBackgroundImage().then(() => {
            const preview = document.getElementById('bg-image-preview');
            const placeholder = document.getElementById('bg-image-placeholder');
            if (preview) { preview.style.backgroundImage = ''; preview.style.display = 'none'; }
            if (placeholder) placeholder.style.display = 'flex';
            document.body.style.removeProperty('--bg-image');
            showToast('背景图片已清除', 'success');
        }).catch(e => showToast('清除失败', 'error'));
    } else if (type === 'avatar') {
        API.clearAvatarImage().then(() => {
            const preview = document.getElementById('avatar-preview');
            const placeholder = document.getElementById('avatar-placeholder');
            if (preview) { preview.style.backgroundImage = ''; preview.style.display = 'none'; }
            if (placeholder) placeholder.style.display = 'flex';
            const homeAvatar = document.getElementById('home-avatar');
            const launchAvatar = document.getElementById('launch-avatar');
            if (homeAvatar) homeAvatar.style.backgroundImage = '';
            if (launchAvatar) launchAvatar.style.backgroundImage = '';
            loadAccounts();
            showToast('头像已清除', 'success');
        }).catch(e => showToast('清除失败', 'error'));
    }
}

function useDefaultImage(type) {
    if (type === 'background') {
        API.clearBackgroundImage().then(() => {
            const preview = document.getElementById('bg-image-preview');
            const placeholder = document.getElementById('bg-image-placeholder');
            if (preview) { preview.style.backgroundImage = ''; preview.style.display = 'none'; }
            if (placeholder) placeholder.style.display = 'flex';
            document.body.style.removeProperty('--bg-image');
            showToast('已恢复默认背景', 'success');
        }).catch(e => showToast('恢复失败', 'error'));
    }
}

function browseJavaPath() {
    if (window.electronAPI && window.electronAPI.showOpenDialog) {
        window.electronAPI.showOpenDialog({
            properties: ['openFile'],
            filters: [{ name: 'Java 可执行文件', extensions: ['exe', ''] }]
        }).then(result => {
            if (!result.canceled && result.filePaths.length > 0) {
                const path = result.filePaths[0];
                const input = document.getElementById('setting-java-path');
                if (input) input.value = path;
            }
        }).catch(() => {});
    } else {
        showToast('请手动输入 Java 路径', 'info');
    }
}
