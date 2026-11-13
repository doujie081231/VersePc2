/**
 * home-download-widget.js — 主页「下载」控件
 *
 * 形态：迷你卡（文件名 + 进度条 + 主按钮），点击「展开」进入完整表单。
 * 行为：
 *   - 只填表，绝不自动开始下载（浏览器接管的安全底线）
 *   - 收到 download-request 事件时自动展开并高亮
 *   - editing 为 true 时全部交互禁用，交给 page-home 的拖动逻辑
 *
 * 依赖：features/versions/custom-download.js（业务）、features/home/home-widgets.js（事件）
 */

import { ref, onMounted, onBeforeUnmount, computed } from 'vue';
import VButton from '@/components/verse/VButton.vue';
import VInput from '@/components/verse/VInput.vue';
import {
  customDlState,
  onCustomDownloadChange,
  startCustomDownload,
  cancelCustomDownload,
  openCustomDlFolder,
  browseCustomDlPath,
  ensureCustomDlFolder,
  pauseCustomDownload,
  resumeCustomDownload,
  openExtensionStore,
} from '../../features/versions/custom-download.js';
import { subscribeDownloadRequest } from '../../features/home/home-widgets.js';
import { showToast } from '../../features/misc/ui-components.js';

export default {
  name: 'HomeDownloadWidget',
  components: { VButton, VInput },
  props: {
    editing: { type: Boolean, default: false },
    title: { type: String, default: '下载' },
  },
  setup(props) {
    const expanded = ref(false);
    const url = ref('');
    const fileName = ref('');
    const savePath = ref('');
    const highlighted = ref(false);
    let _offState = null;
    let _offRequest = null;
    let _hlTimer = null;

    // ── 派生状态 ──
    const status = computed(() => customDlState.status || '');
    const running = computed(() => status.value === 'running');
    const paused = computed(() => status.value === 'paused');
    // 下载中/暂停中 = 已经有任务在跑（此时才显示「暂停/继续」）
    const active = computed(() => running.value || paused.value);
    const hasTask = computed(() => !!customDlState.sessionId || status.value === 'pending');
    const progress = computed(() => Math.max(0, Math.min(100, Math.round(customDlState.progress || 0))));
    const displayName = computed(() => customDlState.fileName || fileName.value || '尚未选择文件');
    const statusText = computed(() => {
      const s = status.value;
      if (s === 'running') {
        const speed = customDlState.speed ? formatSpeed(customDlState.speed) : '';
        return speed ? `${progress.value}% · ${speed}` : `${progress.value}%`;
      }
      if (s === 'paused') return '已暂停';
      if (s === 'completed') return '已完成';
      if (s === 'failed') return '失败';
      if (s === 'cancelled') return '已取消';
      if (s === 'pending') return '待确认';
      return '未开始';
    });
    const canStart = computed(() => {
      if (props.editing) return false;
      return !running.value && !!url.value.trim();
    });

    function formatSpeed(bytesPerSec) {
      if (!bytesPerSec) return '';
      const units = ['B/s', 'KB/s', 'MB/s', 'GB/s'];
      let v = bytesPerSec, i = 0;
      while (v >= 1024 && i < units.length - 1) { v /= 1024; i++; }
      return `${v.toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
    }

    // ── 状态同步 ──
    function syncFromState(s) {
      if (!s) return;
      if (s.url && !url.value) url.value = s.url;
      if (s.fileName) fileName.value = s.fileName;
      if (s.savePath) savePath.value = s.savePath;
    }

    // ── 交互 ──
    function toggleExpand() {
      if (props.editing) return;
      expanded.value = !expanded.value;
      if (expanded.value) ensureFolder();
    }

    async function ensureFolder() {
      const dir = await ensureCustomDlFolder();
      if (dir && !savePath.value) savePath.value = dir;
    }

    async function pickFolder() {
      if (props.editing) return;
      await browseCustomDlPath();
      if (savePath.value) return;
      await ensureFolder();
    }

    async function onStart() {
      if (props.editing) return;
      const u = url.value.trim();
      if (!u) { showToast('请先填写下载地址', 'error'); return; }
      if (!savePath.value.trim()) {
        const dir = await ensureCustomDlFolder();
        if (!dir) { showToast('请选择保存位置', 'error'); return; }
        savePath.value = dir;
      }
      await startCustomDownload({
        url: u,
        savePath: savePath.value.trim(),
        fileName: fileName.value.trim(),
      });
    }

    /** 一个按钮两态：下载中显示「暂停」，暂停时显示「继续」 */
    async function onTogglePause() {
      if (props.editing) return;
      if (paused.value) await resumeCustomDownload();
      else await pauseCustomDownload();
    }

    async function onCancel() {
      if (props.editing) return;
      await cancelCustomDownload();
      // 组件内用的是本地 ref，清掉才能让输入框同步变空
      url.value = '';
      fileName.value = '';
      expanded.value = true;   // 取消后保持展开，方便直接填新地址
    }

    function onOpenFolder() {
      if (props.editing) return;
      openCustomDlFolder();
    }

    // ── 浏览器接管 ──
    function flash() {
      highlighted.value = true;
      if (_hlTimer) clearTimeout(_hlTimer);
      _hlTimer = setTimeout(() => { highlighted.value = false; }, 2600);
    }

    function applyRequest(req) {
      if (!req || !req.url) return;
      url.value = req.url;
      fileName.value = req.filename || '';
      if (req.folder) savePath.value = req.folder;
      expanded.value = true;
      flash();
      ensureFolder();
    }

    onMounted(() => {
      _offState = onCustomDownloadChange(syncFromState);
      _offRequest = subscribeDownloadRequest(applyRequest);
      // 进入主页时补齐上次目录（不阻塞渲染）
      ensureFolder();
    });

    onBeforeUnmount(() => {
      if (_offState) _offState();
      if (_offRequest) _offRequest();
      if (_hlTimer) clearTimeout(_hlTimer);
    });

    return {
      expanded, url, fileName, savePath, highlighted,
      status, running, paused, active, hasTask, progress, displayName, statusText, canStart,
      toggleExpand, pickFolder, onStart, onCancel, onOpenFolder, onTogglePause, openExtensionStore,
    };
  },
  template: `
    <div class="home-download-card" :class="{ 'is-expanded': expanded, 'is-highlight': highlighted, 'is-editing': editing }">
      <div class="home-download-head" @click.stop="toggleExpand">
        <svg class="home-download-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/>
          <polyline points="7 10 12 15 17 10"/>
          <line x1="12" y1="15" x2="12" y2="3"/>
        </svg>
        <div class="home-download-info">
          <span class="home-download-name" :title="displayName">{{ displayName }}</span>
          <span class="home-download-status">{{ statusText }}</span>
        </div>
        <button
          v-if="!editing"
          class="home-download-ext-btn"
          type="button"
          title="安装浏览器扩展：装好后在浏览器里点下载会自动填好"
          @click.stop="openExtensionStore"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M12 2v6M12 2l-3 3M12 2l3 3"/>
            <rect x="3" y="9" width="18" height="12" rx="2"/>
            <path d="M8 15h8"/>
          </svg>
        </button>
        <svg class="home-download-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M6 9l6 6 6-6"/>
        </svg>
      </div>

      <div class="home-download-bar" v-if="hasTask">
        <div class="home-download-bar-fill" :style="{ width: progress + '%' }"></div>
      </div>

      <div class="home-download-body" v-if="expanded">
        <label class="home-download-field">
          <span>下载地址</span>
          <VInput v-model="url" type="text" placeholder="https://example.com/file.zip" :disabled="editing || active" />
        </label>
        <label class="home-download-field">
          <span>保存到</span>
          <div class="home-download-folder">
            <VInput v-model="savePath" type="text" placeholder="点击右侧选择" :disabled="editing || active" />
            <VButton variant="secondary" size="sm" @click.stop="pickFolder">选择</VButton>
          </div>
        </label>
        <label class="home-download-field">
          <span>文件名</span>
          <VInput v-model="fileName" type="text" placeholder="留空则按地址自动推断" :disabled="editing || active" />
        </label>

        <div class="home-download-actions">
          <!-- 未开始：开始下载 -->
          <VButton v-if="!active" variant="accent" size="sm" :disabled="!canStart" @click.stop="onStart">开始下载</VButton>
          <!-- 下载中/暂停中：取消 + 打开文件夹 + 暂停/继续 -->
          <template v-else>
            <VButton variant="secondary" size="sm" @click.stop="onCancel">取消</VButton>
            <VButton variant="secondary" size="sm" @click.stop="onOpenFolder">打开文件夹</VButton>
            <VButton variant="secondary" size="sm" @click.stop="onTogglePause">{{ paused ? '继续' : '暂停' }}</VButton>
          </template>
          <VButton v-if="!active" variant="secondary" size="sm" @click.stop="onOpenFolder">打开文件夹</VButton>
          <VButton v-if="!active" variant="ghost" size="sm" @click.stop="openExtensionStore">安装扩展</VButton>
        </div>
      </div>
    </div>
  `,
};
