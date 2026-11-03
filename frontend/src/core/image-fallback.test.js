// @vitest-environment jsdom
// image-fallback.test.js — 图片加载失败兜底（替代内联 onerror）
import { beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { installImageFallback } from './image-fallback.js';

function makeImg(mode) {
  const img = document.createElement('img');
  if (mode !== undefined) img.setAttribute('data-img-fallback', mode);
  document.body.appendChild(img);
  return img;
}

beforeAll(() => {
  installImageFallback();
});

beforeEach(() => {
  document.body.innerHTML = '';
});

describe('图片加载失败兜底', () => {
  it('非图片元素或未声明模式时不做处理', () => {
    const span = document.createElement('span');
    document.body.appendChild(span);
    expect(() => span.dispatchEvent(new Event('error'))).not.toThrow();

    const img = makeImg();
    img.style.display = '';
    img.dispatchEvent(new Event('error'));
    expect(img.style.display).toBe('');
  });

  it('hide 模式隐藏图片', () => {
    const img = makeImg('hide');
    img.dispatchEvent(new Event('error'));
    expect(img.style.display).toBe('none');
  });

  it('visibility 模式保留布局并隐藏占位', () => {
    const img = makeImg('visibility');
    img.dispatchEvent(new Event('error'));
    expect(img.style.display).toBe('');
    expect(img.style.visibility).toBe('hidden');
  });

  it('sibling 模式显示下一个兄弟节点', () => {
    const img = makeImg('sibling');
    const next = document.createElement('span');
    img.after(next);
    img.dispatchEvent(new Event('error'));
    expect(img.style.display).toBe('none');
    expect(next.style.display).toBe('flex');
  });

  it('modicon 与 modmgr 模式给父节点加兜底样式', () => {
    const parent = document.createElement('div');
    const icon = document.createElement('img');
    icon.setAttribute('data-img-fallback', 'modicon');
    parent.appendChild(icon);
    document.body.appendChild(parent);
    icon.dispatchEvent(new Event('error'));
    expect(parent.classList.contains('mod-icon--fallback')).toBe(true);

    const mgrParent = document.createElement('div');
    const mgrIcon = document.createElement('img');
    mgrIcon.setAttribute('data-img-fallback', 'modmgr');
    mgrParent.appendChild(mgrIcon);
    document.body.appendChild(mgrParent);
    mgrIcon.dispatchEvent(new Event('error'));
    expect(mgrParent.classList.contains('modmgr-icon--fallback')).toBe(true);
  });

  it('remove-parent 模式移除父节点', () => {
    const parent = document.createElement('div');
    const img = document.createElement('img');
    img.setAttribute('data-img-fallback', 'remove-parent');
    parent.appendChild(img);
    document.body.appendChild(parent);
    img.dispatchEvent(new Event('error'));
    expect(document.body.contains(parent)).toBe(false);
  });

  it('未知模式只隐藏图片', () => {
    const img = makeImg('unknown');
    img.dispatchEvent(new Event('error'));
    expect(img.style.display).toBe('none');
  });
});
