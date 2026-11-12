// @vitest-environment jsdom
// download-bridge.test.js — 浏览器下载接管的前端接入层
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { resetDom } from '../../test-utils/dom.js';

// 依赖模块全部 mock，只验证「收取 → 转交」的行为契约
const mocks = vi.hoisted(() => ({
  navigateToPage: vi.fn(),
  showToast: vi.fn(),
  fetchPendingDownload: vi.fn(async () => null),
  applyPendingDownload: vi.fn(),
  ensureWidgetVisible: vi.fn(() => true),
  onPendingDownloadRequest: vi.fn(),
  onTauriEvent: vi.fn(() => () => {}),
}));

vi.mock('../../core/tauri-bridge.js', () => ({
  bridge: { onDownloadRequest: mocks.onTauriEvent },
}));
vi.mock('../../core/navigation.js', () => ({
  navigateToPage: mocks.navigateToPage,
}));
vi.mock('../misc/ui-components.js', () => ({
  showToast: mocks.showToast,
}));
vi.mock('../versions/custom-download.js', () => ({
  fetchPendingDownload: mocks.fetchPendingDownload,
  applyPendingDownload: mocks.applyPendingDownload,
}));
vi.mock('./home-widgets.js', () => ({
  ensureWidgetVisible: mocks.ensureWidgetVisible,
  onPendingDownloadRequest: mocks.onPendingDownloadRequest,
}));

import { handleDownloadRequest } from './download-bridge.js';

beforeEach(() => {
  resetDom();
  vi.clearAllMocks();
  mocks.ensureWidgetVisible.mockReturnValue(true);
  mocks.fetchPendingDownload.mockResolvedValue(null);
});

describe('handleDownloadRequest', () => {
  it('无 url 的请求直接忽略', async () => {
    await handleDownloadRequest(null);
    await handleDownloadRequest({});
    expect(mocks.applyPendingDownload).not.toHaveBeenCalled();
    expect(mocks.navigateToPage).not.toHaveBeenCalled();
  });

  it('把请求转交给下载控件并切回主页', async () => {
    const req = { url: 'https://example.com/a.zip', filename: 'a.zip' };
    await handleDownloadRequest(req);

    expect(mocks.ensureWidgetVisible).toHaveBeenCalledWith('download');
    expect(mocks.onPendingDownloadRequest).toHaveBeenCalledWith(req);
    expect(mocks.applyPendingDownload).toHaveBeenCalledWith(req);
    expect(mocks.navigateToPage).toHaveBeenCalledWith('home');
  });

  it('控件是新添加时提示"已添加到主页"，否则提示确认下载', async () => {
    await handleDownloadRequest({ url: 'https://e.com/a' });
    expect(mocks.showToast.mock.calls[0][0]).toContain('并把「下载」控件添加到主页');

    mocks.showToast.mockClear();
    mocks.ensureWidgetVisible.mockReturnValue(false);
    await handleDownloadRequest({ url: 'https://e.com/b' });
    expect(mocks.showToast.mock.calls[0][0]).toContain('确认后点「开始下载」');
  });

  it('导航失败不影响请求转交', async () => {
    mocks.navigateToPage.mockImplementationOnce(() => { throw new Error('boom'); });
    await expect(handleDownloadRequest({ url: 'https://e.com/c' })).resolves.toBeUndefined();
    expect(mocks.applyPendingDownload).toHaveBeenCalled();
  });
});
