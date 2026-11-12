/**
 * home-widgets.js — 主页可拖动控件注册表与布局持久化
 *
 * 从 page-home.js 抽出的纯逻辑层（不依赖 Vue），负责：
 *   1. 控件清单（覆盖软件全部功能页面）与默认位置
 *   2. 布局读写（localStorage v2）+ v1 旧数据兼容
 *   3. 「已添加控件」集合（visible），编辑布局时通过加号面板增删
 *   4. 待处理下载请求的进程内广播（下载控件、工具箱卡片共用）
 *
 * 依赖方向：features/home → core（单向，符合架构约束）
 */

import { apiGet, apiPost } from '../../core/api.js';

const LAYOUT_KEY = 'versepc_home_layout_v2';
const LEGACY_LAYOUT_KEY = 'versepc_home_layout_v1';

/**
 * 控件注册表 —— 覆盖软件上的全部功能。
 *
 * id     : 布局键名 + DOM data-widget
 * name   : 加号面板里显示的名字
 * desc   : 一句话说明
 * icon   : static-icons.js 的 symbol id
 * page   : 对应 index.html 的 data-page（点击控件主体跳转）
 * core   : 主页核心控件，默认存在、不可移除
 * action : 非 core 控件渲染方式
 *          'nav'      → 小卡片，点击跳页面
 *          'download' → 下载卡（特殊交互，走 home-download-widget）
 */
export const WIDGETS = {
    // ── 核心控件（默认布局，不可移除）──
    title:   { id: 'title',   name: '软件名称', desc: 'VersePC2 标题',           icon: null,          page: 'home',            core: true, removable: false },
    avatar:  { id: 'avatar',  name: '账号头像', desc: '当前账号与皮肤',           icon: 'icon-user',   page: 'accounts',        core: true, removable: false },
    version: { id: 'version', name: '版本卡片', desc: '当前版本与切换入口',       icon: null,          page: 'versions',        core: true, removable: false },
    launch:  { id: 'launch',  name: '启动按钮', desc: '一键启动游戏',             icon: null,          page: 'home',            core: true, removable: false },
    running: { id: 'running', name: '启动任务', desc: '运行中的游戏实例（有实例时才显示）', icon: null, page: 'home',            core: true, removable: false },

    // ── 可添加控件（覆盖侧边栏全部功能）──
    download:      { id: 'download',      name: '下载自定义文件', desc: '浏览器接管 / 任意链接高速下载', icon: 'icon-download',    page: 'toolbox',        core: false, removable: true, action: 'download' },
    versions:      { id: 'versions',      name: '版本下载',       desc: '原版 / Forge / Fabric 安装',   icon: 'icon-package',     page: 'versions',       core: false, removable: true },
    mods:          { id: 'mods',          name: '模组',           desc: 'Modrinth / CurseForge 搜索',   icon: 'icon-package',     page: 'mods',           core: false, removable: true },
    modpacks:      { id: 'modpacks',      name: '整合包',         desc: '整合包浏览与一键导入',         icon: 'icon-package',     page: 'modpacks',       core: false, removable: true },
    datapacks:     { id: 'datapacks',     name: '数据包',         desc: '世界数据包管理',               icon: 'icon-package',     page: 'datapacks',      core: false, removable: true },
    resourcepacks: { id: 'resourcepacks', name: '材质包',         desc: '材质包浏览与安装',             icon: 'icon-package',     page: 'resourcepacks',  core: false, removable: true },
    shaders:       { id: 'shaders',       name: '光影包',         desc: '光影包浏览与安装',             icon: 'icon-package',     page: 'shaders',        core: false, removable: true },
    accounts:      { id: 'accounts',      name: '账户',           desc: '离线 / 微软 / 外置登录',       icon: 'icon-user',        page: 'accounts',       core: false, removable: true },
    toolbox:       { id: 'toolbox',       name: '工具箱',         desc: '常用网站与实用工具',           icon: 'icon-toolbox',     page: 'toolbox',        core: false, removable: true },
    java:          { id: 'java',          name: 'Java 管理',      desc: '检测 / 安装 / 切换 Java',      icon: 'icon-java',        page: 'java',           core: false, removable: true },
    plugins:       { id: 'plugins',       name: '插件',           desc: '插件市场与已装插件',           icon: 'icon-plugins',     page: 'plugins',        core: false, removable: true },
    cloud:         { id: 'cloud',         name: 'Verse 云盘',     desc: '存档同步与文件中转',           icon: 'icon-cloud',       page: 'cloud',          core: false, removable: true },
    'lan-terracotta': { id: 'lan-terracotta', name: '陶瓦联机',   desc: '内网穿透，无需公网 IP',        icon: 'icon-globe',       page: 'lan-terracotta', core: false, removable: true },
    'lan-portmap':    { id: 'lan-portmap',    name: '红石联机',   desc: '红石联机内网穿透',             icon: 'icon-globe',       page: 'lan-portmap',    core: false, removable: true },
    'lan-enderlink':  { id: 'lan-enderlink',  name: 'EnderLink',  desc: 'EnderLink 联机',               icon: 'icon-globe',       page: 'lan-enderlink',  core: false, removable: true },
    'private-server': { id: 'private-server', name: '一起玩',     desc: '私人服务器列表',               icon: 'icon-server',      page: 'private-server', core: false, removable: true },
    'server-host':    { id: 'server-host',    name: '本地开服',   desc: '一键创建并管理服务端',         icon: 'icon-server-host', page: 'server-host',    core: false, removable: true },
    feedback:      { id: 'feedback',      name: '问题反馈',       desc: '提交反馈与日志',               icon: 'icon-feedback',    page: 'feedback',       core: false, removable: true },
    settings:      { id: 'settings',      name: '设置',           desc: '启动 / 个性化 / 其他',         icon: 'icon-settings',    page: 'settings-launch', core: false, removable: true },
};

/** 可在加号面板添加的非核心控件 */
export const OPTIONAL_WIDGETS = Object.values(WIDGETS).filter((w) => !w.core);

/** 核心控件 id 列表 */
export const CORE_WIDGETS = Object.values(WIDGETS).filter((w) => w.core).map((w) => w.id);

/** 所有已注册控件的 id（顺序即渲染顺序） */
export const ALL_WIDGET_IDS = Object.keys(WIDGETS);

// ─────────────────────────────────────────────────────────────
// 布局读写
// ─────────────────────────────────────────────────────────────


// ─────────────────────────────────────────────────────────────
// 后端持久化（settings.json）
//
// localStorage 在 WebView2 被强杀时会丢数据（"编辑布局→确认→重启又变默认"）。
// 所以布局同时写一份到后端 settings.json，启动时以后端为准。
// ─────────────────────────────────────────────────────────────

let _backendLayout = null;   // 启动时从后端拉到的布局
let _backendSynced = false;  // 是否已完成首次拉取

/** 启动时调一次：把后端保存的布局拉进 localStorage（以后端为准） */
export async function hydrateHomeLayoutFromBackend() {
    try {
        const res = await apiGet('/api/home-layout');
        const layout = res && res.layout;
        if (layout && typeof layout === 'object') {
            _backendLayout = normalizeLayout(layout);
            // 后端有就用后端覆盖本地，保证"以最后一次确认为准"
            try { localStorage.setItem(LAYOUT_KEY, JSON.stringify(_backendLayout)); } catch (e) {}
        } else {
            // 后端没有：把本地已有的上传到后端，避免丢失
            const local = loadHomeLayout();
            if (Object.keys(local.visible || {}).length) saveHomeLayout(local);
        }
        _backendSynced = true;
    } catch (e) {
        _backendSynced = true;
    }
}

/** 后台把布局写入 settings.json（不 await，失败不影响交互） */
function pushLayoutToBackend(norm) {
    try {
        apiPost('/api/home-layout', norm).catch(() => {});
    } catch (e) {}
}

/**
 * 读取布局：{ title:{x,y}, avatar:{x,y,s}, ..., visible:{ download:true, ... } }
 * 旧版（v1）没有 visible 字段，迁移时按「未添加」处理。
 */
export function loadHomeLayout() {
    try {
        const raw = localStorage.getItem(LAYOUT_KEY);
        if (raw) {
            const layout = normalizeLayout(JSON.parse(raw));
            // 已添加但缺坐标的控件 → 补坐标并回写，避免叠在一起
            if (healMissingPositions(layout)) {
                try { localStorage.setItem(LAYOUT_KEY, JSON.stringify(layout)); } catch (e) {}
            }
            return layout;
        }
        const legacyRaw = localStorage.getItem(LEGACY_LAYOUT_KEY);
        if (legacyRaw) {
            const migrated = normalizeLayout(JSON.parse(legacyRaw));
            saveHomeLayout(migrated);
            return migrated;
        }
        return normalizeLayout({});
    } catch (e) {
        return normalizeLayout({});
    }
}

/**
 * 自愈：给「已添加但没有坐标」的可选控件补一个不重叠的网格坐标。
 * 历史 bug：toggleWidget 添加控件时不写坐标，applyLayout 又回退到 CSS 默认，
 * 导致所有导航卡叠在一起（用户以为"布局变回默认了"）。
 */
function healMissingPositions(layout) {
    const COL_W = 110, GAP = 14, ROW_H = 64, START_X = 12, START_Y = 12;
    const cols = 6;
    // 已占用的格点
    const taken = new Set();
    const mark = (pos) => {
        if (!pos || !Number.isFinite(pos.x) || !Number.isFinite(pos.y)) return;
        taken.add(Math.round(pos.x / (COL_W + GAP)) + ',' + Math.round(pos.y / (ROW_H + GAP)));
    };
    for (const id of ALL_WIDGET_IDS) {
        if (WIDGETS[id].core) mark(layout[id]);
    }
    let changed = false;
    for (const id of ALL_WIDGET_IDS) {
        if (WIDGETS[id].core) continue;
        if (!layout.visible[id]) continue;
        if (layout[id]) { mark(layout[id]); continue; }
        // 找空格
        let placed = false;
        for (let r = 8; r >= 0 && !placed; r--) {
            for (let c = 0; c < cols && !placed; c++) {
                const key = c + ',' + r;
                if (taken.has(key)) continue;
                layout[id] = { x: c * (COL_W + GAP) + START_X, y: r * (ROW_H + GAP) + START_Y };
                taken.add(key);
                placed = true;
                changed = true;
            }
        }
    }
    return changed;
}

export function saveHomeLayout(layout) {
    try {
        const norm = normalizeLayout(layout);
        const json = JSON.stringify(norm);
        localStorage.setItem(LAYOUT_KEY, json);
        pushLayoutToBackend(norm);
    } catch (e) {
        console.error('[home-widgets] 保存布局失败:', e);
    }
}

function normalizeLayout(input) {
    const src = input && typeof input === 'object' ? input : {};
    const out = {};
    for (const id of ALL_WIDGET_IDS) {
        const pos = src[id];
        if (pos && typeof pos === 'object') {
            const entry = {};
            if (Number.isFinite(pos.x)) entry.x = pos.x;
            if (Number.isFinite(pos.y)) entry.y = pos.y;
            if (id === 'avatar' && Number.isFinite(pos.s)) entry.s = pos.s;
            if (Object.keys(entry).length) out[id] = entry;
        }
    }
    const visible = {};
    const srcVisible = src.visible && typeof src.visible === 'object' ? src.visible : {};
    for (const id of ALL_WIDGET_IDS) {
        if (WIDGETS[id].core) continue;
        if (srcVisible[id] === true) visible[id] = true;
    }
    out.visible = visible;
    return out;
}

// ─────────────────────────────────────────────────────────────
// 控件可见性
// ─────────────────────────────────────────────────────────────

/** 该控件当前是否显示在主页 */
export function isWidgetVisible(layout, id) {
    const w = WIDGETS[id];
    if (!w) return false;
    if (w.core) return true;
    return !!(layout && layout.visible && layout.visible[id]);
}

/** 把控件标记为已添加（写盘），返回 true 表示本次是新添加的 */
export function ensureWidgetVisible(id) {
    const w = WIDGETS[id];
    if (!w || w.core) return false;
    const layout = loadHomeLayout();
    if (layout.visible[id]) return false;
    layout.visible[id] = true;
    saveHomeLayout(layout);
    notifyLayoutChange({ type: 'add', id });
    return true;
}

/** 从主页移除控件（核心控件不可移除） */
export function removeWidget(id) {
    const w = WIDGETS[id];
    if (!w || w.core) return false;
    const layout = loadHomeLayout();
    if (!layout.visible[id]) return false;
    delete layout.visible[id];
    saveHomeLayout(layout);
    notifyLayoutChange({ type: 'remove', id });
    return true;
}

/** 当前已添加的可选控件 id 列表 */
export function listVisibleWidgets() {
    const layout = loadHomeLayout();
    return OPTIONAL_WIDGETS.filter((w) => layout.visible[w.id]).map((w) => w.id);
}

/**
 * 重置布局：只清空坐标，保留「已添加」集合。
 * 个性化页「恢复主页默认布局」调用这里。
 */
export function resetWidgetPositions() {
    const layout = loadHomeLayout();
    saveHomeLayout({ visible: { ...layout.visible } });
    notifyLayoutChange({ type: 'reset' });
    return loadHomeLayout();
}

// ─────────────────────────────────────────────────────────────
// 变更广播
// ─────────────────────────────────────────────────────────────

const LAYOUT_EVENT = 'versepc-home-widgets-change';

function notifyLayoutChange(detail) {
    try {
        window.dispatchEvent(new CustomEvent(LAYOUT_EVENT, { detail }));
    } catch (e) {}
}

/** 订阅布局变化，返回取消函数 */
export function onLayoutChange(fn) {
    if (typeof fn !== 'function') return () => {};
    const handler = (e) => fn(e && e.detail ? e.detail : {});
    window.addEventListener(LAYOUT_EVENT, handler);
    return () => window.removeEventListener(LAYOUT_EVENT, handler);
}

// ─────────────────────────────────────────────────────────────
// 待处理下载请求广播（进程内）
// ─────────────────────────────────────────────────────────────

const REQUEST_EVENT = 'versepc-download-request';
let _pendingRequest = null;

export function onPendingDownloadRequest(req) {
    _pendingRequest = req || null;
    try {
        window.dispatchEvent(new CustomEvent(REQUEST_EVENT, { detail: req }));
    } catch (e) {}
}

export function getPendingDownloadRequest() {
    return _pendingRequest;
}

export function subscribeDownloadRequest(fn) {
    if (typeof fn !== 'function') return () => {};
    const handler = (e) => fn(e && e.detail ? e.detail : null);
    window.addEventListener(REQUEST_EVENT, handler);
    return () => window.removeEventListener(REQUEST_EVENT, handler);
}
