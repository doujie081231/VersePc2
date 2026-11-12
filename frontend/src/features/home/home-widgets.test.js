// @vitest-environment jsdom
// home-widgets.test.js — 主页控件注册表与布局持久化
import { describe, it, expect, beforeEach } from 'vitest';
import { resetDom } from '../../test-utils/dom.js';
import {
  WIDGETS,
  OPTIONAL_WIDGETS,
  CORE_WIDGETS,
  ALL_WIDGET_IDS,
  loadHomeLayout,
  saveHomeLayout,
  isWidgetVisible,
  ensureWidgetVisible,
  removeWidget,
  listVisibleWidgets,
  resetWidgetPositions,
  onLayoutChange,
  onPendingDownloadRequest,
  getPendingDownloadRequest,
  subscribeDownloadRequest,
} from './home-widgets.js';

describe('控件注册表', () => {
  it('核心控件不可移除，可选控件可移除', () => {
    expect(CORE_WIDGETS).toContain('launch');
    expect(CORE_WIDGETS).not.toContain('download');
    // 注册表覆盖软件全部功能页面，下载只是其中之一
    expect(OPTIONAL_WIDGETS.length).toBeGreaterThan(5);
    for (const w of OPTIONAL_WIDGETS) expect(w.id).not.toBe('title');
    expect(ALL_WIDGET_IDS).toEqual(Object.keys(WIDGETS));
  });

  it('每个控件都有 name、desc 与合法页面引用，供控件库展示', () => {
    for (const w of Object.values(WIDGETS)) {
      expect(w.name).toBeTruthy();
      expect(w.desc).toBeTruthy();
      expect(typeof w.removable).toBe('boolean');
    }
    // 全部可选控件都要能被点击跳转（download 的 page 也必须是真实页面）
    for (const w of OPTIONAL_WIDGETS) {
      expect(typeof w.page).toBe('string');
      expect(w.page.length).toBeGreaterThan(0);
    }
  });
});

describe('布局读写', () => {
  beforeEach(() => resetDom());

  it('空存储时返回只含 visible 的空布局', () => {
    const layout = loadHomeLayout();
    expect(layout.visible).toEqual({});
  });

  it('保存后能原样读回坐标', () => {
    saveHomeLayout({ title: { x: 10, y: 20 }, visible: {} });
    const layout = loadHomeLayout();
    expect(layout.title).toEqual({ x: 10, y: 20 });
  });

  it('丢弃非法坐标与未知控件', () => {
    saveHomeLayout({ title: { x: 'a', y: 5 }, unknown: { x: 1, y: 2 }, visible: {} });
    const layout = loadHomeLayout();
    expect(layout.title).toEqual({ y: 5 });
    expect(layout.unknown).toBeUndefined();
  });

  it('头像额外保留缩放比例', () => {
    saveHomeLayout({ avatar: { x: 1, y: 2, s: 1.4 }, visible: {} });
    const layout = loadHomeLayout();
    expect(layout.avatar.s).toBe(1.4);
  });

  it('从 v1 迁移：保留坐标，visible 为空', () => {
    localStorage.setItem('versepc_home_layout_v1', JSON.stringify({ title: { x: 3, y: 4 } }));
    const layout = loadHomeLayout();
    expect(layout.title).toEqual({ x: 3, y: 4 });
    expect(layout.visible).toEqual({});
    // 迁移后应写入 v2，避免每次读都走迁移分支
    expect(localStorage.getItem('versepc_home_layout_v2')).toBeTruthy();
  });
});

describe('控件可见性', () => {
  beforeEach(() => resetDom());

  it('核心控件始终可见', () => {
    const layout = loadHomeLayout();
    expect(isWidgetVisible(layout, 'launch')).toBe(true);
    expect(isWidgetVisible(layout, 'download')).toBe(false);
  });

  it('ensureWidgetVisible 首次返回 true，再次返回 false', () => {
    expect(ensureWidgetVisible('download')).toBe(true);
    expect(ensureWidgetVisible('download')).toBe(false);
    expect(listVisibleWidgets()).toContain('download');
  });

  it('核心控件不能用 ensureWidgetVisible 添加', () => {
    expect(ensureWidgetVisible('launch')).toBe(false);
  });

  it('removeWidget 移除后不可见，核心控件拒绝移除', () => {
    ensureWidgetVisible('download');
    expect(removeWidget('download')).toBe(true);
    expect(listVisibleWidgets()).not.toContain('download');
    expect(removeWidget('launch')).toBe(false);
  });

  it('resetWidgetPositions 清坐标但保留已添加控件', () => {
    ensureWidgetVisible('download');
    saveHomeLayout({ ...loadHomeLayout(), title: { x: 9, y: 9 } });
    resetWidgetPositions();
    const layout = loadHomeLayout();
    expect(layout.title).toBeUndefined();
    expect(layout.visible.download).toBe(true);
  });
});

describe('事件广播', () => {
  beforeEach(() => resetDom());

  it('onLayoutChange 在添加控件时触发', () => {
    const seen = [];
    const off = onLayoutChange((d) => seen.push(d));
    ensureWidgetVisible('download');
    off();
    expect(seen[0]).toMatchObject({ type: 'add', id: 'download' });
  });

  it('取消订阅后不再收到事件', () => {
    const seen = [];
    const off = onLayoutChange((d) => seen.push(d));
    off();
    ensureWidgetVisible('download');
    expect(seen).toHaveLength(0);
  });

  it('下载请求广播可被订阅者读到', () => {
    const got = [];
    const off = subscribeDownloadRequest((r) => got.push(r));
    onPendingDownloadRequest({ url: 'https://x/y.zip', filename: 'y.zip' });
    off();
    expect(got[0].url).toBe('https://x/y.zip');
    expect(getPendingDownloadRequest().filename).toBe('y.zip');
  });
  it('已添加但缺坐标的控件会自动补坐标（不再叠在一起）', () => {
    localStorage.setItem('versepc_home_layout_v2', JSON.stringify({
      visible: { mods: true, lan_portmap: true, settings: true },
    }));
    const layout = loadHomeLayout();
    // 三个控件都该拿到各自的坐标
    expect(layout.mods).toBeTruthy();
    expect(layout.settings).toBeTruthy();
    const a = layout.mods, b = layout.settings;
    expect(a.x + ',' + a.y).not.toBe(b.x + ',' + b.y);
  });
});
