/**
 * VersePC - Minecraft Launcher
 * Copyright (c) 2026 豆杰
 * SPDX-License-Identifier: GPL-3.0-only
 */

/* ==================== 皮肤全身形象渲染 ====================
 * 把皮肤贴到玩家模型上，用 three.js 离屏渲染出正面全身图。
 * 模型与纹理各自缓存，同一时刻只跑一个渲染任务；队列排空后释放
 * WebGL 上下文和缓存，避免长期占用显存。
 */
(function () {
  'use strict';

  const CLASSIC_MODEL_URL = 'models/classic-player.gltf';
  const SLIM_MODEL_URL = 'models/slim-player.gltf';

  // 输出画幅
  const RENDER_WIDTH = 360;
  const RENDER_HEIGHT = 504;

  // 相机：窄视角，正面略偏左，看向头部下方
  const CAMERA_FOV = 20;
  const CAMERA_NEAR = 0.4;
  const CAMERA_FAR = 1000;
  const CAMERA_POSITION = [-1.3, 1, 6.3];
  const LOOK_AT_Y_OFFSET = -0.3;

  // 模型在场景中的摆放
  const GROUP_POSITION = [0, 0.3, 1.95];
  const GROUP_SCALE = 0.8;

  const AMBIENT_LIGHT_INTENSITY = 2;
  const DIRECTIONAL_LIGHT_INTENSITY = 1.2;
  const DIRECTIONAL_LIGHT_POSITION = [2, 4, 3];

  let _libPromise = null;
  let _renderer = null;
  let _scene = null;
  let _camera = null;
  let _group = null;
  let _transparentTexture = null;

  const _modelCache = new Map();
  const _modelPending = new Map();
  const _textureCache = new Map();
  const _texturePending = new Map();

  let _queue = Promise.resolve();
  let _pendingRenders = 0;

  // 旧版皮肤（64×32）转新版（64×64）时需要复制的面，参数为
  // [源 x, 源 y, 目标偏移 x, 目标偏移 y, 宽, 高]，复制时水平镜像
  const LEGACY_FACE_COPIES = [
    [4, 16, 16, 32, 4, 4],
    [8, 16, 16, 32, 4, 4],
    [0, 20, 24, 32, 4, 12],
    [4, 20, 16, 32, 4, 12],
    [8, 20, 8, 32, 4, 12],
    [12, 20, 16, 32, 4, 12],
    [44, 16, -8, 32, 4, 4],
    [48, 16, -8, 32, 4, 4],
    [40, 20, 0, 32, 4, 12],
    [44, 20, -8, 32, 4, 12],
    [48, 20, -16, 32, 4, 12],
    [52, 20, -8, 32, 4, 12]
  ];

  // 需要强制不透明的内部区域 [x1, y1, x2, y2]
  const OPAQUE_PARTS = [
    [0, 0, 32, 16],
    [0, 16, 64, 32],
    [16, 48, 48, 64]
  ];

  const _normalizedCache = new Map();

  // 按需插入脚本标签；已存在同名标签时等待其加载完成
  function _loadScript(src) {
    return new Promise(function (resolve, reject) {
      const existing = document.querySelector('script[src="' + src + '"]');
      if (existing) {
        existing.addEventListener('load', function () { resolve(); });
        existing.addEventListener('error', function () { reject(new Error('Failed to load: ' + src)); });
        return;
      }
      const s = document.createElement('script');
      s.src = src;
      s.onload = function () { resolve(); };
      s.onerror = function () { reject(new Error('Failed to load: ' + src)); };
      document.head.appendChild(s);
    });
  }

  function _loadLib() {
    if (_libPromise) return _libPromise;
    _libPromise = Promise.resolve().then(function () {
      if (window.VerseThree) return window.VerseThree;
      return _loadScript('js/three-glb.bundle.js').then(function () {
        if (!window.VerseThree) throw new Error('three bundle unavailable');
        return window.VerseThree;
      });
    });
    return _libPromise;
  }

  function _loadImage(url) {
    return new Promise(function (resolve, reject) {
      const img = new Image();
      img.crossOrigin = 'anonymous';
      img.onload = function () { resolve(img); };
      img.onerror = function () { reject(new Error('Failed to load image: ' + url)); };
      img.src = url;
    });
  }

  function _setAlpha(buf, x1, y1, x2, y2, alpha) {
    for (let y = y1; y < y2; y++) {
      for (let x = x1; x < x2; x++) {
        buf[(x + y * 64) * 4 + 3] = alpha;
      }
    }
  }

  // 在缓冲区内水平镜像复制一块矩形，源与目标不重叠，可原地操作
  function _copyRectMirrorHorizontally(buf, x, y, offX, offY, width, height) {
    for (let row = 0; row < height; row++) {
      for (let col = 0; col < width; col++) {
        const srcX = x + col;
        const srcY = y + row;
        const dstX = (x + offX) + (width - 1 - col);
        const dstY = (y + offY) + row;
        const si = (srcX + srcY * 64) * 4;
        const di = (dstX + dstY * 64) * 4;
        buf[di] = buf[si];
        buf[di + 1] = buf[si + 1];
        buf[di + 2] = buf[si + 2];
        buf[di + 3] = buf[si + 3];
      }
    }
  }

  // 头部外层全部不透明时整体置为透明（旧版皮肤的透明补丁）
  function _applyNotchTransparencyHack(buf) {
    for (let y = 0; y < 32; y++) {
      for (let x = 32; x < 64; x++) {
        if (buf[(x + y * 64) * 4 + 3] < 128) return;
      }
    }
    _setAlpha(buf, 32, 0, 64, 32, 0);
  }

  /**
   * 把皮肤纹理规整成 64×64：旧版 64×32 补齐为 64×64，并让内部区域不透明。
   * 尺寸不是 64×32 / 64×64 时（例如高清皮肤）返回 null，交由调用方直接用原图。
   */
  function _normalizeSkinTexture(img) {
    const width = img.naturalWidth || img.width;
    const height = img.naturalHeight || img.height;
    if (width !== 64 || (height !== 64 && height !== 32)) return null;

    const source = document.createElement('canvas');
    source.width = 64;
    source.height = height;
    const sourceCtx = source.getContext('2d');
    sourceCtx.drawImage(img, 0, 0);
    const sourceData = sourceCtx.getImageData(0, 0, 64, height).data;

    // 目标缓冲 64×64，初始全透明；先把原图逐行搬进去
    const buf = new Uint8ClampedArray(64 * 64 * 4);
    buf.set(sourceData, 0);

    const isLegacy = height === 32;
    if (isLegacy) {
      for (let i = 0; i < LEGACY_FACE_COPIES.length; i++) {
        const p = LEGACY_FACE_COPIES[i];
        _copyRectMirrorHorizontally(buf, p[0], p[1], p[2], p[3], p[4], p[5]);
      }
      _applyNotchTransparencyHack(buf);
    }

    for (let i = 0; i < OPAQUE_PARTS.length; i++) {
      const p = OPAQUE_PARTS[i];
      _setAlpha(buf, p[0], p[1], p[2], p[3], 255);
    }

    const target = document.createElement('canvas');
    target.width = 64;
    target.height = 64;
    target.getContext('2d').putImageData(new ImageData(buf, 64, 64), 0, 0);
    return target.toDataURL('image/png');
  }

  /** 取得规整后的纹理地址；无法规整时回退原地址 */
  function _normalizedTextureUrl(url) {
    const cached = _normalizedCache.get(url);
    if (cached) return cached;

    const task = _loadImage(url)
      .then(function (img) { return _normalizeSkinTexture(img) || url; })
      .catch(function () { return url; });

    _normalizedCache.set(url, task);
    return task;
  }

  function _loadModel(lib, url) {
    if (_modelCache.has(url)) return Promise.resolve(_modelCache.get(url));
    if (_modelPending.has(url)) return _modelPending.get(url);

    const loader = new lib.GLTFLoader();
    const task = new Promise(function (resolve, reject) {
      loader.load(url, function (gltf) {
        _modelCache.set(url, gltf);
        resolve(gltf);
      }, undefined, reject);
    }).finally(function () {
      _modelPending.delete(url);
    });

    _modelPending.set(url, task);
    return task;
  }

  function _loadTexture(lib, url) {
    if (_textureCache.has(url)) return Promise.resolve(_textureCache.get(url));
    if (_texturePending.has(url)) return _texturePending.get(url);

    const THREE = lib.THREE;
    const loader = new THREE.TextureLoader();
    const task = new Promise(function (resolve, reject) {
      loader.load(url, function (texture) {
        texture.colorSpace = THREE.SRGBColorSpace;
        texture.flipY = false;
        texture.magFilter = THREE.NearestFilter;
        texture.minFilter = THREE.NearestFilter;
        _textureCache.set(url, texture);
        resolve(texture);
      }, undefined, reject);
    }).finally(function () {
      _texturePending.delete(url);
    });

    _texturePending.set(url, task);
    return task;
  }

  function _applyCommonMaterial(mat) {
    mat.metalness = 0;
    mat.color.set(0xffffff);
    mat.roughness = 1;
    mat.depthTest = true;
    mat.depthWrite = true;
  }

  // 皮肤材质：贴图铺满，薄片层（_Layer）走双面透明，其余单面
  function _applyTexture(lib, model, texture) {
    const THREE = lib.THREE;
    model.traverse(function (child) {
      if (!child.isMesh) return;
      const isSkinLayer = child.name.endsWith('_Layer');
      const materials = Array.isArray(child.material) ? child.material : [child.material];

      materials.forEach(function (mat) {
        if (!(mat instanceof THREE.MeshStandardMaterial)) return;
        if (mat.name === 'cape') return;

        mat.map = texture;
        mat.alphaTest = 0.1;
        mat.flatShading = true;
        mat.side = isSkinLayer ? THREE.DoubleSide : THREE.FrontSide;
        mat.toneMapped = false;
        mat.transparent = isSkinLayer;
        _applyCommonMaterial(mat);
        mat.needsUpdate = true;
      });
    });
  }

  // 披风材质：没有披风时用 1×1 透明贴图占位并隐藏
  function _applyCapeTexture(lib, model, texture, transparentTexture) {
    const THREE = lib.THREE;
    model.traverse(function (child) {
      if (!child.isMesh) return;
      const materials = Array.isArray(child.material) ? child.material : [child.material];

      materials.forEach(function (mat) {
        if (!(mat instanceof THREE.MeshStandardMaterial)) return;
        if (mat.name !== 'cape') return;

        mat.map = texture || transparentTexture || null;
        mat.alphaTest = 0.1;
        mat.flatShading = true;
        mat.side = THREE.DoubleSide;
        mat.toneMapped = false;
        mat.transparent = !texture || !!transparentTexture;
        _applyCommonMaterial(mat);
        mat.needsUpdate = true;
        mat.visible = !!texture;
      });
    });
  }

  function _getTransparentTexture(lib) {
    if (_transparentTexture) return _transparentTexture;

    const THREE = lib.THREE;
    const canvas = document.createElement('canvas');
    canvas.width = canvas.height = 1;
    canvas.getContext('2d').clearRect(0, 0, 1, 1);

    const texture = new THREE.CanvasTexture(canvas);
    texture.needsUpdate = true;
    texture.colorSpace = THREE.SRGBColorSpace;
    texture.flipY = false;
    texture.magFilter = THREE.NearestFilter;
    texture.minFilter = THREE.NearestFilter;
    _transparentTexture = texture;
    return texture;
  }

  function _initRenderer(lib) {
    if (_renderer) return;

    const THREE = lib.THREE;
    const canvas = document.createElement('canvas');
    canvas.width = RENDER_WIDTH;
    canvas.height = RENDER_HEIGHT;

    _renderer = new THREE.WebGLRenderer({
      canvas: canvas,
      antialias: true,
      alpha: true,
      preserveDrawingBuffer: true
    });
    _renderer.outputColorSpace = THREE.SRGBColorSpace;
    _renderer.shadowMap.enabled = false;
    _renderer.toneMapping = THREE.NoToneMapping;
    _renderer.toneMappingExposure = 10.0;
    _renderer.setClearColor(0x000000, 0);
    _renderer.setSize(RENDER_WIDTH, RENDER_HEIGHT);

    _scene = new THREE.Scene();
    _camera = new THREE.PerspectiveCamera(
      CAMERA_FOV,
      RENDER_WIDTH / RENDER_HEIGHT,
      CAMERA_NEAR,
      CAMERA_FAR
    );

    const ambientLight = new THREE.AmbientLight(0xffffff, AMBIENT_LIGHT_INTENSITY);
    const directionalLight = new THREE.DirectionalLight(0xffffff, DIRECTIONAL_LIGHT_INTENSITY);
    directionalLight.castShadow = false;
    directionalLight.position.set(
      DIRECTIONAL_LIGHT_POSITION[0],
      DIRECTIONAL_LIGHT_POSITION[1],
      DIRECTIONAL_LIGHT_POSITION[2]
    );
    _scene.add(ambientLight);
    _scene.add(directionalLight);
  }

  function _clearGroup() {
    if (!_scene || !_group) return;
    _scene.remove(_group);
    _group.clear();
    _group = null;
  }

  function _disposeCaches() {
    _textureCache.forEach(function (texture) { texture.dispose(); });
    _textureCache.clear();
    _texturePending.clear();
    _modelCache.clear();
    _modelPending.clear();
  }

  function _disposeRenderer() {
    _clearGroup();

    if (_transparentTexture) {
      _transparentTexture.dispose();
      _transparentTexture = null;
    }
    if (_renderer) {
      _renderer.dispose();
      _renderer.forceContextLoss();
    }

    _renderer = null;
    _scene = null;
    _camera = null;
    _disposeCaches();
  }

  async function _renderSkin(lib, textureUrl, modelUrl) {
    const THREE = lib.THREE;
    _initRenderer(lib);
    _clearGroup();

    const normalizedUrl = await _normalizedTextureUrl(textureUrl);
    const results = await Promise.all([
      _loadModel(lib, modelUrl),
      _loadTexture(lib, normalizedUrl)
    ]);
    const model = results[0].scene.clone();
    _applyTexture(lib, model, results[1]);
    _applyCapeTexture(lib, model, null, _getTransparentTexture(lib));

    const group = new THREE.Group();
    group.add(model);
    group.position.set(GROUP_POSITION[0], GROUP_POSITION[1], GROUP_POSITION[2]);
    group.scale.set(GROUP_SCALE, GROUP_SCALE, GROUP_SCALE);
    _scene.add(group);
    _group = group;

    const head = group.getObjectByName('Head');
    if (!head) throw new Error('Head node not found in model');

    const headPosition = new THREE.Vector3();
    head.getWorldPosition(headPosition);

    _camera.position.set(CAMERA_POSITION[0], CAMERA_POSITION[1], CAMERA_POSITION[2]);
    _camera.lookAt(
      headPosition.x,
      headPosition.y + LOOK_AT_Y_OFFSET,
      headPosition.z
    );

    _renderer.render(_scene, _camera);
    return _renderer.domElement.toDataURL('image/webp', 0.9);
  }

  /** 渲染一张皮肤的正面全身图，返回 dataURL */
  function render(textureUrl, modelType) {
    if (!textureUrl) return Promise.resolve('');

    const modelUrl = modelType === 'slim' ? SLIM_MODEL_URL : CLASSIC_MODEL_URL;
    _pendingRenders++;

    const task = _queue
      .then(function () {
        return _loadLib();
      })
      .then(function (lib) {
        return _renderSkin(lib, textureUrl, modelUrl);
      })
      .finally(function () {
        if (--_pendingRenders === 0) _disposeRenderer();
      });

    _queue = task.then(function () {}, function () {});
    return task;
  }

  window.VerseSkinFigure = { render: render };
})();