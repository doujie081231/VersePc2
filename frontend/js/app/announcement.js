const SUPPORT_MILESTONES = [1, 3, 5, 10, 20, 50, 100, 200, 300, 400, 500, 600, 700, 800, 900, 1000, 1500, 2000, 3000, 5000, 10000];

function getLaunchCount() {
    try { return parseInt(localStorage.getItem('verse_launchCount') || '0', 10); }
    catch (e) { return 0; }
}

var _launchCounted = false;

function incrementLaunchCount() {
    if (_launchCounted) return getLaunchCount();
    _launchCounted = true;
    var c = getLaunchCount() + 1;
    try { localStorage.setItem('verse_launchCount', String(c)); } catch (e) {}
    return c;
}

function isSupportMilestone(c) { return SUPPORT_MILESTONES.indexOf(c) !== -1; }

function checkSupportMilestone() {
    var c = getLaunchCount();
    showSupportModal(c);
}

function showSupportModal(count) {
    count = count || getLaunchCount();
    setTimeout(function() {
        var countEl = document.getElementById('support-modal-count');
        var modalEl = document.getElementById('support-modal');
        if (countEl) countEl.textContent = count;
        if (modalEl) {
            modalEl.style.display = '';
            modalEl.classList.add('modal-visible');
        }
    }, 800);
}

function openSupportPage() {
    window.open('https://ifdian.net/a/versejava?tab=home', '_blank');
    dismissSupportModal();
}

function dismissSupportModal() {
    const modal = document.getElementById('support-modal');
    if (modal) {
        modal.classList.remove('modal-visible');
        modal.style.display = 'none';
    }
}

var ANNOUNCEMENT_CONTENT = {
    version: '1.0.1',
    title: 'VersePC2 v1.0.1 预览版公告',
    body: `
        <div class="announcement-section">
            <p style="font-size:16px;line-height:1.8;">亲爱的小伙伴们，VersePC2 启动器正式开放<strong>预览版</strong>啦！</p>
            <p style="color:#e67e22;font-weight:bold;font-size:15px;margin-top:12px;">预览版可能存在较多 Bug，如果你比较在意稳定性，建议等待八月份发布的正式稳定版。</p>
        </div>

        <div class="announcement-section">
            <h4>功能介绍</h4>
            <ul>
                <li><strong>版本管理</strong> — 一键下载和管理多个 Minecraft 版本，支持原版、Forge、Fabric、NeoForge 等主流加载器</li>
                <li><strong>资源整合</strong> — 内置模组、整合包、资源包、数据包、光影下载，轻松打造你的专属游戏体验</li>
                <li><strong>联机功能</strong> — 支持局域网联机和端口映射，和朋友一起畅玩</li>
                <li><strong>账户管理</strong> — 支持微软正版登录和离线模式</li>
                <li><strong>Java 管理</strong> — 自动检测和管理 Java 环境，告别版本不兼容的烦恼</li>
                <li><strong>工具箱</strong> — 皮肤查看、游戏修复等实用工具</li>
                <li><strong>控制台</strong> — 实时查看游戏日志，方便排查问题</li>
            </ul>
        </div>

        <div class="announcement-section">
            <h4>实验性功能</h4>
            <p>实验性功能（AI 助手等）目前需要<strong>赞助后获取测试版</strong>才能解锁使用。</p>
            <p>如果你对实验性功能感兴趣，欢迎前往 <a href="javascript:void(0)" onclick="window.electronAPI?.openExternal('https://afdian.com/a/versejava')" style="color:#9b59b6;font-weight:bold;">爱发电</a> 支持我们的发展</p>
        </div>

        <div class="announcement-section">
            <h4>Bug 反馈</h4>
            <p>如果你在使用过程中遇到任何问题，欢迎加入我们的腾讯频道反馈：</p>
            <p style="font-size:15px;font-weight:bold;color:#4a9eff;margin-top:8px;">腾讯频道：verselauncher1</p>
        </div>

        <div class="announcement-footer">
            <p style="text-align:right;margin-top:20px;font-weight:bold;">豆杰<br>2026 年 6 月</p>
        </div>
    `
};

async function showAnnouncementModal(forceShow) {
    try {
        var versionResult = await window.electronAPI.updater.getVersion();
        var currentVersion = versionResult ? versionResult.version : '1.0.0';
    } catch (e) {
        var currentVersion = '1.0.0';
    }

    if (!forceShow) {
        try {
            var dismissedVersion = localStorage.getItem('versepc_announcement_dismissed_version');
            if (dismissedVersion === currentVersion) return;
        } catch (e) {}
    }

    var noticeMode = 'show-all';
    try {
        var saved = await window.electronAPI.store.get('versepc_other_settings');
        if (saved) {
            var settings = JSON.parse(saved);
            if (settings.launcherNoticeMode) noticeMode = settings.launcherNoticeMode;
        }
    } catch (e) {}

    if (!forceShow && noticeMode === 'hide') return;

    var versionBadge = document.getElementById('announcement-version-badge');
    var contentEl = document.getElementById('announcement-content');
    var checkEl = document.getElementById('announcement-dismiss-check');

    if (versionBadge) versionBadge.textContent = 'v' + currentVersion;
    if (contentEl) contentEl.innerHTML = ANNOUNCEMENT_CONTENT.body;
    if (checkEl) checkEl.checked = false;

    var modal = document.getElementById('announcement-modal');
    if (!modal) return;

    if (modal.parentElement !== document.body) {
        document.body.appendChild(modal);
    }

    modal.style.display = 'flex';
    requestAnimationFrame(function () {
        modal.classList.add('modal-visible');
        modal.classList.remove('modal-exiting');
    });

    requestAnimationFrame(function () {
        var closeBtn = modal.querySelector('.modal-close');
        if (closeBtn) closeBtn.focus();
    });

    var onKeyDown = function (e) {
        if (e.key === 'Escape') {
            dismissAnnouncementModal();
        }
    };
    modal.addEventListener('keydown', onKeyDown);
    modal._escCleanup = function () { modal.removeEventListener('keydown', onKeyDown); };
}

function dismissAnnouncementModal() {
    var modal = document.getElementById('announcement-modal');
    if (!modal) return;

    var checkEl = document.getElementById('announcement-dismiss-check');
    if (checkEl && checkEl.checked) {
        try {
            var versionBadge = document.getElementById('announcement-version-badge');
            var version = versionBadge ? versionBadge.textContent : '';
            if (version) localStorage.setItem('versepc_announcement_dismissed_version', version.replace(/^v/, ''));
        } catch (e) {}
    }

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
}

function announcementGotoUpdateSettings() {
    dismissAnnouncementModal();
    if (typeof navigateToPage === 'function') {
        navigateToPage('settings-other');
    } else if (typeof switchPage === 'function') {
        switchPage('settings-other');
    }
    setTimeout(function() {
        var btnGroup = document.getElementById('updater-btn-group');
        if (btnGroup) {
            var card = btnGroup.closest('.card');
            if (card) card.scrollIntoView({ behavior: 'smooth', block: 'center' });
        }
    }, 300);
}

async function checkAnnouncementPopup() {
    await showAnnouncementModal(false);
}

async function showUpdateAnnouncement() {
    await showAnnouncementModal(true);
}
