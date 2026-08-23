// ─── 自定义下拉菜单组件 ──────────────────────────────────
class CustomSelect {
    constructor(wrapperId, options = {}) {
        this.wrapper = document.getElementById(wrapperId);
        if (!this.wrapper) return;

        this.trigger = this.wrapper.querySelector('.custom-select-trigger');
        this.valueEl = this.wrapper.querySelector('.custom-select-value');
        this.dropdown = this.wrapper.querySelector('.custom-select-dropdown');
        this.optionsContainer = this.wrapper.querySelector('.custom-select-options');
        this.searchInput = this.wrapper.querySelector('.custom-select-input');
        this.placeholder = this.wrapper.querySelector('.custom-select-value.placeholder');

        this.isOpen = false;
        this.selectedValue = '';
        this.selectedText = '';
        this.allOptions = [];
        this.filteredOptions = [];
        this.onChange = options.onChange || (() => {});
        this._originalParent = this.dropdown ? this.dropdown.parentNode : null;

        this.init();
    }

    init() {
        this.trigger.addEventListener('click', (e) => {
            e.stopPropagation();
            this.toggle();
        });

        if (this.searchInput) {
            this.searchInput.addEventListener('input', (e) => {
                this.filterOptions(e.target.value);
            });
            this.searchInput.addEventListener('click', (e) => {
                e.stopPropagation();
            });
        }

        document.addEventListener('click', (e) => {
            if (this.isOpen && !this.wrapper.contains(e.target) && !this.dropdown.contains(e.target)) {
                this.close();
            }
        }, true);

        document.addEventListener('keydown', (e) => {
            if (e.key === 'Escape' && this.isOpen) this.close();
        });

        window.addEventListener('scroll', () => {
            if (this.isOpen) this.updatePosition();
        }, true);

        window.addEventListener('resize', () => {
            if (this.isOpen) this.updatePosition();
        });
    }

    toggle() {
        this.isOpen ? this.close() : this.open();
    }

    updatePosition() {
        if (!this.trigger || !this.dropdown) return;
        const rect = this.trigger.getBoundingClientRect();
        const vpH = window.innerHeight;
        const vpW = window.innerWidth;
        const ddH = this.dropdown.offsetHeight || 200;

        let top = rect.bottom + 6;
        let left = rect.left;

        if (top + ddH > vpH && rect.top > ddH) {
            top = rect.top - ddH - 6;
        }
        if (top < 4) top = 4;

        if (left + rect.width > vpW) {
            left = vpW - rect.width - 4;
        }
        if (left < 4) left = 4;

        this.dropdown.style.top = Math.round(top) + 'px';
        this.dropdown.style.left = Math.round(left) + 'px';
        this.dropdown.style.width = Math.round(rect.width) + 'px';
    }

    open() {
        this.isOpen = true;
        this.wrapper.classList.add('open');

        document.body.appendChild(this.dropdown);
        this.dropdown.classList.add('custom-select-dropdown-active');

        this.updatePosition();

        if (this.searchInput) {
            setTimeout(() => this.searchInput.focus(), 50);
        }
    }

    close() {
        this.isOpen = false;
        this.wrapper.classList.remove('open');
        this.dropdown.classList.remove('custom-select-dropdown-active');

        this.dropdown.style.top = '';
        this.dropdown.style.left = '';
        this.dropdown.style.width = '';

        if (this._originalParent && this.dropdown.parentNode !== this._originalParent) {
            this._originalParent.appendChild(this.dropdown);
        }

        if (this.searchInput) {
            this.searchInput.value = '';
            this.filterOptions('');
        }
    }

    setOptions(options) {
        this.allOptions = options;
        this.filteredOptions = [...options];
        this.renderOptions();
    }

    filterOptions(query) {
        const q = query.toLowerCase().trim();
        if (!q) {
            this.filteredOptions = [...this.allOptions];
        } else {
            this.filteredOptions = this.allOptions.filter(opt =>
                opt.text.toLowerCase().includes(q) ||
                opt.value.toLowerCase().includes(q)
            );
        }
        this.renderOptions();
    }

    renderOptions() {
        if (!this.optionsContainer) return;

        if (this.filteredOptions.length === 0) {
            this.optionsContainer.innerHTML = '<div class="custom-select-no-results">未找到匹配的版本</div>';
            return;
        }

        const html = this.filteredOptions.map(opt => `
            <div class="custom-select-option ${opt.value === this.selectedValue ? 'selected' : ''}"
                 data-value="${opt.value}">
                ${opt.icon ? `<div class="custom-select-option-icon">${opt.icon}</div>` : ''}
                <div class="custom-select-option-text">
                    <div class="custom-select-option-name">${opt.text}</div>
                    ${opt.subtext ? `<div class="custom-select-option-type">${opt.subtext}</div>` : ''}
                </div>
                <div class="custom-select-option-check">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3">
                        <polyline points="20 6 9 17 4 12"></polyline>
                    </svg>
                </div>
            </div>
        `).join('');

        this.optionsContainer.innerHTML = html;

        this.optionsContainer.querySelectorAll('.custom-select-option').forEach(el => {
            el.addEventListener('click', () => {
                const value = el.dataset.value;
                const opt = this.allOptions.find(o => o.value === value);
                if (opt) {
                    this.select(value, opt.text);
                    this.onChange(value, opt);
                }
            });
        });
    }

    select(value, text) {
        this.selectedValue = value;
        this.selectedText = text;
        this.valueEl.textContent = text || '选择版本...';
        if (this.valueEl) {
            this.valueEl.classList.toggle('placeholder', !text);
        }

        this.optionsContainer.querySelectorAll('.custom-select-option').forEach(el => {
            el.classList.toggle('selected', el.dataset.value === value);
        });

        this.close();
    }

    getValue() {
        return this.selectedValue;
    }

    setValue(value) {
        const opt = this.allOptions.find(o => o.value === value);
        if (opt) {
            this.selectedValue = value;
            this.selectedText = opt.text;
            this.valueEl.textContent = opt.text;
            if (this.valueEl) {
                this.valueEl.classList.toggle('placeholder', !opt.text);
            }
        }
    }
}

let homeVersionCustomSelect = null;
let launchVersionCustomSelect = null;
let modloaderGameVersionCustomSelect = null;
let modloaderVersionCustomSelect = null;

const customSelectInstances = {};

function initAllCustomSelects() {
    if (!customSelectInstances['vset-isolation']) {
        customSelectInstances['vset-isolation'] = new CustomSelect('vset-isolation-wrapper');
        customSelectInstances['vset-isolation'].setOptions([
            { value: 'global', text: '跟随全局设置' },
            { value: 'on', text: '开启' },
            { value: 'off', text: '关闭' }
        ]);
    }

    if (!customSelectInstances['vset-mem-optimize']) {
        customSelectInstances['vset-mem-optimize'] = new CustomSelect('vset-mem-optimize-wrapper');
        customSelectInstances['vset-mem-optimize'].setOptions([
            { value: 'global', text: '跟随全局设置' },
            { value: 'on', text: '开启' },
            { value: 'off', text: '关闭' }
        ]);
    }

    if (!customSelectInstances['mod-filter-loader']) {
        customSelectInstances['mod-filter-loader'] = new CustomSelect('mod-filter-loader-wrapper', {
            onChange: () => loadMods()
        });
        customSelectInstances['mod-filter-loader'].setOptions([
            { value: '', text: '全部' },
            { value: 'fabric', text: 'Fabric' },
            { value: 'forge', text: 'Forge' },
            { value: 'neoforge', text: 'NeoForge' }
        ]);
    }

    if (!customSelectInstances['mod-filter-sort']) {
        customSelectInstances['mod-filter-sort'] = new CustomSelect('mod-filter-sort-wrapper', {
            onChange: () => loadMods()
        });
        customSelectInstances['mod-filter-sort'].setOptions([
            { value: 'relevance', text: '相关度' },
            { value: 'downloads', text: '下载量' },
            { value: 'newest', text: '最新' },
            { value: 'updated', text: '最近更新' }
        ]);
    }

    if (!customSelectInstances['mod-filter-source']) {
        customSelectInstances['mod-filter-source'] = new CustomSelect('mod-filter-source-wrapper', {
            onChange: () => loadMods()
        });
        customSelectInstances['mod-filter-source'].setOptions([
            { value: 'any', text: '全部' },
            { value: 'modrinth', text: 'Modrinth' },
            { value: 'curseforge', text: 'CurseForge' }
        ]);
    }

    if (!customSelectInstances['mod-filter-category']) {
        customSelectInstances['mod-filter-category'] = new CustomSelect('mod-filter-category-wrapper', {
            onChange: () => loadMods()
        });
        customSelectInstances['mod-filter-category'].setOptions([
            { value: '', text: '全部' }
        ]);
    }

    if (!customSelectInstances['mod-filter-version']) {
        customSelectInstances['mod-filter-version'] = new CustomSelect('mod-filter-version-wrapper', {
            onChange: () => loadMods()
        });
        customSelectInstances['mod-filter-version'].setOptions([
            { value: '', text: '全部' }
        ]);
    }

    if (!customSelectInstances['modpack-filter-loader']) {
        customSelectInstances['modpack-filter-loader'] = new CustomSelect('modpack-filter-loader-wrapper', {
            onChange: () => {}
        });
        customSelectInstances['modpack-filter-loader'].setOptions([
            { value: '', text: '全部' },
            { value: 'fabric', text: 'Fabric' },
            { value: 'forge', text: 'Forge' },
            { value: 'neoforge', text: 'NeoForge' },
            { value: 'quilt', text: 'Quilt' }
        ]);
    }

    if (!customSelectInstances['modpack-filter-version']) {
        customSelectInstances['modpack-filter-version'] = new CustomSelect('modpack-filter-version-wrapper');
        customSelectInstances['modpack-filter-version'].setOptions([{ value: '', text: '全部' }]);
    }

    if (!customSelectInstances['modpack-filter-source']) {
        customSelectInstances['modpack-filter-source'] = new CustomSelect('modpack-filter-source-wrapper', {
            onChange: () => {}
        });
        customSelectInstances['modpack-filter-source'].setOptions([
            { value: '', text: '全部' },
            { value: 'modrinth', text: 'Modrinth' },
            { value: 'curseforge', text: 'CurseForge' }
        ]);
    }

    if (!customSelectInstances['datapack-filter-version']) {
        customSelectInstances['datapack-filter-version'] = new CustomSelect('datapack-filter-version-wrapper');
        customSelectInstances['datapack-filter-version'].setOptions([{ value: '', text: '全部' }]);
    }

    if (!customSelectInstances['resourcepack-filter-version']) {
        customSelectInstances['resourcepack-filter-version'] = new CustomSelect('resourcepack-filter-version-wrapper');
        customSelectInstances['resourcepack-filter-version'].setOptions([{ value: '', text: '全部' }]);
    }

    if (!customSelectInstances['resourcepack-filter-resolution']) {
        customSelectInstances['resourcepack-filter-resolution'] = new CustomSelect('resourcepack-filter-resolution-wrapper');
        customSelectInstances['resourcepack-filter-resolution'].setOptions([
            { value: '', text: '全部' },
            { value: '16x', text: '16x' },
            { value: '32x', text: '32x' },
            { value: '64x', text: '64x' },
            { value: '128x', text: '128x' },
            { value: '256x', text: '256x' },
            { value: '512x', text: '512x' }
        ]);
    }

    // 开服页面版本选择器（选项由 serverHostRefreshVersions 动态填充）
    if (!customSelectInstances['server-host-version']) {
        customSelectInstances['server-host-version'] = new CustomSelect('server-host-version-wrapper', {
            onChange: () => { if (typeof serverHostDetectLoader === 'function') serverHostDetectLoader(); }
        });
        customSelectInstances['server-host-version'].setOptions([{ value: '', text: '加载中...' }]);
    }
}

function getCustomSelectValue(id) {
    const instance = customSelectInstances[id];
    return instance ? instance.getValue() : '';
}

function setCustomSelectValue(id, value) {
    const instance = customSelectInstances[id];
    if (instance) instance.setValue(value);
}

function updateCustomSelectOptions(id, options) {
    const instance = customSelectInstances[id];
    if (instance) instance.setOptions(options);
}
