// @vitest-environment jsdom
// home-save-widget.test.js — 主页「版本存档」卡片
//
// 重点锁住一个真实 bug：点存档条目时必须先 vm.openFor(versionId) 初始化版本设置页。
// 早先只 navigateToPage + switchTab，versionId 仍是 null，
// 标题退化成「版本设置」、loadSaves() 因 !versionId 直接返回，页面永远「暂无存档」。

import { beforeEach, describe, expect, it, vi } from 'vitest';

const VERSION_ID = '1.21.1-Fabric-0.20.1';

const invoked = [];
vi.mock('../../core/tauri-bridge.js', () => ({
  bridge: {
    invoke: vi.fn(async (cmd, args) => {
      invoked.push([cmd, args]);
      if (cmd === 'version_list_saves') {
        return { success: true, saves: [{ folder: '新的世界', name: '新的世界' }] };
      }
      return {};
    }),
    store: { get: async () => VERSION_ID, set: async () => undefined },
  },
}));
vi.mock('../../core/api.js', () => ({
  API: {
    getSettings: async () => ({ selectedVersion: VERSION_ID }),
    saveSetting: async () => undefined,
    openVersionFolder: async () => undefined,
  },
}));
vi.mock('../../core/navigation.js', () => ({ navigateToPage: vi.fn() }));
vi.mock('../../features/misc/ui-components.js', () => ({ showToast: vi.fn() }));
vi.mock('../../features/launch/launch.js', () => ({ handleLaunch: vi.fn(async () => undefined) }));

const openFor = vi.fn(async () => undefined);
const switchTab = vi.fn();
const loadSaves = vi.fn(async () => undefined);
const openSaveEditor = vi.fn();

vi.mock('../../core/vm-holders.js', () => ({
  vmHolders: {
    get versionSettings() {
      return { openFor, switchTab, loadSaves, openSaveEditor, saves: [{ folder: '新的世界', name: '新的世界' }], savesLoaded: true };
    },
  },
}));

const { default: HomeSaveWidget } = await import('./home-save-widget.js');

/** 直接调 setup()，拿到暴露给模板的方法 */
function setupWidget(editing = false) {
  return HomeSaveWidget.setup({ editing });
}

beforeEach(() => {
  invoked.length = 0;
  openFor.mockClear(); switchTab.mockClear(); loadSaves.mockClear(); openSaveEditor.mockClear();
});

describe('存档卡片 openSave', () => {
  it('点存档必须先 openFor(versionId)，再切页签', async () => {
    const vm = setupWidget();
    // 等挂载时的初始 load 完成
    await vm.reload();

    await vm.openSave({ folder: '新的世界', name: '新的世界' });

    expect(openFor).toHaveBeenCalledTimes(1);
    expect(openFor).toHaveBeenCalledWith(VERSION_ID, VERSION_ID);
    expect(switchTab).toHaveBeenCalledWith('saves');
    // openFor 必须早于 switchTab，否则页面还是未初始化状态
    expect(openFor.mock.invocationCallOrder[0]).toBeLessThan(switchTab.mock.invocationCallOrder[0]);
  });

  it('打开后会把对应存档交给编辑弹窗', async () => {
    const vm = setupWidget();
    await vm.reload();
    await vm.openSave({ folder: '新的世界', name: '新的世界' });
    expect(openSaveEditor).toHaveBeenCalledTimes(1);
    expect(openSaveEditor.mock.calls[0][0].folder).toBe('新的世界');
  });

  it('编辑布局时不响应点击', async () => {
    const vm = setupWidget(true);
    await vm.reload();
    await vm.openSave({ folder: '新的世界', name: '新的世界' });
    expect(openFor).not.toHaveBeenCalled();
  });
});

describe('存档卡片 launchSave', () => {
  it('点启动按钮会带上 quickPlayWorld 并调 handleLaunch', async () => {
    const { appState } = await import('../../core/app-state.js');
    const { handleLaunch } = await import('../../features/launch/launch.js');
    const vm = setupWidget();
    await vm.reload();
    await vm.launchSave({ folder: '新的世界', name: '新的世界' });
    expect(appState.quickPlayWorld).toBe('新的世界');
    expect(handleLaunch).toHaveBeenCalledTimes(1);
  });
});
