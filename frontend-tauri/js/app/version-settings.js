/**
 * @file version-settings.js
 * @description 版本设置 - 版本信息修改、Java选择、文件修复、诊断、模组管理、导出
 */
let currentSettingsVersionId = null;
let currentVersionSettings = null;
let _modMgrSettingsLoaded = false;
let _exportTreeLoaded = false;

async function openVersionSettings(versionId, versionName) {
  currentSettingsVersionId = versionId;
  _modMgrSettingsLoaded = false;
  _exportTreeLoaded = false;
  const versionInfo = installedVersions.find(v => v.id === versionId);
  const displayName = versionInfo?.customName || versionName || versionId;
  document.getElementById('vset-title').textContent = '版本设置 - ' + displayName;
  document.getElementById('export-name').value = displayName;

  const externalInfoEl = document.getElementById('vset-external-info');
  if (externalInfoEl) {
    if (versionInfo && versionInfo.isExternal) {
      externalInfoEl.style.display = 'block';
      externalInfoEl.innerHTML = `
        <div style="display:flex;align-items:center;gap:8px;padding:10px 14px;border-radius:8px;background:rgba(255,165,0,0.08);border:1px solid rgba(255,165,0,0.2)">
          <svg viewBox="0 0 24 24" fill="none" stroke="#ffa500" stroke-width="2" style="width:18px;height:18px;flex-shrink:0"><path d="M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z"/></svg>
          <div>
            <div style="font-size:13px;color:var(--text-primary);font-weight:500">外部文件夹版本</div>
            <div style="font-size:12px;color:var(--text-muted);margin-top:2px;word-break:break-all">${escapeHtml(versionInfo.externalPath || '')}</div>
          </div>
        </div>`;
    } else {
      externalInfoEl.style.display = 'none';
    }
  }
  
  API.saveSetting('selectedVersion', versionId).catch(e => {
    console.error('[VersionSettings] Failed to set selectedVersion:', e);
  });
  
  navigateToPage('version-settings');
  document.querySelector('.content-area').classList.add('no-scroll');
  switchVSetTab('overview');
  loadVersionSettingsUI();
}

async function loadVersionSettingsUI() {
  if (!currentSettingsVersionId) return;
  try {
    const settings = await API.getVersionSettings(currentSettingsVersionId);
    currentVersionSettings = settings;

    const customNameInput = document.getElementById('vset-custom-name');
    if (customNameInput) customNameInput.value = settings.customName || '';

    const descriptionInput = document.getElementById('vset-description');
    if (descriptionInput) descriptionInput.value = settings.description || '';

    const isolationSelect = document.getElementById('vset-isolation');
    if (isolationSelect) {
      const versionInfo = installedVersions.find(v => v.id === currentSettingsVersionId);
      const isExternal = versionInfo && versionInfo.isExternal;
      isolationSelect.value = settings.isolation || (isExternal ? 'on' : 'global');
    }

    const windowTitle = document.getElementById('vset-window-title');
    if (windowTitle) windowTitle.value = settings.windowTitle || '';

    const customInfo = document.getElementById('vset-custom-info');
    if (customInfo) customInfo.value = settings.customInfo || '';

    if (document.getElementById('vset-java-wrapper') || customSelectInstances['vset-java']) {
      await refreshVsetJavaOptions(settings.javaPath || 'global');
    }

    const memoryMode = document.querySelector(`input[name="vsetMemoryMode"][value="${settings.memoryMode || 'global'}"]`);
    if (memoryMode) memoryMode.checked = true;

    const memoryCustom = document.getElementById('vset-memory-custom');
    if (memoryCustom) memoryCustom.style.display = settings.memoryMode === 'custom' ? 'block' : 'none';

    const memoryValue = document.getElementById('vset-memory-value');
    if (memoryValue) memoryValue.value = settings.memoryValue || 4096;

    const memoryDisplay = document.getElementById('vset-memory-display');
    if (memoryDisplay) memoryDisplay.textContent = (settings.memoryValue || 4096) + ' MB';

    const memOptimize = document.getElementById('vset-mem-optimize');
    if (memOptimize) memOptimize.value = settings.memOptimize || 'global';

    const jvmArgsInput = document.getElementById('vset-jvm-args');
    if (jvmArgsInput) jvmArgsInput.value = settings.jvmArgs || '';

    const gameArgsInput = document.getElementById('vset-game-args');
    if (gameArgsInput) gameArgsInput.value = settings.gameArgs || '';

  } catch (e) {
    console.error('[VersionSettings] Load settings error:', e);
  }
}

function saveCurrentVersionSetting(key, value) {
  if (!currentSettingsVersionId) return;
  const data = { versionId: currentSettingsVersionId, [key]: value };
  API.saveVersionSettings(data).then(r => {
    if (r.success) {
      if (currentVersionSettings) currentVersionSettings[key] = value;
    }
  }).catch(e => console.error('[VersionSettings] Save error:', e));
}

async function refreshVsetJavaOptions(selectValue) {
  try {
    const javaData = await API.getInstalledJava();
    const javaList = javaData.java || [];
    const options = [
      { value: 'global', text: '跟随全局设置' },
      ...javaList.map(j => ({
        value: j.path || j.executable || '',
        text: `${j.version || j.name || 'Java'}${j.arch ? ' (' + j.arch + ')' : ''}${j.majorVersion ? ' [' + j.majorVersion + ']' : ''}`
      }))
    ];
    if (!customSelectInstances['vset-java']) {
      customSelectInstances['vset-java'] = new CustomSelect('vset-java-wrapper', {
        onChange: (value) => saveCurrentVersionSetting('javaPath', value)
      });
    }
    customSelectInstances['vset-java'].setOptions(options);
    if (selectValue) {
      customSelectInstances['vset-java'].setValue(selectValue);
    }
  } catch (e) {
    console.error('[VersionSettings] Refresh Java options error:', e);
  }
}

async function vsetDetectJava() {
  if (!currentSettingsVersionId) return;
  showToast('正在搜索 Java...', 'info');
  try {
    const result = await API.detectJava();
    if (result.javaList && result.javaList.length > 0) {
      const best = result.javaList.find(j => j.majorVersion >= 17) || result.javaList[0];
      await refreshVsetJavaOptions(best.path);
      saveCurrentVersionSetting('javaPath', best.path);
      showToast(`已找到 Java ${best.version}，已自动选中`, 'success');
    } else {
      showToast('未检测到 Java，请尝试手动导入', 'warning');
    }
  } catch (e) {
    showToast('Java 搜索失败', 'error');
  }
}

async function vsetBrowseJava() {
  if (!currentSettingsVersionId) return;
  if (window.electronAPI && window.electronAPI.showOpenDialog) {
    try {
      const result = await window.electronAPI.showOpenDialog({
        properties: ['openFile'],
        filters: [{ name: 'Java 可执行文件', extensions: ['exe', ''] }]
      });
      if (!result.canceled && result.filePaths.length > 0) {
        const javaPath = result.filePaths[0];
        await refreshVsetJavaOptions(javaPath);
        saveCurrentVersionSetting('javaPath', javaPath);
        showToast('已导入 Java，已自动选中', 'success');
      }
    } catch (e) {
      showToast('导入失败', 'error');
    }
  } else {
    showToast('请手动输入 Java 路径', 'info');
  }
}

function refreshVersionDisplayName() {
  if (!currentSettingsVersionId) return;
  const customName = document.getElementById('vset-custom-name')?.value || '';
  const versionInfo = installedVersions.find(v => v.id === currentSettingsVersionId);
  if (versionInfo) versionInfo.customName = customName;
  const displayName = customName || currentSettingsVersionId;
  document.getElementById('vset-title').textContent = '版本设置 - ' + displayName;
  const vItem = document.querySelector(`.version-item[data-version-id="${CSS.escape(currentSettingsVersionId)}"] .version-item-name`);
  if (vItem) vItem.textContent = displayName;
  updateVersionSelects();
}

function closeVersionSettings() {
  currentSettingsVersionId = null;
  currentVersionSettings = null;
  _modDownloadVersionId = '';
  document.querySelector('.content-area').classList.remove('no-scroll');
  navigateToPage(previousPage || 'home');
}

function switchVSetTab(tabName) {
  document.querySelectorAll('.vset-nav-item').forEach(b => b.classList.remove('active'));
  document.querySelector(`.vset-nav-item[data-tab="${tabName}"]`)?.classList.add('active');

  document.querySelectorAll('.vset-panel').forEach(p => p.classList.remove('active'));
  const panel = document.getElementById(`vset-panel-${tabName}`);
  if (panel) panel.classList.add('active');

  if (tabName === 'modmgr') {
    const versionInfo = installedVersions.find(v => v.id === currentSettingsVersionId);
    const isVanilla = versionInfo && !versionInfo.isFabric && !versionInfo.isForge && !versionInfo.isNeoForge;
    const modList = document.getElementById('modmgr-mod-list');
    const modHeader = panel?.querySelector('.modmgr-header-row');
    const modActions = panel?.querySelector('.modmgr-actions');
    if (isVanilla) {
      if (modHeader) modHeader.style.display = 'none';
      if (modActions) modActions.style.display = 'none';
      if (modList) {
        modList.innerHTML = `
          <div style="display:flex;flex-direction:column;align-items:center;justify-content:center;padding:60px 20px;text-align:center;">
            <svg viewBox="0 0 24 24" fill="none" stroke="var(--text-muted)" stroke-width="1.5" style="width:48px;height:48px;margin-bottom:16px;opacity:0.5"><path d="M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9"/><path d="M13.73 21a2 2 0 0 1-3.46 0"/><line x1="1" y1="1" x2="23" y2="23"/></svg>
            <div style="font-size:16px;font-weight:600;color:var(--text-primary);margin-bottom:8px;">原版不支持安装模组</div>
            <div style="font-size:13px;color:var(--text-muted);max-width:320px;line-height:1.6;">此版本为 Minecraft 原版，没有模组加载器。如需安装模组，请先安装 Fabric、Forge 或 NeoForge 模组加载器。</div>
          </div>`;
      }
    } else {
      if (modHeader) modHeader.style.display = '';
      if (modActions) modActions.style.display = '';
      if (!_modMgrSettingsLoaded) {
        loadInstalledModsForSettings();
      }
    }
  } else if (tabName === 'export' && !_exportTreeLoaded) {
    loadExportTreeData();
  }
}

function openVersionFolder() {
  if (!currentSettingsVersionId) return;
  API.openVersionFolder(currentSettingsVersionId, 'version');
}

function openSavesFolder() {
  if (!currentSettingsVersionId) return;
  API.openVersionFolder(currentSettingsVersionId, 'saves');
}

function openModsFolder() {
  if (!currentSettingsVersionId) return;
  API.openVersionFolder(currentSettingsVersionId, 'mods');
}

let _checkingModUpdates = false;

async function checkModUpdatesForVersion() {
  if (!currentSettingsVersionId) {
    showToast('请先选择一个版本', 'error');
    return;
  }
  if (_checkingModUpdates) {
    showToast('正在检查更新，请稍候...', 'info');
    return;
  }
  _checkingModUpdates = true;
  showToast('正在检查模组更新...', 'info');
  try {
    const result = await API.checkModUpdates(currentSettingsVersionId);
    if (result.error) {
      showToast('检查更新失败: ' + result.error, 'error');
      return;
    }
    const updates = result.updates || [];
    if (updates.length === 0) {
      showToast(`已检查 ${result.checked || 0} 个模组，暂无更新`, 'success');
      return;
    }
    showModUpdateDialog(updates, result.checked || 0);
  } catch (e) {
    showToast('检查更新失败: ' + (e.message || '未知错误'), 'error');
  } finally {
    _checkingModUpdates = false;
  }
}

function showModUpdateDialog(updates, checkedCount) {
  const overlay = document.createElement('div');
  overlay.className = 'modal-overlay';
  overlay.style.cssText = 'position:fixed;top:0;left:0;right:0;bottom:0;background:rgba(0,0,0,0.5);z-index:10000;display:flex;align-items:center;justify-content:center;';

  const dialog = document.createElement('div');
  dialog.style.cssText = 'background:var(--bg-primary);border-radius:12px;padding:24px;max-width:560px;width:90%;max-height:70vh;overflow-y:auto;box-shadow:0 8px 32px rgba(0,0,0,0.3);';

  const listHtml = updates.map(u => `
    <div style="display:flex;align-items:center;justify-content:space-between;padding:10px 0;border-bottom:1px solid var(--border-color);">
      <div style="flex:1;min-width:0;">
        <div style="font-weight:600;color:var(--text-primary);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;">${escapeHtml(u.modName)}</div>
        <div style="font-size:12px;color:var(--text-muted);margin-top:2px;">${escapeHtml(u.fileName)} | 当前版本: ${escapeHtml(u.currentVersion)}</div>
      </div>
      <a href="${u.projectUrl}" target="_blank" style="color:var(--accent);font-size:13px;text-decoration:none;white-space:nowrap;margin-left:12px;">查看更新</a>
    </div>
  `).join('');

  dialog.innerHTML = `
    <h3 style="margin:0 0 4px 0;color:var(--text-primary);">模组更新检查</h3>
    <p style="margin:0 0 16px 0;font-size:13px;color:var(--text-muted);">已检查 ${checkedCount} 个模组，发现 ${updates.length} 个可在 Modrinth 上找到</p>
    <div>${listHtml}</div>
    <div style="margin-top:16px;text-align:right;">
      <button class="btn btn-secondary" onclick="this.closest('.modal-overlay').remove()">关闭</button>
    </div>
  `;

  overlay.appendChild(dialog);
  overlay.addEventListener('click', (e) => { if (e.target === overlay) overlay.remove(); });
  document.body.appendChild(overlay);
}

function exportLaunchScript() {
  if (!currentSettingsVersionId) return;
  API.exportLaunchScript(currentSettingsVersionId).then(r => {
    if (r.success) showToast('启动脚本已导出', 'success');
    else showToast(r.error || '导出失败', 'error');
  });
}

let currentRepairSessionId = null;
let repairPollTimer = null;

function showRepairModal(versionId) {
  document.getElementById('repair-modal-title').textContent = `文件修复 - ${versionId}`;
  document.getElementById('repair-progress-fill').style.width = '0%';
  document.getElementById('repair-stage').textContent = '准备中...';
  document.getElementById('repair-percent').textContent = '0%';
  document.getElementById('repair-message').textContent = '';
  document.getElementById('repair-file-count').textContent = '';
  document.getElementById('repair-cancel-btn').style.display = '';
  showModal('repair-modal');
}

function closeRepairModal() {
  hideModal('repair-modal');
  if (repairPollTimer) { clearTimeout(repairPollTimer); repairPollTimer = null; }
  currentRepairSessionId = null;
}

function cancelRepair() {
  if (currentRepairSessionId) {
    API.repairCancel(currentRepairSessionId);
    currentRepairSessionId = null;
  }
  if (repairPollTimer) { clearTimeout(repairPollTimer); repairPollTimer = null; }
  document.getElementById('repair-stage').textContent = '修复已取消';
  document.getElementById('repair-cancel-btn').style.display = 'none';
  showToast('修复已取消', 'info');
  setTimeout(() => hideModal('repair-modal'), 1500);
}

function getRepairStageText(stage) {
  const map = {
    'preparing': '准备修复...',
    'directories': '检查目录结构...',
    'resolve': '解析版本信息...',
    'scanning': '扫描库文件...',
    'client_jar': '检查客户端JAR...',
    'downloading': '下载缺失文件...',
    'complete': '修复完成',
    'failed': '修复失败',
    'cancelled': '已取消'
  };
  return map[stage] || stage || '';
}

function pollRepairProgress(sessionId) {
  const poll = async () => {
    try {
      const data = await API.repairProgress(sessionId);
      const fill = document.getElementById('repair-progress-fill');
      const stage = document.getElementById('repair-stage');
      const percent = document.getElementById('repair-percent');
      const message = document.getElementById('repair-message');
      const fileCount = document.getElementById('repair-file-count');

      if (fill) fill.style.width = `${data.progress || 0}%`;
      if (stage) stage.textContent = getRepairStageText(data.stage);
      if (percent) percent.textContent = `${Math.round(data.progress || 0)}%`;
      if (message) message.textContent = data.message || '';

      if (fileCount) {
        const parts = [];
        if (data.checkedFiles !== undefined && data.totalFiles !== undefined) {
          parts.push(`已检查: ${data.checkedFiles}/${data.totalFiles}`);
        }
        if (data.missingFiles !== undefined) {
          parts.push(`缺失: ${data.missingFiles}`);
        }
        if (data.repairedFiles !== undefined) {
          parts.push(`已修复: ${data.repairedFiles}`);
        }
        if (data.currentFile) {
          parts.push(`当前: ${data.currentFile}`);
        }
        fileCount.textContent = parts.join(' | ');
      }

      if (data.status === 'completed') {
        document.getElementById('repair-progress-fill').style.width = '100%';
        document.getElementById('repair-percent').textContent = '100%';
        document.getElementById('repair-cancel-btn').style.display = 'none';
        showToast(data.message || '文件修复完成！', 'success');
        currentRepairSessionId = null;
        setTimeout(() => hideModal('repair-modal'), 2000);
        return;
      }
      if (data.status === 'failed') {
        document.getElementById('repair-stage').textContent = '修复失败';
        document.getElementById('repair-cancel-btn').style.display = 'none';
        showToast(data.message || '文件修复失败', 'error');
        currentRepairSessionId = null;
        return;
      }
      if (data.status === 'cancelled') {
        currentRepairSessionId = null;
        return;
      }
      repairPollTimer = setTimeout(poll, 500);
    } catch (e) {
      repairPollTimer = setTimeout(poll, 1000);
    }
  };
  poll();
}

async function repairFiles() {
  if (!currentSettingsVersionId) return;

  showRepairModal(currentSettingsVersionId);

  try {
    const result = await API.repairStart(currentSettingsVersionId);
    if (result.success && result.sessionId) {
      currentRepairSessionId = result.sessionId;
      pollRepairProgress(result.sessionId);
    } else {
      document.getElementById('repair-stage').textContent = '启动失败';
      document.getElementById('repair-message').textContent = result.error || '无法启动修复';
      document.getElementById('repair-cancel-btn').style.display = 'none';
      showToast(result.error || '启动修复失败', 'error');
    }
  } catch (e) {
    document.getElementById('repair-stage').textContent = '启动失败';
    document.getElementById('repair-message').textContent = '网络错误，请重试';
    document.getElementById('repair-cancel-btn').style.display = 'none';
    showToast('启动修复失败: ' + e.message, 'error');
  }
}

async function diagnoseVersion() {
  if (!currentSettingsVersionId) {
    showToast('请先选择一个游戏版本', 'error');
    return;
  }

  try {
    const result = await API.diagnoseVersion(currentSettingsVersionId);
    showDiagnoseDialog(result);
  } catch (e) {
    showToast('诊断失败: ' + e.message, 'error');
  }
}

function showDiagnoseDialog(result) {
  const issues = result.issues || [];
  const typeColors = { critical: '#ef4444', warning: '#f59e0b', info: '#3b82f6' };
  const typeLabels = { critical: '严重', warning: '警告', info: '信息' };

  let html = issues.map(issue => `
    <div style="display:flex;align-items:flex-start;gap:8px;padding:8px;border-radius:6px;background:var(--bg-active);margin-bottom:6px;">
      <span style="color:${typeColors[issue.type]};font-weight:600;min-width:36px;">${typeLabels[issue.type]}</span>
      <div>
        <div style="font-size:13px;">${issue.message}</div>
        <div style="font-size:12px;color:var(--text-secondary);margin-top:2px;">${issue.solution}</div>
      </div>
    </div>
  `).join('');

  const overlay = document.createElement('div');
  overlay.className = 'modal';
  overlay.setAttribute('role', 'dialog');
  overlay.setAttribute('aria-modal', 'true');

  overlay.innerHTML = `
    <div class="modal-content" style="width:520px;min-height:auto;max-height:80vh;">
      <div class="modal-header">
        <h3>版本诊断结果</h3>
        <button class="modal-close diagnose-close" aria-label="关闭对话框">&times;</button>
      </div>
      <div class="modal-body" style="overflow-y:auto;max-height:60vh;">
        ${html}
      </div>
      <div class="modal-footer">
        <button class="modal-btn modal-btn--secondary diagnose-close">关闭</button>
      </div>
    </div>
  `;

  document.body.appendChild(overlay);
  requestAnimationFrame(() => overlay.classList.add('modal-visible'));

  const close = () => {
    overlay.classList.add('modal-exiting');
    overlay.classList.remove('modal-visible');
    setTimeout(() => {
      if (overlay.parentElement) overlay.parentElement.removeChild(overlay);
    }, 200);
  };

  overlay.querySelectorAll('.diagnose-close').forEach(btn => btn.addEventListener('click', close));
  overlay.addEventListener('click', (e) => { if (e.target === overlay) close(); });
  overlay.addEventListener('keydown', (e) => { if (e.key === 'Escape') close(); });
}

async function deleteCurrentVersion() {
  if (!currentSettingsVersionId) {
    showToast('未找到版本信息', 'error');
    return;
  }
  const isExternal = currentSettingsVersionId.includes(' [外部');
  if (isExternal) {
    const confirmed = await showConfirmDialog('移除外部版本', '确定要从列表中移除此外部版本吗？（不会删除实际游戏文件）', '移除', '取消');
    if (!confirmed) return;
  } else {
    const ver = installedVersions.find(v => v.id === currentSettingsVersionId);
    let warningParts = [];
    if (ver?.hasMods) warningParts.push('模组');
    if (ver?.hasSaves) warningParts.push('存档');
    if (ver?.hasResourcepacks) warningParts.push('资源包');
    let confirmMsg = `确定要删除版本 ${currentSettingsVersionId} 吗？此操作不可撤销！`;

    let chainInfo = '';
    try {
      const chainResult = await API.getDeleteChain(currentSettingsVersionId);
      if (chainResult.success && chainResult.willDelete && chainResult.willDelete.length > 1) {
        const otherVersions = chainResult.willDelete.filter(id => id !== currentSettingsVersionId);
        if (otherVersions.length > 0) {
          chainInfo = `\n\n同时将删除关联版本：\n${otherVersions.map(id => '• ' + id).join('\n')}`;
        }
      }
    } catch (_) {}

    if (warningParts.length > 0) {
      confirmMsg += `\n\n⚠ 由于该版本开启了版本隔离，删除版本时该版本对应的${warningParts.join('、')}等文件也将被一并删除！`;
    }
    if (chainInfo) confirmMsg += chainInfo;

    const confirmed = await showConfirmDialog('版本删除确认', confirmMsg, '删除', '取消');
    if (!confirmed) return;
  }
  const deletedVersionId = currentSettingsVersionId;
  try {
    const r = await API.deleteVersion(deletedVersionId);
    if (r.success) {
      const deletedNames = r.deleted ? r.deleted.join('、') : deletedVersionId;
      showToast(`版本 ${deletedNames} 已删除`, 'success');
      closeVersionSettings();
      await loadVersions(true);
      const installedContainer = document.getElementById('installed-versions-list');
      if (installedContainer) renderInstalledVersionsInto(installedContainer);
    } else {
      showToast(r.error || '删除失败', 'error');
    }
  } catch (e) {
    showToast('删除失败', 'error');
  }
}

document.querySelectorAll('input[name="vsetMemoryMode"]').forEach(r => {
  if (r._vsBound) return;
  r._vsBound = true;
  r.addEventListener('change', function() {
    document.getElementById('vset-memory-custom').style.display = this.value === 'custom' ? 'block' : 'none';
    saveCurrentVersionSetting('memoryMode', this.value);
  });
});

const memSlider = getDOMElement('vset-memory-value');
if (memSlider && !memSlider._vsBound) {
  memSlider._vsBound = true;
  const memDisplay = getDOMElement('vset-memory-display');
  memSlider.addEventListener('input', throttle(function() {
    if (memDisplay) memDisplay.textContent = this.value + ' MB';
  }, 50));
  memSlider.addEventListener('change', function() {
    saveCurrentVersionSetting('memoryValue', parseInt(this.value, 10));
  });
}


const vsetIsolation = document.getElementById('vset-isolation');
if (vsetIsolation && !vsetIsolation._vsBound) {
  vsetIsolation._vsBound = true;
  vsetIsolation.addEventListener('change', function() {
    saveCurrentVersionSetting('isolation', this.value);
  });
}

const vsetWindowTitle = document.getElementById('vset-window-title');
if (vsetWindowTitle && !vsetWindowTitle._vsBound) {
  vsetWindowTitle._vsBound = true;
  vsetWindowTitle.addEventListener('change', function() {
    saveCurrentVersionSetting('windowTitle', this.value);
  });
}

const vsetCustomInfo = document.getElementById('vset-custom-info');
if (vsetCustomInfo && !vsetCustomInfo._vsBound) {
  vsetCustomInfo._vsBound = true;
  vsetCustomInfo.addEventListener('change', function() {
    saveCurrentVersionSetting('customInfo', this.value);
  });
}

if (customSelectInstances['vset-java']) {
  customSelectInstances['vset-java'].onChange = (value) => saveCurrentVersionSetting('javaPath', value);
}

if (customSelectInstances['vset-mem-optimize']) {
  customSelectInstances['vset-mem-optimize'].onChange = (value) => saveCurrentVersionSetting('memOptimize', value);
}

const vsetJvmArgs = document.getElementById('vset-jvm-args');
if (vsetJvmArgs && !vsetJvmArgs._vsBound) {
  vsetJvmArgs._vsBound = true;
  vsetJvmArgs.addEventListener('change', function() {
    saveCurrentVersionSetting('jvmArgs', this.value);
  });
}

const vsetGameArgs = document.getElementById('vset-game-args');
if (vsetGameArgs && !vsetGameArgs._vsBound) {
  vsetGameArgs._vsBound = true;
  vsetGameArgs.addEventListener('change', function() {
    saveCurrentVersionSetting('gameArgs', this.value);
  });
}

async function loadInstalledModsForSettings() {
  if (!currentSettingsVersionId) return;
  const container = document.getElementById('modmgr-mod-list');
  if (container) {
    container.innerHTML = '<p class="empty-text" style="padding:30px 0;text-align:center;color:var(--text-muted)">加载中...</p>';
  }
  try {
    const mods = await API.getVersionMods(currentSettingsVersionId);
    _modMgrSettingsLoaded = true;
    renderModMgrList(mods || []);
  } catch (e) {
    console.error('[ModMgr] Load error:', e);
  }
}

function renderModMgrList(mods) {
  const container = document.getElementById('modmgr-mod-list');
  const countAll = document.getElementById('modmgr-count-all');
  const countUpdate = document.getElementById('modmgr-count-update');

  if (!container) return;

  if (!mods || mods.length === 0) {
    container.innerHTML = '<p class="empty-text" style="padding:30px 0;text-align:center;color:var(--text-muted)">暂无已安装的模组</p>';
    if (countAll) countAll.textContent = '0';
    if (countUpdate) countUpdate.textContent = '0';
    return;
  }

  const BATCH_SIZE = 30;
  const total = mods.length;
  container.innerHTML = '';

  function renderBatch(start) {
    const fragment = document.createDocumentFragment();
    const end = Math.min(start + BATCH_SIZE, total);
    for (let i = start; i < end; i++) {
      const m = mods[i];
      const iconUrl = m.icon || '';
      const desc = (m.description || '').substring(0, 60);
      const verStr = m.version || '';
      const author = m.author || '';
      const projectId = m.projectId || m.slug || '';
      const isDisabled = m.disabled || false;
      const fileName = m.fileName || m.name || '';
      const toggleLabel = isDisabled ? '启用' : '禁用';
      const toggleClass = isDisabled ? 'btn-primary' : 'btn-secondary';
      const nameStyle = isDisabled ? 'opacity:0.5;text-decoration:line-through;' : '';
      const iconHtml = iconUrl
        ? `<div class="modmgr-icon"><img src="${iconUrl}" alt="" loading="lazy" onerror="this.style.display='none';this.parentElement.classList.add('modmgr-icon--fallback')"></div>`
        : '<div class="modmgr-icon modmgr-icon--fallback"></div>';
      const wrapper = document.createElement('div');
      wrapper.className = `modmgr-item${isDisabled ? ' mod-disabled' : ''}`;
      wrapper.dataset.name = m.name || '';
      wrapper.dataset.desc = desc;
      wrapper.innerHTML = `${iconHtml}
      <div class="modmgr-info">
        <div class="modmgr-name" style="${nameStyle}">${escapeHtml(formatModNameWithChinese(m.slug || m.id || m.fileName, m.name))}${isDisabled ? ' (已禁用)' : ''}</div>
        <div class="modmgr-meta">${author ? escapeHtml(author) : ''}${verStr ? ' | ' + escapeHtml(verStr) : ''}</div>
        <div class="modmgr-desc">${escapeHtml(desc)}</div>
      </div>
      <div class="modmgr-actions-row">
        <button class="btn ${toggleClass} btn-sm" onclick="event.stopPropagation();toggleModInManager('${escapeOnclick(fileName)}',${!isDisabled})">${toggleLabel}</button>
        ${projectId ? `<button class="btn btn-secondary btn-sm" onclick="event.stopPropagation();previewMod('${escapeOnclick(projectId)}')">预览</button>` : ''}
        <button class="btn btn-secondary btn-sm" onclick="event.stopPropagation();openModTranslateDialog('${escapeOnclick(fileName)}','${escapeOnclick(m.name || fileName)}')">汉化</button>
        <button class="btn btn-danger btn-sm" onclick="event.stopPropagation();removeModFromManager('${escapeOnclick(fileName)}')">移除</button>
      </div>`;
      fragment.appendChild(wrapper);
    }
    container.appendChild(fragment);
    if (end < total) {
      requestAnimationFrame(() => renderBatch(end));
    }
  }

  renderBatch(0);
  if (countAll) countAll.textContent = mods.length;
  if (countUpdate) countUpdate.textContent = '0';
}

function previewMod(projectId) {
  if (!projectId) return;
  openModDetail(projectId, 'modrinth');
}

function filterInstalledMods() {
  const keyword = (document.getElementById('modmgr-search')?.value || '').toLowerCase();
  document.querySelectorAll('.modmgr-item').forEach(item => {
    const name = (item.dataset.name || '').toLowerCase();
    const desc = (item.dataset.desc || '').toLowerCase();
    item.style.display = (name.includes(keyword) || desc.includes(keyword)) ? 'flex' : 'none';
  });
}

function filterModMgrTab(filter) {
  document.querySelectorAll('.modmgr-tab').forEach(t => t.classList.remove('active'));
  document.querySelector(`.modmgr-tab[data-filter="${filter}"]`)?.classList.add('active');
}

function selectAllMods() {
  showToast('已选择所有模组', 'info');
}

function installModFromFile() {
  showToast('请选择要安装的 Mod 文件（.jar）', 'info');
  API.selectModFile().then(result => {
    if (result && result.filePath) {
      installModByFile(result.filePath);
    }
  });
}

function installModByFile(filePath) {
  if (!currentSettingsVersionId) {
    showToast('请先选择一个版本', 'error');
    return;
  }
  API.installModFromFile(currentSettingsVersionId, filePath).then(r => {
    if (r.success) {
      showToast('Mod 安装成功', 'success');
      loadInstalledModsForSettings();
    } else {
      showToast(r.error || '安装失败', 'error');
    }
  }).catch(e => showToast('安装失败: ' + e.message, 'error'));
}

function openBrowseMods() {
  _modDownloadVersionId = '';
  navigateToPage('mods');
}

function goDownloadMods() {
  if (!currentSettingsVersionId) {
    _modDownloadVersionId = '';
    navigateToPage('mods');
    return;
  }
  _modDownloadVersionId = currentSettingsVersionId;
  
  const versionInfo = installedVersions.find(v => v.id === currentSettingsVersionId);
  
  let gameVersion = '';
  if (versionInfo && versionInfo.baseVersion) {
    gameVersion = versionInfo.baseVersion;
  } else if (versionInfo && versionInfo.inheritsFrom) {
    gameVersion = versionInfo.inheritsFrom;
  } else {
    gameVersion = currentSettingsVersionId.split('-')[0];
  }
  
  let loaderType = '';
  if (versionInfo) {
    if (versionInfo.isFabric) loaderType = 'fabric';
    else if (versionInfo.isForge) loaderType = 'forge';
    else if (versionInfo.isNeoForge) loaderType = 'neoforge';
  }

  navigateToPage('mods');
  
  setTimeout(() => {
    if (gameVersion && customSelectInstances['mod-filter-version']) {
      customSelectInstances['mod-filter-version'].setValue(gameVersion);
    }
    
    if (loaderType && customSelectInstances['mod-filter-loader']) {
      customSelectInstances['mod-filter-loader'].setValue(loaderType);
    }
    
    modSearchOffset = 0;
    loadMods();
  }, 100);
}

function toggleModInManager(fileName, disable) {
  if (!currentSettingsVersionId) return;
  API.toggleMod(fileName, !disable, currentSettingsVersionId).then(r => {
    if (r.success) {
      showToast(disable ? '已禁用' : '已启用', 'success');
      loadInstalledModsForSettings();
    } else {
      showToast(r.error || '操作失败', 'error');
    }
  }).catch(e => showToast(e.message || '操作失败', 'error'));
}

async function removeModFromManager(fileName) {
  if (!currentSettingsVersionId) return;
  const confirmed = await showConfirmDialog('删除模组', `确定要删除 ${fileName} 吗？`, '删除', '取消');
  if (!confirmed) return;
  API.removeMod(currentSettingsVersionId, fileName).then(r => {
    if (r.success) {
      showToast('已删除', 'success');
      loadInstalledModsForSettings();
    } else {
      showToast(r.error || '删除失败', 'error');
    }
  });
}

function toggleExportTree(el) {
  el.classList.toggle('expanded');
}

async function loadExportTreeData() {
  if (!currentSettingsVersionId) return;

  try {
    const data = await API.getVersionExportInfo(currentSettingsVersionId);
    _exportTreeLoaded = true;

    if (data.gameDesc) {
      const el = document.getElementById('export-game-desc');
      if (el) el.textContent = data.gameDesc;
    }

    if (data.modCount !== undefined) {
      const el = document.getElementById('export-mod-count');
      if (el) el.textContent = `${data.modCount} 个`;
    }

    if (data.savesCount !== undefined) {
      const el = document.getElementById('export-saves-desc');
      if (el) el.textContent = `${data.savesCount} 个存档`;
    }

    const rpList = document.getElementById('export-rp-list');
    if (rpList && data.resourcePacks && data.resourcePacks.length > 0) {
      rpList.innerHTML = data.resourcePacks.map(rp =>
        `<div class="export-tree-item"><input type="checkbox" checked class="export-cb" data-key="rp_${escapeHtml(rp)}"><span class="export-label">${escapeHtml(rp)}</span></div>`
      ).join('');
    } else if (rpList) {
      rpList.innerHTML = '<div class="export-tree-item"><span class="export-label" style="color:var(--text-muted)">暂无资源包</span></div>';
    }

    const savesList = document.getElementById('export-saves-list');
    if (savesList && data.saves && data.saves.length > 0) {
      savesList.innerHTML = data.saves.slice(0, 10).map(s =>
        `<div class="export-tree-item"><input type="checkbox" checked class="export-cb" data-key="save_${escapeHtml(s)}"><span class="export-label">${escapeHtml(s)}</span></div>`
      ).join('') + (data.saves.length > 10 ? `<div class="export-tree-item"><span class="export-label" style="color:var(--text-muted)">... 还有 ${data.saves.length - 10} 个存档</span></div>` : '');
    } else if (savesList) {
      savesList.innerHTML = '<div class="export-tree-item"><span class="export-label" style="color:var(--text-muted)">暂无存档</span></div>';
    }
  } catch (e) {
    console.error('[Export] Load tree data error:', e);
  }
}

function startExport() {
  if (!currentSettingsVersionId) return;
  const name = document.getElementById('export-name')?.value || '';
  const version = document.getElementById('export-version')?.value || '1.0.0';
  const author = document.getElementById('export-author')?.value || '';
  const description = document.getElementById('export-description')?.value || '';

  if (!name.trim()) { showToast('请输入整合包名称', 'error'); return; }

  const selectedKeys = [];
  document.querySelectorAll('.export-cb:checked').forEach(cb => selectedKeys.push(cb.dataset.key));

  showToast('正在导出整合包...', 'info');
  API.exportModpack(currentSettingsVersionId, name, version, author, description, selectedKeys).then(r => {
    if (r.success) {
      showToast(`整合包已导出到 ${r.path}`, 'success');
    } else {
      showToast(r.error || '导出失败', 'error');
    }
  }).catch(e => showToast('导出失败: ' + (e.message || ''), 'error'));
}

/* ===================== 模组汉化功能 ===================== */

let _translateModJarPath = '';
let _translateModName = '';
let _translateLangInfo = null;

async function openModTranslateDialog(fileName, modName) {
  if (!currentSettingsVersionId) {
    showToast('请先选择版本', 'error');
    return;
  }
  _translateModName = modName || fileName;
  const modsDir = await API.getVersionModsDir ? '' : '';
  const versionsDir = window.electronAPI?.mods ? '' : '';
  const jarPath = await _resolveModJarPath(fileName);
  if (!jarPath) {
    showToast('找不到模组文件: ' + fileName, 'error');
    return;
  }
  _translateModJarPath = jarPath;

  showModal('mod-translate-modal');
  document.getElementById('mod-translate-loading').style.display = '';
  document.getElementById('mod-translate-content').style.display = 'none';

  try {
    const result = await window.electronAPI.mods.findLangFiles(jarPath);
    _translateLangInfo = result;

    document.getElementById('mod-translate-loading').style.display = 'none';
    document.getElementById('mod-translate-content').style.display = '';
    document.getElementById('mod-translate-modname').textContent = _translateModName;

    let langText = '';
    if (result && result.langFiles && result.langFiles.length > 0) {
      const enFile = result.langFiles.find(f => f.isEnglish) || result.langFiles[0];
      const hasZh = result.langFiles.some(f => f.isChinese);
      langText = `共 ${result.langFiles.length} 个语言文件`;
      if (enFile) langText += `，英文源: ${enFile.name}`;
      if (hasZh) langText += '，已有中文翻译';
    } else {
      langText = '未找到语言文件（此模组可能没有可翻译的文本）';
    }
    document.getElementById('mod-translate-langinfo').textContent = langText;

    const aiConfig = _getTranslateAIConfig();
    const warning = document.getElementById('mod-translate-ai-warning');
    const startBtn = document.getElementById('mod-translate-start-btn');
    if (!aiConfig || !aiConfig.provider || !aiConfig.apiKey) {
      warning.style.display = '';
      startBtn.disabled = true;
      startBtn.textContent = '未配置 AI';
    } else {
      warning.style.display = 'none';
      startBtn.disabled = false;
      startBtn.textContent = '开始机翻';
    }

    document.getElementById('mod-translate-progress').style.display = 'none';
    document.getElementById('mod-translate-result').style.display = 'none';

    switchTranslateTab('quick');
  } catch (e) {
    document.getElementById('mod-translate-loading').innerHTML =
      `<p style="color:var(--text-muted);">检查失败: ${e.message}</p>`;
  }
}

async function _resolveModJarPath(fileName) {
  try {
    const r = await fetch(`/api/mods/installed?versionId=${encodeURIComponent(currentSettingsVersionId)}`);
    const mods = await r.json();
    const mod = (mods || []).find(m => m.fileName === fileName);
    if (mod && mod.jarPath) return mod.jarPath;
  } catch (_) {}
  try {
    const dir = await window.electronAPI.mods.getVersionModsDir(currentSettingsVersionId);
    if (dir) {
      const path = await import('path');
      return dir + '/' + fileName;
    }
  } catch (_) {}
  return '';
}

function _getTranslateAIConfig() {
  try { return JSON.parse(localStorage.getItem('v-island-ai-config') || '{}'); }
  catch (_) { return {}; }
}

const _AI_PROVIDERS = {
  zhipu:    { apiFormat: 'openai',    endpoint: 'https://open.bigmodel.cn/api/paas/v4/chat/completions' },
  deepseek: { apiFormat: 'openai',    endpoint: 'https://api.deepseek.com/chat/completions' },
  qwen:     { apiFormat: 'openai',    endpoint: 'https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions' },
  openai:   { apiFormat: 'openai',    endpoint: 'https://api.openai.com/v1/chat/completions' },
  anthropic:{ apiFormat: 'anthropic', endpoint: 'https://api.anthropic.com/v1/messages' },
  google:   { apiFormat: 'google',    endpoint: '' },
  custom:   null,
};

function switchTranslateTab(tab) {
  document.querySelectorAll('.mod-translate-tab').forEach(t => {
    t.classList.toggle('active', t.dataset.tab === tab);
  });
  document.getElementById('mod-translate-tab-quick').style.display = tab === 'quick' ? '' : 'none';
  document.getElementById('mod-translate-tab-auto').style.display = tab === 'auto' ? '' : 'none';
  document.getElementById('mod-translate-tab-manual').style.display = tab === 'manual' ? '' : 'none';
}

async function startQuickTranslate() {
  if (!_translateLangInfo || !_translateLangInfo.langFiles || _translateLangInfo.langFiles.length === 0) {
    showToast('没有可翻译的语言文件', 'error');
    return;
  }

  const enFile = _translateLangInfo.langFiles.find(f => f.isEnglish);
  if (!enFile) {
    showToast('未找到英文语言文件，无法翻译', 'error');
    return;
  }

  const startBtn = document.getElementById('mod-translate-quick-btn');
  const progressBar = document.getElementById('mod-translate-quick-progress-bar');
  const progressBox = document.getElementById('mod-translate-quick-progress');
  const progressText = document.getElementById('mod-translate-quick-progress-text');
  const resultBox = document.getElementById('mod-translate-quick-result');

  startBtn.disabled = true;
  startBtn.textContent = '翻译中...';
  progressBox.style.display = '';
  progressBar.style.width = '0%';
  progressText.textContent = '正在读取语言文件...';
  resultBox.style.display = 'none';

  try {
    const readResult = await window.electronAPI.mods.readJarEntry(_translateModJarPath, enFile.name);
    if (!readResult || !readResult.success || !readResult.content) {
      throw new Error(readResult?.error || '读取语言文件失败');
    }
    const content = readResult.content;

    let entries;
    if (enFile.name.endsWith('.json')) {
      entries = JSON.parse(content);
    } else {
      entries = {};
      content.split('\n').forEach(line => {
        const match = line.match(/^(.+?)=(.+)$/);
        if (match) entries[match[1].trim()] = match[2].trim();
      });
    }

    const keys = Object.keys(entries);
    if (keys.length === 0) {
      throw new Error('语言文件为空');
    }

    const values = keys.map(k => entries[k]);
    const BATCH_SIZE = 40;
    const translatedValues = [];

    progressText.textContent = '共 ' + keys.length + ' 条文本，正在翻译...';
    progressBar.style.width = '2%';

    for (let i = 0; i < values.length; i += BATCH_SIZE) {
      const batch = values.slice(i, i + BATCH_SIZE);
      const res = await window.electronAPI.ai.translateBatch({ texts: batch, source: 'en', target: 'zh-CN' });
      if (!res || !res.ok || !res.results) {
        throw new Error(res?.error || '翻译接口失败（批次 ' + (Math.floor(i / BATCH_SIZE) + 1) + '）');
      }
      // 确保结果数量匹配
      const results = res.results;
      while (results.length < batch.length) results.push(batch[results.length]);
      translatedValues.push(...results.slice(0, batch.length));

      const done = Math.min(i + BATCH_SIZE, values.length);
      progressBar.style.width = Math.round((done / values.length) * 100) + '%';
      progressText.textContent = '已翻译 ' + done + '/' + values.length + ' 条文本...';
    }

    // 组装翻译后的 entries
    const translatedEntries = {};
    keys.forEach((k, idx) => { translatedEntries[k] = translatedValues[idx]; });

    // 确定输出文件名
    const zhFileName = enFile.name.replace(/en_us\.json$/i, 'zh_cn.json').replace(/en_us\.lang$/i, 'zh_cn.lang').replace(/en\.lang$/i, 'zh_cn.lang');
    const isJson = zhFileName.endsWith('.json');
    const outputContent = isJson ? JSON.stringify(translatedEntries, null, 2) : keys.map(k => k + '=' + translatedEntries[k]).join('\n');

    const writeResult = await window.electronAPI.mods.writeJarEntry(_translateModJarPath, zhFileName, outputContent);
    if (!writeResult || !writeResult.success) {
      throw new Error(writeResult?.error || '写入翻译文件失败');
    }

    progressBar.style.width = '100%';
    progressText.textContent = '';
    resultBox.style.display = '';
    resultBox.style.background = 'rgba(34,197,94,0.1)';
    resultBox.style.color = '#22c55e';
    resultBox.textContent = '汉化完成！共翻译 ' + keys.length + ' 条文本，已写入 ' + zhFileName;

    showToast('快速汉化完成，共翻译 ' + keys.length + ' 条', 'success');
  } catch (e) {
    resultBox.style.display = '';
    resultBox.style.background = 'rgba(239,68,68,0.1)';
    resultBox.style.color = '#ef4444';
    resultBox.textContent = '汉化失败：' + e.message;
    showToast('汉化失败：' + e.message, 'error');
  } finally {
    startBtn.disabled = false;
    startBtn.textContent = '开始快速汉化';
  }
}

async function startAutoTranslate() {
  if (!_translateLangInfo || !_translateLangInfo.langFiles || _translateLangInfo.langFiles.length === 0) {
    showToast('没有可翻译的语言文件', 'error');
    return;
  }

  const aiConfig = _getTranslateAIConfig();
  if (!aiConfig || !aiConfig.provider || !aiConfig.apiKey) {
    showToast('请先在 V岛设置中配置 AI 供应商', 'error');
    return;
  }

  const enFile = _translateLangInfo.langFiles.find(f => f.isEnglish) || _translateLangInfo.langFiles[0];
  if (!enFile) {
    showToast('找不到英文源语言文件', 'error');
    return;
  }

  const zhName = enFile.zhName || enFile.name.replace(/en_us/i, 'zh_cn');
  const startBtn = document.getElementById('mod-translate-start-btn');
  const progressDiv = document.getElementById('mod-translate-progress');
  const progressBar = document.getElementById('mod-translate-progress-bar');
  const progressText = document.getElementById('mod-translate-progress-text');
  const resultDiv = document.getElementById('mod-translate-result');

  startBtn.disabled = true;
  startBtn.textContent = '翻译中...';
  progressDiv.style.display = '';
  resultDiv.style.display = 'none';
  progressBar.style.width = '10%';
  progressText.textContent = '正在读取语言文件...';

  try {
    const readResult = await window.electronAPI.mods.readJarEntry(_translateModJarPath, enFile.name);
    if (!readResult || !readResult.success || !readResult.content) {
      throw new Error(readResult?.error || '读取语言文件失败');
    }
    const content = readResult.content;

    let entries;
    if (enFile.name.endsWith('.json')) {
      entries = JSON.parse(content);
    } else {
      entries = {};
      content.split('\n').forEach(line => {
        const match = line.match(/^(.+?)=(.+)$/);
        if (match) entries[match[1].trim()] = match[2].trim();
      });
    }

    const keys = Object.keys(entries);
    if (keys.length === 0) {
      throw new Error('语言文件为空');
    }

    progressBar.style.width = '20%';
    progressText.textContent = `共 ${keys.length} 条文本，正在翻译...`;

    const isJson = enFile.name.endsWith('.json');
    const translated = await _translateWithAI(entries, aiConfig, isJson, (done, total) => {
      const pct = 20 + Math.round((done / total) * 70);
      progressBar.style.width = pct + '%';
      progressText.textContent = `已翻译 ${done}/${total} 条...`;
    });

    progressBar.style.width = '90%';
    progressText.textContent = '正在写入翻译结果...';

    let outputContent;
    if (isJson) {
      outputContent = JSON.stringify(translated, null, 2);
    } else {
      outputContent = keys.map(k => `${k}=${translated[k] || entries[k]}`).join('\n');
    }

    await window.electronAPI.mods.writeJarEntry(_translateModJarPath, zhName, outputContent);

    progressBar.style.width = '100%';
    progressText.textContent = '完成！';
    resultDiv.style.display = '';
    resultDiv.style.background = 'rgba(34,197,94,0.1)';
    resultDiv.style.color = '#22c55e';
    resultDiv.textContent = `汉化完成！已翻译 ${keys.length} 条文本，写入到 ${zhName}`;

    startBtn.textContent = '重新翻译';
    startBtn.disabled = false;
    showToast(`「${_translateModName}」汉化完成`, 'success');
  } catch (e) {
    progressDiv.style.display = 'none';
    resultDiv.style.display = '';
    resultDiv.style.background = 'rgba(239,68,68,0.1)';
    resultDiv.style.color = '#ef4444';
    resultDiv.textContent = '翻译失败: ' + e.message;
    startBtn.textContent = '重试';
    startBtn.disabled = false;
    showToast('汉化失败: ' + e.message, 'error');
  }
}

async function _translateWithAI(entries, aiConfig, isJson, onProgress) {
  const keys = Object.keys(entries);
  const BATCH_SIZE = 50;
  const PARALLEL = 4;
  const result = {};

  const provider = aiConfig.provider;
  const providerInfo = _AI_PROVIDERS[provider];
  const baseConfig = {
    provider: provider,
    apiKey: aiConfig.apiKey,
    model: aiConfig.model,
    messages: [],
    maxTokens: 4096,
    timeout: 120000,
  };
  if (provider === 'custom') {
    if (!aiConfig.endpoint) throw new Error('自定义供应商未配置接口地址');
    baseConfig.endpoint = aiConfig.endpoint;
    baseConfig.apiFormat = aiConfig.apiFormat || 'openai';
  } else if (providerInfo) {
    baseConfig.endpoint = providerInfo.endpoint;
    baseConfig.apiFormat = providerInfo.apiFormat;
    if (provider === 'google') {
      baseConfig.endpoint = `https://generativelanguage.googleapis.com/v1beta/models/${aiConfig.model}:generateContent`;
    }
  } else {
    throw new Error('不支持的供应商: ' + provider);
  }

  const batches = [];
  for (let i = 0; i < keys.length; i += BATCH_SIZE) {
    const batchKeys = keys.slice(i, i + BATCH_SIZE);
    const batchEntries = {};
    batchKeys.forEach(k => { batchEntries[k] = entries[k]; });
    batches.push({ index: i, keys: batchKeys, entries: batchEntries });
  }

  const systemPrompt = '你是 Minecraft 模组翻译专家。将用户提供的 JSON 对象中所有英文值翻译成简体中文。要求：1. 只翻译值，不翻译键名 2. 保留所有格式化符号如 %s %d §a &e 等 3. 只返回 JSON 对象，不要使用 markdown 代码块 4. 不要添加任何解释文字';

  let completed = 0;

  async function translateOneBatch(batch) {
    const batchNum = Math.floor(batch.index / BATCH_SIZE) + 1;
    const reqConfig = Object.assign({}, baseConfig);
    reqConfig.messages = [
      { role: 'system', content: systemPrompt },
      { role: 'user', content: JSON.stringify(batch.entries) },
    ];

    let res;
    let lastError;
    let attempts = 0;
    for (let retry = 0; retry < 3; retry++) {
      attempts = retry + 1;
      try {
        res = await window.electronAPI.ai.chat(reqConfig);
        if (res && res.ok && res.reply) {
          const reply = res.reply;
          const jsonStart = reply.indexOf('{');
          const jsonEnd = reply.lastIndexOf('}');
          if (jsonStart >= 0 && jsonEnd > jsonStart) {
            let jsonStr = reply.substring(jsonStart, jsonEnd + 1);
            let translatedBatch = JSON.parse(jsonStr);
            Object.assign(result, translatedBatch);
            completed += batch.keys.length;
            if (onProgress) onProgress(Math.min(completed, keys.length), keys.length);
            return;
          }
        }
        lastError = (res && res.error) || (res && res.reply === '(空回复)' ? '空回复' : '返回格式异常');
        // 余额不足、Key 无效等不可恢复的错误，立即停止重试
        if (lastError && (lastError.indexOf('余额不足') !== -1 ||
            lastError.indexOf('API Key 无效') !== -1 ||
            lastError.indexOf('权限不足') !== -1 ||
            lastError.indexOf('接口地址或模型名称') !== -1)) {
          break;
        }
      } catch (e) {
        lastError = e.message;
      }
      if (retry < 2) await new Promise(r => setTimeout(r, 1500));
    }
    throw new Error('AI 请求失败（批次 ' + batchNum + '，重试' + attempts + '次）：' + lastError);
  }

  for (let i = 0; i < batches.length; i += PARALLEL) {
    const group = batches.slice(i, i + PARALLEL);
    await Promise.all(group.map(b => translateOneBatch(b)));
  }

  return result;
}

async function exportModLangFile() {
  if (!_translateLangInfo || !_translateLangInfo.langFiles || _translateLangInfo.langFiles.length === 0) {
    showToast('没有可导出的语言文件', 'error');
    return;
  }

  const enFile = _translateLangInfo.langFiles.find(f => f.isEnglish) || _translateLangInfo.langFiles[0];
  if (!enFile) {
    showToast('找不到英文源语言文件', 'error');
    return;
  }

  try {
    const readResult = await window.electronAPI.mods.readJarEntry(_translateModJarPath, enFile.name);
    if (!readResult || !readResult.success || !readResult.content) {
      showToast(readResult?.error || '读取语言文件失败', 'error');
      return;
    }
    const content = readResult.content;

    const blob = new Blob([content], { type: 'application/octet-stream' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = enFile.name.split('/').pop();
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
    showToast(`已导出 ${enFile.name.split('/').pop()}`, 'success');
  } catch (e) {
    showToast('导出失败: ' + e.message, 'error');
  }
}

async function importModLangFile() {
  if (!_translateLangInfo) {
    showToast('请先打开模组汉化弹窗', 'error');
    return;
  }

  const fileInput = document.getElementById('mod-translate-file-input');
  fileInput.onchange = async (e) => {
    const file = e.target.files[0];
    if (!file) return;
    fileInput.value = '';

    try {
      const content = await file.text();
      const enFile = _translateLangInfo.langFiles.find(f => f.isEnglish) || _translateLangInfo.langFiles[0];
      const zhName = (enFile ? enFile.zhName : null) || 'zh_cn.json';

      await window.electronAPI.mods.writeJarEntry(_translateModJarPath, zhName, content);
      showToast(`已导入 ${file.name} → ${zhName}`, 'success');
    } catch (e) {
      showToast('导入失败: ' + e.message, 'error');
    }
  };
  fileInput.click();
}
