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
                return '<div class="mod-item">' +
                    '<div class="mod-icon"><img src="' + escapeHtml(mod.icon || '') + '" alt="" loading="lazy" onerror="this.style.display=\'none\';this.parentElement.classList.add(\'mod-icon--fallback\')"></div>' +
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
                        '<button class="btn btn-sm ' + (mod.enabled ? 'btn-secondary' : 'btn-primary') + '" onclick="toggleMod(\'' + escapeOnclick(mod.fileName || mod.id) + '\', ' + (!mod.enabled) + ')">' + (mod.enabled ? '禁用' : '启用') + '</button>' +
                        '<button class="btn btn-danger btn-sm" onclick="deleteMod(\'' + escapeOnclick(mod.fileName || mod.id) + '\')">删除</button>' +
                    '</div>' +
                '</div>';
            }).join('');
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
        ...allVersions.filter(v => v.type === 'release').slice(0, 30).map(v => ({
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

function translateChineseModQuery(query) {
    if (!query || !/[\u4e00-\u9fff]/.test(query)) return query;
    const q = query.toLowerCase().trim();
    const matches = [];
    for (const [slug, chineseName] of Object.entries(MOD_CHINESE_NAMES)) {
        const parts = chineseName.split(/[·（(]/);
        const mainName = (parts[0] || '').trim();
        if (mainName.includes(q) || chineseName.includes(q)) {
            matches.push({ slug, weight: mainName === q ? 100 : mainName.startsWith(q) ? 50 : chineseName.includes(q) ? 20 : 0 });
        }
    }
    if (matches.length > 0) {
        matches.sort((a, b) => b.weight - a.weight);
        const slugs = [...new Set(matches.filter(m => m.weight >= 20).map(m => m.slug))];
        if (slugs.length > 0) return slugs.slice(0, 5).join(' ');
    }
    return query;
}

async function loadMods() {
    const container = document.getElementById('mod-browse-list');
    container.innerHTML = '<div class="loading-spinner"><div class="spinner"></div><p>加载中...</p></div>';

    const title = document.getElementById('mod-browse-title');
    title.textContent = modSearchQuery ? `搜索 "${modSearchQuery}" 的结果` : '热门模组';

    const loader = getCustomSelectValue('mod-filter-loader');
    const version = getCustomSelectValue('mod-filter-version');
    const category = getCustomSelectValue('mod-filter-category');
    const sort = getCustomSelectValue('mod-filter-sort');
    const sourceFilter = getCustomSelectValue('mod-filter-source') || 'any';

    const translatedQuery = translateChineseModQuery(modSearchQuery);

    try {
        const data = await API.searchMods(translatedQuery, sourceFilter, loader, version, category, sort, 15, modSearchOffset);
        const hits = data.hits || [];
        modSearchTotal = data.total || 0;
        modSearchResults = hits;
        hits.forEach(function(h) { _projectDataCache.set(h.id, h); });

        if (hits.length === 0) {
            container.innerHTML = '<p class="empty-text">未找到模组</p>';
        } else {
            container.innerHTML = hits.map(function (mod) {
                var isSelected = modSelectedIds.has(mod.id);
                var isFav = _favorites.some(function(f) { return f.favs.includes(mod.id); });
                return '<div class="mod-item mod-item-clickable' + (modMultiSelectMode ? ' mod-multiselect-active' : '') + '" onclick="openModDetail(\'' + mod.id + '\', \'' + mod.source + '\')" onmouseenter="preloadModVersions(\'' + mod.id + '\', \'' + mod.source + '\')">' +
                    (modMultiSelectMode ? '<div class="mod-checkbox' + (isSelected ? ' checked' : '') + '" data-mod-id="' + mod.id + '" onclick="event.stopPropagation();toggleModSelect(\'' + mod.id + '\')"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3"><polyline points="20 6 9 17 4 12"/></svg></div>' : '') +
                    '<div class="mod-icon"><img src="' + escapeHtml(mod.icon || '') + '" alt="" loading="lazy" onerror="this.style.display=\'none\';this.parentElement.classList.add(\'mod-icon--fallback\')"></div>' +
                    '<div class="mod-info">' +
                        '<div class="mod-name">' + escapeHtml(formatModNameWithChinese(mod.slug || mod.id, mod.title)) +
                            (sourceFilter === 'any' ? ' <span style="font-size:10px;padding:1px 5px;border-radius:3px;background:' + (mod.source === 'curseforge' ? '#f1643620;color:#f16436;border:1px solid #f1643630' : '#4caf5020;color:#4caf50;border:1px solid #4caf5030') + ';font-weight:500;vertical-align:middle">' + (mod.source === 'curseforge' ? 'CF' : 'MR') + '</span>' : '') +
                        '</div>' +
                        '<div class="mod-desc">' + escapeHtml(mod.description) + '</div>' +
                        '<div class="mod-meta">' +
                            '<span>\u2B07 ' + formatNumber(mod.downloads) + '</span>' +
                            '<span>\u2764 ' + escapeHtml(mod.author) + '</span>' +
                            '<span>' + (mod.categories || []).slice(0, 3).join(', ') + '</span>' +
                        '</div>' +
                    '</div>' +
                    '<div class="mod-actions" onclick="event.stopPropagation()">' +
                        '<button class="fav-heart-btn' + (isFav ? ' active' : '') + '" data-project-id="' + escapeHtml(mod.id) + '" onclick="event.stopPropagation(); showFavSelectDropdown(\'' + escapeOnclick(mod.id) + '\', this)"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M20.84 4.61a5.5 5.5 0 00-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 00-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 000-7.78z"/></svg></button>' +
                        '<button class="btn btn-primary btn-sm" onclick="event.stopPropagation();openModDetail(\'' + mod.id + '\', \'' + mod.source + '\')">安装</button>' +
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
    const currentPage = Math.floor(modSearchOffset / 15) + 1;
    const totalPages = Math.max(1, Math.ceil(modSearchTotal / 15));

    pagination.style.display = 'flex';
    document.getElementById('mod-page-info').textContent = `${currentPage}/${totalPages}`;
    document.getElementById('mod-prev-btn').disabled = modSearchOffset <= 0;
    document.getElementById('mod-next-btn').disabled = modSearchOffset + 15 >= modSearchTotal;
}

async function loadFeaturedMods() {
    modSearchQuery = '';
    modSearchOffset = 0;
    await loadMods();
}

async function searchMods() {
    modSearchOffset = 0;
    await loadMods();
}
