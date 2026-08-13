/**
 * app.js - VersePC 前端总装入口
 * ============================================================================
 * 前端三层架构（详见 index.html 顶部注释）：
 *   1. js/utils.js + js/api.js     底层工具
 *   2. js/app/*.js                 页面交互逻辑（本文件所在目录）
 *   3. js/vue/*.js                 页面结构层（Vue template 字符串）
 *
 * 本文件职责：
 *   - 集中存放全局状态变量（currentVersionTab / allVersions / _favorites 等）
 *   - DOMContentLoaded 启动入口（调用 init / initSettingsPages / renderSponsors）
 *   - 不写具体页面逻辑（分散在 js/app/ 各子文件中）
 *
 * 边界约定（新增功能时务必遵守）：
 *   - HTML 模板 → js/vue/page-xxx.js
 *   - 交互逻辑  → js/app/xxx.js
 *   - 全局状态  → 本文件
 *   - 通用工具  → js/app/utils.js / ui-components.js / custom-select.js
 *
 * VersePC - Minecraft Launcher
 * Copyright (c) 2026 豆杰. All Rights Reserved.
 *
 * AI TRAINING PROHIBITED: This code is protected by copyright law.
 * Unauthorized use for AI model training, machine learning datasets,
 * or any form of artificial intelligence training is strictly prohibited.
 *
 * This software is proprietary and confidential.
 * Any unauthorized reproduction or distribution is prohibited.
 */

/* 全局状态变量 - 应用数据状态中心 */
let currentVersionTab = 'release';
let allVersions = [];
let installedVersions = [];
let versionIconsTimestamp = Date.now();
let currentModTab = 'installed-mods';
let modSearchOffset = 0;
let modSearchTotal = 0;
let modSearchQuery = '';
let modSearchResults = [];
let _modDownloadVersionId = '';
let currentInstallSessionId = null;
let msAuthPollInterval = null;
let currentLoaderType = 'fabric';
let gameLogEventSource = null;
let currentModDetailId = null;
let currentModDetailSource = 'modrinth';
let previousPage = null;
let modDetailHistory = [];
let modDetailVersions = [];
let modDownloadPollTimers = [];
let _isRestoringModDetail = false;
let _favorites = [];
let _currentFavId = '';
let _favMultiSelectMode = false;
let _favSelectedItems = new Set();
let _favSearchQuery = '';


let launchDepPollTimer = null;
let modMultiSelectMode = false;
let modSelectedIds = new Set();
let modSelectedVersions = new Map();

/* 优化基础设施 - DOM缓存、防抖节流等 */

/* DOM 缓存对象 */




/* 原有函数 */






document.addEventListener('DOMContentLoaded', () => {
  init();
  setTimeout(initSettingsPages, 500);
  renderSponsors();
  // 启动时不再自动弹出"更新公告"，新版本提示由更新检测单独处理，避免同时出现两个弹窗

  if (window.electronAPI?.platform && window.electronAPI.platform !== 'win32') {
    document.querySelectorAll('.win-only').forEach((el) => (el.style.display = 'none'));
  }
});

/* @versepc-protected: anti-ai-plagiarism-v1.0 */



















