/* page-installed-versions.js - 已安装版本页 Vue 组件（渐进式改造第三步）
 * 原则：
 *   1. CSS 一行不动（class 名保留原样）
 *   2. HTML 结构原样搬运（标签、层级、id 全部不变）
 *   3. JS 函数全部复用（navigateToPage / addExternalFolder / refreshInstalledVersions
 *      / renderInstalledVersionsInto 仍来自 js/app/*.js）
 */
const PageInstalledVersions = {
  name: 'PageInstalledVersions',
  data() {
    return {
      folders: [],
      selectedFolder: '__internal',
      dropdownOpen: false,
      renamingPath: '',
      renameDraft: ''
    };
  },
  computed: {
    selectedFolderName() {
      if (this.selectedFolder === '__internal') return '游戏文件夹';
      const f = this.folders.find(x => x.path === this.selectedFolder);
      if (!f) return '游戏文件夹';
      return f.name || this._basename(f.path) || '外部文件夹';
    }
  },
  mounted() {
    window.VerseInstalledVM = this;
    this.selectedFolder = getSelectedFolder();
    this.loadFolders();
    document.addEventListener('click', this._onDocClick);
  },
  beforeUnmount() {
    document.removeEventListener('click', this._onDocClick);
  },
  methods: {
    _basename(path) {
      if (!path) return '';
      const parts = String(path).replace(/[\\/]+$/, '').split(/[\\/]/);
      return parts[parts.length - 1] || '';
    },
    _onDocClick(e) {
      const wrap = this.$refs.folderWrap;
      if (wrap && wrap.contains(e.target)) return;
      this.dropdownOpen = false;
      this.renamingPath = '';
    },
    async loadFolders() {
      try {
        const result = await API.listExternalFolders();
        this.folders = (result && result.folders) ? result.folders : [];
      } catch (e) {
        this.folders = [];
      }
      if (this.selectedFolder !== '__internal' && !this.folders.some(x => x.path === this.selectedFolder)) {
        this.selectedFolder = '__internal';
        setSelectedFolder('__internal');
      }
    },
    toggleDropdown() {
      this.dropdownOpen = !this.dropdownOpen;
      if (this.dropdownOpen) this.renamingPath = '';
    },
    selectFolder(path) {
      this.setSelected(path);
      this.dropdownOpen = false;
      this.renamingPath = '';
    },
    setSelected(path) {
      this.selectedFolder = path;
      setSelectedFolder(path);
      const container = document.getElementById('installed-versions-list');
      if (container) renderInstalledVersionsInto(container);
    },
    folderName(f) {
      return f.name || this._basename(f.path) || '外部文件夹';
    },
    folderExtra(f) {
      return (f.versionCount != null && String(f.versionCount) !== '') ? '(' + f.versionCount + ')' : '';
    },
    startRename(f) {
      this.renamingPath = f.path;
      this.renameDraft = f.name || this._basename(f.path) || '';
    },
    cancelRename() {
      this.renamingPath = '';
      this.renameDraft = '';
    },
    async confirmRename(f) {
      const name = (this.renameDraft || '').trim();
      if (!name) {
        showToast('名称不能为空', 'error');
        return;
      }
      try {
        const r = await API.renameExternalFolder(f.path, name);
        if (r.success) {
          showToast('重命名成功', 'success');
          f.name = name;
          this.renamingPath = '';
          this.renameDraft = '';
        } else {
          showToast(r.error || '重命名失败', 'error');
        }
      } catch (e) {
        showToast('重命名失败: ' + (e.message || ''), 'error');
      }
    },
    openFolder(f) {
      if (f.path && API.openInExplorer) API.openInExplorer(f.path);
    },
    async removeFolder(f) {
      const name = f.name || this._basename(f.path) || '该文件夹';
      const confirmed = await showConfirmDialog('移除外部文件夹', `确定要移除 "${name}" 吗？（不会删除实际游戏文件）`, '移除', '取消');
      if (!confirmed) return;
      try {
        const r = await API.removeExternalFolder(f.path);
        if (r.success) {
          showToast('已移除', 'success');
          await this.loadFolders();
          if (this.selectedFolder === f.path) this.setSelected('__internal');
          const container = document.getElementById('installed-versions-list');
          if (container) renderInstalledVersionsInto(container);
        } else {
          showToast(r.error || '移除失败', 'error');
        }
      } catch (e) {
        showToast('移除失败: ' + (e.message || ''), 'error');
      }
    }
  },
  template: `
    <div class="page-header" style="gap:12px;padding:16px 24px;flex-wrap:wrap">
      <button class="btn btn-icon" onclick="navigateToPage('home')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:20px;height:20px"><path d="M15 18l-6-6 6-6"/></svg>
      </button>
      <h2>已安装版本</h2>
      <div id="folder-selector-wrapper" ref="folderWrap" class="folder-dropdown" style="display:flex;align-items:center;gap:6px;">
        <button class="folder-dropdown-toggle" type="button" @click.stop="toggleDropdown()">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:16px;height:16px;opacity:0.7;flex-shrink:0"><path d="M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z"/></svg>
          <span class="folder-dropdown-label" :title="selectedFolder !== '__internal' ? selectedFolder : ''">{{ selectedFolderName }}</span>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="folder-dropdown-caret" :class="{ open: dropdownOpen }" style="width:14px;height:14px;opacity:0.7;flex-shrink:0;transition:transform .2s"><polyline points="6 9 12 15 18 9"/></svg>
        </button>
        <div v-if="dropdownOpen" class="folder-dropdown-menu" @click.stop>
          <div class="folder-dropdown-item" :class="{ selected: selectedFolder==='__internal' }" @click="selectFolder('__internal')">
            <div class="folder-dropdown-main">
              <span class="folder-dropdown-name">游戏文件夹</span>
              <span class="folder-dropdown-path">内部版本目录</span>
            </div>
          </div>
          <div v-for="f in folders" :key="f.path" class="folder-dropdown-item" :class="{ selected: selectedFolder===f.path }" @click="renamingPath!==f.path && selectFolder(f.path)">
            <div class="folder-dropdown-main">
              <span class="folder-dropdown-name">{{ folderName(f) }} <span class="folder-dropdown-count">{{ folderExtra(f) }}</span></span>
              <span class="folder-dropdown-path" :title="f.path">{{ f.path }}</span>
              <div v-if="renamingPath===f.path" class="folder-rename-inline" @click.stop>
                <input v-model="renameDraft" class="folder-rename-input" placeholder="输入名称" @keydown.enter="confirmRename(f)" @keydown.esc="cancelRename()">
                <button class="folder-rename-confirm" type="button" title="确定" @click="confirmRename(f)">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="20 6 9 17 4 12"/></svg>
                </button>
                <button class="folder-rename-cancel" type="button" title="取消" @click="cancelRename()">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
                </button>
              </div>
            </div>
            <div v-if="renamingPath!==f.path" class="folder-dropdown-actions">
              <button type="button" class="folder-action-btn" title="重命名" @click.stop="startRename(f)">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M11 4H4a2 2 0 00-2 2v14a2 2 0 002 2h14a2 2 0 002-2v-7"/><path d="M18.5 2.5a2.121 2.121 0 013 3L12 15l-4 1 1-4 9.5-9.5z"/></svg>
              </button>
              <button type="button" class="folder-action-btn" title="打开文件夹" @click.stop="openFolder(f)">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z"/><line x1="2" y1="13" x2="22" y2="13"/></svg>
              </button>
              <button type="button" class="folder-action-btn folder-action-btn-danger" title="移除" @click.stop="removeFolder(f)">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 01-2 2H7a2 2 0 01-2-2V6m3 0V4a2 2 0 012-2h4a2 2 0 012 2v2"/></svg>
              </button>
            </div>
          </div>
        </div>
      </div>
      <div class="page-actions" style="margin-left:auto;">
        <button class="btn btn-secondary" onclick="addExternalFolder()">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z"/><line x1="12" y1="11" x2="12" y2="17"/><line x1="9" y1="14" x2="15" y2="14"/></svg>
          添加已有文件夹
        </button>
        <button class="btn btn-secondary" onclick="refreshInstalledVersions()">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="23 4 23 10 17 10"/><path d="M20.49 15a9 9 0 11-2.12-9.36L23 10"/></svg>
          刷新
        </button>
      </div>
    </div>
    <div id="installed-versions-list" class="version-list"></div>
  `
};

window.VersePC = window.VersePC || {};
window.VersePC.PageInstalledVersions = PageInstalledVersions;