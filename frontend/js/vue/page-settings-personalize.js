/* page-settings-personalize.js - 个性化设置页 Vue 组件（卡片组件化 + 数据驱动）
 * 结构：
 *   1. 共享响应式状态 window.VersePC.personalizeState（Vue.reactive），供三个卡片组件渲染
 *   2. ThemeAppearanceCard / BackgroundCard / VisualEffectsCard 三个卡片子组件
 *   3. 壳组件 PageSettingsPersonalize：组合卡片 + 保存/重置
 * 原则：
 *   1. CSS 一行不动（class 名保留原样）
 *   2. DOM 结构/id/data-* 属性保留原样（全局选择器依赖 data-theme/data-wallpaper/data-color）
 *   3. 副作用与持久化复用 js/app/*.js 的值参数核心函数（applyXxxByName/applyXxxValue）
 */
(function () {
  'use strict';

  window.VersePC = window.VersePC || {};

  // 共享响应式状态（Vue 已加载；字段默认值与后端 store 默认一致）
  if (!window.VersePC.personalizeState && window.Vue) {
    window.VersePC.personalizeState = window.Vue.reactive({
      theme: 'light',
      wallpaper: 'none',
      customColor: '#4c8dff',
      customLight: false,
      customColorGroupVisible: false,
      glassEffect: false,
      liquidGlass: false,
      panoramaTheme: 'overworld',
      panoramaSpeed: 5,
      panoramaFollow: false,
      opacity: 100,
      blur: 0,
      fit: 'cover',
      customFileName: '未选择',
      weWallpapers: [],
      weWallpaper: null,
      wePickerOpen: false,
      weLoading: false
    });
  }

  // ============== 主题外观卡片 ==============

  const ThemeAppearanceCard = {
    name: 'ThemeAppearanceCard',
    computed: {
      state() { return window.VersePC.personalizeState; }
    },
    methods: {
      pickTheme(theme) {
        if (typeof applyThemeByName === 'function') applyThemeByName(theme);
      },
      onColorInput(e) {
        if (typeof applyCustomThemeColor === 'function') applyCustomThemeColor(e.target.value);
      },
      pickPreset(color) {
        if (typeof applyCustomThemeColor === 'function') applyCustomThemeColor(color);
      },
      async onLightModeChange(checked) {
        const st = this.state;
        if (st) st.customLight = checked;
        if (typeof setCustomThemeMode === 'function') await setCustomThemeMode(checked);
        if (typeof applyCustomThemeColor === 'function') await applyCustomThemeColor(st ? st.customColor : '#4c8dff');
      }
    },
    template: `
          <div class="card">
            <h3>主题外观</h3>
            <div class="form-group">
              <label>主题颜色</label>
              <div class="theme-picker-grid">
                <div class="theme-option" :class="{active: state.theme === 'dark'}" data-theme="dark" @click="pickTheme('dark')">
                  <div class="theme-swatch">
                    <div class="theme-dot" style="background: #ffffff;"></div>
                    <div class="theme-dot" style="background: #d0d0d0;"></div>
                    <div class="theme-dot" style="background: #0a0a0a;"></div>
                  </div>
                  <span class="theme-name">黑色</span>
                </div>
                <div class="theme-option" :class="{active: state.theme === 'light'}" data-theme="light" @click="pickTheme('light')">
                  <div class="theme-swatch">
                    <div class="theme-dot" style="background: #1a1a1a;"></div>
                    <div class="theme-dot" style="background: #333333;"></div>
                    <div class="theme-dot" style="background: #ffffff;"></div>
                  </div>
                  <span class="theme-name">白色</span>
                </div>
                <div class="theme-option" :class="{active: state.theme === 'custom'}" data-theme="custom" @click="pickTheme('custom')">
                  <div class="theme-swatch">
                    <div class="theme-dot" id="custom-theme-swatch-dot" :style="{background: state.customColor}"></div>
                    <div class="theme-dot" style="background: #14171d;"></div>
                    <div class="theme-dot" style="background: #2a2f3a;"></div>
                  </div>
                  <span class="theme-name">自定义</span>
                </div>
              </div>
            </div>
            <div class="form-group" id="custom-theme-color-group" v-show="state.customColorGroupVisible">
              <label>自定义主题色 <span id="custom-theme-color-value" class="custom-theme-color-value">{{ state.customColor }}</span></label>
              <div class="custom-theme-color-row">
                <input type="color" id="custom-theme-color-input" :value="state.customColor" @input="onColorInput">
                <div class="custom-theme-presets">
                  <div class="custom-theme-preset" :class="{active: state.customColor === '#4c8dff'}" data-color="#4c8dff" style="background: #4c8dff;" @click="pickPreset('#4c8dff')" title="湛蓝"></div>
                  <div class="custom-theme-preset" :class="{active: state.customColor === '#3dd68c'}" data-color="#3dd68c" style="background: #3dd68c;" @click="pickPreset('#3dd68c')" title="翠绿"></div>
                  <div class="custom-theme-preset" :class="{active: state.customColor === '#a06cff'}" data-color="#a06cff" style="background: #a06cff;" @click="pickPreset('#a06cff')" title="幽紫"></div>
                  <div class="custom-theme-preset" :class="{active: state.customColor === '#ff8a3d'}" data-color="#ff8a3d" style="background: #ff8a3d;" @click="pickPreset('#ff8a3d')" title="暖橙"></div>
                  <div class="custom-theme-preset" :class="{active: state.customColor === '#ff6b9d'}" data-color="#ff6b9d" style="background: #ff6b9d;" @click="pickPreset('#ff6b9d')" title="樱粉"></div>
                  <div class="custom-theme-preset" :class="{active: state.customColor === '#3dc8d6'}" data-color="#3dc8d6" style="background: #3dc8d6;" @click="pickPreset('#3dc8d6')" title="青碧"></div>
                </div>
              </div>
              <label class="checkbox-label" style="margin-top:10px;">
                <input type="checkbox" id="custom-theme-light-mode" :checked="state.customLight" @change="onLightModeChange($event.target.checked)">
                <span>使用浅色背景</span>
              </label>
              <span class="form-hint">开启后自定义主题将使用白色基底，关闭则为深色基底</span>
            </div>
          </div>
  `
  };

  // ============== 背景卡片 ==============

  const BackgroundCard = {
    name: 'BackgroundCard',
    data() {
      return {
        _previewCache: {},
        _previewLoading: {}
      };
    },
    computed: {
      state() { return window.VersePC.personalizeState; },
      isCustomMode() {
        return this.state.wallpaper === 'customImage' || this.state.wallpaper === 'customVideo';
      },
      isVideoMode() {
        return this.state.wallpaper === 'customVideo';
      },
      isCustomOrAurora() {
        return this.isCustomMode || this.state.wallpaper === 'auroraVideo';
      },
      isWindows() {
        return !!(window.bridge && window.bridge.isWindows);
      }
    },
    methods: {
      pickWallpaper(mode) {
        const st = this.state;
        if (st) st.wallpaper = mode;
        if (typeof applyWallpaperByName === 'function') applyWallpaperByName(mode);
      },
      pickPanoramaTheme(theme) {
        if (typeof applyPanoramaThemeByName === 'function') applyPanoramaThemeByName(theme);
      },
      onPanoramaSpeedInput() {
        if (typeof applyPanoramaSpeedValue === 'function') applyPanoramaSpeedValue(this.state.panoramaSpeed);
      },
      onPanoramaFollowChange(enabled) {
        if (typeof applyPanoramaFollowValue === 'function') applyPanoramaFollowValue(enabled);
      },
      pickFile() {
        if (typeof pickCustomWallpaperFile === 'function') pickCustomWallpaperFile();
      },
      onOpacityInput() {
        if (typeof applyWallpaperOpacityValue === 'function') applyWallpaperOpacityValue(this.state.opacity);
      },
      onBlurInput() {
        if (typeof applyWallpaperBlurValue === 'function') applyWallpaperBlurValue(this.state.blur);
      },
      onFitChange(e) {
        const v = e.target.value;
        if (this.state) this.state.fit = v;
        if (typeof applyWallpaperFitValue === 'function') applyWallpaperFitValue(v);
      },
      onDragOver(e) {
        e.currentTarget.classList.add('drag-over');
      },
      onDragLeave(e) {
        e.currentTarget.classList.remove('drag-over');
      },
      onDrop(e) {
        e.currentTarget.classList.remove('drag-over');
        this.handleDropFile(e);
      },
      async handleDropFile(e) {
        const isVideo = this.state.wallpaper === 'customVideo';
        const file = e.dataTransfer.files && e.dataTransfer.files[0];
        if (!file) return;
        const filePath = (window.electronAPI && window.electronAPI.getDroppedFilePath) ? window.electronAPI.getDroppedFilePath(file) : '';
        if (!filePath) return;

        const validImageExts = ['.jpg', '.jpeg', '.png', '.gif', '.bmp', '.webp'];
        const validVideoExts = ['.mp4', '.webm', '.mkv', '.avi'];
        const ext = '.' + file.name.split('.').pop().toLowerCase();

        if (isVideo && !validVideoExts.includes(ext)) {
          if (typeof showToast === 'function') showToast('请拖放视频文件', 'error');
          return;
        }
        if (!isVideo && !validImageExts.includes(ext)) {
          if (typeof showToast === 'function') showToast('请拖放图片文件', 'error');
          return;
        }

        if (typeof _applyCustomWallpaperFile === 'function') {
          await _applyCustomWallpaperFile(filePath, isVideo);
        }
      },
      // ── Wallpaper Engine 壁纸 ──
      pickWe() {
        const st = this.state;
        st.wePickerOpen = true;
        if (!st.weWallpapers.length) this.loadWe();
      },
      async loadWe() {
        const st = this.state;
        st.weLoading = true;
        try {
          let result = null;
          if (window.bridge && window.bridge.invoke) {
            result = await window.bridge.invoke('wallpaper_engine_list');
          }
          st.weWallpapers = (result && result.wallpapers) || [];
        } catch (e) {
          console.error('[WallpaperEngine] 扫描壁纸失败:', e);
          st.weWallpapers = [];
        } finally {
          st.weLoading = false;
        }
      },
      applyWe(wp) {
        if (typeof applyWeWallpaper === 'function') applyWeWallpaper(wp);
        this.state.wePickerOpen = false;
      },
      typeLabel(t) {
        return ({ video: '视频', web: '网页', scene: '场景', application: '应用' })[t] || t || '未知';
      },
      previewUrl(wp) {
        if (!wp || !wp.previewPath) return '';
        if (this._previewCache[wp.id]) return this._previewCache[wp.id];
        if (this._previewLoading[wp.id]) return '';
        this._previewLoading[wp.id] = true;
        const self = this;
        Promise.resolve(window.bridge && window.bridge.readFileBuffer(wp.previewPath)).then((buffer) => {
          try {
            const u8 = typeof window.decodeFileBuffer === 'function'
              ? window.decodeFileBuffer(buffer)
              : (buffer instanceof Uint8Array ? buffer : new Uint8Array(buffer || []));
            if (!u8 || u8.byteLength === 0) return;
            const ext = String(wp.previewPath).toLowerCase().split('.').pop();
            const mimeMap = { jpg: 'image/jpeg', jpeg: 'image/jpeg', png: 'image/png', gif: 'image/gif', bmp: 'image/bmp', webp: 'image/webp' };
            const blob = new Blob([u8], { type: mimeMap[ext] || 'image/png' });
            self._previewCache[wp.id] = URL.createObjectURL(blob);
          } catch (e) {}
        }).catch(() => {});
        return '';
      }
    },
    template: `
          <div class="card">
            <h3>背景</h3>
            <div class="form-group">
              <label>选择背景风格</label>
              <div class="wallpaper-picker-grid wallpaper-picker-text-only">
                <div class="wallpaper-option" :class="{active: state.wallpaper === 'none'}" data-wallpaper="none" @click="pickWallpaper('none')">
                  <span class="wallpaper-name">无背景</span>
                </div>
                <div class="wallpaper-option" :class="{active: state.wallpaper === 'panorama'}" data-wallpaper="panorama" @click="pickWallpaper('panorama')">
                  <span class="wallpaper-name">MC全景</span>
                </div>
                <div class="wallpaper-option" :class="{active: state.wallpaper === 'customImage'}" data-wallpaper="customImage" @click="pickWallpaper('customImage')">
                  <span class="wallpaper-name">自定义图片</span>
                </div>
                <div class="wallpaper-option" :class="{active: state.wallpaper === 'customVideo'}" data-wallpaper="customVideo" @click="pickWallpaper('customVideo')">
                  <span class="wallpaper-name">自定义视频</span>
                </div>
                <div class="wallpaper-option" :class="{active: state.wallpaper === 'auroraVideo'}" data-wallpaper="auroraVideo" @click="pickWallpaper('auroraVideo')">
                  <span class="wallpaper-name">麦香</span>
                </div>
                <div v-if="isWindows" class="wallpaper-option" :class="{active: state.wallpaper === 'wallpaperEngine'}" data-wallpaper="wallpaperEngine" @click="pickWe">
                  <span class="wallpaper-name">Wallpaper Engine</span>
                </div>
                <span class="form-hint">除 无背景 选项，其他选项均会大量消耗性能与内存。开启后卡顿为正常现象</span>
              </div>
            </div>
            <div v-if="isWindows && state.wePickerOpen" class="we-wallpaper-picker" style="border:1px solid var(--border,#2a2f3a);border-radius:10px;padding:14px;margin-top:12px;background:var(--bg-card,#14171d);">
              <div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:10px;">
                <span style="font-weight:600;">选择 Wallpaper Engine 壁纸</span>
                <button class="btn btn-secondary btn-sm" @click="state.wePickerOpen = false">关闭</button>
              </div>
              <div v-if="state.weLoading" style="padding:16px 0;text-align:center;color:var(--text-muted);font-size:13px;">正在扫描 Steam 创意工坊...</div>
              <div v-else-if="state.weWallpapers.length === 0" style="padding:16px 0;text-align:center;color:var(--text-muted);font-size:13px;">
                <p>未找到 Wallpaper Engine 壁纸。</p>
                <p style="font-size:12px;">请确认已通过 Steam 订阅壁纸（workshop/content/431960）后，点下方按钮重新扫描。</p>
                <button class="btn btn-secondary btn-sm" style="margin-top:10px;" @click="loadWe">重新扫描</button>
              </div>
              <div v-else style="max-height:320px;overflow-y:auto;display:grid;grid-template-columns:repeat(auto-fill,minmax(120px,1fr));gap:10px;">
                <div v-for="wp in state.weWallpapers" :key="wp.id" @click="applyWe(wp)" style="cursor:pointer;border-radius:8px;overflow:hidden;background:var(--bg-secondary,#1a1d24);position:relative;">
                  <div style="position:relative;width:100%;aspect-ratio:16/9;background:#0d0f13;display:flex;align-items:center;justify-content:center;">
                    <img v-if="previewUrl(wp)" :src="previewUrl(wp)" style="width:100%;height:100%;object-fit:cover;" alt="">
                    <span v-else style="color:var(--text-tertiary,#555a66);font-size:12px;">无预览</span>
                    <span style="position:absolute;top:4px;right:4px;font-size:11px;padding:1px 6px;border-radius:8px;background:rgba(0,0,0,0.65);color:#fff;">{{ typeLabel(wp.type) }}</span>
                  </div>
                  <div style="padding:6px 8px;">
                    <div style="font-size:12px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;">{{ wp.title }}</div>
                    <div v-if="wp.author" style="font-size:11px;color:var(--text-muted);white-space:nowrap;overflow:hidden;text-overflow:ellipsis;">{{ wp.author }}</div>
                  </div>
                </div>
              </div>
            </div>
            <div class="form-group" id="panorama-theme-group" v-show="state.wallpaper === 'panorama'">
              <label>全景主题</label>
              <div class="wallpaper-picker-grid wallpaper-picker-text-only">
                <div class="panorama-theme-option" :class="{active: state.panoramaTheme === 'overworld'}" data-theme="overworld" @click="pickPanoramaTheme('overworld')">
                  <span class="wallpaper-name">主世界</span>
                </div>
                <div class="panorama-theme-option" :class="{active: state.panoramaTheme === 'nether'}" data-theme="nether" @click="pickPanoramaTheme('nether')">
                  <span class="wallpaper-name">下界</span>
                </div>
                <div class="panorama-theme-option" :class="{active: state.panoramaTheme === 'wild'}" data-theme="wild" @click="pickPanoramaTheme('wild')">
                  <span class="wallpaper-name">荒野</span>
                </div>
                <div class="panorama-theme-option" :class="{active: state.panoramaTheme === 'end'}" data-theme="end" @click="pickPanoramaTheme('end')">
                  <span class="wallpaper-name">试炼</span>
                </div>
                <div class="panorama-theme-option" :class="{active: state.panoramaTheme === 'darkforest'}" data-theme="darkforest" @click="pickPanoramaTheme('darkforest')">
                  <span class="wallpaper-name">蜜蜂林地</span>
                </div>
                <div class="panorama-theme-option" :class="{active: state.panoramaTheme === 'desert'}" data-theme="desert" @click="pickPanoramaTheme('desert')">
                  <span class="wallpaper-name">地下洞穴</span>
                </div>
                <div class="panorama-theme-option" :class="{active: state.panoramaTheme === 'mountains'}" data-theme="mountains" @click="pickPanoramaTheme('mountains')">
                  <span class="wallpaper-name">山脉</span>
                </div>
                <div class="panorama-theme-option" :class="{active: state.panoramaTheme === 'cherry'}" data-theme="cherry" @click="pickPanoramaTheme('cherry')">
                  <span class="wallpaper-name">樱花</span>
                </div>
                <div class="panorama-theme-option" :class="{active: state.panoramaTheme === 'deep_dark'}" data-theme="deep_dark" @click="pickPanoramaTheme('deep_dark')">
                  <span class="wallpaper-name">苍白森林</span>
                </div>
              </div>
            </div>
            <div class="wallpaper-control-row" id="panoramaSpeedRow" v-show="state.wallpaper === 'panorama'">
              <label>转速</label>
              <input type="range" min="1" max="20" value="5" step="1" id="panoramaSpeedSlider" v-model.number="state.panoramaSpeed" @input="onPanoramaSpeedInput" style="flex:1;margin:0 12px;">
              <span id="panoramaSpeedLabel" style="min-width:20px;text-align:center;">{{ state.panoramaSpeed }}</span>
            </div>
            <div class="wallpaper-control-row" id="panoramaMouseFollowRow" v-show="state.wallpaper === 'panorama'">
              <label>跟随鼠标</label>
              <label class="switch" style="margin-left:12px;">
                <input type="checkbox" id="panoramaMouseFollowToggle" :checked="state.panoramaFollow" @change="onPanoramaFollowChange($event.target.checked)">
                <span class="slider"></span>
              </label>
            </div>
            <div class="form-group" id="custom-wallpaper-file-group" v-show="isCustomMode">
              <label id="custom-wallpaper-file-label">{{ isVideoMode ? '选择视频文件' : '选择图片文件' }}</label>
              <div class="custom-wallpaper-file-row">
                <button class="btn btn-secondary btn-sm" @click="pickFile">选择文件</button>
                <span id="custom-wallpaper-file-name" class="custom-wallpaper-file-name">{{ state.customFileName }}</span>
              </div>
              <div id="custom-wallpaper-drop-zone" class="custom-wallpaper-drop-zone" @dragover.prevent="onDragOver" @dragleave.prevent="onDragLeave" @drop.prevent="onDrop">
                {{ isVideoMode ? '拖放视频到此处' : '拖放图片到此处' }}
              </div>
            </div>
            <div class="form-group" id="wallpaper-opacity-group" v-show="isCustomMode">
              <label>不透明度 <span id="wallpaper-opacity-value">{{ state.opacity }}%</span></label>
              <input type="range" id="wallpaper-opacity-slider" min="0" max="100" value="100" class="wallpaper-slider" v-model.number="state.opacity" @input="onOpacityInput">
            </div>
            <div class="form-group" id="wallpaper-blur-group" v-show="isCustomMode">
              <label>背景模糊 <span id="wallpaper-blur-value">{{ state.blur }}px</span></label>
              <input type="range" id="wallpaper-blur-slider" min="0" max="40" value="0" class="wallpaper-slider" v-model.number="state.blur" @input="onBlurInput">
            </div>
            <div class="form-group" id="wallpaper-fit-group" v-show="isCustomOrAurora">
              <label>自适应方式</label>
              <select id="wallpaper-fit-select" class="custom-select" :value="state.fit" @change="onFitChange">
                <option value="smart">智能</option>
                <option value="center">居中</option>
                <option value="cover">适应</option>
                <option value="stretch">拉伸</option>
                <option value="tile">平铺</option>
                <option value="topLeft">左上</option>
                <option value="topRight">右上</option>
                <option value="bottomLeft">左下</option>
                <option value="bottomRight">右下</option>
              </select>
            </div>
          </div>
  `
  };

  // ============== 视觉效果卡片 ==============

  const VisualEffectsCard = {
    name: 'VisualEffectsCard',
    computed: {
      state() { return window.VersePC.personalizeState; }
    },
    methods: {
      onGlassChange(enabled) {
        if (typeof applyGlassEffectValue === 'function') applyGlassEffectValue(enabled);
      },
      onLiquidChange(enabled) {
        if (typeof applyLiquidGlassEffectValue === 'function') applyLiquidGlassEffectValue(enabled);
      }
    },
    template: `
          <div class="card">
            <h3>视觉效果</h3>
            <div class="form-group">
              <label class="checkbox-label">
                <input type="checkbox" id="setting-glass-effect" :checked="state.glassEffect" @change="onGlassChange($event.target.checked)">
                <span>毛玻璃效果</span>
              </label>
              <span class="form-hint">开启后界面元素将呈现半透明毛玻璃质感，关闭可提升性能</span>
            </div>
            <div class="form-group">
              <label class="checkbox-label">
                <input type="checkbox" id="setting-liquid-glass" :checked="state.liquidGlass" @change="onLiquidChange($event.target.checked)">
                <span>液态玻璃效果</span>
              </label>
              <span class="form-hint">更通透鲜艳的水晶质感，仅作用于标题栏、侧边栏、卡片、弹窗等重要区域</span>
            </div>
          </div>
  `
  };

  // ============== 壳组件 ==============

  const PageSettingsPersonalize = {
    name: 'PageSettingsPersonalize',
    components: {
      ThemeAppearanceCard: ThemeAppearanceCard,
      BackgroundCard: BackgroundCard,
      VisualEffectsCard: VisualEffectsCard
    },
    template: `
          <div class="page-header">
            <h2>个性化设置</h2>
          </div>
          <div class="settings-container">
            <theme-appearance-card></theme-appearance-card>
            <background-card></background-card>
            <visual-effects-card></visual-effects-card>
            <div class="form-actions">
              <button class="btn btn-primary" @click="save">保存设置</button>
              <button class="btn btn-secondary" @click="reset">重置默认</button>
            </div>
          </div>
  `,
    mounted() {
      // 把 store 中保存的设置灌入共享状态（卡片由此渲染当前选中项）
      if (typeof loadPersonalizeStateIntoStore === 'function') {
        loadPersonalizeStateIntoStore();
      }
    },
    methods: {
      save() {
        if (typeof savePersonalizeSettings === 'function') savePersonalizeSettings();
      },
      reset() {
        if (typeof resetPersonalizeSettings === 'function') resetPersonalizeSettings();
      }
    }
  };

  window.VersePC.PageSettingsPersonalize = PageSettingsPersonalize;
})();
