/* page-version-settings.js - 版本设置页 Vue 组件（完整数据驱动）
 * 交互逻辑全部迁入组件（data/methods/computed），卡片用 v-for/v-if 渲染。
 * class 与整体 DOM 结构保持不变，确保既有 CSS 零修改。
 * 弹窗类流程（文件修复/诊断/模组汉化/模组更新）仍复用全局函数。
 */
const PageVersionSettings = {
  name: 'PageVersionSettings',
  data() {
    return {
      versionId: null,
      title: '版本设置',
      // 版本信息
      customName: '',
      description: '',
      // 设置卡片
      isolation: 'global',
      windowTitle: '',
      customInfo: '',
      javaPath: 'global',
      javaOptions: [
        { value: 'global', text: '跟随全局设置' },
        { value: 'detect', text: '加载中...' }
      ],
      memoryMode: 'global',
      memoryValue: 4096,
      memOptimize: 'global',
      jvmArgs: '',
      gameArgs: '',
      usedMemory: '--',
      gameMemory: '--',
      // 外部版本提示
      isExternal: false,
      externalPath: '',
      // 面板
      activeTab: 'overview',
      // 模组卡片
      mods: [],
      modsLoading: false,
      modSearch: '',
      isVanilla: false,
      modsLoaded: false,
      // 导出卡片
      exportName: '',
      exportVersion: '1.0.0',
      exportAuthor: '',
      exportDescription: '',
      exportGameDesc: '正式版',
      exportModCount: '0 个',
      exportSavesCount: '',
      exportResourcePacks: [],
      exportSaves: [],
      exportSavesMore: 0,
      exportKeys: [],
      exportLoaded: false,
      exportTrees: {
        game: true, mods: true, resourcepacks: true, saves: true
      },
      // 其它
      checkingUpdates: false,
      repairSessionId: null,
      _memDisplayTimer: null
    };
  },
  computed: {
    displayName() {
      return this.customName || this.versionId || '';
    },
    memoryDisplayText() {
      return this.memoryValue + ' MB';
    },
    filteredMods() {
      const kw = (this.modSearch || '').toLowerCase();
      if (!kw) return this.mods;
      return this.mods.filter(m => {
        const name = (m.name || '').toLowerCase();
        const desc = (m.description || '').toLowerCase();
        return name.includes(kw) || desc.includes(kw);
      });
    },
    vanishPromptShown() {
      return this.activeTab === 'modmgr' && this.isVanilla;
    }
  },
  watch: {
    activeTab(val) {
      if (val === 'modmgr' && !this.isVanilla && !this.modsLoaded) {
        this.loadMods();
      } else if (val === 'export' && !this.exportLoaded) {
        this.loadExportTree();
      }
    }
  },
  mounted() {
    window.VersepcSettingsVM = this;
    this.initCustomSelects();
  },
  methods: {
    // ===== 打开 / 关闭 =====
    async openFor(versionId, versionName) {
      this.versionId = versionId;
      this.modsLoaded = false;
      this.exportLoaded = false;
      this.versionId = versionId;
      const versionInfo = (typeof installedVersions !== 'undefined' && installedVersions || []).find(v => v.id === versionId);
      this.customName = (versionInfo && versionInfo.customName) || versionName || versionId;
      this.exportName = this.customName;
      this.isExternal = !!(versionInfo && versionInfo.isExternal);
      this.externalPath = (versionInfo && versionInfo.externalPath) || '';
      this.title = '版本设置 - ' + this.customName;
      this.activeTab = 'overview';

      API.saveSetting('selectedVersion', versionId).catch(e => {
        console.error('[VersionSettings] Failed to set selectedVersion:', e);
      });

      navigateToPage('version-settings');
      document.querySelector('.content-area').classList.add('no-scroll');
      this.loadSettings();
    },
    close() {
      if (typeof _modDownloadVersionId !== 'undefined') _modDownloadVersionId = '';
      const ca = document.querySelector('.content-area');
      if (ca) ca.classList.remove('no-scroll');
      navigateToPage((typeof previousPage !== 'undefined' && previousPage) || 'home');
      this.versionId = null;
    },
    // ===== 设置加载 / 保存 =====
    async loadSettings() {
      if (!this.versionId) return;
      try {
        const settings = await API.getVersionSettings(this.versionId);
        const s = settings || {};
        this.customName = s.customName || this.customName;
        this.description = s.description || '';
        this.windowTitle = s.windowTitle || '';
        this.customInfo = s.customInfo || '';
        this.isolation = s.isolation || (this.isExternal ? 'on' : 'global');
        this.jvmArgs = s.jvmArgs || '';
        this.gameArgs = s.gameArgs || '';
        this.memoryMode = s.memoryMode || 'global';
        this.memoryValue = s.memoryValue || 4096;
        this.memOptimize = s.memOptimize || 'global';
        this.javaPath = s.javaPath || 'global';
        this.title = '版本设置 - ' + (this.customName || this.versionId);
        this.exportName = this.customName || this.versionId;

        this.refreshJavaOptions(this.javaPath);
        this.syncCustomSelects();
        this.refreshMemoryUsage();
      } catch (e) {
        console.error('[VersionSettings] Load settings error:', e);
      }
    },
    saveSetting(key, value) {
      if (!this.versionId) return;
      const data = { versionId: this.versionId, [key]: value };
      API.saveVersionSettings(data).then(r => {
        if (!r.success) console.warn('[VersionSettings] Save failed for', key);
      }).catch(e => console.error('[VersionSettings] Save error:', e));
    },
    onCustomNameInput() {
      this.title = '版本设置 - ' + (this.customName || this.versionId);
      if (typeof installedVersions !== 'undefined' && installedVersions) {
        const vInfo = installedVersions.find(v => v.id === this.versionId);
        if (vInfo) vInfo.customName = this.customName;
        try { updateVersionSelects(); } catch (e) {}
      }
      this.exportName = this.customName;
    },
    // ===== Java =====
    async refreshJavaOptions(selectValue) {
      try {
        const javaData = await API.getInstalledJava();
        const javaList = (javaData && javaData.java) || [];
        this.javaOptions = [
          { value: 'global', text: '跟随全局设置' },
          ...javaList.map(j => ({
            value: j.path || j.executable || '',
            text: `${j.version || j.name || 'Java'}${j.arch ? ' (' + j.arch + ')' : ''}${j.majorVersion ? ' [' + j.majorVersion + ']' : ''}`
          }))
        ];
        this.javaPath = selectValue || 'global';
        this.setCustomSelectValue('vset-java', this.javaPath);
      } catch (e) {
        console.error('[VersionSettings] Refresh Java options error:', e);
      }
    },
    async detectJava() {
      if (!this.versionId) return;
      showToast('正在搜索 Java...', 'info');
      try {
        const result = await API.detectJava();
        if (result.javaList && result.javaList.length > 0) {
          const best = result.javaList.find(j => j.majorVersion >= 17) || result.javaList[0];
          await this.refreshJavaOptions(best.path);
          this.saveSetting('javaPath', best.path);
          showToast(`已找到 Java ${best.version}，已自动选中`, 'success');
        } else {
          showToast('未检测到 Java，请尝试手动导入', 'warning');
        }
      } catch (e) {
        showToast('Java 搜索失败', 'error');
      }
    },
    async browseJava() {
      if (!this.versionId) return;
      if (window.electronAPI && window.electronAPI.showOpenDialog) {
        try {
          const result = await window.electronAPI.showOpenDialog({
            properties: ['openFile'],
            filters: [{ name: 'Java 可执行文件', extensions: ['exe', ''] }]
          });
          if (!result.canceled && result.filePaths.length > 0) {
            const javaPath = result.filePaths[0];
            await this.refreshJavaOptions(javaPath);
            this.saveSetting('javaPath', javaPath);
            showToast('已导入 Java，已自动选中', 'success');
          }
        } catch (e) {
          showToast('导入失败', 'error');
        }
      } else {
        showToast('请手动输入 Java 路径', 'info');
      }
    },
    // ===== CustomSelect 下拉（集成既有组件）=====
    initCustomSelects() {
      const self = this;
      if (typeof CustomSelect === 'undefined') return;
      if (typeof customSelectInstances !== 'undefined') {
        // java
        if (!customSelectInstances['vset-java'] && document.getElementById('vset-java-wrapper')) {
          customSelectInstances['vset-java'] = new CustomSelect('vset-java-wrapper', {
            onChange: (value) => { self.javaPath = value; self.saveSetting('javaPath', value); }
          });
        }
        // mem-optimize
        if (!customSelectInstances['vset-mem-optimize'] && document.getElementById('vset-mem-optimize-wrapper')) {
          customSelectInstances['vset-mem-optimize'] = new CustomSelect('vset-mem-optimize-wrapper', {
            onChange: (value) => { self.memOptimize = value; self.saveSetting('memOptimize', value); }
          });
        }
      }
    },
    syncCustomSelects() {
      this.setCustomSelectValue('vset-java', this.javaPath);
      this.setCustomSelectValue('vset-mem-optimize', this.memOptimize);
    },
    setCustomSelectValue(key, value) {
      if (typeof customSelectInstances === 'undefined') return;
      const inst = customSelectInstances[key];
      if (inst && typeof inst.setValue === 'function') {
        try { inst.setValue(value); } catch (e) {}
      }
    },
    // ===== 快捷入口 =====
    openVersionFolder() {
      if (this.versionId) API.openVersionFolder(this.versionId, 'version');
    },
    openSavesFolder() {
      if (this.versionId) API.openVersionFolder(this.versionId, 'saves');
    },
    openModsFolder() {
      if (this.versionId) API.openVersionFolder(this.versionId, 'mods');
    },
    // ===== 模组卡片 =====
    async loadMods() {
      if (!this.versionId) return;
      this.modsLoading = true;
      this.mods = [];
      try {
        const mods = await API.getVersionMods(this.versionId);
        this.mods = (mods && Array.isArray(mods)) ? mods : [];
        this.mods.forEach(m => { m.iconHtml = (m.icon && !String(m.icon).startsWith('/api/')) ? m.icon : ''; m.iconDataUrl = ''; });
        this.modsLoaded = true;
      } catch (e) {
        console.error('[ModMgr] Load error:', e);
      } finally {
        this.modsLoading = false;
      }
    },
    loadIcon(mod) {
      if (mod.iconHtml || mod.iconDataUrl) return;
      const iconUrl = mod.icon || '';
      const isApi = String(iconUrl).startsWith('/api/');
      if (!isApi) return;
      const mm = iconUrl.match(/hash=([^&]+)/);
      const hash = mm ? decodeURIComponent(mm[1]) : '';
      if (!hash) return;
      API.getModIcon(hash).then(r => {
        if (r && r.dataUrl) {
          mod.iconDataUrl = r.dataUrl;
        }
      }).catch(() => {});
    },
    switchTab(tabName) {
      this.activeTab = tabName;
    },
    formatModName(m) {
      try { return formatModNameWithChinese(m.slug || m.id || m.fileName, m.name); }
      catch (e) { return m.name || m.fileName || ''; }
    },
    previewMod(projectId) {
      if (!projectId) return;
      try { openModDetail(projectId, 'modrinth'); } catch (e) {}
    },
    openTranslate(mod) {
      try { openModTranslateDialog(mod.fileName || mod.name, mod.name || mod.fileName); } catch (e) {}
    },
    toggleMod(mod) {
      if (!this.versionId) return;
      const fileName = mod.fileName || mod.name;
      const disable = !!(mod && mod.disabled);
      API.toggleMod(fileName, !disable, this.versionId).then(r => {
        if (r.success) {
          showToast(disable ? '已禁用' : '已启用', 'success');
          this.loadMods();
        } else {
          showToast(r.error || '操作失败', 'error');
        }
      }).catch(e => showToast(e.message || '操作失败', 'error'));
    },
    async removeMod(mod) {
      if (!this.versionId) return;
      const fileName = mod.fileName || mod.name;
      const confirmed = await showConfirmDialog('删除模组', `确定要删除 ${fileName} 吗？`, '删除', '取消');
      if (!confirmed) return;
      API.removeMod(this.versionId, fileName).then(r => {
        if (r.success) {
          showToast('已删除', 'success');
          this.loadMods();
        } else {
          showToast(r.error || '删除失败', 'error');
        }
      });
    },
    installModFromFile() {
      showToast('请选择要安装的 Mod 文件（.jar）', 'info');
      API.selectModFile().then(result => {
        if (result && result.filePath) this.installModByFile(result.filePath);
      });
    },
    installModByFile(filePath) {
      if (!this.versionId) {
        showToast('请先选择一个版本', 'error');
        return;
      }
      API.installModFromFile(this.versionId, filePath).then(r => {
        if (r.success) {
          showToast('Mod 安装成功', 'success');
          this.loadMods();
        } else {
          showToast(r.error || '安装失败', 'error');
        }
      }).catch(e => showToast('安装失败: ' + e.message, 'error'));
    },
    goDownloadMods() {
      if (typeof _modDownloadVersionId !== 'undefined') _modDownloadVersionId = this.versionId || '';
      const versionInfo = this.versionId && (typeof installedVersions !== 'undefined' && installedVersions || []).find(v => v.id === this.versionId);
      let gameVersion = '';
      if (versionInfo && versionInfo.baseVersion) gameVersion = versionInfo.baseVersion;
      else if (versionInfo && versionInfo.inheritsFrom) gameVersion = versionInfo.inheritsFrom;
      else gameVersion = this.versionId ? this.versionId.split('-')[0] : '';
      let loaderType = '';
      if (versionInfo) {
        if (versionInfo.isFabric) loaderType = 'fabric';
        else if (versionInfo.isForge) loaderType = 'forge';
        else if (versionInfo.isNeoForge) loaderType = 'neoforge';
      }
      navigateToPage('mods');
      setTimeout(() => {
        if (gameVersion && typeof customSelectInstances !== 'undefined' && customSelectInstances['mod-filter-version']) {
          customSelectInstances['mod-filter-version'].setValue(gameVersion);
        }
        if (loaderType && typeof customSelectInstances !== 'undefined' && customSelectInstances['mod-filter-loader']) {
          customSelectInstances['mod-filter-loader'].setValue(loaderType);
        }
        if (typeof modSearchOffset !== 'undefined') modSearchOffset = 0;
        try { if (typeof loadMods === 'function') loadMods(); } catch (e) {}
      }, 100);
    },
    // ===== 概览卡片：脚本导出 / 文件修复 / 诊断 / 删除 =====
    exportLaunchScript() {
      if (!this.versionId) return;
      API.exportLaunchScript(this.versionId).then(r => {
        if (r.success) showToast('启动脚本已导出', 'success');
        else showToast(r.error || '导出失败', 'error');
      });
    },
    async repairFiles() {
      if (!this.versionId) return;
      try { showRepairModal(this.versionId); } catch (e) {}
      try {
        const result = await API.repairStart(this.versionId);
        if (result.success && result.sessionId) {
          this.repairSessionId = result.sessionId;
          pollRepairProgress(result.sessionId);
        } else {
          try {
            document.getElementById('repair-stage').textContent = '启动失败';
            document.getElementById('repair-message').textContent = result.error || '无法启动修复';
            document.getElementById('repair-cancel-btn').style.display = 'none';
          } catch (e) {}
          showToast(result.error || '启动修复失败', 'error');
        }
      } catch (e) {
        try {
          document.getElementById('repair-stage').textContent = '启动失败';
          document.getElementById('repair-message').textContent = '网络错误，请重试';
          document.getElementById('repair-cancel-btn').style.display = 'none';
        } catch (err) {}
        showToast('启动修复失败: ' + e.message, 'error');
      }
    },
    async diagnoseVersion() {
      if (!this.versionId) {
        showToast('请先选择一个游戏版本', 'error');
        return;
      }
      try {
        const result = await API.diagnoseVersion(this.versionId);
        if (typeof showDiagnoseDialog === 'function') showDiagnoseDialog(result);
        else showToast('诊断完成', 'info');
      } catch (e) {
        showToast('诊断失败: ' + e.message, 'error');
      }
    },
    async deleteCurrentVersion() {
      if (!this.versionId) {
        showToast('未找到版本信息', 'error');
        return;
      }
      const isExternal = String(this.versionId).includes(' [外部');
      if (isExternal) {
        const confirmed = await showConfirmDialog('移除外部版本', '确定要从列表中移除此外部版本吗？（不会删除实际游戏文件）', '移除', '取消');
        if (!confirmed) return;
      } else {
        const ver = (typeof installedVersions !== 'undefined' && installedVersions || []).find(v => v.id === this.versionId);
        let warningParts = [];
        if (ver && ver.hasMods) warningParts.push('模组');
        if (ver && ver.hasSaves) warningParts.push('存档');
        if (ver && ver.hasResourcepacks) warningParts.push('资源包');
        let confirmMsg = `确定要删除版本 ${this.versionId} 吗？此操作不可撤销！`;
        let chainInfo = '';
        try {
          const chainResult = await API.getDeleteChain(this.versionId);
          if (chainResult.success && chainResult.willDelete && chainResult.willDelete.length > 1) {
            const otherVersions = chainResult.willDelete.filter(id => id !== this.versionId);
            if (otherVersions.length > 0) {
              chainInfo = `\n\n同时将删除关联版本：\n${otherVersions.map(id => '• ' + id).join('\n')}`;
            }
          }
        } catch (e) {}
        if (warningParts.length > 0) {
          confirmMsg += `\n\n⚠ 由于该版本开启了版本隔离，删除版本时该版本对应的${warningParts.join('、')}等文件也将被一并删除！`;
        }
        if (chainInfo) confirmMsg += chainInfo;
        const confirmed = await showConfirmDialog('版本删除确认', confirmMsg, '删除', '取消');
        if (!confirmed) return;
      }
      const deletedVersionId = this.versionId;
      try {
        const r = await API.deleteVersion(deletedVersionId);
        if (r.success) {
          const deletedNames = r.deleted ? r.deleted.join('、') : deletedVersionId;
          showToast(`版本 ${deletedNames} 已删除`, 'success');
          this.close();
          if (typeof loadVersions === 'function') await loadVersions(true);
          const installedContainer = document.getElementById('installed-versions-list');
          if (installedContainer && typeof renderInstalledVersionsInto === 'function') renderInstalledVersionsInto(installedContainer);
        } else {
          showToast(r.error || '删除失败', 'error');
        }
      } catch (e) {
        showToast('删除失败', 'error');
      }
    },
    // ===== 模组更新（弹窗，复用全局）=====
    async checkModUpdates() {
      if (!this.versionId) {
        showToast('请先选择一个版本', 'error');
        return;
      }
      if (this.checkingUpdates) {
        showToast('正在检查更新，请稍候...', 'info');
        return;
      }
      this.checkingUpdates = true;
      showToast('正在检查模组更新...', 'info');
      try {
        const result = await API.checkModUpdates(this.versionId);
        if (result.error) {
          showToast('检查更新失败: ' + result.error, 'error');
          return;
        }
        const updates = result.updates || [];
        if (updates.length === 0) {
          showToast(`已检查 ${result.checked || 0} 个模组，暂无更新`, 'success');
          return;
        }
        try { showModUpdateDialog(updates, result.checked || 0); } catch (e) {}
      } catch (e) {
        showToast('检查更新失败: ' + (e.message || '未知错误'), 'error');
      } finally {
        this.checkingUpdates = false;
      }
    },
    // ===== 导出卡片 =====
    async loadExportTree() {
      if (!this.versionId) return;
      try {
        const data = await API.getVersionExportInfo(this.versionId);
        this.exportLoaded = true;
        if (data.gameDesc) this.exportGameDesc = data.gameDesc;
        if (data.modCount !== undefined) this.exportModCount = `${data.modCount} 个`;
        if (data.savesCount !== undefined) this.exportSavesCount = `${data.savesCount} 个存档`;
        this.exportResourcePacks = (data.resourcePacks && data.resourcePacks.length) ? data.resourcePacks : [];
        this.exportSaves = (data.saves && data.saves.length) ? data.saves.slice(0, 10) : [];
        this.exportSavesMore = (data.saves && data.saves.length > 10) ? data.saves.length - 10 : 0;
      } catch (e) {
        console.error('[Export] Load tree data error:', e);
      }
    },
    toggleExportTree(key, expanded) {
      const cur = this.exportTrees[key];
      this.exportTrees = { ...this.exportTrees, [key]: cur ? false : true };
    },
    toggleExportKey(key) {
      const i = this.exportKeys.indexOf(key);
      if (i >= 0) this.exportKeys.splice(i, 1);
      else this.exportKeys.push(key);
    },
    startExport() {
      if (!this.versionId) return;
      const name = (this.exportName || '').trim();
      if (!name) { showToast('请输入整合包名称', 'error'); return; }
      const version = this.exportVersion || '1.0.0';
      const author = this.exportAuthor || '';
      const description = this.exportDescription || '';
      showToast('正在导出整合包...', 'info');
      API.exportModpack(this.versionId, name, version, author, description, this.exportKeys).then(r => {
        if (r.success) showToast(`整合包已导出到 ${r.path}`, 'success');
        else showToast(r.error || '导出失败', 'error');
      }).catch(e => showToast('导出失败: ' + (e.message || ''), 'error'));
    },
    // ===== 内存 =====
    refreshMemoryUsage() {
      try {
        API.getSystemMemory().then(info => {
          if (info && info.usedMB !== undefined) {
            this.usedMemory = info.usedMB + ' MB';
            if (info.autoMB !== undefined) {
              this.gameMemory = Math.min(this.memoryValue, info.autoMB) + ' MB';
            }
          }
        }).catch(() => {});
      } catch (e) {}
    }
  },
  template: `
    <div>
      <div class="vset-header">
        <button class="btn btn-icon" @click="close()">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:20px;height:20px"><path d="M15 18l-6-6 6-6"/></svg>
        </button>
        <span class="vset-title">{{ title }}</span>
        <button class="btn btn-icon" @click="close()">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:20px;height:20px"><path d="M6 18L18 6M6 6l12 12"/></svg>
        </button>
      </div>
      <div class="vset-body">
        <div class="vset-sidebar">
          <button class="vset-nav-item" :class="{ active: activeTab==='overview' }" @click="switchTab('overview')">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="square" stroke-linejoin="miter"><rect x="3" y="3" width="8" height="8"/><rect x="13" y="3" width="8" height="8"/><rect x="3" y="13" width="8" height="8"/><rect x="13" y="13" width="8" height="8"/></svg>
            <span>概览</span>
          </button>
          <button class="vset-nav-item" :class="{ active: activeTab==='settings' }" @click="switchTab('settings')">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="square" stroke-linejoin="miter"><circle cx="12" cy="12" r="3"/><path d="M12 2V5"/><path d="M12 19V22"/><path d="M2 12H5"/><path d="M19 12H22"/><path d="M4.93 4.93L7.05 7.05"/><path d="M16.95 16.95L19.07 19.07"/><path d="M4.93 19.07L7.05 16.95"/><path d="M16.95 7.05L19.07 4.93"/></svg>
            <span>设置</span>
          </button>
          <button class="vset-nav-item" :class="{ active: activeTab==='modmgr' }" @click="switchTab('modmgr')">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="square" stroke-linejoin="miter"><path d="M12 2L21 7V17L12 22L3 17V7L12 2Z"/><path d="M12 8L16 10.5V15.5L12 18L8 15.5V10.5L12 8Z"/></svg>
            <span>模组</span>
          </button>
          <button class="vset-nav-item" :class="{ active: activeTab==='export' }" @click="switchTab('export')">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="square" stroke-linejoin="miter"><rect x="4" y="3" width="16" height="3" rx="0"/><rect x="4" y="18" width="16" height="3" rx="0"/><path d="M12 6V15"/><path d="M8 12L12 16L16 12"/></svg>
            <span>导出</span>
          </button>
        </div>
        <div class="vset-content">
          <!-- 概览 -->
          <div class="vset-panel" :class="{ active: activeTab==='overview' }">
            <div v-if="isExternal" class="vset-external-banner" style="display:flex;align-items:center;gap:8px;padding:10px 14px;border-radius:8px;background:rgba(255,165,0,0.08);border:1px solid rgba(255,165,0,0.2);margin-bottom:16px">
              <svg viewBox="0 0 24 24" fill="none" stroke="#ffa500" stroke-width="2" style="width:18px;height:18px;flex-shrink:0"><path d="M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z"/></svg>
              <div>
                <div style="font-size:13px;color:var(--text-primary);font-weight:500">外部文件夹版本</div>
                <div style="font-size:12px;color:var(--text-muted);margin-top:2px;word-break:break-all">{{ externalPath }}</div>
              </div>
            </div>
            <div class="vset-section">
              <div class="vset-section-title">快捷方式</div>
              <div class="vset-btn-group">
                <button class="btn btn-secondary btn-sm" @click="openVersionFolder()">版本文件夹</button>
                <button class="btn btn-secondary btn-sm" @click="openSavesFolder()">存档文件夹</button>
                <button class="btn btn-secondary btn-sm" @click="openModsFolder()">Mod文件夹</button>
              </div>
            </div>
            <div class="vset-section">
              <div class="vset-section-title">高级管理</div>
              <div class="vset-btn-group">
                <button class="btn btn-secondary btn-sm win-only" @click="exportLaunchScript()">导出启动脚本</button>
                <button class="btn btn-secondary btn-sm" @click="repairFiles()">补全文件</button>
                <button class="btn btn-secondary btn-sm" @click="diagnoseVersion()">诊断</button>
                <button class="btn btn-danger btn-sm" @click="deleteCurrentVersion()">删除版本</button>
              </div>
            </div>
          </div>
          <!-- 设置 -->
          <div class="vset-panel" :class="{ active: activeTab==='settings' }">
            <div class="vset-section">
              <div class="vset-section-title">版本信息</div>
              <div class="vset-form-row">
                <label class="vset-label">自定义版本名</label>
                <input type="text" class="vset-input" v-model="customName" placeholder="留空使用默认名称" @input="onCustomNameInput();saveSetting('customName',$event.target.value)" @change="saveSetting('customName',customName)">
              </div>
              <div class="vset-form-row">
                <label class="vset-label">版本描述</label>
                <input type="text" class="vset-input" v-model="description" placeholder="留空使用默认描述" @change="saveSetting('description',description)">
              </div>
            </div>
            <div class="vset-section">
              <div class="vset-section-title">启动选项</div>
              <div class="vset-form-row">
                <label class="vset-label">版本隔离</label>
                <select class="vset-select" id="vset-isolation" v-model="isolation" @change="saveSetting('isolation',isolation)">
                  <option value="global">跟随全局设置</option>
                  <option value="on">开启版本隔离</option>
                  <option value="off">关闭版本隔离</option>
                </select>
              </div>
              <div class="vset-form-row">
                <label class="vset-label">游戏窗口标题</label>
                <input type="text" class="vset-input" v-model="windowTitle" placeholder="跟随全局设置" @change="saveSetting('windowTitle',windowTitle)">
              </div>
              <div class="vset-form-row">
                <label class="vset-label">自定义信息</label>
                <input type="text" class="vset-input" v-model="customInfo" placeholder="跟随全局设置" @change="saveSetting('customInfo',customInfo)">
              </div>
              <div class="vset-form-row">
                <label class="vset-label">游戏 Java</label>
                <div style="display:flex;gap:8px;align-items:center;width:100%;">
                  <select class="vset-select" id="vset-java" v-model="javaPath" @change="saveSetting('javaPath',javaPath)" style="flex:1;">
                    <option v-for="opt in javaOptions" :key="opt.value" :value="opt.value">{{ opt.text }}</option>
                  </select>
                  <button class="btn btn-secondary btn-sm" @click="detectJava()" title="自动搜索">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><circle cx="11" cy="11" r="8"/><path d="M21 21l-4.35-4.35"/></svg>
                    自动搜索
                  </button>
                  <button class="btn btn-secondary btn-sm" @click="browseJava()" title="手动导入">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
                    手动导入
                  </button>
                </div>
              </div>
            </div>
            <div class="vset-section">
              <div class="vset-section-title">内存分配</div>
              <div class="vset-radio-group">
                <label class="vset-radio"><input type="radio" name="vsetMemoryMode" value="global" v-model="memoryMode"><span>跟随全局设置</span></label>
                <label class="vset-radio"><input type="radio" name="vsetMemoryMode" value="auto" v-model="memoryMode"><span>自动配置</span></label>
                <label class="vset-radio"><input type="radio" name="vsetMemoryMode" value="custom" v-model="memoryMode"><span>自定义</span></label>
              </div>
              <div class="vset-slider-wrap" v-show="memoryMode==='custom'">
                <input type="range" class="vset-slider" min="512" max="16384" step="256" v-model.number="memoryValue">
                <div class="vset-slider-info">
                  <span>{{ memoryDisplayText }}</span>
                  <span style="color:var(--text-muted);font-size:12px">已用内存: <span>{{ usedMemory }}</span></span>
                  <span style="color:var(--text-muted);font-size:12px">游戏分配: <span>{{ gameMemory }}</span></span>
                </div>
              </div>
              <div class="vset-form-row" style="margin-top:10px">
                <label class="vset-label">启动游戏前进行内存优化</label>
                <select class="vset-select" id="vset-mem-optimize" v-model="memOptimize" @change="saveSetting('memOptimize',memOptimize)">
                  <option value="global">跟随全局设置</option>
                  <option value="on">开启</option>
                  <option value="off">关闭</option>
                </select>
              </div>
            </div>
            <div class="vset-section">
              <div class="vset-section-title">高级启动选项</div>
              <div class="vset-form-row">
                <label class="vset-label">JVM 参数</label>
                <input type="text" class="vset-input" v-model="jvmArgs" placeholder="跟随全局设置，如 -XX:+UseG1GC" @change="saveSetting('jvmArgs',jvmArgs)">
              </div>
              <div class="vset-form-row">
                <label class="vset-label">游戏参数</label>
                <input type="text" class="vset-input" v-model="gameArgs" placeholder="跟随全局设置，如 --demo" @change="saveSetting('gameArgs',gameArgs)">
              </div>
            </div>
            <div class="vset-info-bar">这些设置只对该游戏版本生效，不影响其他版本。</div>
          </div>
          <!-- 模组 -->
          <div class="vset-panel" :class="{ active: activeTab==='modmgr' }">
            <template v-if="isVanilla">
              <div style="display:flex;flex-direction:column;align-items:center;justify-content:center;padding:60px 20px;text-align:center;">
                <svg viewBox="0 0 24 24" fill="none" stroke="var(--text-muted)" stroke-width="1.5" style="width:48px;height:48px;margin-bottom:16px;opacity:0.5"><path d="M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9"/><path d="M13.73 21a2 2 0 0 1-3.46 0"/><line x1="1" y1="1" x2="23" y2="23"/></svg>
                <div style="font-size:16px;font-weight:600;color:var(--text-primary);margin-bottom:8px;">原版不支持安装模组</div>
                <div style="font-size:13px;color:var(--text-muted);max-width:320px;line-height:1.6;">此版本为 Minecraft 原版，没有模组加载器。如需安装模组，请先安装 Fabric、Forge 或 NeoForge 模组加载器。</div>
              </div>
            </template>
            <template v-else>
              <div class="modmgr-header-row">
                <div class="modmgr-search-box">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="modmgr-search-icon"><circle cx="11" cy="11" r="8"/><path d="M21 21l-4.35-4.35"/></svg>
                  <input type="text" class="modmgr-search-input" placeholder="搜索已安装模组..." v-model="modSearch">
                </div>
                <button class="btn btn-primary btn-sm" @click="goDownloadMods()">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:14px;height:14px"><path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
                  下载模组
                </button>
              </div>
              <div class="modmgr-actions">
                <button class="btn btn-secondary btn-sm" @click="openModsFolder()">打开文件夹</button>
                <button class="btn btn-secondary btn-sm" @click="installModFromFile()">安装 Mod 文件</button>
                <button class="btn btn-secondary btn-sm" @click="checkModUpdates()">检查更新</button>
              </div>
              <div class="modmgr-list">
                <p v-if="modsLoading" class="empty-text" style="padding:30px 0;text-align:center;color:var(--text-muted)">加载中...</p>
                <p v-else-if="filteredMods.length===0" class="empty-text" style="padding:30px 0;text-align:center;color:var(--text-muted)">暂无已安装的模组</p>
                <div v-for="m in filteredMods" :key="m.fileName || m.id || m.name" class="modmgr-item" :class="{ 'mod-disabled': m.disabled }">
                  <div class="modmgr-icon" :class="{ 'modmgr-icon--fallback': !(m.icon && !String(m.icon).startsWith('/api/')) && !m.iconDataUrl }">
                    <img v-if="m.icon && !String(m.icon).startsWith('/api/')" :src="m.icon" alt="" loading="lazy" style="width:100%;height:100%;object-fit:cover;">
                    <img v-else-if="m.iconDataUrl" :src="m.iconDataUrl" alt="" style="width:100%;height:100%;object-fit:cover;">
                  </div>
                  <div class="modmgr-info">
                    <div class="modmgr-name" :style="m.disabled ? 'opacity:0.5;text-decoration:line-through;' : ''">{{ formatModName(m) }}{{ m.disabled ? ' (已禁用)' : '' }}</div>
                    <div class="modmgr-meta">{{ m.author ? m.author : '' }}{{ m.version ? ' | ' + m.version : '' }}</div>
                    <div class="modmgr-desc">{{ (m.description || '').substring(0, 60) }}</div>
                  </div>
                  <div class="modmgr-actions-row">
                    <button class="btn btn-sm" :class="m.disabled ? 'btn-primary' : 'btn-secondary'" @click="toggleMod(m)">{{ m.disabled ? '启用' : '禁用' }}</button>
                    <button v-if="m.projectId || m.slug" class="btn btn-secondary btn-sm" @click="previewMod(m.projectId || m.slug)">预览</button>
                    <button class="btn btn-secondary btn-sm" @click="openTranslate(m)">汉化</button>
                    <button class="btn btn-danger btn-sm" @click="removeMod(m)">移除</button>
                  </div>
                </div>
              </div>
            </template>
          </div>
          <!-- 导出 -->
          <div class="vset-panel" :class="{ active: activeTab==='export' }">
            <div class="export-header-row">
              <div class="export-field">
                <label class="vset-label">整合包名称</label>
                <input type="text" class="vset-input" v-model="exportName" placeholder="输入整合包名称">
              </div>
              <div class="export-field">
                <label class="vset-label">整合包版本</label>
                <input type="text" class="vset-input" v-model="exportVersion" placeholder="1.0.0">
              </div>
            </div>
            <div class="export-header-row" style="margin-top:12px">
              <div class="export-field" style="flex:1">
                <label class="vset-label">作者</label>
                <input type="text" class="vset-input" v-model="exportAuthor" placeholder="输入作者名称">
              </div>
            </div>
            <div class="export-field" style="margin-top:12px">
              <label class="vset-label">描述</label>
              <textarea class="vset-textarea" v-model="exportDescription" placeholder="输入整合包描述..." style="width:100%;min-height:60px;resize:vertical;padding:8px 12px;border-radius:8px;border:1px solid var(--border-color);background:var(--bg-secondary);color:var(--text-primary);font-size:13px"></textarea>
            </div>
            <div class="export-section-title">导出内容列表</div>
            <div class="export-tree">
              <div class="export-tree-item" :class="{ expanded: exportTrees.game }" @click="toggleExportTree('game')">
                <input type="checkbox" checked class="export-cb" value="game" v-model="exportKeys" @click.stop>
                <span class="export-toggle">▾</span>
                <span class="export-label">游戏本体</span>
                <span class="export-desc">{{ exportGameDesc }}</span>
                <div class="export-children">
                  <div class="export-tree-item">
                    <input type="checkbox" checked class="export-cb" value="game_settings" v-model="exportKeys" @click.stop>
                    <span class="export-label">游戏设置</span>
                    <span class="export-desc">options.txt</span>
                  </div>
                  <div class="export-tree-item">
                    <input type="checkbox" checked class="export-cb" value="servers" v-model="exportKeys" @click.stop>
                    <span class="export-label">服务器列表</span>
                    <span class="export-desc">servers.dat</span>
                  </div>
                </div>
              </div>
              <div class="export-tree-item" :class="{ expanded: exportTrees.mods }" @click="toggleExportTree('mods')">
                <input type="checkbox" checked class="export-cb" value="mods" v-model="exportKeys" @click.stop>
                <span class="export-toggle">▾</span>
                <span class="export-label">Mod</span>
                <span class="export-desc">模组</span>
                <div class="export-children">
                  <div class="export-tree-item">
                    <input type="checkbox" checked class="export-cb" value="mod_files" v-model="exportKeys" @click.stop>
                    <span class="export-label">Mod 文件</span>
                    <span class="export-desc">{{ exportModCount }}</span>
                  </div>
                  <div class="export-tree-item">
                    <input type="checkbox" checked class="export-cb" value="mod_configs" v-model="exportKeys" @click.stop>
                    <span class="export-label">Mod 配置</span>
                    <span class="export-desc">config 文件夹</span>
                  </div>
                </div>
              </div>
              <div class="export-tree-item" :class="{ expanded: exportTrees.resourcepacks }" @click="toggleExportTree('resourcepacks')">
                <input type="checkbox" class="export-cb" value="resourcepacks" v-model="exportKeys" @click.stop>
                <span class="export-toggle">▾</span>
                <span class="export-label">资源包</span>
                <span class="export-desc">纹理包/材质包</span>
                <div class="export-children">
                  <div v-if="exportResourcePacks.length===0" class="export-tree-item">
                    <span class="export-label" style="color:var(--text-muted)">暂无资源包</span>
                  </div>
                  <div v-for="rp in exportResourcePacks" :key="rp" class="export-tree-item">
                    <input type="checkbox" checked class="export-cb" :value="'rp_' + rp" v-model="exportKeys" @click.stop>
                    <span class="export-label">{{ rp }}</span>
                  </div>
                </div>
              </div>
              <div class="export-tree-item">
                <input type="checkbox" class="export-cb" value="shaderpacks" v-model="exportKeys" @click.stop>
                <span class="export-toggle" style="visibility:hidden">▾</span>
                <span class="export-label">光影包</span>
                <span class="export-desc">shaderpacks 文件夹</span>
              </div>
              <div class="export-tree-item" :class="{ expanded: exportTrees.saves }" @click="toggleExportTree('saves')">
                <input type="checkbox" class="export-cb" value="saves" v-model="exportKeys" @click.stop>
                <span class="export-toggle">▾</span>
                <span class="export-label">存档</span>
                <span class="export-desc">{{ exportSavesCount || '游戏存档' }}</span>
                <div class="export-children">
                  <div v-if="exportSaves.length===0" class="export-tree-item">
                    <span class="export-label" style="color:var(--text-muted)">暂无存档</span>
                  </div>
                  <div v-for="s in exportSaves" :key="s" class="export-tree-item">
                    <input type="checkbox" checked class="export-cb" :value="'save_' + s" v-model="exportKeys" @click.stop>
                    <span class="export-label">{{ s }}</span>
                  </div>
                  <div v-if="exportSavesMore > 0" class="export-tree-item">
                    <span class="export-label" style="color:var(--text-muted)">... 还有 {{ exportSavesMore }} 个存档</span>
                  </div>
                </div>
              </div>
              <div class="export-tree-item">
                <input type="checkbox" class="export-cb" value="screenshots" v-model="exportKeys" @click.stop>
                <span class="export-toggle" style="visibility:hidden">▾</span>
                <span class="export-label">截图</span>
                <span class="export-desc">screenshots 文件夹</span>
              </div>
              <div class="export-tree-item" :class="{ expanded: exportTrees.more }" @click="toggleExportTree('more')">
                <span class="export-toggle">▾</span>
                <span class="export-label" style="color:var(--text-muted)">更多选项</span>
                <div class="export-children">
                  <div class="export-tree-item">
                    <input type="checkbox" class="export-cb" value="defaultconfigs" v-model="exportKeys" @click.stop>
                    <span class="export-label">默认配置</span>
                    <span class="export-desc">defaultconfigs</span>
                  </div>
                  <div class="export-tree-item">
                    <input type="checkbox" class="export-cb" value="kubejs" v-model="exportKeys" @click.stop>
                    <span class="export-label">KubeJS</span>
                    <span class="export-desc">kubejs 脚本</span>
                  </div>
                  <div class="export-tree-item">
                    <input type="checkbox" class="export-cb" value="journeymap" v-model="exportKeys" @click.stop>
                    <span class="export-label">JourneyMap</span>
                    <span class="export-desc">地图数据</span>
                  </div>
                  <div class="export-tree-item">
                    <input type="checkbox" class="export-cb" value="waystones" v-model="exportKeys" @click.stop>
                    <span class="export-label">Waystones</span>
                    <span class="export-desc">传送点数据</span>
                  </div>
                </div>
              </div>
            </div>
            <div class="export-footer">
              <button class="btn btn-primary btn-lg" @click="startExport()">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:16px;height:16px;margin-right:6px"><path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>
                开始导出
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>
  `
};

window.VersePC = window.VersePC || {};
window.VersePC.PageVersionSettings = PageVersionSettings;

// 外部入口桥接：版本列表等仍通过 openVersionSettings / closeVersionSettings 触发
async function openVersionSettings(versionId, versionName) {
  if (window.VersepcSettingsVM) {
    await window.VersepcSettingsVM.openFor(versionId, versionName);
    return;
  }
  let tries = 0;
  const waitFor = setInterval(() => {
    if (window.VersepcSettingsVM) {
      clearInterval(waitFor);
      window.VersepcSettingsVM.openFor(versionId, versionName);
    } else if (++tries > 60) {
      clearInterval(waitFor);
    }
  }, 50);
}
function closeVersionSettings() {
  if (window.VersepcSettingsVM) {
    window.VersepcSettingsVM.close();
  }
}