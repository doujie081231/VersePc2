// 版本设置页核心方法：打开/关闭、设置加载保存、Java、下拉、快捷入口、内存、存档与置顶。
import { API } from '../../core/api.js';
import { bridge } from '../../core/tauri-bridge.js';
import { decodeFileBuffer } from '../../core/utils.js';
import { handleLaunch } from '../../features/launch/launch.js';
import { CustomSelect, customSelectInstances } from '../../features/misc/custom-select.js';
import { showToast } from '../../features/misc/ui-components.js';
import { isPinned as isPinnedItem, PIN_ICONS, togglePinSave } from '../../features/versions/pinned-store.js';
import { updateVersionSelects } from '../../features/versions/versions-list.js';
import { navigateToPage } from '../../core/navigation.js';
import { appState } from '../../core/app-state.js';

export const coreMethods = {
    // ===== 打开 / 关闭 =====
    async openFor(versionId, versionName) {
      this.versionId = versionId;
      this.modsLoaded = false;
      this.exportLoaded = false;
      // 存档也属于「跟版本走」的懒加载数据：不重置的话，
      // 打开过 A 版本的存档页后 savesLoaded 一直是 true，
      // 再打开 B 版本时 watch 里的 !this.savesLoaded 不成立，
      // loadSaves() 永远不再执行 → 一直显示「暂无存档」。
      this.savesLoaded = false;
      this.saves = [];
      this.savesLoading = false;
      this.versionId = versionId;
      const versionInfo = (typeof appState.installedVersions !== 'undefined' && appState.installedVersions || []).find(v => v.id === versionId);
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
      if (typeof appState._modDownloadVersionId !== 'undefined') appState._modDownloadVersionId = '';
      const ca = document.querySelector('.content-area');
      if (ca) ca.classList.remove('no-scroll');
      navigateToPage((typeof appState.previousPage !== 'undefined' && appState.previousPage) || 'home');
      this.versionId = null;
      // 清掉本次的存档缓存，下次打开别的版本不会先闪一下上一个版本的存档
      this.savesLoaded = false;
      this.saves = [];
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
      if (typeof appState.installedVersions !== 'undefined' && appState.installedVersions) {
        const vInfo = appState.installedVersions.find(v => v.id === this.versionId);
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
      if (bridge && bridge.showOpenDialog) {
        try {
          const result = await bridge.showOpenDialog({
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
    },
    // ===== 存档管理 =====
    async loadSaves() {
      // 没有 versionId 时不标记 savesLoaded：
      // 否则下次切到存档页会因为 !savesLoaded 不成立而永远不加载。
      if (!this.versionId) return;
      this.savesLoading = true;
      try {
        const res = await bridge.invoke('version_list_saves', { versionId: this.versionId });
        this.saves = (res && res.saves) || [];
        this.savesLoaded = true;
      } catch (e) {
        console.error('[Saves] Load error:', e);
        this.saves = [];
      } finally {
        this.savesLoading = false;
      }
    },
    saveIconUrl(s) {
      if (!s || !s.icon) return '';
      if (this._saveIconCache[s.folder]) return this._saveIconCache[s.folder];
      if (this._saveIconLoading[s.folder]) return '';
      this._saveIconLoading[s.folder] = true;
      const self = this;
      Promise.resolve(bridge && bridge.readFileBuffer(s.icon)).then((buffer) => {
        try {
          const u8 = typeof decodeFileBuffer === 'function'
            ? decodeFileBuffer(buffer)
            : (buffer instanceof Uint8Array ? buffer : new Uint8Array(buffer || []));
          if (!u8 || u8.byteLength === 0) return;
          const blob = new Blob([u8], { type: 'image/png' });
          self._saveIconCache[s.folder] = URL.createObjectURL(blob);
        } catch (e) {}
      }).catch(() => {});
      return '';
    },
    openSaveEditor(s) {
      const rules = {};
      Object.keys(this.ruleDefs).forEach(k => {
        rules[k] = (s.rules && s.rules[k] !== undefined) ? s.rules[k] : String(this.ruleDefs[k].def);
      });
      this.saveEditing = {
        folder: s.folder,
        name: s.name,
        difficulty: (typeof s.difficulty === 'number') ? s.difficulty : 2,
        rules: rules
      };
    },
    closeSaveEditor() {
      this.saveEditing = null;
    },
    ruleBool(rules, key) {
      return String(rules[key]) === 'true';
    },
    toggleRule(rules, key) {
      rules[key] = String(rules[key]) === 'true' ? 'false' : 'true';
    },
    async saveSaveEdit() {
      const ed = this.saveEditing;
      if (!ed) return;
      const changed = {};
      Object.keys(this.ruleDefs).forEach(k => {
        if (ed.rules[k] !== undefined) changed[k] = ed.rules[k];
      });
      try {
        const res = await bridge.invoke('version_save_update', {
          versionId: this.versionId,
          folder: ed.folder,
          displayName: ed.name,
          difficulty: ed.difficulty,
          rules: changed
        });
        if (res && res.success) {
          showToast('存档已保存', 'success');
          this.closeSaveEditor();
          this.loadSaves();
        } else {
          showToast((res && res.error) || '保存失败', 'error');
        }
      } catch (e) {
        showToast('保存失败: ' + (e && e.message ? e.message : String(e)), 'error');
      }
    },
    launchSave(s) {
      if (!this.versionId) return;
      // 让启动流程以当前配置的版本为准，并带上世界名直接进存档
      if (typeof appState.currentLaunchVersionId !== 'undefined') appState.currentLaunchVersionId = this.versionId;
      if (typeof appState.quickPlayWorld === 'undefined') {
        showToast('启动入口未就绪', 'error');
        return;
      }
      appState.quickPlayWorld = s.folder;
      if (typeof handleLaunch === 'function') {
        handleLaunch();
      } else {
        showToast('启动入口未就绪', 'error');
      }
    },
    isPinned(type, id) {
      return isPinnedItem(type, id);
    },
    ic(name) {
      return PIN_ICONS[name] || '';
    },
    pinSave(s) {
      togglePinSave(s.folder, s.name, this.versionId, s.icon || '', s.difficulty);
    },
    difficultyLabel(d) {
      return { 0: '和平', 1: '简单', 2: '普通', 3: '困难' }[d] || '未知';
    },
    lastPlayedText(ms) {
      if (!ms) return '从未游玩';
      const d = new Date(ms);
      const p = n => String(n).padStart(2, '0');
      return `${d.getFullYear()}/${p(d.getMonth() + 1)}/${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
    }
};
