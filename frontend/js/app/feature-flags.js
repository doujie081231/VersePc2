/**
 * feature-flags.js - 功能开关
 * ----------------------------------------------------------------------------
 * 集中管理功能启停：true=开放，false=关闭。
 * 关闭的功能代码全部保留，把对应值改回 true 即可恢复。
 * 注意：后端 src-tauri/src/api/mod.rs 与 download.rs 中的同名开关需同步修改。
 */
window.VERSE_FEATURES = {
    // Minecraft 版本下载/安装（关闭后：进入下载页展示合规提示，后端同时拒绝安装请求；
    // 已安装版本正常启动，模组/整合包等资源下载不受影响）
    versionDownload: true,
    // 联机功能：陶瓦联机 / 红石联机 / EnderLink联机 / 一起玩（关闭后：进入相关页面展示合规提示，
    // 后端同时拒绝 LAN/EasyTier 请求与联机命令）
    lan: true,
};

// 进入被关闭功能页面时，显示统一的合规提示页
window.showLegalNotice = function (pageName) {
    var target = document.getElementById('page-legal-notice');
    if (!target) return;

    var current = document.querySelector('.page.active');
    if (current && current !== target) {
        current.classList.remove('active');
        current.style.animation = '';
    }

    document.querySelectorAll('.nav-btn').forEach(function (b) { b.classList.remove('active'); });
    document.querySelectorAll('.nav-sub-btn').forEach(function (b) { b.classList.remove('active'); });

    var navBtn = document.querySelector('.nav-btn[data-page="' + pageName + '"]') ||
                 document.querySelector('.nav-sub-btn[data-page="' + pageName + '"]');
    if (navBtn) {
        navBtn.classList.add('active');
        var group = navBtn.closest('.nav-submenu-group');
        if (group) group.classList.add('open');
    }

    target.classList.add('active');
    target.scrollTop = 0;
};
