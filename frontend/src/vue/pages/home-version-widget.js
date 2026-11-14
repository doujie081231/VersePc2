/**
 * home-version-widget.js — 主页「版本页面」卡片
 *
 * 显示已安装版本列表（最多若干条），点某一条直接切到该版本，
 * 点右上「全部」跳到版本管理页。编辑布局时整卡可拖动，交互禁用。
 *
 * 数据来源：GET /api/versions?installed=true（只取已安装版本）
 */

import { ref, computed, onMounted, onBeforeUnmount } from 'vue';
import { apiGet } from '../../core/api.js';
import { selectLaunchVersion } from '../../features/versions/versions-list.js';
import { appState } from '../../core/app-state.js';
import { bridge } from '../../core/tauri-bridge.js';
import { navigateToPage } from '../../core/navigation.js';
import { showToast } from '../../features/misc/ui-components.js';

export default {
  name: 'HomeVersionWidget',
  props: {
    editing: { type: Boolean, default: false },
    title: { type: String, default: '版本页面' },
  },
  setup(props) {
    const versions = ref([]);
    const loading = ref(true);
    const failed = ref(false);
    const current = ref('');
    let timer = null;

    const shown = computed(() => versions.value.slice(0, 5));
    const more = computed(() => Math.max(0, versions.value.length - shown.value.length));

    async function load() {
      try {
        // 只取「已安装」的版本：卡片是给人一键启动用的，不是版本管理列表
        const res = await apiGet('/api/versions', { installed: 'true' });
        const list = Array.isArray(res) ? res : (res && res.versions) || [];
        versions.value = list.filter((v) => v && v.id).map((v) => ({
          id: v.id,
          name: v.id,
          loader: v.type || v.loader || '',
          icon: v.icon || '',
        }));
        // 当前启动版本：以 appState 为准（启动实际读的就是它）
        let cur = appState.currentLaunchVersionId || '';
        if (!cur) {
          try { cur = (await bridge.store.get('versepc_last_launch_version')) || ''; } catch (e) {}
        }
        if (!cur) {
          const txt = (document.querySelector('#home-current-version-card') || {}).innerText || '';
          cur = txt.split('\n')[0].trim();
        }
        current.value = cur;
        failed.value = false;
      } catch (e) {
        failed.value = true;
      } finally {
        loading.value = false;
      }
    }

    /** 点某个版本号：立刻切成启动版本（等价于版本页里点选中） */
    async function pick(v) {
      if (props.editing) return;
      try {
        // 先本地立刻反馈，再等落盘完成，避免「刚切完就退出」丢设置
        current.value = v.id;
        const p = selectLaunchVersion(v.id);
        if (p && typeof p.then === 'function') await p;
        // selectLaunchVersion 只管底部选择器，这里再显式写一次兜底
        try { await bridge.store.set('versepc_last_launch_version', v.id); } catch (e) {}
        showToast('已切换到 ' + v.name, 'success');
      } catch (err) {
        console.error('[home-version] 切换失败:', err);
        showToast('切换版本失败', 'error');
      }
    }

    function openAll() {
      if (props.editing) return;
      navigateToPage('versions');
    }

    onMounted(() => {
      load();
      // 每 30 秒刷新一次，装完新版本不用手动刷
      timer = setInterval(load, 30000);
    });
    onBeforeUnmount(() => { if (timer) clearInterval(timer); });

    return { versions, shown, more, loading, failed, current, pick, openAll, reload: load };
  },
  template: `
    <div class="home-side-card" :class="{ 'is-editing': editing }">
      <div class="home-side-card-head">
        <span class="home-side-card-title">{{ title }}</span>
        <span v-if="!editing" class="home-side-card-more" @click.stop="openAll">全部 ›</span>
      </div>

      <div v-if="loading" class="home-side-empty">读取中…</div>
      <div v-else-if="failed" class="home-side-empty">
        读取失败，<a href="#" @click.stop.prevent="reload">重试</a>
      </div>
      <div v-else-if="shown.length === 0" class="home-side-empty">还没有安装版本</div>
      <div v-else class="home-side-list">
        <div v-for="v in shown" :key="v.id"
             class="home-side-item"
             :class="{ 'is-current': v.id === current }"
             :title="editing ? '拖动调整位置' : ('切换到 ' + v.name)"
             @click.stop="pick(v)">
          <span class="home-side-item-name">{{ v.name }}</span>
          <span v-if="v.loader" class="home-side-item-tag">{{ v.loader }}</span>
        </div>
        <div v-if="more > 0" class="home-side-more" @click.stop="openAll">还有 {{ more }} 个…</div>
      </div>
    </div>
  `,
};
