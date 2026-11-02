import { API } from '../../core/api.js';
import { escapeHtml, normalizeIconUrl } from '../../core/utils.js';
import { getCustomSelectValue, setCustomSelectValue, updateCustomSelectOptions } from '../misc/custom-select.js';
import { formatNumber } from '../misc/ui-components.js';
import { _projectDataCache } from './mod-detail.js';

import { appState } from '../../core/app-state.js';
import { formatModNameWithChinese } from './chinese-names.js';
import { isPinned, PIN_ICONS, pinBtnToggle } from '../versions/pinned-store.js';
import { actionAttrs, actionAttrsStop, registerActions } from '../../core/actions.js';
import { preloadModVersions } from './mod-detail.js';
import { registerLazyLoader } from '../../core/lazy-loaders.js';

// 搜索结果卡片悬停时预加载版本列表
document.addEventListener('mouseover', (event) => {
    const el = event.target instanceof Element ? event.target.closest('[data-preload-mod]') : null;
    if (!el) return;
    preloadModVersions(el.getAttribute('data-preload-mod'), el.getAttribute('data-preload-source') || 'modrinth');
});

async function loadInstalledMods() {
    try {
        const result = await API.getInstalledMods();
        const mods = Array.isArray(result) ? result : (result.mods || []);
        const warnings = Array.isArray(result) ? [] : (result.warnings || []);
        const container = document.getElementById('installed-mods-list');
        if (!container) return;
        if (mods.length === 0) {
            container.innerHTML = '<p class="empty-text">暂无已安装的模组</p>';
        } else {
            let warningHtml = '';
            if (warnings.length > 0) {
                warningHtml = warnings.map(w =>
                    `<div class="mod-warning ${w.type === 'conflict' ? 'warning-conflict' : 'warning-duplicate'}">
                        <span class="warning-icon">${w.type === 'conflict' ? '⚠️' : '🔄'}</span>
                        <span>${escapeHtml(w.message)}</span>
                    </div>`
                ).join('');
            }
            container.innerHTML = warningHtml + mods.map(function (mod) {
                const icon = normalizeIconUrl(mod.icon || '');
                const isApiIcon = icon.indexOf('/api/') === 0;
                const hash = isApiIcon && icon.match(/hash=([^&]+)/) ? decodeURIComponent(icon.match(/hash=([^&]+)/)[1]) : '';
                let iconHtml;
                if (icon && !isApiIcon) {
                    iconHtml = '<div class="mod-icon"><img src="' + escapeHtml(icon) + '" alt="" loading="lazy" data-img-fallback="modicon"></div>';
                } else {
                    iconHtml = '<div class="mod-icon mod-icon--fallback"' + (hash ? ' data-sh="' + escapeHtml(hash) + '"' : '') + '></div>';
                }
                return '<div class="mod-item">' + iconHtml +
                    '<div class="mod-info">' +
                        '<div class="mod-name">' + escapeHtml(formatModNameWithChinese(mod.slug || mod.id || mod.fileName, mod.name)) + '</div>' +
                        '<div class="mod-desc">' + escapeHtml(mod.description) + '</div>' +
                        '<div class="mod-meta">' +
                            '<span>' + mod.size + '</span>' +
                            '<span>' + (mod.enabled ? '已启用' : '已禁用') + '</span>' +
                            (mod.author ? '<span>' + escapeHtml(mod.author) + '</span>' : '') +
                            (mod.version && mod.version !== '1.0' ? '<span>v' + escapeHtml(mod.version) + '</span>' : '') +
                        '</div>' +
                    '</div>' +
                    '<div class="mod-actions">' +
                        '<button class="btn btn-sm ' + (mod.enabled ? 'btn-secondary' : 'btn-primary') + '"' + actionAttrs('toggleMod', mod.fileName || mod.id, !mod.enabled) + '>' + (mod.enabled ? '禁用' : '启用') + '</button>' +
                        '<button class="btn btn-danger btn-sm"' + actionAttrs('deleteMod', mod.fileName || mod.id) + '>删除</button>' +
                    '</div>' +
                '</div>';
            }).join('');
            container.querySelectorAll('.mod-icon[data-sh]').forEach(function (el) {
                API.getModIcon(el.getAttribute('data-sh')).then(function (r) {
                    console.warn('[modinst-icon] ok', el.getAttribute('data-sh'), (r && r.dataUrl) ? ('len=' + r.dataUrl.length) : JSON.stringify(r));
                    if (r && r.dataUrl) {
                        el.classList.remove('mod-icon--fallback');
                        el.removeAttribute('data-sh');
                        el.innerHTML = '<img src="' + r.dataUrl + '" alt="" style="width:100%;height:100%;object-fit:cover;">';
                    }
                }).catch(function (e) { console.warn('[modinst-icon] err', el.getAttribute('data-sh'), String(e && e.message || e)); });
            });
        }
        document.getElementById('stat-mods').textContent = mods.length;
    } catch (e) { console.error('[Mods] Failed to load installed mods:', e); }
}

const MODRINTH_CATEGORY_ZH = {
    'adventure': '冒险', 'cursed': '诅咒', 'decoration': '装饰', 'equipment': '装备',
    'food': '食物', 'library': '前置库', 'magic': '魔法', 'optimization': '优化',
    'storage': '存储', 'technology': '科技', 'transportation': '交通', 'utility': '实用',
    'world-gen': '世界生成', 'game-mechanics': '游戏机制', 'social': '社交',
    'automation': '自动化', 'biomes': '生物群系', 'blocks': '方块', 'bosses': 'Boss',
    'building': '建筑', 'chat': '聊天', 'combat': '战斗', 'dimensions': '维度',
    'economy': '经济', 'entities': '实体', 'environment': '环境', 'farming': '农业',
    'hud': 'HUD', 'items': '物品', 'management': '管理', 'map': '地图',
    'minigame': '小游戏', 'mobs': '生物', 'modded': '模组化', 'models': '模型',
    'multimedia': '多媒体', 'performance': '性能', 'quests': '任务', 'redstone': '红石',
    'resource-pack': '资源包', 'server': '服务器', 'skin': '皮肤', 'sound': '声音',
    'structures': '结构', 'tweaks': '调整', 'vanilla-like': '原版风格',
    '8x-': '8x-', '16x': '16x', '32x': '32x', '64x': '64x', '128x': '128x',
    '256x': '256x', '512x+': '512x+', 'animation': '动画', 'core-shaders': '核心着色器',
    'compatibility': '兼容性', 'cartoon': '卡通', 'fantasy': '奇幻', 'medieval': '中世纪',
    'modern': '现代', 'photo-realistic': '写实', 'semi-realistic': '半写实',
    'simplistic': '简约', 'traditional': '传统', 'pbr': 'PBR', 'colored-lighting': '彩色光照',
    'path-tracing': '光线追踪', 'reflections': '反射', 'shadows': '阴影',
    'volumetric-light': '体积光', 'datapack': '数据包'
};

async function loadModFilterOptions() {
    try {
        const data = await API.getModCategories();
        const categories = data.categories || [];
        const options = [
            { value: '', text: '全部' },
            ...categories.map(cat => ({ value: cat.name, text: MODRINTH_CATEGORY_ZH[cat.name] || cat.name }))
        ];
        updateCustomSelectOptions('mod-filter-category', options);
    } catch (e) { console.error('[Mods] Failed to load filter options:', e); }
}

function populateModVersionFilter() {
    const versionOptions = [
        { value: '', text: '全部' },
        ...appState.allVersions.filter(v => v.type === 'release').slice(0, 30).map(v => ({
            value: v.id,
            text: v.id
        }))
    ];

    const currentVal = getCustomSelectValue('mod-filter-version');
    updateCustomSelectOptions('mod-filter-version', versionOptions);
    if (currentVal) {
        setCustomSelectValue('mod-filter-version', currentVal);
    }

    updateCustomSelectOptions('modpack-filter-version', versionOptions);
    updateCustomSelectOptions('datapack-filter-version', versionOptions);
    updateCustomSelectOptions('resourcepack-filter-version', versionOptions);
}

async function loadMods() {
    populateModVersionFilter();
    const container = document.getElementById('mod-browse-list');
    container.innerHTML = '<div class="loading-spinner"><div class="spinner"></div><p>加载中...</p></div>';

    const title = document.getElementById('mod-browse-title');
    title.textContent = appState.modSearchQuery ? `搜索 "${appState.modSearchQuery}" 的结果` : '热门模组';

    const loader = getCustomSelectValue('mod-filter-loader');
    const version = getCustomSelectValue('mod-filter-version');
    const category = getCustomSelectValue('mod-filter-category');
    const sort = getCustomSelectValue('mod-filter-sort');
    const sourceFilter = getCustomSelectValue('mod-filter-source') || 'any';

    const translatedQuery = appState.modSearchQuery;

    try {
        const data = await API.searchMods(translatedQuery, sourceFilter, loader, version, category, sort, 15, appState.modSearchOffset);
        const hits = data.hits || [];
        appState.modSearchTotal = data.total || 0;
        appState.modSearchResults = hits;
        hits.forEach(function(h) { _projectDataCache.set(h.id, h); });

        if (hits.length === 0) {
            container.innerHTML = '<p class="empty-text">未找到模组</p>';
        } else {
            container.innerHTML = hits.map(function (mod) {
                var isSelected = appState.modSelectedIds.has(mod.id);
                var isFav = appState._favorites.some(function(f) { return f.favs.includes(mod.id); });
                var isPin = isPinned('mod', mod.id);
                var pinBtnHtml = '<button class="pin-btn' + (isPin ? ' active' : '') + '" title="' + (isPin ? '取消置顶' : '置顶模组') + '"' + actionAttrsStop('pinMod', mod.id, formatModNameWithChinese(mod.slug || mod.id, mod.title), mod.source || 'modrinth', mod.icon || '') + '>' + PIN_ICONS.pin + '</button>';
                return '<div class="mod-item mod-item-clickable' + (appState.modMultiSelectMode ? ' mod-multiselect-active' : '') + '"' + actionAttrs('openModDetail', mod.id, mod.source) + ' data-preload-mod="' + escapeHtml(mod.id) + '" data-preload-source="' + escapeHtml(mod.source) + '">' +
                    (appState.modMultiSelectMode ? '<div class="mod-checkbox' + (isSelected ? ' checked' : '') + '" data-mod-id="' + mod.id + '"' + actionAttrsStop('toggleModSelect', mod.id) + '><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3"><polyline points="20 6 9 17 4 12"/></svg></div>' : '') +
                    '<div class="mod-icon"><img src="' + escapeHtml(normalizeIconUrl(mod.icon || '')) + '" alt="" loading="lazy" data-img-fallback="modicon"></div>' +
                    '<div class="mod-info">' +
                        '<div class="mod-name">' + escapeHtml(formatModNameWithChinese(mod.slug || mod.id, mod.title)) +
                            (sourceFilter === 'any' ? ' <span style="font-size:10px;padding:1px 5px;border-radius:3px;background:' + (mod.source === 'curseforge' ? 'color-mix(in srgb, var(--orange) 14%, transparent);color:var(--orange);border:1px solid color-mix(in srgb, var(--orange) 24%, transparent)' : 'color-mix(in srgb, var(--green) 14%, transparent);color:var(--green);border:1px solid color-mix(in srgb, var(--green) 24%, transparent)') + ';font-weight:500;vertical-align:middle">' + (mod.source === 'curseforge' ? 'CF' : 'MR') + '</span>' : '') +
                        '</div>' +
                        '<div class="mod-desc">' + escapeHtml(mod.description) + '</div>' +
                        '<div class="mod-meta">' +
                            '<span>\u2B07 ' + formatNumber(mod.downloads) + '</span>' +
                            '<span>\u2764 ' + escapeHtml(mod.author) + '</span>' +
                            '<span>' + (mod.categories || []).slice(0, 3).join(', ') + '</span>' +
                        '</div>' +
                    '</div>' +
                    '<div class="mod-actions" data-stop="1">' +
                        pinBtnHtml +
                        '<button class="fav-heart-btn' + (isFav ? ' active' : '') + '" data-project-id="' + escapeHtml(mod.id) + '"' + actionAttrsStop('showFavSelectDropdown', mod.id) + '><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M20.84 4.61a5.5 5.5 0 00-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 00-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 000-7.78z"/></svg></button>' +
                        '<button class="btn btn-primary btn-sm"' + actionAttrsStop('openModDetail', mod.id, mod.source) + '>安装</button>' +
                    '</div>' +
                '</div>';
            }).join('');
        }

        updateModPagination();
    } catch (e) {
        container.innerHTML = '<p class="empty-text">加载失败</p>';
    }
}

function updateModPagination() {
    const pagination = document.getElementById('mod-pagination');
    const currentPage = Math.floor(appState.modSearchOffset / 15) + 1;
    const totalPages = Math.max(1, Math.ceil(appState.modSearchTotal / 15));

    pagination.style.display = 'flex';
    document.getElementById('mod-page-info').textContent = `${currentPage}/${totalPages}`;
    document.getElementById('mod-prev-btn').disabled = appState.modSearchOffset <= 0;
    document.getElementById('mod-next-btn').disabled = appState.modSearchOffset + 15 >= appState.modSearchTotal;
}

async function loadFeaturedMods() {
    appState.modSearchQuery = '';
    appState.modSearchOffset = 0;
    await loadMods();
}

async function searchMods() {
    appState.modSearchOffset = 0;
    await loadMods();
}

registerActions({
    pinMod: (event, el, id, name, source, icon) => {
        pinBtnToggle(el, 'mod', id, name, { projectId: id, source, icon }, '取消置顶', '置顶模组');
    },
});

// 迁移过渡：保持旧脚本与模板可用的全局名（Phase 5 移除）
export { loadInstalledMods, loadModFilterOptions, populateModVersionFilter, loadMods, updateModPagination, loadFeaturedMods, searchMods, MODRINTH_CATEGORY_ZH };

// 注册筛选刷新入口，供公共下拉组件按需触发
registerLazyLoader('mods:refresh', () => loadMods());
registerLazyLoader('mods:installed', loadInstalledMods);
registerLazyLoader('mods:populate-version-filter', populateModVersionFilter);
