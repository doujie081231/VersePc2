function showJavaInstallModal(requiredVersion) {
    const existing = document.getElementById('java-install-modal');
    if (existing) existing.remove();

    const modalHtml = `
    <div class="modal" id="java-install-modal" style="display:flex;">
        <div class="modal-content java-install-modal-content">
            <div class="modal-header">
                <h3>☕ Java 运行环境</h3>
                <button class="modal-close" onclick="closeJavaInstallModal()">&times;</button>
            </div>
            <div class="modal-body">
                <div class="java-install-info">
                    <div class="java-install-icon">
                        <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" stroke-width="1.5">
                            <path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2z"/>
                            <path d="M12 6v6l4 2"/>
                        </svg>
                    </div>
                    <div class="java-install-text">
                        <p class="java-install-title">未检测到 Java ${requiredVersion}+</p>
                        <p class="java-install-desc">Minecraft 需要 Java 运行环境才能启动。请前往 Java 管理页面手动安装或配置 Java 路径。</p>
                    </div>
                </div>
            </div>
            <div class="modal-footer" id="java-install-footer">
                <button class="btn btn-secondary" onclick="closeJavaInstallModal()">稍后处理</button>
                <button class="btn btn-primary" onclick="closeJavaInstallModal();navigateToPage('java')">
                    <span>前往 Java 管理</span>
                </button>
            </div>
        </div>
    </div>`;

    document.body.insertAdjacentHTML('beforeend', modalHtml);
    requestAnimationFrame(() => {
        const modal = document.getElementById('java-install-modal');
        if (modal) modal.classList.add('modal-visible');
    });
}

async function loadJavaDownloadSources() {
    try {
        const result = await API.getJavaDownloadSources();
        const listEl = document.getElementById('java-source-list');
        if (!listEl || !result.sources) return;

        result.sources.forEach(source => {
            const item = document.createElement('div');
            item.className = 'java-source-item';
            item.dataset.source = source.id;
            item.innerHTML = `
                <span class="java-source-dot"></span>
                <span class="java-source-name">${source.name}</span>
                <span class="java-source-desc">${source.description}</span>
            `;
            listEl.appendChild(item);
        });
    } catch (e) { console.error('[Java] Failed to load download sources:', e); }
}

function closeJavaInstallModal() {
    if (javaInstallPollTimer) { clearInterval(javaInstallPollTimer); javaInstallPollTimer = null; }
    const modal = document.getElementById('java-install-modal');
    if (modal) {
        modal.classList.remove('modal-visible');
        setTimeout(() => modal.remove(), 300);
    }
}

async function startJavaAutoInstall(requiredVersion) {
    const installBtn = document.getElementById('java-install-btn');
    const progressDiv = document.getElementById('java-install-progress');
    const footerDiv = document.getElementById('java-install-footer');
    const sourceList = document.getElementById('java-source-list');

    if (installBtn) installBtn.disabled = true;
    if (progressDiv) progressDiv.style.display = 'block';
    if (sourceList) sourceList.style.display = 'none';

    try {
        const result = await API.autoInstallJava(requiredVersion);

        if (result.success && result.sessionId) {
            pollJavaInstallProgress(result.sessionId, requiredVersion);
        } else {
            showToast('Java检测请求失败', 'error');
            if (installBtn) installBtn.disabled = false;
        }
    } catch (e) {
        showToast('Java检测请求失败: ' + e.message, 'error');
        if (installBtn) installBtn.disabled = false;
    }
}

function pollJavaInstallProgress(sessionId, requiredVersion) {
    const progressBar = document.getElementById('java-install-progress-bar');
    const progressText = document.getElementById('java-progress-text');
    const progressStatus = document.getElementById('java-progress-status');
    const progressSource = document.getElementById('java-progress-source');
    const progressSpeed = document.getElementById('java-progress-speed');
    const progressSize = document.getElementById('java-progress-size-text');
    const installBtn = document.getElementById('java-install-btn');

    if (javaInstallPollTimer) clearInterval(javaInstallPollTimer);

    javaInstallPollTimer = setInterval(async () => {
        try {
            const status = await API.getJavaInstallStatus(sessionId);

            if (progressBar) {
                progressBar.style.width = (status.progress || 0) + '%';
            }
            if (progressStatus) {
                const statusMap = {
                    'detecting': '🔍 检测Java环境...',
                    'pending': '⏳ 准备下载...',
                    'downloading': '📥 下载中...',
                    'configuring': '⚙️ 配置环境变量...',
                    'completed': '✅ 安装完成',
                    'failed': '❌ 安装失败',
                    'need_manual': '⚠️ 需要手动配置'
                };
                progressStatus.textContent = statusMap[status.status] || status.message;
            }
            if (progressSource && status.source) {
                progressSource.textContent = `来源: ${status.source}`;
            }
            if (progressText) {
                progressText.textContent = status.message || '';
            }
            if (progressSpeed && status.speed) {
                progressSpeed.textContent = formatSpeed(status.speed);
            }
            if (progressSize && status.totalBytes) {
                progressSize.textContent = `${formatSize(status.downloadedBytes || 0)} / ${formatSize(status.totalBytes)}`;
            }

            if (status.status === 'completed') {
                clearInterval(javaInstallPollTimer);
                javaInstallPollTimer = null;

                if (status.result) {
                    const statJava = document.getElementById('stat-java');
                    if (statJava && status.result.majorVersion) {
                        statJava.textContent = status.result.majorVersion;
                    }
                    const javaPathInput = document.getElementById('setting-java-path');
                    if (javaPathInput && status.result.path) {
                        javaPathInput.value = status.result.path;
                    }
                }

                showToast('Java 安装成功！环境变量已自动配置', 'success');
                setTimeout(() => closeJavaInstallModal(), 1500);
            } else if (status.status === 'failed') {
                clearInterval(javaInstallPollTimer);
                javaInstallPollTimer = null;
                showToast(status.message || 'Java安装失败', 'error');
                if (installBtn) installBtn.disabled = false;
            } else if (status.status === 'need_manual') {
                clearInterval(javaInstallPollTimer);
                javaInstallPollTimer = null;
                showToast(status.message || '未找到合适的Java运行环境，请在设置中手动安装或配置', 'warning');
                if (installBtn) installBtn.disabled = false;
            }
        } catch (e) {
            clearInterval(javaInstallPollTimer);
            javaInstallPollTimer = null;
            showToast('获取安装状态失败', 'error');
            if (installBtn) installBtn.disabled = false;
        }
    }, 500);
}

async function ensureJavaForLaunch(requiredVersion) {
    try {
        const result = await API.detectJava();
        if (result.javaList && result.javaList.length > 0) {
            const suitable = result.javaList.find(j => j.majorVersion >= requiredVersion);
            if (suitable) return true;
        }

        showJavaInstallModal(requiredVersion);
        return new Promise((resolve) => {
            const checkInterval = setInterval(() => {
                const modal = document.getElementById('java-install-modal');
                if (!modal) {
                    clearInterval(checkInterval);
                    API.detectJava().then(r => {
                        if (r.javaList) {
                            const suitable = r.javaList.find(j => j.majorVersion >= requiredVersion);
                            resolve(!!suitable);
                        } else {
                            resolve(false);
                        }
                    }).catch(() => resolve(false));
                }
            }, 500);
        });
    } catch (e) {
        return false;
    }
}

async function openFolder(folder) {
    try { await API.openFolder(folder); }
    catch (e) { showToast('无法打开文件夹', 'error'); }
}

let _cleanupData = null;
async function cleanupScan() {
    const btn = document.getElementById('cleanup-scan-btn');
    const info = document.getElementById('cleanup-size-info');
    const details = document.getElementById('cleanup-details');
    const runBtn = document.getElementById('cleanup-run-btn');
    if (btn) { btn.disabled = true; btn.textContent = '扫描中...'; }
    try {
        const res = await API.cleanupScan();
        if (!res.success) { showToast('扫描失败: ' + (res.error || ''), 'error'); return; }
        _cleanupData = res;
        if (info) info.textContent = `可清理 ${res.totalMB} MB`;
        if (runBtn) runBtn.disabled = res.totalBytes <= 0;
        if (details) {
            const labels = { gameLogs: '游戏日志', tempFiles: '临时文件', natives: '本地库缓存', iconCache: '图标缓存', modpackCache: '整合包缓存', cache: '下载缓存' };
            const items = Object.entries(res.details || {}).filter(([, v]) => v > 0).map(([k, v]) => {
                const label = labels[k] || k;
                const mb = Math.round(v / (1024 * 1024) * 100) / 100;
                return `${label}: ${mb} MB`;
            });
            if (items.length === 0) {
                details.innerHTML = '<span style="color:var(--green)">暂无可清理的垃圾文件</span>';
                if (runBtn) runBtn.disabled = true;
            } else {
                details.innerHTML = items.map(s => `<div>• ${s}</div>`).join('');
            }
            details.style.display = 'block';
        }
    } catch (e) {
        showToast('扫描失败: ' + (e.message || e), 'error');
    } finally {
        if (btn) { btn.disabled = false; btn.textContent = '扫描'; }
    }
}

async function cleanupRun() {
    if (!_cleanupData || _cleanupData.totalBytes <= 0) { showToast('暂无可清理的内容', 'info'); return; }
    const runBtn = document.getElementById('cleanup-run-btn');
    const scanBtn = document.getElementById('cleanup-scan-btn');
    const info = document.getElementById('cleanup-size-info');
    const details = document.getElementById('cleanup-details');
    if (runBtn) { runBtn.disabled = true; runBtn.textContent = '清理中...'; }
    if (scanBtn) scanBtn.disabled = true;
    try {
        const res = await API.cleanupRun();
        if (!res.success) { showToast('清理失败: ' + (res.error || ''), 'error'); return; }
        showToast(res.message || '清理完成', 'success');
        _cleanupData = null;
        if (info) info.textContent = '已清理';
        if (details) { details.style.display = 'none'; details.innerHTML = ''; }
    } catch (e) {
        showToast('清理失败: ' + (e.message || e), 'error');
    } finally {
        if (runBtn) { runBtn.disabled = false; runBtn.textContent = '一键清理'; }
        if (scanBtn) scanBtn.disabled = false;
    }
}

function applyAccentColor(color) {
    if (!color || typeof color !== 'string' || !/^#[0-9a-fA-F]{6}$/.test(color)) return;
    document.documentElement.style.setProperty('--accent', color);
    const r = parseInt(color.slice(1, 3), 16);
    const g = parseInt(color.slice(3, 5), 16);
    const b = parseInt(color.slice(5, 7), 16);
    document.documentElement.style.setProperty('--accent-glow', `rgba(${r}, ${g}, ${b}, 0.3)`);
    document.documentElement.style.setProperty('--accent-hover', `rgba(${r}, ${g}, ${b}, 0.85)`);
    document.documentElement.style.setProperty('--accent-rgb', `${r}, ${g}, ${b}`);
}
