const dlManager = {
    tasks: new Map(),
    order: [],
    add(id, name, type, sessionId, iconUrl) {
        if (this.tasks.has(id)) return;
        this.tasks.set(id, { id, name, type, sessionId, iconUrl: iconUrl || '', progress: 0, status: 'downloading', message: '', files: [], stageHistory: [], expanded: false });
        this.order.push(id);
        this.updateFab();
        this.render();
        // 同步到 V 岛（如果开启）
        if (window.DynamicIsland && typeof window.DynamicIsland.isEnabled === 'function' && window.DynamicIsland.isEnabled()) {
            try { window.DynamicIsland.show(name); } catch (e) {}
        }
    },
    remove(id) {
        const task = this.tasks.get(id);
        this.tasks.delete(id);
        this.order = this.order.filter(i => i !== id);
        this.updateFab();
        this.render();
        // V 岛任务在 update(status=completed/failed) 时自动移除，这里不额外处理
    },
    async cancel(id) {
        const task = this.tasks.get(id);
        if (!task || task.status !== 'downloading') return;
        task.status = 'cancelling';
        task.message = '正在取消...';
        this.updateDom(id);
        try {
            if (task.sessionId) {
                if (task.type === 'java') {
                    await API.cancelJavaDownload(task.sessionId);
                } else {
                    await fetch(`/api/install-cancel?sessionId=${encodeURIComponent(task.sessionId)}`, { method: 'POST' });
                }
            }
            task.status = 'failed';
            task.message = '已取消';
            task.progress = Math.min(task.progress, 100);
        } catch (e) {
            task.status = 'failed';
            task.message = '取消失败: ' + (e.message || e);
        }
        this.updateFab();
        this.updateDom(id);
    },
    update(id, data) {
        const task = this.tasks.get(id);
        if (!task) return;
        const targetProgress = Math.min(data.progress || 0, 100);
        let smoothProgress;
        if (data.status === 'completed' || data.status === 'failed') {
            smoothProgress = data.status === 'completed' ? 100 : Math.min(targetProgress, 100);
        } else if (task.type === 'modpack' && task._smoothProgress !== undefined) {
            const cur = task._smoothProgress || 0;
            if (targetProgress <= cur || targetProgress <= 0) {
                smoothProgress = targetProgress;
            } else {
                const smoothed = cur * 0.7 + targetProgress * 0.3;
                smoothProgress = Math.max(cur, Math.min(Math.round(smoothed), 100));
            }
        } else {
            smoothProgress = targetProgress;
        }
        Object.assign(task, data);
        task.progress = Math.min(task.progress || 0, 100);
        if (data.status === 'completed' || data.status === 'failed') {
            task.progress = data.status === 'completed' ? 100 : Math.min(task.progress, 100);
            task._smoothProgress = task.progress;
        } else if (task.type === 'modpack') {
            task.progress = smoothProgress;
            task._smoothProgress = smoothProgress;
        }
        const now = Date.now();
        const isTerminal = data.status === 'completed' || data.status === 'failed';
        const lastUpdate = task._lastDomUpdate || 0;
        if (!isTerminal && now - lastUpdate < 200) {
            return;
        }
        task._lastDomUpdate = now;
        this.updateFab();
        this.updateDom(id);
        // 同步到 V 岛（如果开启）
        if (window.DynamicIsland && typeof window.DynamicIsland.isEnabled === 'function' && window.DynamicIsland.isEnabled()) {
            try {
                window.DynamicIsland.update({
                    name: task.name,
                    progress: task.progress,
                    status: task.status,
                    message: task.message,
                    speed: task.speed,
                    files: task.files,
                    stageHistory: task.stageHistory,
                    currentFile: task.currentFile
                });
            } catch (e) {}
        }
    },
    updateDom(id) {
        const taskEl = document.querySelector('.dl-task[data-task-id="' + id + '"]');
        if (!taskEl) return;
        const t = this.tasks.get(id);
        if (!t) return;
        // 图标 URL 变化时同步更新（例如版本安装任务异步获取到 base64 图标后）
        const iconContainer = taskEl.querySelector('.dl-task-icon');
        if (iconContainer && t.iconUrl) {
            let img = iconContainer.querySelector('.dl-task-icon-img');
            if (!img) {
                iconContainer.innerHTML = '<img src="' + escapeHtml(t.iconUrl) + '" alt="" class="dl-task-icon-img" onerror="this.style.display=\'none\';this.nextElementSibling.style.display=\'flex\'"><div class="dl-task-icon-fallback dl-task-icon-svg" style="display:none">' + (svgIcons[t.type] || svgIcons.other) + '</div>';
            } else if (img.src !== t.iconUrl) {
                img.src = t.iconUrl;
                img.style.display = '';
                const fallback = iconContainer.querySelector('.dl-task-icon-fallback');
                if (fallback) fallback.style.display = 'none';
            }
        }
        const fill = taskEl.querySelector('.dl-task-progress-fill');
        const percent = taskEl.querySelector('.dl-task-percent');
        if (fill) {
            fill.style.width = t.progress + '%';
            fill.className = 'dl-task-progress-fill' + (t.status === 'completed' ? ' dl-task-progress-fill--completed' : (t.status === 'failed' || t.status === 'cancelling') ? ' dl-task-progress-fill--failed' : '');
        }
        if (percent) percent.textContent = Math.round(t.progress) + '%';
        const statusEl = taskEl.querySelector('.dl-task-status');
        if (statusEl) {
            statusEl.textContent = t.status === 'completed' ? (t.message || '下载完成') : t.status === 'failed' ? (t.message || '下载失败') : (t.message || '下载中...');
        }
        let detailEl = taskEl.querySelector('.dl-task-detail');
        const hasStageData = t.type === 'modpack' && t.stageHistory && t.stageHistory.length > 0;
        const hasFileData = t.files && t.files.length > 0;
        if (!detailEl && t.type !== 'mod' && (hasStageData || hasFileData)) {
            detailEl = document.createElement('div');
            detailEl.className = 'dl-task-detail';
            const headerEl = taskEl.querySelector('.dl-task-header');
            if (headerEl && headerEl.nextSibling) {
                taskEl.insertBefore(detailEl, headerEl.nextSibling);
            } else {
                taskEl.appendChild(detailEl);
            }
        }
        if (detailEl) {
            if (hasStageData) {
                var stageHash = '';
                for (var i = 0; i < t.stageHistory.length; i++) {
                    var s = t.stageHistory[i];
                    stageHash += s.stage + '_' + s.progress + '_' + s.message + ';';
                }
                if (stageHash !== t._lastStageHash) {
                    t._lastStageHash = stageHash;
                    detailEl.innerHTML = this.buildStageHistoryHtml(t.stageHistory);
                }
            } else if (hasFileData) {
                var hash = '';
                for (var i = 0; i < t.files.length; i++) {
                    var f = t.files[i];
                    hash += f.name + '_' + f.status + '_' + f.progress + ';';
                }
                if (hash !== t._lastFilesHash) {
                    t._lastFilesHash = hash;
                    detailEl.innerHTML = this.buildFilesHtml(t.files);
                }
            }
        }
        if (t.status !== 'downloading' && t.status !== 'cancelling') {
            if (!taskEl.querySelector('.dl-task-actions')) {
                const actionsDiv = document.createElement('div');
                actionsDiv.className = 'dl-task-actions';
                const btn = document.createElement('button');
                btn.className = 'btn btn-secondary btn-sm';
                btn.textContent = '移除';
                btn.addEventListener('click', () => dlManager.remove(id));
                actionsDiv.appendChild(btn);
                taskEl.appendChild(actionsDiv);
            }
        } else if (t.status === 'downloading' && !taskEl.querySelector('.dl-task-actions')) {
            const actionsDiv = document.createElement('div');
            actionsDiv.className = 'dl-task-actions';
            const btn = document.createElement('button');
            btn.className = 'btn btn-danger btn-sm';
            btn.textContent = '取消';
            btn.addEventListener('click', () => dlManager.cancel(id));
            actionsDiv.appendChild(btn);
            taskEl.appendChild(actionsDiv);
        }
    },
    buildFilesHtml(files) {
        return files.map(f => {
            const fProgress = f.progress || 0;
            const fFillClass = f.status === 'completed' ? 'dl-task-progress-fill--completed' : f.status === 'failed' ? 'dl-task-progress-fill--failed' : '';
            const sIcon = f.status === 'completed' ? '✓' : f.status === 'failed' ? '✗' : f.status === 'downloading' ? '↓' : '○';
            const sClass = 'dl-file-status--' + (f.status || 'pending');
            const progressBar = (f.status === 'downloading' || f.status === 'pending') ? '<div class="dl-file-progress-bar"><div class="dl-file-progress-fill ' + fFillClass + '" style="width:' + fProgress + '%"></div></div><span class="dl-file-percent">' + fProgress + '%</span>' : '';
            return '<div class="dl-file-item"><span class="dl-file-status ' + sClass + '">' + sIcon + '</span><span class="dl-file-name">' + escapeHtml(f.name || '') + '</span>' + (f.size ? '<span class="dl-file-size">' + f.size + '</span>' : '') + '</div>' + (progressBar ? '<div class="dl-file-progress">' + progressBar + '</div>' : '');
        }).join('');
    },
    buildStageHistoryHtml(stages) {
        // stage list: shows each import stage with progress % and status icon
        return stages.map(s => {
            const progress = s.progress || 0;
            let statusIcon = '○';
            let statusClass = 'dl-stage-status--pending';
            let progressHtml = '';
            if (progress >= 100) {
                statusIcon = '✓';
                statusClass = 'dl-stage-status--completed';
            } else if (progress > 0) {
                statusIcon = '↓';
                statusClass = 'dl-stage-status--active';
                progressHtml = '<div class="dl-stage-progress-bar"><div class="dl-stage-progress-fill" style="width:' + Math.round(progress) + '%"></div></div><span class="dl-stage-percent">' + Math.round(progress) + '%</span>';
            }
            return '<div class="dl-stage-item"><span class="dl-stage-status ' + statusClass + '">' + statusIcon + '</span><span class="dl-stage-name">' + escapeHtml(s.message || s.stage || '') + '</span>' + (progressHtml ? '<div class="dl-stage-progress">' + progressHtml + '</div>' : '') + '</div>';
        }).join('');
    },
    toggleExpand(id) {
        const task = this.tasks.get(id);
        if (!task) return;
        task.expanded = !task.expanded;
        const taskEl = document.querySelector('.dl-task[data-task-id="' + id + '"]');
        if (taskEl) {
            if (task.expanded) {
                taskEl.classList.add('dl-task--expanded');
            } else {
                taskEl.classList.remove('dl-task--expanded');
            }
            let detailEl = taskEl.querySelector('.dl-task-detail');
            if (!detailEl) {
                detailEl = document.createElement('div');
                detailEl.className = 'dl-task-detail';
                const headerEl = taskEl.querySelector('.dl-task-header');
                if (headerEl && headerEl.nextSibling) {
                    taskEl.insertBefore(detailEl, headerEl.nextSibling);
                } else {
                    taskEl.appendChild(detailEl);
                }
            }
            if (task.type === 'modpack' && task.stageHistory && task.stageHistory.length > 0) {
                detailEl.innerHTML = this.buildStageHistoryHtml(task.stageHistory);
            } else if (task.files && task.files.length > 0) {
                detailEl.innerHTML = this.buildFilesHtml(task.files);
            } else if (task.expanded) {
                detailEl.innerHTML = '<div style="color:var(--text-muted);font-size:12px;padding:4px 0;">' + (task.status === 'downloading' ? '等待进度数据...' : '暂无详细信息') + '</div>';
            }
        } else {
            this.render();
        }
    },
    updateFab() {
        const fab = document.getElementById('dl-fab');
        const badge = document.getElementById('dl-fab-badge');
        if (!fab) return;
        const active = [...this.tasks.values()].filter(t => t.status === 'downloading').length;
        const total = this.tasks.size;
        if (total === 0) {
            fab.style.display = 'none';
        } else {
            fab.style.display = 'flex';
            if (badge) {
                badge.style.display = active > 0 ? 'flex' : 'none';
                badge.textContent = active;
            }
        }
    },
    render() {
        const list = document.getElementById('download-queue-list');
        if (!list) return;
        if (this.order.length === 0) {
            list.innerHTML = '<p class="empty-text" id="dl-empty-hint">暂无下载任务</p>';
            return;
        }
        const svgIcons = {
            mod: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:20px;height:20px"><path d="M20 16V7a2 2 0 00-2-2H6a2 2 0 00-2 2v9m16 0H4m16 0l1.28 2.55a1 1 0 01-.9 1.45H3.62a1 1 0 01-.9-1.45L4 16"/></svg>',
            modpack: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:20px;height:20px"><path d="M21 16V8a2 2 0 00-1-1.73l-7-4a2 2 0 00-2 0l-7 4A2 2 0 003 8v8a2 2 0 001 1.73l7 4a2 2 0 002 0l7-4A2 2 0 0021 16z"/><path d="M3.27 6.96L12 12.01l8.73-5.05M12 22.08V12"/></svg>',
            version: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:20px;height:20px"><rect x="2" y="6" width="20" height="12" rx="2"/><path d="M6 12h.01M10 12h.01M14 12h4"/></svg>',
            java: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:20px;height:20px"><path d="M17 8h1a4 4 0 110 8h-1M3 8h14v9a4 4 0 01-4 4H7a4 4 0 01-4-4V8zm0 0V6a2 2 0 012-2h2m4-2v2"/></svg>',
            other: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:20px;height:20px"><path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/><path d="M14 2v6h6M16 13H8m8 4H8m2-8H8"/></svg>'
        };
        list.innerHTML = this.order.map(id => {
            const t = this.tasks.get(id);
            if (!t) return '';
            const iconClass = 'dl-task-icon--' + (t.type || 'other');
            const iconHtml = t.iconUrl
                ? '<img src="' + t.iconUrl + '" alt="" class="dl-task-icon-img" onerror="this.style.display=\'none\';this.nextElementSibling.style.display=\'flex\'"><div class="dl-task-icon-fallback dl-task-icon-svg" style="display:none">' + (svgIcons[t.type] || svgIcons.other) + '</div>'
                : svgIcons[t.type] || svgIcons.other;
            const fillClass = t.status === 'completed' ? 'dl-task-progress-fill--completed' : t.status === 'failed' ? 'dl-task-progress-fill--failed' : '';
            const statusText = t.status === 'completed' ? (t.message || '下载完成') : t.status === 'failed' ? (t.message || '下载失败') : (t.message || '下载中...');
            const isExpandable = t.type !== 'mod';
            const expandedClass = t.expanded && isExpandable ? 'dl-task--expanded' : '';
            let detailHtml = '';
            if (isExpandable) {
                let innerDetail = '';
                if (t.type === 'modpack' && t.stageHistory && t.stageHistory.length > 0) {
                    innerDetail = this.buildStageHistoryHtml(t.stageHistory);
                } else if (t.files && t.files.length > 0) {
                    innerDetail = this.buildFilesHtml(t.files);
                }
                detailHtml = '<div class="dl-task-detail">' + innerDetail + '</div>';
            }
            let actionsHtml = '';
            if (t.status === 'completed' || t.status === 'failed') {
                actionsHtml = '<div class="dl-task-actions"><button class="btn btn-secondary btn-sm dl-task-remove-btn" data-task-id="' + escapeHtml(id) + '">移除</button></div>';
            } else if (t.status === 'downloading') {
                actionsHtml = '<div class="dl-task-actions"><button class="btn btn-danger btn-sm dl-task-cancel-btn" data-task-id="' + escapeHtml(id) + '">取消</button></div>';
            } else if (t.status === 'cancelling') {
                actionsHtml = '<div class="dl-task-actions"><button class="btn btn-secondary btn-sm" disabled>取消中...</button></div>';
            }
            const arrowHtml = isExpandable ? '<svg class="dl-task-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M6 9l6 6 6-6"/></svg>' : '';
            const headerClass = isExpandable ? 'dl-task-header dl-task-toggle-btn' : 'dl-task-header';
            return '<div class="dl-task ' + expandedClass + '" data-task-id="' + escapeHtml(id) + '">' +
                '<div class="' + headerClass + '" data-task-id="' + escapeHtml(id) + '">' +
                '<div class="dl-task-icon ' + iconClass + '">' + iconHtml + '</div>' +
                '<div class="dl-task-info">' +
                '<div class="dl-task-name">' + escapeHtml(t.name) + '</div>' +
                '<div class="dl-task-status">' + escapeHtml(statusText) + '</div>' +
                '</div>' +
                '<div class="dl-task-progress">' +
                '<div class="dl-task-progress-bar"><div class="dl-task-progress-fill ' + fillClass + '" style="width:' + t.progress + '%"></div></div>' +
                '<span class="dl-task-percent">' + Math.round(t.progress) + '%</span>' +
                '</div>' +
                arrowHtml +
                '</div>' +
                detailHtml +
                actionsHtml +
                '</div>';
        }).join('');

        list.querySelectorAll('.dl-task-toggle-btn').forEach(el => {
            el.addEventListener('click', () => dlManager.toggleExpand(el.dataset.taskId));
        });
        list.querySelectorAll('.dl-task-remove-btn').forEach(el => {
            el.addEventListener('click', () => dlManager.remove(el.dataset.taskId));
        });
        list.querySelectorAll('.dl-task-cancel-btn').forEach(el => {
            el.addEventListener('click', () => dlManager.cancel(el.dataset.taskId));
        });
    }
};

function clearCompletedDownloads() {
    const toRemove = [...dlManager.tasks.entries()].filter(([_, t]) => t.status === 'completed' || t.status === 'failed').map(([id]) => id);
    toRemove.forEach(id => dlManager.remove(id));
}

let _customDlSessionId = null;
let _customDlPollTimer = null;
let _customDlSavePath = '';

async function browseCustomDlPath() {
    const result = await window.electronAPI.showOpenDialog({ properties: ['openDirectory'], title: '选择保存位置' });
    if (result && result.filePaths && result.filePaths.length > 0) {
        document.getElementById('custom-dl-path').value = result.filePaths[0];
        _customDlSavePath = result.filePaths[0];
    }
}

function openCustomDlFolder() {
    const p = _customDlSavePath || document.getElementById('custom-dl-path')?.value || '';
    if (p) {
        fetch('/api/open-folder', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ folder: 'custom', customPath: p }) });
    } else {
        showToast('请先选择保存位置', 'info');
    }
}

async function startCustomDownload() {
    const url = document.getElementById('custom-dl-url')?.value?.trim() || '';
    const savePath = document.getElementById('custom-dl-path')?.value?.trim() || '';
    const fileName = document.getElementById('custom-dl-filename')?.value?.trim() || '';

    if (!url) { showToast('请输入下载地址', 'error'); return; }
    if (!savePath) { showToast('请选择保存位置', 'error'); return; }

    try { new URL(url); } catch (e) { showToast('请输入有效的下载地址', 'error'); return; }

    document.getElementById('custom-dl-start-btn').style.display = 'none';
    document.getElementById('custom-dl-cancel-btn').style.display = '';
    document.getElementById('custom-dl-progress').style.display = '';
    document.getElementById('custom-dl-progress-fill').style.width = '0%';
    document.getElementById('custom-dl-progress-text').textContent = '0%';
    document.getElementById('custom-dl-status').textContent = '正在连接...';

    try {
        const res = await fetch('/api/download-custom', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ url, savePath, fileName })
        });
        const result = await res.json();
        if (result.error) {
            showToast(result.error, 'error');
            resetCustomDlUI();
            return;
        }
        _customDlSessionId = result.sessionId;
        _customDlSavePath = savePath;
        pollCustomDlProgress();
    } catch (e) {
        showToast('下载请求失败: ' + e.message, 'error');
        resetCustomDlUI();
    }
}

function pollCustomDlProgress() {
    if (_customDlPollTimer) clearInterval(_customDlPollTimer);
    _customDlPollTimer = setInterval(async () => {
        if (!_customDlSessionId) { clearInterval(_customDlPollTimer); return; }
        try {
            const res = await fetch(`/api/download-custom/status?sessionId=${encodeURIComponent(_customDlSessionId)}`);
            const data = await res.json();
            if (data.status === 'not_found') { clearInterval(_customDlPollTimer); resetCustomDlUI(); return; }

            document.getElementById('custom-dl-progress-fill').style.width = data.progress + '%';
            document.getElementById('custom-dl-progress-text').textContent = data.progress + '%';
            document.getElementById('custom-dl-status').textContent = data.message || '';

            if (data.status === 'completed') {
                clearInterval(_customDlPollTimer);
                showToast('下载完成！', 'success');
                resetCustomDlUI();
            } else if (data.status === 'failed' || data.status === 'cancelled') {
                clearInterval(_customDlPollTimer);
                showToast(data.message || '下载失败', 'error');
                resetCustomDlUI();
            }
        } catch (e) {}
    }, 500);
}

async function cancelCustomDownload() {
    if (_customDlSessionId) {
        try {
            await fetch('/api/download-custom/cancel', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ sessionId: _customDlSessionId })
            });
        } catch (e) {}
    }
    if (_customDlPollTimer) clearInterval(_customDlPollTimer);
    resetCustomDlUI();
}

function resetCustomDlUI() {
    _customDlSessionId = null;
    document.getElementById('custom-dl-start-btn').style.display = '';
    document.getElementById('custom-dl-cancel-btn').style.display = 'none';
}
