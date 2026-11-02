// versions-loaders.js — 加载器选择与 Fabric API 选择器
import { API as API } from '../../core/api.js';
import { bridge as bridge } from '../../core/tauri-bridge.js';
import { escapeHtml as escapeHtml } from '../../core/utils.js';
import { CustomSelect as CustomSelect } from '../misc/custom-select.js';
import { showToast as showToast, showModal as showModal, hideModal as hideModal } from '../misc/ui-components.js';
import { appState as appState } from '../../core/app-state.js';
import { actionAttrs as actionAttrs } from '../../core/actions.js';
import { versionsState as S } from './versions-state.js';
import { loadVersions } from './versions-list.js';

function openModLoaderModal(gameVersion) {
  showModal('modloader-modal');

  if (!appState.modloaderGameVersionCustomSelect) {
    appState.modloaderGameVersionCustomSelect = new CustomSelect('modloader-game-version-wrapper', {
      onChange: () => loadModLoaderVersions()
    });
  }

  const installedBase = appState.installedVersions.filter(v => !v.isFabric && !v.isForge && !v.isNeoForge);
  const versions = installedBase.length > 0 ? installedBase : appState.allVersions.filter(v => v.type === 'release').slice(0, 20);

  const options = versions.map(v => ({
    value: v.id,
    text: v.id
  }));

  appState.modloaderGameVersionCustomSelect.setOptions(options);

  if (gameVersion && options.find(o => o.value === gameVersion)) {
    appState.modloaderGameVersionCustomSelect.setValue(gameVersion);
  }

  loadModLoaderVersions();
  document.getElementById('modloader-install-btn').onclick = installModLoader;
}

function closeModLoaderModal() {
  hideModal('modloader-modal');
  // 重置 Fabric API 选择视图状态
  const pickerView = document.getElementById('fabric-api-picker-view');
  if (pickerView) pickerView.style.display = 'none';
  const formGroups = document.querySelectorAll('#modloader-modal .modal-body > .form-group');
  formGroups.forEach(g => g.style.display = '');
  const footer = document.querySelector('#modloader-modal .modal-footer');
  if (footer) footer.style.display = '';
}

async function loadModLoaderVersions() {
  const gameVersion = appState.modloaderGameVersionCustomSelect ? appState.modloaderGameVersionCustomSelect.getValue() : '';

  if (!appState.modloaderVersionCustomSelect) {
    appState.modloaderVersionCustomSelect = new CustomSelect('modloader-version-wrapper');
  }

  appState.modloaderVersionCustomSelect.setOptions([{ value: '', text: '加载中...' }]);
  try {
    if (appState.currentLoaderType === 'fabric') {
      const versions = await API.getModLoaderVersions(gameVersion, 'fabric');
      const options = versions.map(v => ({
        value: v.version,
        text: `${v.version} ${v.stable ? '(稳定)' : ''}`
      }));
      appState.modloaderVersionCustomSelect.setOptions(options);
      const stable = versions.find(v => v.stable);
      if (stable) appState.modloaderVersionCustomSelect.setValue(stable.version);
      // 游戏版本变化时重置 Fabric API 选择状态（列表在点击卡片时懒加载）
      S._fabricApiPickerLoadedFor = '';
      S._fabricApiSelectedId = '';
      S._fabricApiSelectedText = '';
      S._fabricApiRecommendedId = '';
      S._fabricApiVersionsCache = [];
      const cardText = document.getElementById('fabric-api-card-text');
      if (cardText) {
        cardText.textContent = '不安装';
        cardText.style.color = 'var(--text-muted)';
      }
    } else if (appState.currentLoaderType === 'forge') {
      const versions = await API.getModLoaderVersions(gameVersion, 'forge');
      const options = versions.map(v => ({
        value: v.version,
        text: `${v.version} (${v.type})`
      }));
      appState.modloaderVersionCustomSelect.setOptions(options);
    } else if (appState.currentLoaderType === 'neoforge') {
      const versions = await API.getModLoaderVersions(gameVersion, 'neoforge');
      const options = versions.map(v => ({
        value: v.version,
        text: `${v.version} ${v.type ? '(' + v.type + ')' : ''}`
      }));
      appState.modloaderVersionCustomSelect.setOptions(options);
      if (versions.length > 0) appState.modloaderVersionCustomSelect.setValue(versions[0].version);
    }
  } catch (e) { appState.modloaderVersionCustomSelect.setOptions([{ value: '', text: '加载失败' }]); }

  // 非 Fabric 类型时隐藏 Fabric API 区域
  const apiSection = document.getElementById('fabric-api-section');
  if (apiSection) {
    apiSection.style.display = (appState.currentLoaderType === 'fabric' && gameVersion) ? '' : 'none';
  }
}

function openFabricApiPicker() {
  const gameVersion = appState.modloaderGameVersionCustomSelect ? appState.modloaderGameVersionCustomSelect.getValue() : '';
  if (!gameVersion) {
    showToast('请先选择游戏版本', 'error');
    return;
  }
  // 隐藏主表单，显示选择视图
  const formGroups = document.querySelectorAll('#modloader-modal .modal-body > .form-group');
  formGroups.forEach(g => g.style.display = 'none');
  const pickerView = document.getElementById('fabric-api-picker-view');
  if (pickerView) pickerView.style.display = '';
  // 底部按钮隐藏（选择视图不需要安装按钮）
  const footer = document.querySelector('#modloader-modal .modal-footer');
  if (footer) footer.style.display = 'none';

  // 如果游戏版本变了或没加载过，重新加载
  if (S._fabricApiPickerLoadedFor !== gameVersion) {
    loadFabricApiVersions(gameVersion);
  }
}

function backFromFabricApiPicker() {
  const pickerView = document.getElementById('fabric-api-picker-view');
  if (pickerView) pickerView.style.display = 'none';
  const formGroups = document.querySelectorAll('#modloader-modal .modal-body > .form-group');
  formGroups.forEach(g => g.style.display = '');
  // Fabric 类型时确保 Fabric API 区域可见
  if (appState.currentLoaderType === 'fabric') {
    const apiSection = document.getElementById('fabric-api-section');
    if (apiSection) apiSection.style.display = '';
  }
  const footer = document.querySelector('#modloader-modal .modal-footer');
  if (footer) footer.style.display = '';
}

async function loadFabricApiVersions(gameVersion) {
  const listEl = document.getElementById('fabric-api-list');
  if (!listEl) return;
  listEl.innerHTML = '<div style="padding:20px;text-align:center;color:var(--text-muted)">加载中...</div>';

  try {
    const data = await API.getFabricApiVersions(gameVersion);
    S._fabricApiVersionsCache = data.versions || [];
    S._fabricApiRecommendedId = data.recommended || '';
    S._fabricApiPickerLoadedFor = gameVersion;
    if (S._fabricApiVersionsCache.length === 0) {
      listEl.innerHTML = '<div style="padding:20px;text-align:center;color:var(--text-muted)">该游戏版本暂无可用的 Fabric API</div>';
      return;
    }
    // 首次加载时默认选中推荐版本
    if (!S._fabricApiSelectedId && S._fabricApiRecommendedId) {
      const rec = S._fabricApiVersionsCache.find(v => v.versionId === S._fabricApiRecommendedId);
      if (rec) {
        const typeLabel = rec.releaseType === 'release' ? '稳定' : (rec.releaseType === 'beta' ? '测试' : rec.releaseType);
        S._fabricApiSelectedId = rec.versionId;
        S._fabricApiSelectedText = `${rec.versionNumber} (${typeLabel})`;
        // 同步主表单卡片显示
        const cardText = document.getElementById('fabric-api-card-text');
        if (cardText) {
          cardText.textContent = S._fabricApiSelectedText;
          cardText.style.color = 'var(--text-primary)';
        }
      }
    }
    renderFabricApiList();
  } catch (e) {
    listEl.innerHTML = '<div style="padding:20px;text-align:center;color:var(--red)">加载失败：' + escapeHtml(e.message) + '</div>';
  }
}

function renderFabricApiList() {
  const listEl = document.getElementById('fabric-api-list');
  if (!listEl) return;
  const items = [];
  // 第一项：不安装
  items.push(`
    <div class="fabric-api-item${S._fabricApiSelectedId === '' ? ' selected' : ''}"${actionAttrs('selectFabricApiVersion', '', '不安装')} style="padding:12px 14px;border:1px solid ${S._fabricApiSelectedId === '' ? 'var(--accent)' : 'var(--border)'};border-radius:8px;background:${S._fabricApiSelectedId === '' ? 'rgba(99,102,241,0.08)' : 'var(--bg-secondary)'};cursor:pointer;display:flex;align-items:center;justify-content:space-between;transition:all 0.2s">
      <div>
        <div style="font-size:14px;font-weight:500;color:var(--text-primary)">不安装 Fabric API</div>
        <div style="font-size:12px;color:var(--text-muted);margin-top:2px">仅安装 Fabric 加载器，不安装 API</div>
      </div>
      ${S._fabricApiSelectedId === '' ? '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:18px;height:18px;color: var(--accent-ink)"><polyline points="20 6 9 17 4 12"/></svg>' : ''}
    </div>
  `);
  S._fabricApiVersionsCache.forEach(v => {
    const isSelected = v.versionId === S._fabricApiSelectedId;
    const typeLabel = v.releaseType === 'release' ? '稳定' : (v.releaseType === 'beta' ? '测试' : v.releaseType);
    const date = v.datePublished ? v.datePublished.substring(0, 10) : '';
    const isRecommended = v.versionId === S._fabricApiRecommendedId;
    items.push(`
      <div class="fabric-api-item${isSelected ? ' selected' : ''}"${actionAttrs('selectFabricApiVersion', v.versionId)} style="padding:12px 14px;border:1px solid ${isSelected ? 'var(--accent)' : 'var(--border)'};border-radius:8px;background:${isSelected ? 'rgba(99,102,241,0.08)' : 'var(--bg-secondary)'};cursor:pointer;display:flex;align-items:center;justify-content:space-between;transition:all 0.2s">
        <div style="min-width:0">
          <div style="display:flex;align-items:center;gap:6px">
            <span style="font-size:14px;font-weight:500;color:var(--text-primary)">${escapeHtml(v.versionNumber)}</span>
            ${isRecommended ? '<span style="font-size:10px;padding:1px 6px;border-radius:4px;background:var(--accent);color:var(--accent-text);font-weight:500">推荐</span>' : ''}
          </div>
          <div style="font-size:12px;color:var(--text-muted);margin-top:2px">${escapeHtml(typeLabel)}${date ? ' · ' + date : ''}</div>
        </div>
        ${isSelected ? '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:18px;height:18px;color: var(--accent-ink);flex-shrink:0"><polyline points="20 6 9 17 4 12"/></svg>' : ''}
      </div>
    `);
  });
  listEl.innerHTML = items.join('');
}

function selectFabricApiVersion(versionId) {
  let versionText = '不安装';
  S._fabricApiSelectedFile = { url: '', filename: '' };
  if (versionId) {
    const v = S._fabricApiVersionsCache.find(item => item.versionId === versionId);
    if (v) {
      const typeLabel = v.releaseType === 'release' ? '稳定' : (v.releaseType === 'beta' ? '测试' : v.releaseType);
      versionText = v.versionNumber + ' (' + typeLabel + ')';
      S._fabricApiSelectedFile = { url: v.url || '', filename: v.filename || '' };
    }
  }
  S._fabricApiSelectedId = versionId;
  S._fabricApiSelectedText = versionText;
  // 更新主表单卡片显示
  const cardText = document.getElementById('fabric-api-card-text');
  if (cardText) {
    cardText.textContent = versionId ? versionText : '不安装';
    cardText.style.color = versionId ? 'var(--text-primary)' : 'var(--text-muted)';
  }
  backFromFabricApiPicker();
}

async function installModLoader() {
  const gameVersion = appState.modloaderGameVersionCustomSelect ? appState.modloaderGameVersionCustomSelect.getValue() : '';
  const loaderVersion = appState.modloaderVersionCustomSelect ? appState.modloaderVersionCustomSelect.getValue() : '';
  if (!gameVersion) { showToast('请选择游戏版本', 'error'); return; }

  // 读取 Fabric API 选项（来自二级页面选择）
  let installFabricApi = false;
  let fabricApiVersionId = '';
  if (appState.currentLoaderType === 'fabric' && S._fabricApiSelectedId) {
    installFabricApi = true;
    fabricApiVersionId = S._fabricApiSelectedId;
  }

  const installBtn = document.getElementById('modloader-install-btn');
  const originalText = installBtn ? installBtn.textContent : '';
  if (installBtn) { installBtn.disabled = true; installBtn.textContent = '安装中...'; }

  try {
    let result;
    const loaderNames = { fabric: 'Fabric', forge: 'Forge', neoforge: 'NeoForge' };
    if (appState.currentLoaderType === 'fabric') {
      result = await API.installFabric(gameVersion, loaderVersion);
    } else if (appState.currentLoaderType === 'forge') {
      if (!loaderVersion) { showToast('请选择Forge版本', 'error'); return; }
      result = await API.installForge(gameVersion, loaderVersion);
    } else if (appState.currentLoaderType === 'neoforge') {
      if (!loaderVersion) { showToast('请选择NeoForge版本', 'error'); return; }
      result = await API.installNeoForge(gameVersion, loaderVersion);
    } else {
      showToast('不支持的加载器类型', 'error');
      return;
    }
    if (result.success) {
      let installedId = result.versionId;
      if (!installedId) {
        if (appState.currentLoaderType === 'fabric') installedId = `fabric-loader-${loaderVersion}-${gameVersion}`;
        else if (appState.currentLoaderType === 'forge') installedId = `${gameVersion}-forge-${loaderVersion}`;
        else if (appState.currentLoaderType === 'neoforge') installedId = `${gameVersion}-neoforge-${loaderVersion}`;
        else installedId = `${gameVersion}-${appState.currentLoaderType}-${loaderVersion}`;
      }

      // Fabric 安装成功后，按需安装 Fabric API
      let apiOk = true;
      if (installFabricApi && appState.currentLoaderType === 'fabric') {
        if (installBtn) installBtn.textContent = '正在安装 Fabric API...';
        try {
          const apiFile = S._fabricApiVersionsCache.find(v => v.versionId === fabricApiVersionId);
          const apiResult = await API.installFabricApi(
            gameVersion,
            fabricApiVersionId,
            installedId,
            apiFile ? (apiFile.url || '') : '',
            apiFile ? (apiFile.filename || '') : ''
          );
          if (!apiResult.success) {
            apiOk = false;
            showToast(`Fabric API 安装失败：${apiResult.error || '未知错误'}`, 'error', 5000);
          }
        } catch (e) {
          apiOk = false;
          showToast(`Fabric API 安装失败：${e.message}`, 'error', 5000);
        }
      }

      const msg = apiOk
        ? `${loaderNames[appState.currentLoaderType] || appState.currentLoaderType} 安装成功！${installFabricApi ? ' Fabric API 已一并安装' : ''}`
        : `${loaderNames[appState.currentLoaderType] || appState.currentLoaderType} 安装成功，但 Fabric API 安装失败`;
      showToast(msg, apiOk ? 'success' : 'warning', 4000);
      closeModLoaderModal();
      await loadVersions(true);
      if (appState.launchVersionCustomSelect) {
        appState.launchVersionCustomSelect.setValue(installedId);
        appState._cachedLastLaunchVersion = installedId;
        try { bridge.store.set('versepc_last_launch_version', installedId); } catch (_) {}
      }
    } else {
      showToast(result.error || '安装失败', 'error');
    }
  } catch (e) {
    showToast('安装失败：' + (e.message || ''), 'error');
  } finally {
    if (installBtn) { installBtn.disabled = false; installBtn.textContent = originalText; }
  }
}

function openFabricApiDetailPicker() {
  const gameVersion = S.currentVersionDetail ? S.currentVersionDetail.id : '';
  if (!gameVersion) return;

  // 隐藏主区域，显示选择视图
  const body = document.getElementById('pvd-body');
  if (body) body.style.display = 'none';
  const main = document.getElementById('pvd-main');
  if (main) main.style.display = 'none';
  const footer = document.getElementById('pvd-footer');
  if (footer) footer.style.display = 'none';

  // 把 picker 移到 body 下，避免 .page.active 的 transform:translateZ(0) 和
  // .content-area 的 will-change:transform 创建包含块导致 position:fixed 失效
  const picker = document.getElementById('fabric-api-picker-detail');
  if (picker) {
    if (picker.parentElement !== document.body) {
      document.body.appendChild(picker);
    }
    picker.style.display = '';
  }

  // 如果游戏版本变了或没加载过，重新加载
  if (S._fabricApiDetailLoadedFor !== gameVersion) {
    loadFabricApiDetailVersions(gameVersion);
  }
}

function backFromFabricApiDetailPicker() {
  const picker = document.getElementById('fabric-api-picker-detail');
  if (picker) {
    picker.style.display = 'none';
    // 把 picker 移回版本详情页容器，避免 Vue 重新渲染时残留孤儿节点
    const pageContainer = document.getElementById('page-version-detail');
    if (pageContainer && picker.parentElement !== pageContainer) {
      pageContainer.appendChild(picker);
    }
  }
  const body = document.getElementById('pvd-body');
  if (body) body.style.display = '';
  const main = document.getElementById('pvd-main');
  if (main) main.style.display = '';
  const footer = document.getElementById('pvd-footer');
  if (footer) footer.style.display = '';
}

async function loadFabricApiDetailVersions(gameVersion) {
  const listEl = document.getElementById('fabric-api-list-detail');
  if (!listEl) return;
  listEl.innerHTML = '<div style="padding:20px;text-align:center;color:var(--text-muted)">加载中...</div>';

  try {
    const data = await API.getFabricApiVersions(gameVersion);
    S._fabricApiDetailVersions = data.versions || [];
    S._fabricApiDetailRecommendedId = data.recommended || '';
    S._fabricApiDetailLoadedFor = gameVersion;
    if (S._fabricApiDetailVersions.length === 0) {
      listEl.innerHTML = '<div style="padding:20px;text-align:center;color:var(--text-muted)">该游戏版本暂无可用的 Fabric API</div>';
      return;
    }
    // 首次加载时默认选中推荐版本
    if (!S._fabricApiDetailId && S._fabricApiDetailRecommendedId) {
      const rec = S._fabricApiDetailVersions.find(v => v.versionId === S._fabricApiDetailRecommendedId);
      if (rec) {
        const typeLabel = rec.releaseType === 'release' ? '稳定' : (rec.releaseType === 'beta' ? '测试' : rec.releaseType);
        S._fabricApiDetailId = rec.versionId;
        S._fabricApiDetailText = `${rec.versionNumber} (${typeLabel})`;
        // 同步卡片显示
        const cardText = document.getElementById('fabric-api-card-text-detail');
        if (cardText) {
          cardText.textContent = S._fabricApiDetailText;
          cardText.style.color = 'var(--text-primary)';
        }
      }
    }
    renderFabricApiDetailList();
  } catch (e) {
    listEl.innerHTML = '<div style="padding:20px;text-align:center;color:var(--red)">加载失败：' + escapeHtml(e.message) + '</div>';
  }
}

function renderFabricApiDetailList() {
  const listEl = document.getElementById('fabric-api-list-detail');
  if (!listEl) return;
  const items = [];
  // 第一项：不安装
  items.push(`
    <div class="fabric-api-item${S._fabricApiDetailId === '' ? ' selected' : ''}"${actionAttrs('selectFabricApiDetailVersion', '', '不安装')} style="padding:12px 14px;border:1px solid ${S._fabricApiDetailId === '' ? 'var(--accent)' : 'var(--border)'};border-radius:8px;background:${S._fabricApiDetailId === '' ? 'rgba(99,102,241,0.08)' : 'var(--bg-secondary)'};cursor:pointer;display:flex;align-items:center;justify-content:space-between;transition:all 0.2s">
      <div>
        <div style="font-size:14px;font-weight:500;color:var(--text-primary)">不安装 Fabric API</div>
        <div style="font-size:12px;color:var(--text-muted);margin-top:2px">仅安装 Fabric 加载器，不安装 API</div>
      </div>
      ${S._fabricApiDetailId === '' ? '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:18px;height:18px;color: var(--accent-ink)"><polyline points="20 6 9 17 4 12"/></svg>' : ''}
    </div>
  `);
  S._fabricApiDetailVersions.forEach(v => {
    const isSelected = v.versionId === S._fabricApiDetailId;
    const typeLabel = v.releaseType === 'release' ? '稳定' : (v.releaseType === 'beta' ? '测试' : v.releaseType);
    const date = v.datePublished ? v.datePublished.substring(0, 10) : '';
    const isRecommended = v.versionId === S._fabricApiDetailRecommendedId;
    items.push(`
      <div class="fabric-api-item${isSelected ? ' selected' : ''}"${actionAttrs('selectFabricApiDetailVersion', v.versionId, v.versionNumber + ' (' + typeLabel + ')')} style="padding:12px 14px;border:1px solid ${isSelected ? 'var(--accent)' : 'var(--border)'};border-radius:8px;background:${isSelected ? 'rgba(99,102,241,0.08)' : 'var(--bg-secondary)'};cursor:pointer;display:flex;align-items:center;justify-content:space-between;transition:all 0.2s">
        <div style="min-width:0">
          <div style="display:flex;align-items:center;gap:6px">
            <span style="font-size:14px;font-weight:500;color:var(--text-primary)">${escapeHtml(v.versionNumber)}</span>
            ${isRecommended ? '<span style="font-size:10px;padding:1px 6px;border-radius:4px;background:var(--accent);color:var(--accent-text);font-weight:500">推荐</span>' : ''}
          </div>
          <div style="font-size:12px;color:var(--text-muted);margin-top:2px">${escapeHtml(typeLabel)}${date ? ' · ' + date : ''}</div>
        </div>
        ${isSelected ? '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:18px;height:18px;color: var(--accent-ink);flex-shrink:0"><polyline points="20 6 9 17 4 12"/></svg>' : ''}
      </div>
    `);
  });
  listEl.innerHTML = items.join('');
}

function selectFabricApiDetailVersion(versionId, versionText) {
  S._fabricApiDetailId = versionId;
  S._fabricApiDetailText = versionText || '不安装';
  // 更新卡片显示
  const cardText = document.getElementById('fabric-api-card-text-detail');
  if (cardText) {
    cardText.textContent = versionId ? versionText : '不安装';
    cardText.style.color = versionId ? 'var(--text-primary)' : 'var(--text-muted)';
  }
  backFromFabricApiDetailPicker();
}

export { openModLoaderModal, closeModLoaderModal, loadModLoaderVersions, openFabricApiPicker, backFromFabricApiPicker, loadFabricApiVersions, renderFabricApiList, selectFabricApiVersion, installModLoader, openFabricApiDetailPicker, backFromFabricApiDetailPicker, loadFabricApiDetailVersions, renderFabricApiDetailList, selectFabricApiDetailVersion };
