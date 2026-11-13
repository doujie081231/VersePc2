// @vitest-environment jsdom
// custom-select.test.js — 自定义下拉组件交互与全局选择器注册单元测试
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { installTauriMock } from '../../test-utils/tauri-mock.js';

let CustomSelect;
let customSelectInstances;
let getCustomSelectValue;
let setCustomSelectValue;
let updateCustomSelectOptions;
let initAllCustomSelects;

const OPTIONS = [
  { value: 'fabric', text: 'Fabric', subtext: '轻量' },
  { value: 'forge', text: 'Forge' },
  { value: 'neoforge', text: 'NeoForge', icon: '<b>N</b>' },
];

function buildWrapper(id = 'test-wrapper') {
  const wrapper = document.createElement('div');
  wrapper.id = id;
  wrapper.innerHTML = `
    <div class="custom-select-trigger">
      <span class="custom-select-value placeholder">选择版本...</span>
    </div>
    <div class="custom-select-dropdown">
      <div class="custom-select-options"></div>
      <input class="custom-select-input" />
    </div>
  `;
  document.body.appendChild(wrapper);
  return wrapper;
}

function create(wrapperId = 'test-wrapper', options = {}) {
  return new CustomSelect(wrapperId, options);
}

beforeAll(async () => {
  installTauriMock();
  vi.resetModules();
  const mod = await import('./custom-select.js');
  CustomSelect = mod.CustomSelect;
  customSelectInstances = mod.customSelectInstances;
  getCustomSelectValue = mod.getCustomSelectValue;
  setCustomSelectValue = mod.setCustomSelectValue;
  updateCustomSelectOptions = mod.updateCustomSelectOptions;
  initAllCustomSelects = mod.initAllCustomSelects;
});

beforeEach(() => {
  installTauriMock();
  Object.keys(customSelectInstances).forEach((key) => { delete customSelectInstances[key]; });
  document.body.innerHTML = '';
});

afterEach(() => {
  vi.restoreAllMocks();
  document.body.innerHTML = '';
});

describe('CustomSelect 基础行为', () => {
  it('容器不存在时静默返回', () => {
    expect(() => create('missing-wrapper')).not.toThrow();
  });

  it('setOptions 渲染选项、子文本与图标', () => {
    buildWrapper();
    const select = create();
    select.setOptions(OPTIONS);

    const items = document.querySelectorAll('.custom-select-option');
    expect(items.length).toBe(3);
    expect(items[0].dataset.value).toBe('fabric');
    expect(items[0].querySelector('.custom-select-option-name').textContent).toBe('Fabric');
    expect(items[0].textContent).toContain('轻量');
    expect(items[2].querySelector('.custom-select-option-icon').innerHTML).toBe('<b>N</b>');
  });

  it('setOptions 后 setValue 会更新触发器文字与选中态', () => {
    buildWrapper();
    const select = create();
    select.setOptions(OPTIONS);

    select.setValue('forge');

    const value = document.querySelector('.custom-select-value');
    expect(value.textContent).toBe('Forge');
    expect(value.classList.contains('placeholder')).toBe(false);
    expect(select.getValue()).toBe('forge');
    const selected = document.querySelector('.custom-select-option.selected');
    expect(selected?.dataset.value).toBe('forge');
  });

  it('点击选项更新值并回调 onChange', () => {
    buildWrapper();
    const onChange = vi.fn();
    const select = create('test-wrapper', { onChange });
    select.setOptions(OPTIONS);

    document.querySelectorAll('.custom-select-option')[1].click();

    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange.mock.calls[0][0]).toBe('forge');
    expect(onChange.mock.calls[0][1]).toMatchObject({ value: 'forge', text: 'Forge' });
    expect(select.getValue()).toBe('forge');
    expect(document.querySelector('.custom-select-value').textContent).toBe('Forge');
    expect(select.isOpen).toBe(false);
  });

  it('setValue 支持有效值与无效值', () => {
    buildWrapper();
    const select = create();
    select.setOptions(OPTIONS);

    select.setValue('fabric');
    expect(select.getValue()).toBe('fabric');
    expect(document.querySelector('.custom-select-value').textContent).toBe('Fabric');

    select.setValue('unknown');
    expect(select.getValue()).toBe('fabric');
  });

  it('select 传空文本时回退占位文案', () => {
    buildWrapper();
    const select = create();
    select.setOptions([{ value: '', text: '全部' }]);

    select.select('', '');
    const valueEl = document.querySelector('.custom-select-value');
    expect(valueEl.textContent).toBe('选择版本...');
    expect(valueEl.classList.contains('placeholder')).toBe(true);
  });

  it('搜索过滤按文本与值匹配并展示空态', () => {
    buildWrapper();
    const select = create();
    select.setOptions(OPTIONS);
    const input = document.querySelector('.custom-select-input');

    input.value = 'forge';
    input.dispatchEvent(new Event('input'));
    expect(document.querySelectorAll('.custom-select-option').length).toBe(2);

    input.value = 'nothing';
    input.dispatchEvent(new Event('input'));
    expect(document.querySelector('.custom-select-no-results').textContent).toContain('未找到匹配的版本');

    input.value = '';
    input.dispatchEvent(new Event('input'));
    expect(document.querySelectorAll('.custom-select-option').length).toBe(3);
  });
});

describe('CustomSelect 展开与定位', () => {
  it('点击触发器展开并挂载到 body，再次点击关闭还原', () => {
    const wrapper = buildWrapper();
    const select = create();
    select.setOptions(OPTIONS);
    const trigger = wrapper.querySelector('.custom-select-trigger');
    const dropdown = wrapper.querySelector('.custom-select-dropdown');

    trigger.click();

    expect(select.isOpen).toBe(true);
    expect(wrapper.classList.contains('open')).toBe(true);
    expect(dropdown.parentElement).toBe(document.body);
    expect(dropdown.classList.contains('custom-select-dropdown-active')).toBe(true);

    trigger.click();

    expect(select.isOpen).toBe(false);
    expect(wrapper.classList.contains('open')).toBe(false);
    expect(dropdown.parentElement).toBe(wrapper);
    expect(dropdown.classList.contains('custom-select-dropdown-active')).toBe(false);
  });

  it('点击组件外部关闭，内部点击不关闭', () => {
    const wrapper = buildWrapper();
    const select = create();
    const trigger = wrapper.querySelector('.custom-select-trigger');
    trigger.click();
    expect(select.isOpen).toBe(true);

    trigger.click(); // 先关闭
    trigger.click(); // 再打开
    wrapper.click();
    expect(select.isOpen).toBe(true);

    document.body.click();
    expect(select.isOpen).toBe(false);
  });

  it('Esc 关闭下拉', () => {
    buildWrapper();
    const select = create();
    select.open();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    expect(select.isOpen).toBe(false);
  });

  it('展开时按触发器位置计算下拉坐标', () => {
    const wrapper = buildWrapper();
    const select = create();
    const trigger = wrapper.querySelector('.custom-select-trigger');
    const dropdown = wrapper.querySelector('.custom-select-dropdown');
    trigger.getBoundingClientRect = () => ({ top: 100, bottom: 130, left: 10, width: 200 });

    select.open();

    expect(dropdown.style.top).toBe('136px');
    expect(dropdown.style.left).toBe('10px');
    expect(dropdown.style.width).toBe('200px');
  });

  it('空间不足时向上翻转并对左侧越界做修正', () => {
    const wrapper = buildWrapper();
    const select = create();
    const trigger = wrapper.querySelector('.custom-select-trigger');
    const dropdown = wrapper.querySelector('.custom-select-dropdown');
    trigger.getBoundingClientRect = () => ({ top: 700, bottom: 730, left: 900, width: 200 });

    select.open();

    expect(dropdown.style.top).toBe('494px');
    expect(dropdown.style.left).toBe('820px');
  });

  it('滚动与窗口缩放会重新定位', () => {
    const wrapper = buildWrapper();
    const select = create();
    const trigger = wrapper.querySelector('.custom-select-trigger');
    const dropdown = wrapper.querySelector('.custom-select-dropdown');
    let top = 100;
    trigger.getBoundingClientRect = () => ({ top, bottom: top + 30, left: 10, width: 200 });
    select.open();
    expect(dropdown.style.top).toBe('136px');

    top = 200;
    window.dispatchEvent(new Event('scroll'));
    expect(dropdown.style.top).toBe('236px');

    top = 300;
    window.dispatchEvent(new Event('resize'));
    expect(dropdown.style.top).toBe('336px');
  });

  it('关闭时清空搜索并恢复完整列表', () => {
    buildWrapper();
    const select = create();
    select.setOptions(OPTIONS);
    select.open();
    const input = document.querySelector('.custom-select-input');
    input.value = 'fabric';
    input.dispatchEvent(new Event('input'));
    expect(document.querySelectorAll('.custom-select-option').length).toBe(1);

    select.close();

    expect(input.value).toBe('');
    expect(document.querySelectorAll('.custom-select-option').length).toBe(3);
  });
});

describe('全局选择器注册', () => {
  it('只为存在的容器创建实例，并写入默认选项', () => {
    buildWrapper('vset-isolation-wrapper');
    buildWrapper('mod-filter-loader-wrapper');

    initAllCustomSelects();

    expect(customSelectInstances['vset-isolation']).toBeTruthy();
    expect(customSelectInstances['vset-isolation'].allOptions.length).toBe(3);
    expect(customSelectInstances['mod-filter-loader'].allOptions).toEqual([
      { value: '', text: '全部' },
      { value: 'fabric', text: 'Fabric' },
      { value: 'forge', text: 'Forge' },
      { value: 'neoforge', text: 'NeoForge' },
    ]);
    expect(getCustomSelectValue('vset-isolation')).toBe('');
  });

  it('重复初始化不会替换已有实例', () => {
    buildWrapper('vset-isolation-wrapper');
    initAllCustomSelects();
    const first = customSelectInstances['vset-isolation'];

    initAllCustomSelects();

    expect(customSelectInstances['vset-isolation']).toBe(first);
  });

  it('容器晚于初始化出现时重建失效实例', () => {
    // 启动时插件页面尚未挂载，构造拿不到容器，实例处于失效状态
    initAllCustomSelects();
    expect(customSelectInstances['server-host-version'].valueEl).toBeFalsy();

    buildWrapper('server-host-version-wrapper');
    initAllCustomSelects();

    const inst = customSelectInstances['server-host-version'];
    expect(inst.wrapper.id).toBe('server-host-version-wrapper');
    expect(inst.valueEl).toBeTruthy();
    expect(() => inst.setValue('')).not.toThrow();
  });

  it('setCustomSelectValue / updateCustomSelectOptions 仅作用于已注册实例', () => {
    buildWrapper('mod-filter-loader-wrapper');
    initAllCustomSelects();

    setCustomSelectValue('mod-filter-loader', 'fabric');
    expect(getCustomSelectValue('mod-filter-loader')).toBe('fabric');

    updateCustomSelectOptions('mod-filter-loader', [{ value: 'quilt', text: 'Quilt' }]);
    expect(customSelectInstances['mod-filter-loader'].allOptions).toEqual([{ value: 'quilt', text: 'Quilt' }]);

    expect(() => setCustomSelectValue('missing', 'x')).not.toThrow();
    expect(() => updateCustomSelectOptions('missing', [])).not.toThrow();
    expect(getCustomSelectValue('missing')).toBe('');
  });

  it('注册实例切换选项时触发对应刷新动作', () => {
    buildWrapper('mod-filter-loader-wrapper');
    initAllCustomSelects();
    const select = customSelectInstances['mod-filter-loader'];
    select.setOptions([{ value: 'fabric', text: 'Fabric' }]);
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

    document.querySelector('.custom-select-option').click();

    expect(select.getValue()).toBe('fabric');
    expect(warnSpy).toHaveBeenCalledWith('[lazy-loaders] 加载器尚未注册:', 'mods:refresh');
  });
});
