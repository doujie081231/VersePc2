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
import { navigateToPage } from '../../core/navigation.js';
import { vmHolders } from '../../core/vm-holders.js';

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

    async function load() {
      try {
        // 先拿当前启动版本
        let vid = '';
        try {
          const s = await API.getSettings();
          vid = (s && s.selectedVersion) || '';
        } catch (e) {}
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

    function openSave(s) {
      if (props.editing) return;
      // 复用版本设置页：跳过去并打开「存档」页签
      try {
        API.saveSetting('selectedVersion', versionId.value).catch(() => {});
        navigateToPage('version-settings');
        setTimeout(() => {
          const vm = vmHolders && vmHolders.versionSettings;
          if (vm && typeof vm.switchTab === 'function') vm.switchTab('saves');
          if (vm && typeof vm.openSaveEditor === 'function' && s.folder) {
            const hit = (vm.saves || []).find((x) => (x.folder || x.name) === s.folder);
            if (hit) vm.openSaveEditor(hit);
          }
        }, 320);
      } catch (e) {
        showToast('打开存档失败', 'error');
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
    });
    onBeforeUnmount(() => { if (timer) clearInterval(timer); });

    return { saves, shown, more, loading, failed, versionId, openSave, openFolder, reload: load };
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
        </div>
        <div v-if="more > 0" class="home-side-more" @click.stop="openFolder">还有 {{ more }} 个…</div>
      </div>
    </div>
  `,
};
