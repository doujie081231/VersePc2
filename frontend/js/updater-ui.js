/* updater-ui.js - 更新检查 UI 逻辑（从 index.html 内嵌 script 抽出） */
(function() {
    const api = window.electronAPI;
    if (!api || !api.updater) return;

    let currentUpdateVersion = null;
    let _latestVersion = null;

    async function initUpdaterUI() {
        try {
            const el = document.getElementById('updater-version-info');
            if (!el) {
                // Vue 还没渲染完，稍后重试
                setTimeout(initUpdaterUI, 200);
                return;
            }
            const result = await api.updater.getVersion();
            // Tauri 端 get_app_version 直接返回版本字符串（如 "0.3.0"），Electron 端返回 { version }
            const verText = (result && typeof result === 'object' && result.version) ? result.version : result;
            el.textContent = '当前版本：v' + verText;
        } catch (e) {
            const el = document.getElementById('updater-version-info');
            if (el) el.textContent = '当前版本：未知';
        }

        // 自更新重启后：读取一次性"已更新"提示并弹出"软件已更新"卡片
        try {
            if (api.updater.getPendingNotice) {
                const notice = await api.updater.getPendingNotice();
                if (notice && notice.version) {
                    showUpdatedNotice(notice);
                }
            }
        } catch (e) {}

        // 启动时自动检查更新：仅 Windows 平台做自更新检查（自更新依赖 .bat+tasklist 替换 exe 且
        // update.json 仅提供 win-x64 包）；macOS/Linux 上跳过，不弹"发现新版本"卡片。
        try {
            if (api.updater.checkForUpdates && api.isWindows) {
                await api.updater.checkForUpdates();
            }
        } catch (e) {
            console.warn('[updater-ui] 启动自动检查更新失败:', e);
        }
    }

    // 显示"软件已更新至 xxx 版本"弹窗（自更新重启后触发）
    function showUpdatedNotice(notice) {
        const modal = document.getElementById('update-modal');
        if (!modal) return;
        const titleEl = document.getElementById('update-modal-title');
        const verEl = document.getElementById('update-modal-version');
        const contentEl = document.getElementById('update-modal-content');
        const gotoBtn = document.getElementById('update-modal-goto-btn');
        const quarkBtn = document.getElementById('update-modal-quark-btn');
        const ackBtn = document.getElementById('update-modal-acknowledge-btn');
        if (titleEl) titleEl.textContent = '更新完成';
        if (verEl) verEl.textContent = '软件已更新至 v' + notice.version + ' 版本';
        if (contentEl) {
            const notes = notice.notes || '';
            const render = () => {
                contentEl.innerHTML = '<div class="update-status__title" style="margin-bottom:8px;">更新内容：</div>' +
                    (typeof marked !== 'undefined' ? marked.parse(notes) : (notes ? notes.replace(/\n/g, '<br>') : '<p>详情请前往官网查看</p>'));
            };
            if (typeof marked === 'undefined') {
                _lazyLoadScript('js/marked.min.js').then(render).catch(render);
            } else {
                render();
            }
        }
        if (gotoBtn) gotoBtn.style.display = 'none';
        if (quarkBtn) quarkBtn.style.display = 'none';
        if (ackBtn) ackBtn.style.display = '';
        if (modal.parentElement !== document.body) {
            document.body.appendChild(modal);
        }
        modal.style.display = 'flex';
        requestAnimationFrame(function () {
            modal.classList.add('modal-visible');
            modal.classList.remove('modal-exiting');
        });
        var onKeyDown = function (e) {
            if (e.key === 'Escape') dismissUpdateModal();
        };
        modal.addEventListener('keydown', onKeyDown);
        modal._escCleanup = function () { modal.removeEventListener('keydown', onKeyDown); };
    }

    function resetButtons() {
        document.getElementById('updater-check-btn').style.display = '';
        document.getElementById('updater-check-btn').disabled = false;
        document.getElementById('updater-skip-btn').style.display = 'none';
        document.getElementById('updater-release-btn').style.display = 'none';
        document.getElementById('updater-download-btn').style.display = 'none';
        document.getElementById('updater-install-btn').style.display = 'none';
        document.getElementById('updater-release-notes').style.display = 'none';
    }

    function formatBytes(bytes) {
        if (!bytes || bytes === 0) return '0 B';
        const k = 1024;
        const sizes = ['B', 'KB', 'MB', 'GB'];
        const i = Math.floor(Math.log(bytes) / Math.log(k));
        return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
    }

    async function showReleaseNotes(notes) {
        const container = document.getElementById('updater-release-notes');
        const content = document.getElementById('updater-release-notes-content');
        if (notes && notes.trim()) {
            if (typeof marked === 'undefined') {
                try { await _lazyLoadScript('js/marked.min.js'); } catch (e) {}
            }
            if (typeof marked !== 'undefined') {
                content.innerHTML = marked.parse(notes.trim());
            } else {
                content.textContent = notes.trim();
            }
            container.style.display = '';
        } else {
            container.style.display = 'none';
        }
    }

    function escapeHtml(str) {
        if (str == null) return '';
        return String(str).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;').replace(/'/g, '&#39;');
    }

    api.updater.onStatusChanged(({ channel, data }) => {
        const statusArea = document.getElementById('updater-status-area');
        const checkBtn = document.getElementById('updater-check-btn');
        const skipBtn = document.getElementById('updater-skip-btn');
        const releaseBtn = document.getElementById('updater-release-btn');
        const downloadBtn = document.getElementById('updater-download-btn');
        const installBtn = document.getElementById('updater-install-btn');

        switch (channel) {
            case 'checking-for-update':
                statusArea.innerHTML = '<div class="update-status update-status--loading"><span class="update-status__icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 12a9 9 0 11-6.219-8.56"/></svg></span><span class="update-status__body">正在检查更新...</span></div>';
                checkBtn.disabled = true;
                break;

            case 'update-available':
                currentUpdateVersion = data.version;
                _latestVersion = data.version;
                statusArea.innerHTML = '<div class="update-status update-status--info"><span class="update-status__icon"></span><div class="update-status__body"><div class="update-status__title">发现新版本 v' + data.version + '</div><div class="update-status__desc">点击下方按钮下载或查看详情</div></div></div>';
                checkBtn.style.display = 'none';
                skipBtn.style.display = '';
                releaseBtn.style.display = '';
                downloadBtn.style.display = '';
                installBtn.style.display = 'none';
                showReleaseNotes(data.releaseNotes);
                showUpdatePopup(data);
                addUpdateDots();
                break;

            case 'update-not-available':
                statusArea.innerHTML = '<div class="update-status update-status--success"><span class="update-status__icon"></span><span class="update-status__body">已是最新版本</span></div>';
                checkBtn.disabled = false;
                skipBtn.style.display = 'none';
                releaseBtn.style.display = 'none';
                break;

            case 'update-skipped':
                statusArea.innerHTML = '<div class="update-status update-status--loading"><span class="update-status__icon"></span><span class="update-status__body">已跳过 v' + escapeHtml(data.version) + '</span></div>';
                checkBtn.disabled = false;
                break;

            case 'update-error':
                const errorType = data.errorType || 'unknown';
                const hint = data.hint || '';
                let hintHtml = '';
                if (hint) hintHtml = '<div class="update-status__hint">' + hint + '</div>';

                let mirrorBar = '';

                statusArea.innerHTML =
                    '<div class="update-status update-status--error">' +
                    '<span class="update-status__icon"></span>' +
                    '<div class="update-status__body"><div class="update-status__title">检查更新失败</div>' +
                    '<div class="update-status__desc">' + escapeHtml(data.message || '未知错误') + '</div>' +
                    hintHtml + mirrorBar + '</div></div>';
                checkBtn.disabled = false;
                break;

            case 'start-download':
                statusArea.innerHTML = '<div class="update-status update-status--loading"><span class="update-status__icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 12a9 9 0 11-6.219-8.56"/></svg></span><span class="update-status__body">正在下载更新...</span></div>';
                downloadBtn.disabled = true;
                downloadBtn.textContent = '下载中...';
                break;

            case 'download-progress': {
                const pct = data.percent ? data.percent.toFixed(1) : 0;
                const speed = formatBytes(data.bytesPerSecond);
                const transferred = formatBytes(data.transferred);
                const total = formatBytes(data.total);
                statusArea.innerHTML =
                    '<div class="update-progress">' +
                    '<div class="update-progress__header"><span class="update-progress__label"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 12a9 9 0 11-6.219-8.56"/></svg>正在下载更新...</span><span class="update-progress__percent">' + pct + '%</span></div>' +
                    '<div class="update-progress__track"><div class="update-progress__fill" style="width:' + pct + '%;"></div></div>' +
                    '<div class="update-progress__info"><span>' + speed + '/s</span><span>' + transferred + ' / ' + total + '</span></div></div>';
                break;
            }

            case 'update-downloaded':
                currentUpdateVersion = null;
                statusArea.innerHTML = '<div class="update-status update-status--success"><span class="update-status__icon"></span><div class="update-status__body"><div class="update-status__title">更新已下载完成 (v' + data.version + ')</div><div class="update-status__desc">点击安装并重启以完成更新</div></div></div>';
                skipBtn.style.display = 'none';
                releaseBtn.style.display = 'none';
                downloadBtn.style.display = 'none';
                installBtn.style.display = '';
                break;
        }
    });

    window.handleCheckUpdate = async function() {
        try {
            await api.updater.checkForUpdates();
        } catch (e) {
            document.getElementById('updater-status-area').innerHTML =
                '<div style="color:#ef4444;">检查失败：' + e.message + '</div>';
        }
    };

    window.handleDownloadUpdate = async function() {
        const downloadBtn = document.getElementById('updater-download-btn');
        downloadBtn.disabled = true;
        downloadBtn.textContent = '下载中...';
        try {
            await api.updater.downloadUpdate();
        } catch (e) {
            document.getElementById('updater-status-area').innerHTML =
                '<div style="color:#ef4444;">下载失败：' + e.message + '</div>';
            downloadBtn.disabled = false;
            downloadBtn.textContent = '下载更新';
        }
    };

    window.handleInstallUpdate = async function() {
        await api.updater.installUpdate();
    };

    window.handleSkipVersion = async function() {
        if (currentUpdateVersion) {
            await api.updater.skipVersion(currentUpdateVersion);
            resetButtons();
            document.getElementById('updater-status-area').innerHTML =
                '<div style="color:var(--text-muted);">已跳过 v' + currentUpdateVersion + '，下次启动时不再提醒此版本</div>';
        }
    };

    window.dismissUpdateCard = function() {
        const card = document.getElementById('update-card');
        if (card) card.style.display = 'none';
    };

    window.downloadFromGitHub = function() {
        window.electronAPI?.openExternal?.('https://www.verselauncher.cn/');
    };

    window.downloadFromQuark = function() {
        window.electronAPI?.openExternal?.('https://pan.quark.cn/s/04ec149d8a93');
    };

    function showUpdatePopup(data) {
        const modal = document.getElementById('update-modal');
        if (!modal) return;
        const verEl = document.getElementById('update-modal-version');
        const contentEl = document.getElementById('update-modal-content');
        const titleEl = document.getElementById('update-modal-title');
        if (titleEl) titleEl.textContent = '发现新版本';
        // 重置按钮：显示"前往更新/夸克网盘更新"，隐藏"我已知晓"
        const gotoBtn = document.getElementById('update-modal-goto-btn');
        const quarkBtn = document.getElementById('update-modal-quark-btn');
        const ackBtn = document.getElementById('update-modal-acknowledge-btn');
        if (gotoBtn) gotoBtn.style.display = '';
        if (quarkBtn) quarkBtn.style.display = '';
        if (ackBtn) ackBtn.style.display = 'none';
        if (verEl) verEl.textContent = 'v' + (data.currentVersion || '?.?.?') + ' → v' + data.version;
        if (contentEl && data.releaseNotes) {
            const renderNotes = () => {
                const html = typeof marked !== 'undefined'
                    ? marked.parse(data.releaseNotes)
                    : data.releaseNotes.replace(/\n/g, '<br>');
                contentEl.innerHTML = html;
            };
            if (typeof marked === 'undefined') {
                _lazyLoadScript('js/marked.min.js').then(renderNotes).catch(() => {
                    contentEl.innerHTML = data.releaseNotes.replace(/\n/g, '<br>');
                });
            } else {
                renderNotes();
            }
        } else if (contentEl) {
            contentEl.innerHTML = '<p>更新内容加载中...</p>';
        }
        if (modal.parentElement !== document.body) {
            document.body.appendChild(modal);
        }
        modal.style.display = 'flex';
        requestAnimationFrame(function () {
            modal.classList.add('modal-visible');
            modal.classList.remove('modal-exiting');
        });
        var onKeyDown = function (e) {
            if (e.key === 'Escape') dismissUpdateModal();
        };
        modal.addEventListener('keydown', onKeyDown);
        modal._escCleanup = function () { modal.removeEventListener('keydown', onKeyDown); };
    }

    window.dismissUpdateModal = function() {
        const modal = document.getElementById('update-modal');
        if (!modal) return;
        if (typeof modal._escCleanup === 'function') {
            modal._escCleanup();
            modal._escCleanup = null;
        }
        modal.setAttribute('data-state', 'closed');
        modal.classList.add('modal-exiting');
        modal.classList.remove('modal-visible');
        setTimeout(function () {
            modal.classList.remove('modal-exiting');
            modal.style.display = 'none';
        }, 200);
    };

    // 立即更新：关闭弹窗 + 跳转到设置页的更新区
    window.gotoUpdateSettings = function() {
        window.dismissUpdateModal();
        // 跳转到其他设置页面
        if (typeof navigateToPage === 'function') {
            navigateToPage('settings-other');
        } else if (typeof switchPage === 'function') {
            switchPage('settings-other');
        }
        // 滚动到"关于 VersePC"卡片
        setTimeout(function() {
            var btnGroup = document.getElementById('updater-btn-group');
            if (btnGroup) {
                var card = btnGroup.closest('.card');
                if (card) card.scrollIntoView({ behavior: 'smooth', block: 'center' });
            }
        }, 300);
    };

    function addUpdateDots() {
        const settingsBtn = document.querySelector('#settings-submenu-group > .nav-btn');
        if (settingsBtn && !settingsBtn.querySelector('.update-dot')) {
            const dot = document.createElement('div');
            dot.className = 'update-dot';
            settingsBtn.style.position = 'relative';
            settingsBtn.appendChild(dot);
        }
        const updateBtn = document.getElementById('updater-check-btn');
        if (updateBtn && !updateBtn.querySelector('.update-dot')) {
            const dot = document.createElement('div');
            dot.className = 'update-dot';
            updateBtn.style.position = 'relative';
            updateBtn.appendChild(dot);
        }
    }

    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', initUpdaterUI);
    } else {
        initUpdaterUI();
    }
})();
