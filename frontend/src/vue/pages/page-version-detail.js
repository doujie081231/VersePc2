/* page-version-detail - 版本详情页 Vue 组件
 * 通用按钮走 components/verse 语义层；返回用的 .btn-icon 是紧凑图标按钮，保留原样式。
 */
import { selectLoaderCard, confirmInstallVersion } from '../../features/versions/versions-install.js';
import { openFabricApiDetailPicker, backFromFabricApiDetailPicker } from '../../features/versions/versions-loaders.js';
import { navigateToPage } from '../../core/navigation.js';
import VButton from '@/components/verse/VButton.vue';

const PageVersionDetail = {
  components: { VButton },
  setup() {
    return { navigateToPage, selectLoaderCard, openFabricApiDetailPicker, backFromFabricApiDetailPicker, confirmInstallVersion };
  },
  template: `
        <div id="pvd-main" class="page-header detail-header">
          <button class="btn btn-icon" @click="navigateToPage('versions')">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:20px;height:20px"><path d="M15 18l-6-6 6-6"/></svg>
          </button>
          <img id="verdetail-icon" src="" alt="" class="version-detail-icon">
          <div class="verdetail-title-wrap">
            <span id="verdetail-name" class="verdetail-name"></span>
            <span id="verdetail-meta" class="verdetail-meta"></span>
          </div>
        </div>
        <div id="pvd-body" class="verdetail-body">
          <div class="verdetail-section">
            <div class="verdetail-section-title">下载源</div>
            <div class="download-source-list" id="download-source-list">
              <label class="ds-item">
                <input type="radio" name="download-source" value="china-first" checked>
                <span class="ds-name">国内优先（默认）</span>
                <span class="ds-desc">优先使用 BMCLAPI/MCIM 国内镜像，失败后回退官方源</span>
              </label>
              <label class="ds-item">
                <input type="radio" name="download-source" value="auto">
                <span class="ds-name">智能混合</span>
                <span class="ds-desc">优先官方源，加载慢时自动切换国内镜像</span>
              </label>
              <label class="ds-item">
                <input type="radio" name="download-source" value="official-first">
                <span class="ds-name">官方优先</span>
                <span class="ds-desc">先尝试官方源，失败后再回退镜像</span>
              </label>
              <label class="ds-item">
                <input type="radio" name="download-source" value="mojang">
                <span class="ds-name">直连 Mojang</span>
                <span class="ds-desc">只使用 Mojang 官方源，不使用镜像</span>
              </label>
            </div>
          </div>
          <div class="verdetail-section">
            <div class="verdetail-section-title">模组加载器</div>
            <div class="loader-cards" id="loader-cards">
              <div class="loader-card selected" data-loader="" @click="selectLoaderCard('')">
                <div class="loader-card-icon"><img src="img/Grass.png" alt="" style="width:28px;height:28px;image-rendering:pixelated"></div>
                <div class="loader-card-info">
                  <div class="loader-card-name">不安装（原版）</div>
                  <div class="loader-card-desc">纯净的Minecraft，无任何模组加载器</div>
                </div>
                <div class="loader-card-check">✓</div>
              </div>
              <div class="loader-card" data-loader="forge" @click="selectLoaderCard('forge')">
                <div class="loader-card-icon"><img src="img/Anvil.png" alt="" style="width:28px;height:28px;image-rendering:pixelated"></div>
                <div class="loader-card-info">
                  <div class="loader-card-name">Forge</div>
                  <div class="loader-card-desc" id="loader-desc-forge">经典模组加载器，兼容性强</div>
                </div>
                <div class="loader-card-check"></div>
              </div>
              <div class="loader-card" data-loader="neoforge" @click="selectLoaderCard('neoforge')">
                <div class="loader-card-icon"><img src="img/NeoForge.png" alt="" style="width:28px;height:28px;image-rendering:pixelated"></div>
                <div class="loader-card-info">
                  <div class="loader-card-name">NeoForge</div>
                  <div class="loader-card-desc" id="loader-desc-neoforge">新一代模组加载器，性能更优</div>
                </div>
                <div class="loader-card-check"></div>
              </div>
              <div class="loader-card" data-loader="fabric" @click="selectLoaderCard('fabric')">
                <div class="loader-card-icon"><img src="img/Fabric.png" alt="" style="width:28px;height:28px;image-rendering:pixelated"></div>
                <div class="loader-card-info">
                  <div class="loader-card-name">Fabric</div>
                  <div class="loader-card-desc" id="loader-desc-fabric">轻量级模组加载器，更新快</div>
                </div>
                <div class="loader-card-check"></div>
              </div>
              <div class="loader-card" data-loader="optifine" @click="selectLoaderCard('optifine')">
                <div class="loader-card-icon"><img src="img/OptiFabric.png" alt="" style="width:28px;height:28px;image-rendering:pixelated"></div>
                <div class="loader-card-info">
                  <div class="loader-card-name">OptiFine</div>
                  <div class="loader-card-desc" id="loader-desc-optifine">高清修复与性能优化</div>
                </div>
                <div class="loader-card-check"></div>
              </div>
            </div>
            <div id="fabric-api-section-detail" class="verdetail-section" style="display:none;margin-top:12px">
              <div class="verdetail-section-title" style="display:flex;align-items:center;justify-content:space-between">
                <span>Fabric API（可选）</span>
              </div>
              <div class="fabric-api-card-detail" id="fabric-api-card-detail" @click="openFabricApiDetailPicker()" style="padding:12px 16px;border:1px solid var(--border);border-radius:var(--radius-lg);background:var(--bg-secondary);cursor:pointer;display:flex;align-items:center;justify-content:space-between;transition:border-color 0.2s">
                <div style="display:flex;align-items:center;gap:10px;min-width:0">
                  <span style="width:8px;height:8px;border-radius:50%;background:var(--accent);flex-shrink:0"></span>
                  <span id="fabric-api-card-text-detail" style="font-size:14px;color:var(--text-muted);overflow:hidden;text-overflow:ellipsis;white-space:nowrap">不安装</span>
                </div>
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:16px;height:16px;color:var(--text-muted);flex-shrink:0"><polyline points="9 18 15 12 9 6"></polyline></svg>
              </div>
            </div>
            <div id="loader-version-section" style="display:none;margin-top:12px">
              <div class="verdetail-section-title">选择版本</div>
              <div class="loader-version-list" id="loader-version-list">
                <p class="empty-text" style="padding:20px 0;text-align:center;color:var(--text-muted)">加载中...</p>
              </div>
            </div>
          </div>
        </div>
        <div id="fabric-api-picker-detail" style="display:none;position:absolute;top:0;left:0;width:100%;height:100%;padding:24px;z-index:100;background:var(--bg-primary);box-sizing:border-box;display:flex;flex-direction:column">
          <div id="pvd-fabric-header" style="display:flex;align-items:center;gap:10px;margin-bottom:14px">
            <VButton variant="secondary" size="sm" @click="backFromFabricApiDetailPicker()" style="display:flex;align-items:center;gap:4px">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:14px;height:14px"><polyline points="15 18 9 12 15 6"></polyline></svg>
              返回
            </VButton>
            <h4 style="margin:0;font-size:15px;font-weight:600">选择 Fabric API 版本</h4>
          </div>
          <div id="fabric-api-list-detail" style="flex:1;overflow-y:auto;display:flex;flex-direction:column;gap:6px;min-height:0">
            <div style="padding:20px;text-align:center;color:var(--text-muted)">加载中...</div>
          </div>
        </div>
        <div id="pvd-footer" class="verdetail-footer">
          <VButton variant="secondary" @click="navigateToPage('versions')">取消</VButton>
          <VButton variant="accent" size="lg" @click="confirmInstallVersion()">确认下载</VButton>
        </div>
  `
};

export default PageVersionDetail;
