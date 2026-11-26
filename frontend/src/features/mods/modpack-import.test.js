// @vitest-environment jsdom
// modpack-import.test.js — 拖拽整合包导入、命名弹窗与进度同步单元测试
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { getApiCalls, installTauriMock, mockApiRoute } from '../../test-utils/tauri-mock.js';

let featureBridge;
let dlManager;
const listeners = {};

function buildDom() {
  document.body.innerHTML = `
    <div id="toast-container"></div>
    <div id="download-queue-list"></div>
    <div id="dl-fab"><span id="dl-fab-badge"></span></div>
  `;
}

function drag(paths) {
  const cb = listeners['tauri://drag-drop'];
  if (!cb) throw new Error('未捕获 drag-drop 监听');
  cb({ payload: { paths } });
}

async function openNameModal(path = 'C:\\packs\\pack.mrpack') {
  drag([path]);
  await vi.waitFor(() => {
    expect(document.getElementById('import-name-modal')).toBeTruthy();
  }, { timeout: 10000 });
  return document.getElementById('import-name-modal');
}

async function confirmName(name) {
  const input = document.getElementById('inm-input');
  input.value = name;
  input.dispatchEvent(new Event('input'));
  await vi.waitFor(() => {
    expect(document.getElementById('inm-confirm').disabled).toBe(false);
  });
  document.getElementById('inm-confirm').click();
}

function findTask(taskIdPart) {
  return [...dlManager.tasks.values()].find((t) => t.id.includes(taskIdPart));
}

function captureTauriListeners() {
  const capture = async (name, cb) => {
    listeners[name] = cb;
    return () => {};
  };
  window.__TAURI_INTERNALS__.listen = capture;
  window.__TAURI_INTERNALS__.event = { listen: capture };
}

beforeAll(async () => {
  installTauriMock();
  captureTauriListeners();
  featureBridge = (await import('../../core/feature-bridge.js')).featureBridge;
  dlManager = (await import('../versions/custom-download.js')).dlManager;
  await import('./modpack-import.js');
});

beforeEach(() => {
  installTauriMock({
    commands: {
      select_folder: () => ({ cancelled: false, path: 'D:/mods' }),
      get_default_mod_path: () => null,
    },
  });
  captureTauriListeners();
  buildDom();
  featureBridge.modpackImporting = false;
  dlManager.tasks.clear();
  dlManager.order = [];

  mockApiRoute('/api/check-version-name', { available: true });
  mockApiRoute('/api/versions', { versions: [], installed: [] });
  mockApiRoute('/api/modpack/import', { success: true, name: 'MyPack' });
});

afterEach(() => {
  vi.restoreAllMocks();
  document.body.innerHTML = '';
  featureBridge.modpackImporting = false;
});

describe('拖拽过滤', () => {
  it('忽略非整合包文件', () => {
    drag(['C:\\packs\\readme.txt']);
    expect(document.getElementById('import-name-modal')).toBeNull();
  });

  it('忽略空路径列表', () => {
    drag([]);
    expect(document.getElementById('import-name-modal')).toBeNull();
  });

  it('导入进行中时忽略新的拖拽', () => {
    featureBridge.modpackImporting = true;
    drag(['C:\\packs\\pack.zip']);
    expect(document.getElementById('import-name-modal')).toBeNull();
  });

  it('zip 整合包也进入命名流程', async () => {
    await openNameModal('C:\\packs\\pack.zip');
    const input = document.getElementById('inm-input');
    expect(input.value).toBe('pack');
  });
});

describe('命名弹窗', () => {
  it('取消命名时不发起导入', async () => {
    await openNameModal();
    document.getElementById('inm-cancel').click();

    await vi.waitFor(() => {
      expect(document.getElementById('import-name-modal')).toBeNull();
    });
    expect(getApiCalls().some((c) => c.path === '/api/modpack/import')).toBe(false);
    expect(featureBridge.modpackImporting).toBe(false);
  });

  it('名称不可用时禁用确认按钮', async () => {
    mockApiRoute('/api/check-version-name', { available: false, reason: '名称已存在' });
    await openNameModal();

    const input = document.getElementById('inm-input');
    input.value = 'duplicated';
    input.dispatchEvent(new Event('input'));

    await vi.waitFor(() => {
      expect(document.getElementById('inm-hint').textContent).toBe('');
      expect(document.getElementById('inm-warn').textContent).toContain('名称已存在');
    });
    expect(document.getElementById('inm-confirm').disabled).toBe(true);
  });

  it('空名称禁用确认按钮', async () => {
    await openNameModal();
    const input = document.getElementById('inm-input');
    input.value = '   ';
    input.dispatchEvent(new Event('input'));

    await vi.waitFor(() => {
      expect(document.getElementById('inm-confirm').disabled).toBe(true);
    });
  });
});

describe('导入流程', () => {
  it('确认后上报进度、完成后刷新版本列表', async () => {
    let resolveImport;
    mockApiRoute('/api/modpack/import', () => new Promise((resolve) => { resolveImport = resolve; }));
    await openNameModal();
    await confirmName('MyPack');

    await vi.waitFor(() => {
      expect(getApiCalls().some((c) => c.path === '/api/modpack/import')).toBe(true);
    });
    const importCall = getApiCalls().find((c) => c.path === '/api/modpack/import');
    expect(importCall.args.body).toEqual({ filePath: 'C:\\packs\\pack.mrpack', customName: 'MyPack' });
    expect(featureBridge.modpackImporting).toBe(true);
    await vi.waitFor(() => {
      expect(findTask('modpack-')).toBeTruthy();
    });

    const progressCb = listeners['import-progress'];
    expect(progressCb).toBeTruthy();
    progressCb({
      payload: {
        progress: 42,
        message: '下载模组',
        stage: 'download',
        files: [{ name: 'a.jar', size: 1024, status: 'downloading', progress: 10 }],
      },
    });

    await vi.waitFor(() => {
      const task = findTask('modpack-');
      expect(task).toBeTruthy();
      expect(task.progress).toBe(42);
      expect(task.message).toBe('下载整合包内容...');
      expect(task.files[0].size).toBe('1.0 KB');
      expect(task.stageHistory.some((s) => s.stage === 'download')).toBe(true);
    });

    resolveImport({ status: 200, body: { success: true, name: 'MyPack' } });

    await vi.waitFor(() => {
      expect(document.getElementById('toast-container').textContent).toContain('导入成功');
    });
    expect(findTask('modpack-').status).toBe('completed');
    expect(findTask('modpack-').progress).toBe(100);
    expect(featureBridge.modpackImporting).toBe(false);
    await vi.waitFor(() => {
      expect(getApiCalls().some((c) => c.path === '/api/versions')).toBe(true);
    });
  });

  it('后端提示部分模组缺失时也要展示警告', async () => {
    mockApiRoute('/api/modpack/import', {
      success: true,
      name: 'PartialPack',
      warning: '2/10 个Mod下载失败: a, b。请检查网络后重试。',
    });
    await openNameModal();
    await confirmName('PartialPack');

    await vi.waitFor(() => {
      expect(document.getElementById('toast-container').textContent).toContain('2/10 个Mod下载失败');
    });
    expect(findTask('modpack-').status).toBe('completed');
  });

  it('后端返回失败时标记任务失败并提示', async () => {
    mockApiRoute('/api/modpack/import', { success: false, error: '压缩包损坏' });
    await openNameModal();
    await confirmName('BrokenPack');

    await vi.waitFor(() => {
      expect(document.getElementById('toast-container').textContent).toContain('导入失败: 压缩包损坏');
    });
    expect(findTask('modpack-').status).toBe('failed');
    expect(featureBridge.modpackImporting).toBe(false);
  });

  it('导入请求异常时提示出错', async () => {
    mockApiRoute('/api/modpack/import', () => { throw new Error('IPC 断开'); });
    await openNameModal();
    await confirmName('ErrorPack');

    await vi.waitFor(() => {
      expect(document.getElementById('toast-container').textContent).toContain('导入出错: IPC 断开');
    });
    expect(findTask('modpack-').status).toBe('failed');
    expect(featureBridge.modpackImporting).toBe(false);
  });

  it('终态进度立即生效', async () => {
    let resolveImport;
    mockApiRoute('/api/modpack/import', () => new Promise((resolve) => { resolveImport = resolve; }));
    await openNameModal();
    await confirmName('FastPack');
    await vi.waitFor(() => {
      expect(getApiCalls().some((c) => c.path === '/api/modpack/import')).toBe(true);
    });
    await vi.waitFor(() => {
      expect(findTask('modpack-')).toBeTruthy();
    });

    listeners['import-progress']({ payload: { progress: 100, message: '导入完成' } });

    await vi.waitFor(() => {
      expect(findTask('modpack-').progress).toBe(100);
      expect(findTask('modpack-').message).toBe('导入完成');
    });
    resolveImport({ status: 200, body: { success: true, name: 'FastPack' } });
    await vi.waitFor(() => {
      expect(featureBridge.modpackImporting).toBe(false);
    });
  });
});
