/* page-skin-library.js — Verse 皮肤库页
 *
 * 数据来源：Verse 皮肤站的受控接口 /api/verse-skins（携带应用级 API Key），
 * 由 Rust 中继 /api/verse-skins/* 取回并适配成本页字段；不再直连站点公开的 /skinlib/*。
 * 卡片预览是皮肤站的 3D 效果模型图 /preview/{tid}，属于公开静态资源。
 * 更换皮肤时复用账户域已有的 /api/upload-skin、/api/ms-skins/import、/api/ms-skins/apply，
 * 保证离线账户与微软账户的写入路径与「导入自定义皮肤」完全一致。
 */
import { apiGet, apiPost } from '../../core/api.js';
import { requestLazyLoad } from '../../core/lazy-loaders.js';
import { showToast, showConfirmDialog } from '../../features/misc/ui-components.js';
import { accountsState } from '../../features/accounts/accounts-state.js';
import { _invalidateAvatarCache, _refreshAccountAvatars } from '../../features/accounts/accounts-avatar.js';
import { switchPage } from '../../features/settings/window-theme.js';
import VButton from '@/components/verse/VButton.vue';

const PREVIEW_BASE = 'https://skin.verselauncher.cn/preview/';

function escapeHtml(value) {
  return String(value == null ? '' : value).replace(/[&<>"']/g, (c) => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;'
  })[c]);
}

const PageSkinLibrary = {
  components: { VButton },
  data() {
    return {
      skins: [],
      page: 1,
      lastPage: 1,
      total: 0,
      loading: false,
      error: '',
      applyingTid: null,
      failedPreviews: {},
      account: null
    };
  },
  computed: {
    accountTypeLabel() {
      if (!this.account) return '';
      return this.account.type === 'microsoft' ? '微软账户' :
        this.account.type === 'offline' ? '离线账户' : '第三方账户';
    }
  },
  mounted() {
    this.onOpen();
    document.addEventListener('verse:skin-library-open', this.onOpen);
  },
  beforeUnmount() {
    document.removeEventListener('verse:skin-library-open', this.onOpen);
  },
  methods: {
    // 每次从账户详情页进入都重新读取账户快照并回到第一页
    onOpen() {
      this.account = accountsState._skinLibraryAccount || null;
      this.load(1);
    },
    previewUrl(tid) {
      return PREVIEW_BASE + tid;
    },
    onPreviewError(tid) {
      this.failedPreviews[tid] = true;
    },
    async load(page) {
      if (this.loading) return;
      const target = Math.max(1, Math.min(Number(page) || 1, this.lastPage));
      this.loading = true;
      this.error = '';
      try {
        const data = await apiGet('/api/verse-skins/list', { page: target });
        if (!data || !data.success) throw new Error((data && data.error) || '皮肤库加载失败');
        this.skins = Array.isArray(data.skins) ? data.skins : [];
        this.page = Number(data.page) || target;
        this.lastPage = Math.max(1, Number(data.lastPage) || 1);
        this.total = Number(data.total) || 0;
        this.failedPreviews = {};
      } catch (e) {
        this.error = (e && e.message) || '皮肤库加载失败';
      } finally {
        this.loading = false;
      }
    },
    goBack() {
      switchPage('accounts');
    },
    async openSkin(skin) {
      if (this.applyingTid !== null) return;
      const acc = accountsState._skinLibraryAccount || this.account;
      if (!acc) {
        showToast('请先从账户详情页进入皮肤库', 'warning');
        return;
      }
      const skinName = skin.name || ('皮肤 #' + skin.tid);
      const message = '确定把账户「' + escapeHtml(acc.name || acc.id) + '」的当前皮肤更换为「' +
        escapeHtml(skinName) + '」吗？<br><span style="font-size:12px;color:var(--text-muted)">' +
        '皮肤将从 Verse 皮肤站下载并立即应用到该账户。</span>';
      const confirmed = await showConfirmDialog('更换当前皮肤', message, '更换皮肤', '取消');
      if (!confirmed) return;

      this.applyingTid = skin.tid;
      showToast('正在下载并应用皮肤…', 'info');
      try {
        // url 由受控列表接口下发，原样回传给 Rust 中继下载纹理；
        // 中继会严格校验必须是皮肤站 /textures/<64位hash>，防止被当任意 URL 代理。
        const texture = await apiPost('/api/verse-skins/texture', {
          tid: skin.tid,
          url: skin.url,
          name: skin.name,
          model: skin.type === 'alex' ? 'slim' : 'default'
        });
        if (!texture || !texture.success || !texture.fileBase64) {
          throw new Error((texture && texture.error) || '获取皮肤纹理失败');
        }
        const payload = {
          accountId: acc.id,
          model: texture.model || 'default',
          fileBase64: texture.fileBase64
        };
        const displayName = texture.name || skinName;
        let result;
        if (acc.type === 'microsoft') {
          const imported = await apiPost('/api/ms-skins/import', {
            ...payload,
            name: displayName
          });
          if (!imported || !imported.success || !imported.skin || !imported.skin.id) {
            throw new Error((imported && imported.error) || '导入本地皮肤库失败');
          }
          result = await apiPost('/api/ms-skins/apply', {
            accountId: acc.id,
            skinId: imported.skin.id
          });
        } else {
          result = await apiPost('/api/upload-skin', payload);
        }
        if (!result || !result.success) {
          throw new Error((result && result.error) || '皮肤应用失败');
        }
        showToast('皮肤已更换为「' + displayName + '」', 'success');
        try {
          _invalidateAvatarCache(acc);
          _refreshAccountAvatars();
          requestLazyLoad('accounts:reload');
        } catch (e) {
          // 头像缓存刷新失败不影响皮肤已写入账户
        }
      } catch (e) {
        showToast((e && e.message) || '皮肤更换失败', 'error');
      } finally {
        this.applyingTid = null;
      }
    }
  },
  template: `
    <div class="flex flex-col gap-4 h-full p-6 overflow-y-auto">
      <div class="flex items-center gap-3 flex-wrap">
        <VButton variant="ghost" size="sm" @click="goBack()">← 返回账户</VButton>
        <h2 class="text-xl font-semibold m-0">Verse 皮肤库</h2>
        <span class="text-sm text-muted-foreground">共 {{ total }} 款皮肤</span>
        <VButton variant="secondary" size="sm" class="ml-auto" :class="loading ? 'opacity-50 pointer-events-none' : ''" @click="load(page)">刷新</VButton>
      </div>

      <div v-if="account" class="text-sm text-muted-foreground">
        当前账户：<span class="text-foreground">{{ account.name || account.id }}</span>（{{ accountTypeLabel }}）· 点击皮肤卡片即可更换
      </div>
      <div v-else class="text-sm text-muted-foreground">
        未指定账户，请从账户详情页点击「皮肤库」进入
      </div>

      <div v-if="loading" class="text-sm text-muted-foreground py-8 text-center">正在从 Verse 皮肤站加载…</div>
      <div v-else-if="error" class="flex flex-col items-center gap-3 py-8">
        <span class="text-sm text-muted-foreground">{{ error }}</span>
        <VButton variant="secondary" size="sm" @click="load(page)">重试</VButton>
      </div>
      <div v-else-if="!skins.length" class="text-sm text-muted-foreground py-8 text-center">这一页暂无公开皮肤</div>
      <div v-else class="grid gap-4" style="grid-template-columns: repeat(auto-fill, minmax(168px, 1fr));">
        <button
          v-for="skin in skins"
          :key="skin.tid"
          type="button"
          class="flex flex-col gap-2 p-3 rounded-lg border border-border bg-card text-left hover:border-primary transition"
          :class="applyingTid === skin.tid ? 'opacity-60' : ''"
          @click="openSkin(skin)"
        >
          <div class="flex items-center justify-center h-44 rounded-md bg-secondary overflow-hidden">
            <img
              v-if="!failedPreviews[skin.tid]"
              :src="previewUrl(skin.tid)"
              :alt="skin.name"
              loading="lazy"
              class="max-h-full max-w-full object-contain"
              @error="onPreviewError(skin.tid)"
            >
            <span v-else class="text-xs text-muted-foreground">预览加载失败</span>
          </div>
          <span class="text-sm font-medium truncate" :title="skin.name">{{ skin.name }}</span>
          <span class="text-xs text-muted-foreground">
            {{ skin.nickname || '匿名' }} · {{ skin.type === 'alex' ? 'Alex' : 'Steve' }} · ♥ {{ skin.likes || 0 }}
          </span>
        </button>
      </div>

      <div class="flex items-center justify-center gap-3 pt-2">
        <VButton variant="secondary" size="sm" :class="page <= 1 || loading ? 'opacity-50 pointer-events-none' : ''" @click="load(page - 1)">上一页</VButton>
        <span class="text-sm text-muted-foreground">第 {{ page }} / {{ lastPage }} 页</span>
        <VButton variant="secondary" size="sm" :class="page >= lastPage || loading ? 'opacity-50 pointer-events-none' : ''" @click="load(page + 1)">下一页</VButton>
      </div>
    </div>
  `
};

export default PageSkinLibrary;
