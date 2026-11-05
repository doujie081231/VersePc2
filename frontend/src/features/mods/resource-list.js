// 资源列表：页面初始化、筛选事件绑定、列表加载与竞态保护、版本预加载。
import { API } from '../../core/api.js';
import { escapeHtml, normalizeIconUrl } from '../../core/utils.js';
import { getCustomSelectValue, customSelectInstances } from '../misc/custom-select.js';
import { formatNumber } from '../misc/ui-components.js';
import { _projectDataCache, _versionPreloadCache, _versionPreloadInFlight } from './mod-detail.js';
import { formatModNameWithChinese } from './chinese-names.js';
import { isPinned, PIN_ICONS } from '../versions/pinned-store.js';
import { actionAttrs, actionAttrsStop } from '../../core/actions.js';
import { resourceState, _resourceReqId, typeNames } from './resource-state.js';

// 资源卡片悬停时预加载版本列表
document.addEventListener('mouseover', (event) => {
  const el = event.target instanceof Element ? event.target.closest('[data-preload-resource]') : null;
  if (!el) return;
  preloadResourceVersions(el.getAttribute('data-preload-resource'), el.getAttribute('data-preload-source') || 'modrinth');
});

function loadResourcePage(type) {
  const state = resourceState[type];
  state.offset = 0;
  state.query = '';
  loadResourceList(type);
  setupResourceEvents(type);
}

function setupResourceEvents(type) {
  const searchInput = document.getElementById(`${type === 'resourcepack' ? 'resourcepack' : type === 'shader' ? 'shader' : type === 'datapack' ? 'datapack' : 'modpack'}-search-input`);
  const searchBtn = document.getElementById(`${type === 'resourcepack' ? 'resourcepack' : type === 'shader' ? 'shader' : type === 'datapack' ? 'datapack' : 'modpack'}-search-btn`);
  const prevBtn = document.getElementById(`${type === 'resourcepack' ? 'resourcepack' : type === 'shader' ? 'shader' : type === 'datapack' ? 'datapack' : 'modpack'}-prev-btn`);
  const nextBtn = document.getElementById(`${type === 'resourcepack' ? 'resourcepack' : type === 'shader' ? 'shader' : type === 'datapack' ? 'datapack' : 'modpack'}-next-btn`);

  const prefix = type === 'resourcepack' ? 'resourcepack' : type === 'shader' ? 'shader' : type === 'datapack' ? 'datapack' : 'modpack';

  if (searchBtn && !searchBtn._bound) {
    searchBtn._bound = true;
    searchBtn.addEventListener('click', () => {
      resourceState[type].query = searchInput.value.trim();
      resourceState[type].offset = 0;
      loadResourceList(type);
    });
  }
  if (searchInput && !searchInput._bound) {
    searchInput._bound = true;
    searchInput.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' && !e.isComposing) {
        resourceState[type].query = searchInput.value.trim();
        resourceState[type].offset = 0;
        loadResourceList(type);
      }
    });
  }
  if (prevBtn && !prevBtn._bound) {
    prevBtn._bound = true;
    prevBtn.addEventListener('click', () => {
      if (resourceState[type].offset >= 15) {
        resourceState[type].offset -= 15;
        loadResourceList(type);
      }
    });
  }
  if (nextBtn && !nextBtn._bound) {
    nextBtn._bound = true;
    nextBtn.addEventListener('click', () => {
      resourceState[type].offset += 15;
      loadResourceList(type);
    });
  }

  const loaderInstance = customSelectInstances[`${prefix}-filter-loader`];
  const versionInstance = customSelectInstances[`${prefix}-filter-version`];
  if (loaderInstance && !loaderInstance._resourceBound) {
    loaderInstance._resourceBound = true;
    loaderInstance.onChange = () => {
      resourceState[type].offset = 0;
      loadResourceList(type);
    };
  }
  if (versionInstance && !versionInstance._resourceBound) {
    versionInstance._resourceBound = true;
    const origOnChange = versionInstance.onChange;
    versionInstance.onChange = () => {
      if (origOnChange) origOnChange();
      resourceState[type].offset = 0;
      loadResourceList(type);
    };
  }
  const sourceInstance = customSelectInstances[`${prefix}-filter-source`];
  if (sourceInstance && !sourceInstance._resourceBound) {
    sourceInstance._resourceBound = true;
    sourceInstance.onChange = () => {
      resourceState[type].offset = 0;
      loadResourceList(type);
    };
  }
  if (type === 'resourcepack') {
    const resolutionInstance = customSelectInstances['resourcepack-filter-resolution'];
    if (resolutionInstance && !resolutionInstance._resourceBound) {
      resolutionInstance._resourceBound = true;
      resolutionInstance.onChange = () => {
        resourceState[type].offset = 0;
        loadResourceList(type);
      };
    }
  }
}

async function loadResourceList(type) {
  const prefix = type === 'resourcepack' ? 'resourcepack' : type === 'shader' ? 'shader' : type === 'datapack' ? 'datapack' : 'modpack';
  const container = document.getElementById(`${prefix}-browse-list`);
  if (!container) return;

  // 递增请求 ID，用于 await 后校验是否为最新请求
  const myReqId = ++_resourceReqId[type];

  container.innerHTML = `<div class="loading-spinner"><div class="spinner"></div><p>正在获取${typeNames[type] || '资源'}列表...</p></div>`;

  const state = resourceState[type];
  const loader = getCustomSelectValue(`${prefix}-filter-loader`);
  const version = getCustomSelectValue(`${prefix}-filter-version`);
  const resolution = type === 'resourcepack' ? getCustomSelectValue('resourcepack-filter-resolution') : '';
  const source = getCustomSelectValue(`${prefix}-filter-source`) || '';

  try {
    const data = await API.searchResources(state.query, type, loader, version, resolution, 'downloads', 15, state.offset, source);

    // 竞态校验：如果不是最新请求，丢弃结果（避免旧请求覆盖新请求）
    if (_resourceReqId[type] !== myReqId) return;

    const hits = data.hits || [];
    state.total = data.total || 0;
    hits.forEach(item => _projectDataCache.set(item.id, item));

    if (hits.length === 0) {
      if (state.query) {
        container.innerHTML = `<p class="empty-text">暂无匹配的${typeNames[type]}</p><p class="empty-hint">试试其他关键词吧</p>`;
      } else {
        container.innerHTML = `<p class="empty-text">暂无${typeNames[type]}</p>`;
      }
    } else {
      container.innerHTML = hits.map(item => `
        <div class="mod-item mod-item-clickable"${actionAttrs('openResourceDetail', item.id, type, item.source || 'modrinth')} data-preload-resource="${escapeHtml(item.id)}" data-preload-source="${escapeHtml(item.source || 'modrinth')}">
          ${item.icon ? `<div class="mod-icon"><img src="${normalizeIconUrl(item.icon)}" alt="" data-img-fallback="remove-parent"></div>` : ''}
          <div class="mod-info">
            <div class="mod-name">${escapeHtml(formatModNameWithChinese(item.slug || item.id, item.title))}
              ${(source === 'any' || !source) ? `<span style="font-size:10px;padding:1px 5px;border-radius:3px;margin-left:6px;background:${item.source === 'curseforge' ? 'color-mix(in srgb, var(--orange) 14%, transparent);color:var(--orange);border:1px solid color-mix(in srgb, var(--orange) 24%, transparent)' : 'color-mix(in srgb, var(--green) 14%, transparent);color:var(--green);border:1px solid color-mix(in srgb, var(--green) 24%, transparent)'};font-weight:500;vertical-align:middle">${item.source === 'curseforge' ? 'CF' : 'MR'}</span>` : ''}
            </div>
            <div class="mod-desc">${escapeHtml(item.description)}</div>
            <div class="mod-meta">
              <span>⬇ ${formatNumber(item.downloads)}</span>
              <span>❤ ${escapeHtml(item.author)}</span>
              <span>${(item.categories || []).slice(0, 3).join(', ')}</span>
            </div>
          </div>
          <div class="mod-actions" data-stop="1">
            ${type === 'shader' ? `<button class="pin-btn${isPinned('shader', item.id) ? ' active' : ''}" title="${isPinned('shader', item.id) ? '取消置顶' : '置顶光影'}"${actionAttrsStop('pinResource', item.id, formatModNameWithChinese(item.slug || item.id, item.title), item.source || 'modrinth', item.icon || '')}>${PIN_ICONS.pin}</button>` : ''}
            <button class="btn btn-primary btn-sm"${actionAttrsStop('openResourceDetail', item.id, type)}>安装</button>
          </div>
        </div>
      `).join('');
    }

    const pageInfo = document.getElementById(`${prefix}-page-info`);
    const totalPages = Math.max(1, Math.ceil(state.total / 15));
    const currentPage = Math.floor(state.offset / 15) + 1;
    if (pageInfo) pageInfo.textContent = `${currentPage}/${totalPages}`;
  } catch (e) {
    // 竞态校验：旧请求报错不覆盖新请求
    if (_resourceReqId[type] !== myReqId) return;
    container.innerHTML = `<p class="empty-text">加载失败</p><button class="btn btn-secondary btn-sm"${actionAttrs('loadResourceList', type)} style="margin-top:8px">重试</button>`;
  }
}

// 资源专用版本预加载：与详情页使用同一接口（getResourceVersions），避免缓存数据不一致
function preloadResourceVersions(projectId, source) {
  if (_versionPreloadCache.has(projectId) || _versionPreloadInFlight.has(projectId)) return;
  _versionPreloadInFlight.add(projectId);
  API.getResourceVersions(projectId, source || 'modrinth').then(data => {
    _versionPreloadCache.set(projectId, data);
    _versionPreloadInFlight.delete(projectId);
  }).catch(() => { _versionPreloadInFlight.delete(projectId); });
}

export { loadResourcePage, setupResourceEvents, loadResourceList, preloadResourceVersions };
