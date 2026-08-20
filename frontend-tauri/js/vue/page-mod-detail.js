const PageModDetail = {
  template: `
    <div class="moddetail-page-header">
      <button class="btn btn-icon" onclick="goBackFromDetail()">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:20px;height:20px"><path d="M15 18l-6-6 6-6"/></svg>
      </button>
      <div class="moddetail-top-info">
        <div class="moddetail-icon-wrap">
          <img id="md-icon-img" src="" alt="" onerror="this.style.display='none'">
          <span id="md-icon-fallback" class="icon-fallback" style="display:none"></span>
        </div>
        <div class="moddetail-text-info">
          <h2 id="md-name"></h2>
          <p id="md-desc" class="md-desc"></p>
          <div class="md-stats-row">
            <span id="md-downloads" class="md-stat">\u2B07 0</span>
            <span id="md-followers" class="md-stat">\u2764 0</span>
            <span id="md-updated" class="md-stat"></span>
            <span id="md-source-badge" class="md-source-tag">Modrinth</span>
          </div>
        </div>
      </div>
    </div>
    <div class="md-action-bar">
      <button class="btn btn-primary md-action-btn" id="md-install-btn" onclick="installCurrentDetailVersion()">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:14px;height:14px"><path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
        安装
      </button>
      <button class="btn btn-secondary md-action-btn" id="md-fav-btn" onclick="showFavSelectDropdown(currentModDetailId, this)">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:14px;height:14px"><path d="M20.84 4.61a5.5 5.5 0 00-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 00-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 000-7.78z"/></svg>
        收藏
      </button>
      <button class="btn btn-secondary md-action-btn" onclick="openModSourceUrl()">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:14px;height:14px"><path d="M18 13v6a2 2 0 01-2 2H5a2 2 0 01-2-2V8a2 2 0 012-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/></svg>
        打开源
      </button>
      <button class="btn btn-secondary md-action-btn" onclick="copyModName()">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:14px;height:14px"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1"/></svg>
        复制名称
      </button>
    </div>
    <div id="md-deps-section" class="md-deps-section" style="display:none">
      <div class="md-deps-header">
        <div style="display:flex;align-items:center;gap:8px;flex:1;cursor:pointer" onclick="toggleMdDepsSection()">
          <svg viewBox="0 0 24 24" fill="none" stroke="var(--warning)" stroke-width="2" style="width:18px;height:18px"><path d="M10.29 3.86L1.82 18a2 2 0 001.71 3h16.94a2 2 0 001.71-3L13.71 3.86a2 2 0 00-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
          <span style="font-size:14px;font-weight:600;color:var(--text-primary)">前置模组</span>
          <span id="md-deps-count" style="font-size:12px;color:var(--text-muted)"></span>
        </div>
        <div style="display:flex;align-items:center;gap:8px">
          <button id="md-deps-download-all-btn" class="md-deps-download-all" onclick="event.stopPropagation();downloadAllDeps()" title="一键下载所有前置模组">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" style="width:14px;height:14px"><path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
            <span>一键下载</span>
          </button>
          <svg id="md-deps-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:16px;height:16px;transition:transform 0.2s;cursor:pointer" onclick="event.stopPropagation();toggleMdDepsSection()"><path d="M6 9l6 6 6-6"/></svg>
        </div>
      </div>
      <div id="md-deps-list" class="md-deps-list"></div>
    </div>
    <div class="md-body">
      <div class="md-layout">
        <div class="md-layout-main">
          <div class="md-tabs">
            <button class="md-tab active" data-tab="description" onclick="switchMdTab('description')">描述</button>
            <button class="md-tab" data-tab="versions" onclick="switchMdTab('versions')">版本</button>
            <button class="md-tab" data-tab="gallery" onclick="switchMdTab('gallery')" id="md-gallery-tab" style="display:none">画廊</button>
          </div>
          <div id="md-tab-description" class="md-tab-content active">
            <div id="md-body-content" class="md-body-content"></div>
          </div>
          <div id="md-tab-versions" class="md-tab-content">
            <div class="md-version-tabs" id="md-version-tabs">
              <button class="md-vtab active" data-ver="" onclick="switchMdVersionTab('')">全部</button>
            </div>
            <div class="md-version-list" id="md-version-list">
              <p class="empty-text" style="padding:30px 0;text-align:center;color:var(--text-muted)">选择版本标签查看文件列表</p>
            </div>
          </div>
          <div id="md-tab-gallery" class="md-tab-content">
            <div class="md-gallery" id="md-gallery"></div>
          </div>
        </div>
        <div class="md-layout-sidebar">
          <div class="md-sidebar-section" id="md-compat-section">
            <div class="md-sidebar-section-title">兼容性</div>
            <div id="md-sidebar-loaders" class="md-sidebar-tags"></div>
            <div id="md-sidebar-versions" class="md-sidebar-tags" style="margin-top:8px"></div>
          </div>
          <div class="md-sidebar-section">
            <div class="md-sidebar-section-title">分类</div>
            <div id="md-sidebar-categories" class="md-sidebar-tags"></div>
          </div>
          <div class="md-sidebar-section">
            <div class="md-sidebar-section-title">链接</div>
            <div id="md-sidebar-links" class="md-sidebar-links"></div>
          </div>
          <div class="md-sidebar-section">
            <div class="md-sidebar-section-title">详情</div>
            <div id="md-sidebar-details" class="md-sidebar-details"></div>
          </div>
          <div class="md-sidebar-section">
            <div class="md-sidebar-section-title">许可证</div>
            <div id="md-sidebar-license" class="md-sidebar-details"></div>
          </div>
        </div>
      </div>
    </div>
    <div id="md-gallery-lightbox" class="md-gallery-lightbox" style="display:none" onclick="closeGalleryLightbox()">
      <div class="md-gallery-lightbox-content" onclick="event.stopPropagation()">
        <button class="md-gallery-lightbox-close" onclick="closeGalleryLightbox()">&times;</button>
        <button class="md-gallery-lightbox-nav md-gallery-lightbox-prev" onclick="prevGalleryImage()">&#8249;</button>
        <img id="md-gallery-lightbox-img" src="" alt="" />
        <button class="md-gallery-lightbox-nav md-gallery-lightbox-next" onclick="nextGalleryImage()">&#8250;</button>
        <div class="md-gallery-lightbox-caption" id="md-gallery-lightbox-caption"></div>
      </div>
    </div>
  `
};

window.VersePC = window.VersePC || {};
window.VersePC.PageModDetail = PageModDetail;
