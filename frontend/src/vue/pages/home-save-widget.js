/**
 * home-save-widget.js — 主页「版本存档」卡片
 *
 * 显示当前选中版本的存档列表（最多若干条），点某一条进入该存档的编辑弹窗，
 * 点「打开」直接开文件夹。编辑布局时整卡可拖动。
 *
 * 数据来源：后端命令 version_list_saves（与版本设置页「存档」页签同源）
 */

import { ref, computed, onMounted, onBeforeUnmount } from 'vue';
import { API } from '../../core/api.js';
import { bridge } from '../../core/tauri-bridge.js';
import { showToast } from '../../features/misc/ui-components.js';
import { vmHolders } from '../../core/vm-holders.js';
import { onLaunchVersionChanged } from '../../features/home/home-widgets.js';
import { appState } from '../../core/app-state.js';
import { handleLaunch } from '../../features/launch/launch.js';

export default {
  name: 'HomeSaveWidget',
  props: {
    editing: { type: Boolean, default: false },
    title: { type: String, default: '版本存档' },
  },
  setup(props) {
    const saves = ref([]);
    const loading = ref(true);
    const failed = ref(false);
    const versionId = ref('');
    let timer = null;
    let _offVersion = null;

    const shown = computed(() => saves.value.slice(0, 4));
    const more = computed(() => Math.max(0, saves.value.length - shown.value.length));

    /** 等主页版本卡渲染出当前版本名（最多约 3 秒），拿不到返回空串 */
    function waitCurrentVersion() {
      return new Promise((resolve) => {
        let tries = 0;
        const tick = () => {
          const el = document.querySelector('#home-current-version-card');
          const first = ((el && el.innerText) || '').split('\n')[0].trim();
          if (first && !/^\d+%$/.test(first)) { resolve(first); return; }
          if (++tries > 15) { resolve(''); return; }
          setTimeout(tick, 200);
        };
        tick();
      });
    }

    /**
     * 加载存档。
     * @param {string} [overrideVid] 指定版本（版本卡切换时直接传进来，省一次查询）
     */
    async function load(overrideVid) {
      try {
        let vid = overrideVid || '';
        if (!vid) {
          // 先拿当前启动版本：appState 是运行时的权威来源
          vid = appState.currentLaunchVersionId || '';
        }
        if (!vid) {
          try {
            const s = await API.getSettings();
            vid = (s && s.selectedVersion) || '';
          } catch (e) {}
        }
        if (!vid) {
          // settings 里没存（没手动切过版本）→ 用主页版本卡当前显示的那个。
          // 卡片可能在主页版本卡渲染之前就挂载了，这里轮询等一会儿（最多 3 秒）。
          vid = await waitCurrentVersion();
        }
        versionId.value = vid;
        if (!vid) { saves.value = []; loading.value = false; return; }

        const res = await bridge.invoke('version_list_saves', { versionId: vid });
        saves.value = ((res && res.saves) || []).map((s) => ({
          folder: s.folder || s.name || '',
          name: s.name || s.folder || '未命名',
          icon: s.icon || '',
          lastPlayed: s.lastPlayed || s.mtime || '',
          size: s.size || 0,
        }));
        failed.value = false;
      } catch (e) {
        failed.value = true;
      } finally {
        loading.value = false;
      }
    }

    /**
     * 点存档条目：打开该存档的编辑弹窗。
     *
     * 关键：必须先 vm.openFor(versionId) 把版本设置页初始化到这个版本。
     * 早先只 navigateToPage + switchTab，versionId 还是 null：
     * 标题退化成「版本设置」，loadSaves() 又因 !versionId 直接返回，
     * 于是页面永远显示「暂无存档」。
     */
    async function openSave(s) {
      if (props.editing) return;
      const vid = versionId.value;
      if (!vid) { showToast('还没选择启动版本', 'error'); return; }

      try {
        // 等页面组件挂载（它可能还没被创建过）
        const vm = await waitVersionSettingsVm();
        if (!vm || typeof vm.openFor !== 'function') {
          showToast('版本设置页还没准备好，请稍后重试', 'error');
          return;
        }
        // 1) 初始化到本版本（会重置 savesLoaded 并跳转到该页）
        await vm.openFor(vid, vid);
        // 2) 切到存档页签：watch 会自动 loadSaves（savesLoaded 刚被 openFor 重置）
        if (typeof vm.switchTab === 'function') vm.switchTab('saves');
        // 兜底：若 watch 没触发（activeTab 本来就是 saves），这里补一次
        await new Promise((r) => setTimeout(r, 60));
        if (!vm.savesLoaded && typeof vm.loadSaves === 'function') await vm.loadSaves();
        // 3) 打开对应存档的编辑弹窗
        if (s.folder && typeof vm.openSaveEditor === 'function') {
          const hit = (vm.saves || []).find((x) => (x.folder || x.name) === s.folder);
          if (hit) vm.openSaveEditor(hit);
        }
      } catch (e) {
        console.error('[home-save] 打开存档失败:', e);
        showToast('打开存档失败', 'error');
      }
    }

    /** 等版本设置页组件挂载好（最多约 3 秒） */
    function waitVersionSettingsVm() {
      return new Promise((resolve) => {
        let tries = 0;
        const tick = () => {
          const vm = vmHolders && vmHolders.versionSettings;
          if (vm && typeof vm.openFor === 'function') { resolve(vm); return; }
          if (++tries > 30) { resolve(null); return; }
          setTimeout(tick, 100);
        };
        tick();
      });
    }

    /**
     * 启动并直接进入该存档。
     * 与版本设置页存档卡上的 ▶ 按钮完全等价：设 appState.quickPlayWorld 后走 handleLaunch，
     * 后端拿 quickPlaySingleplayer 变量拼出 --quickPlaySingleplayer <世界名>。
     */
    async function launchSave(s) {
      if (props.editing) return;
      if (!versionId.value) { showToast('还没选择启动版本', 'error'); return; }
      try {
        // 让启动流程以这个版本为准（和版本设置页的 launchSave 一致）
        appState.currentLaunchVersionId = versionId.value;
        appState.quickPlayWorld = s.folder;
        await handleLaunch();
      } catch (e) {
        console.error('[home-save] 启动存档失败:', e);
        showToast('启动失败：' + ((e && e.message) || e), 'error');
      }
    }

    function openFolder() {
      if (props.editing) return;
      if (!versionId.value) return;
      API.openVersionFolder(versionId.value, 'saves').catch(() => {});
    }

    onMounted(() => {
      load();
      timer = setInterval(load, 45000);
      // 版本卡切换版本 → 立刻换成那个版本的存档
      _offVersion = onLaunchVersionChanged((vid) => {
        loading.value = true;
        load(vid);
      });
    });
    onBeforeUnmount(() => {
      if (timer) clearInterval(timer);
      if (_offVersion) _offVersion();
    });

    return { saves, shown, more, loading, failed, versionId, openSave, launchSave, openFolder, reload: load };
  },
  template: `
    <div class="home-side-card" :class="{ 'is-editing': editing }">
      <div class="home-side-card-head">
        <span class="home-side-card-title">{{ title }}</span>
        <span v-if="!editing" class="home-side-card-more" @click.stop="openFolder">存档文件夹 ›</span>
      </div>

      <div v-if="loading" class="home-side-empty">读取中…</div>
      <div v-else-if="failed" class="home-side-empty">
        读取失败，<a href="#" @click.stop.prevent="reload">重试</a>
      </div>
      <div v-else-if="!versionId" class="home-side-empty">还没选择启动版本</div>
      <div v-else-if="shown.length === 0" class="home-side-empty">这个版本还没有存档</div>
      <div v-else class="home-side-list">
        <div v-for="s in shown" :key="s.folder"
             class="home-side-item"
             :title="editing ? '拖动调整位置' : ('打开 ' + s.name)"
             @click.stop="openSave(s)">
          <span class="home-side-item-name">{{ s.name }}</span>
          <button
            v-if="!editing"
            type="button"
            class="home-save-launch-btn"
            :title="'启动并直接进入「' + s.name + '」'"
            @click.stop="launchSave(s)"
          >
            <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
              <polygon points="6 4 20 12 6 20 6 4"/>
            </svg>
          </button>
        </div>
        <div v-if="more > 0" class="home-side-more" @click.stop="openFolder">还有 {{ more }} 个…</div>
      </div>
    </div>
  `,
};
