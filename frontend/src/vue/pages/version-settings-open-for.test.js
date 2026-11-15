// @vitest-environment jsdom
// version-settings-open-for.test.js — 打开版本设置时的懒加载状态重置
//
// 锁住一个真实 bug：openFor() 重置了 modsLoaded / exportLoaded，
// 但漏了 savesLoaded。于是「打开过 A 版本的存档页 → 再打开 B 版本」时，
// watch 里的 !this.savesLoaded 不成立，loadSaves() 永远不再执行，
// 存档页一直显示「暂无存档」。

import { beforeEach, describe, expect, it, vi } from 'vitest';

const invokeCalls = [];
vi.mock('../../core/tauri-bridge.js', () => ({
  bridge: {
    invoke: vi.fn(async (cmd, args) => {
      invokeCalls.push([cmd, args]);
      if (cmd === 'version_list_saves') {
        return { success: true, saves: [{ folder: 'w-' + args.versionId, name: 'w' }] };
      }
      return {};
    }),
    store: { get: async () => '', set: async () => undefined },
  },
}));
vi.mock('../../core/navigation.js', () => ({ navigateToPage: vi.fn() }));
vi.mock('../../core/api.js', () => ({
  API: {
    getVersionSettings: async () => ({}),
    saveSetting: async () => undefined,
  },
}));

const { coreMethods } = await import('./version-settings-methods-core.js');

/** 造一个最小可用的 vm（只带 openFor/loadSaves 会用到的字段） */
function makeVm() {
  return {
    versionId: null,
    modsLoaded: true,
    exportLoaded: true,
    savesLoaded: true,      // 关键：模拟「之前已经加载过存档」
    saves: [{ folder: 'old', name: 'old' }],
    savesLoading: false,
    customName: '',
    exportName: '',
    isExternal: false,
    externalPath: '',
    title: '',
    activeTab: 'saves',
    loadSettings: () => {},
    loadSaves: coreMethods.loadSaves,
    openFor: coreMethods.openFor,
  };
}

beforeEach(() => { invokeCalls.length = 0; });

describe('openFor 重置存档懒加载状态', () => {
  it('打开另一个版本后 savesLoaded 必须被重置', async () => {
    const vm = makeVm();
    document.body.innerHTML = '<div class="content-area"></div>';

    await vm.openFor('1.21.1-Fabric-0.20.1', '1.21.1');

    expect(vm.savesLoaded).toBe(false);
    expect(vm.saves).toEqual([]);
    expect(vm.savesLoading).toBe(false);
  });

  it('重置后再次加载存档能拿到新版本的数据（不串档）', async () => {
    const vm = makeVm();
    document.body.innerHTML = '<div class="content-area"></div>';

    await vm.openFor('26.3', '26.3');
    await vm.loadSaves();

    const calls = invokeCalls.filter(([cmd]) => cmd === 'version_list_saves');
    expect(calls).toHaveLength(1);
    expect(calls[0][1].versionId).toBe('26.3');
    expect(vm.saves[0].folder).toBe('w-26.3');
    expect(vm.savesLoaded).toBe(true);
  });

  it('没有 versionId 时 loadSaves 不标记已加载（避免永远不加载）', async () => {
    const vm = makeVm();
    vm.savesLoaded = false;
    vm.versionId = null;

    await vm.loadSaves();

    expect(vm.savesLoaded).toBe(false);
    expect(invokeCalls.filter(([cmd]) => cmd === 'version_list_saves')).toHaveLength(0);
  });
});
