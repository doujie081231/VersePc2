// @vitest-environment jsdom
// vset-mods.test.js — 模组更新检查与批量更新单元测试
import { beforeEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  applyModUpdates: vi.fn(),
  getVersionMods: vi.fn(),
  checkModUpdates: vi.fn(),
}));
const ui = vi.hoisted(() => ({
  showToast: vi.fn(),
  showConfirmDialog: vi.fn(),
}));

vi.mock('../../core/api.js', () => ({ API: api }));
vi.mock('../misc/ui-components.js', async (importOriginal) => {
  const actual = await importOriginal();
  return { ...actual, showToast: ui.showToast, showConfirmDialog: ui.showConfirmDialog };
});

import { appState } from '../../core/app-state.js';
import { S } from './vset-state.js';
import { applyAllModUpdates, showModUpdateDialog } from './vset-mods.js';

const UPDATES = [
  {
    fileName: 'old-mod.jar',
    modName: '示例模组',
    currentVersion: '1.0.0',
    targetVersion: '2.0.0',
    downloadUrl: 'https://cdn.modrinth.com/data/x/versions/2/mod.jar',
    newFileName: 'new-mod.jar',
    sha1: 'a'.repeat(40),
    size: 1024,
    projectUrl: 'https://modrinth.com/mod/demo',
  },
];

beforeEach(() => {
  document.body.innerHTML = '';
  ui.showToast.mockReset();
  ui.showConfirmDialog.mockReset();
  api.applyModUpdates.mockReset();
  api.getVersionMods.mockReset();
  api.getVersionMods.mockResolvedValue([]);
  S._pendingModUpdates = [];
  appState.currentSettingsVersionId = '1.20.1-fabric';
});

describe('showModUpdateDialog', () => {
  it('列出目标版本并提供更新全部按钮', () => {
    showModUpdateDialog(UPDATES, 12);
    const text = document.body.textContent;
    expect(text).toContain('示例模组');
    expect(text).toContain('1.0.0 → 2.0.0');
    expect(text).toContain('更新全部 (1)');
    expect(S._pendingModUpdates).toEqual(UPDATES);
  });
});

describe('applyAllModUpdates', () => {
  function mountButton() {
    showModUpdateDialog(UPDATES, 1);
    return document.querySelector('.modal-overlay button.btn-primary');
  }

  it('确认后提交批量更新并关闭弹窗', async () => {
    ui.showConfirmDialog.mockResolvedValue(true);
    api.applyModUpdates.mockResolvedValue({ success: true, updated: 1, failed: 0 });
    const btn = mountButton();
    await applyAllModUpdates(btn);

    expect(api.applyModUpdates).toHaveBeenCalledWith('1.20.1-fabric', UPDATES);
    expect(ui.showToast).toHaveBeenCalledWith('已更新 1 个模组', 'success');
    expect(document.querySelector('.modal-overlay')).toBeNull();
    expect(api.getVersionMods).toHaveBeenCalled();
  });

  it('取消时不发起更新请求', async () => {
    ui.showConfirmDialog.mockResolvedValue(false);
    const btn = mountButton();
    await applyAllModUpdates(btn);
    expect(api.applyModUpdates).not.toHaveBeenCalled();
  });

  it('部分失败时提示失败数量并保留弹窗', async () => {
    ui.showConfirmDialog.mockResolvedValue(true);
    api.applyModUpdates.mockResolvedValue({ success: false, updated: 0, failed: 1 });
    const btn = mountButton();
    await applyAllModUpdates(btn);

    expect(ui.showToast).toHaveBeenCalledWith('已更新 0 个模组，1 个失败', 'error');
    expect(document.querySelector('.modal-overlay')).toBeTruthy();
  });

  it('没有待更新项时直接提示', async () => {
    S._pendingModUpdates = [];
    await applyAllModUpdates();
    expect(ui.showToast).toHaveBeenCalledWith('没有可更新的模组', 'info');
  });
});
