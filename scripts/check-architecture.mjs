// check-architecture.mjs — 架构护栏检查
// 用法：node scripts/check-architecture.mjs [--strict]
// 默认只输出报告；--strict 时把违规项作为错误退出，供 CI 使用。

import { execSync } from 'node:child_process';
import { readFileSync, existsSync, readdirSync, statSync } from 'node:fs';
import { join, relative, posix } from 'node:path';

const root = process.cwd();
const strict = process.argv.includes('--strict');
const errors = [];
const warnings = [];

function trackedFiles(dir) {
  try {
    const out = execSync(`git ls-files "${dir}"`, { encoding: 'utf8' });
    return out.split(/\r?\n/).filter(Boolean);
  } catch {
    return [];
  }
}

function walk(dir) {
  if (!existsSync(dir)) return [];
  const out = [];
  for (const entry of readdirSync(dir)) {
    const p = join(dir, entry);
    const st = statSync(p);
    if (st.isDirectory()) out.push(...walk(p));
    else out.push(p);
  }
  return out;
}

function countLines(file) {
  const text = readFileSync(file, 'utf8');
  return text.split('\n').length;
}

// 1. 生成目录不得被 Git 跟踪
for (const dir of ['frontend-tauri', 'frontend-dist']) {
  const files = trackedFiles(dir);
  if (files.length > 0) {
    errors.push(`${dir} 仍被 Git 跟踪（${files.length} 个文件），应改为构建生成物`);
  }
}

// 2. index.html 业务脚本数量
const htmlPath = join(root, 'frontend', 'index.html');
if (existsSync(htmlPath)) {
  const html = readFileSync(htmlPath, 'utf8');
  const scripts = [...html.matchAll(/<script[^>]*\bsrc\s*=\s*"([^"]+)"/g)].map((m) => m[1]);
  const isVendor = (src) =>
    /(^|\/)(three[^/]*\.bundle\.js|three-glb[^/]*\.bundle\.js|skinview3d[^/]*\.bundle\.js|vue\.global\.prod\.js|marked\.min\.js)$/.test(src);
  const business = scripts.filter((s) => !isVendor(s));
  if (business.length > 1) {
    errors.push(`frontend/index.html 仍有 ${business.length} 个业务脚本标签，目标为 1 个模块入口`);
  }
}

// 3. window.* 赋值只允许集中在 legacy 过渡层
const jsRoots = [join(root, 'frontend', 'js'), join(root, 'frontend', 'src')];
const vendorPattern = /(three[^/]*\.bundle\.js|skinview3d[^/]*\.bundle\.js|vue\.global\.prod\.js|marked\.min\.js)$/;
const windowAssignments = [];
for (const dir of jsRoots) {
  for (const file of walk(dir)) {
    if (!file.endsWith('.js') || vendorPattern.test(file)) continue;
    // 测试文件只在测试环境运行，允许模拟宿主全局
    if (file.endsWith('.test.js')) continue;
    if (file.includes(`${join('src', 'legacy')}`)) continue;
    // 插件兼容层是唯一允许向 window 暴露插件接口的文件
    const rel = relative(root, file).replace(/\\/g, '/');
    if (rel === 'frontend/src/core/plugin-compat.js') continue;
    // 测试辅助层允许模拟浏览器/宿主全局，不参与生产运行
    if (rel.startsWith('frontend/src/test-utils/')) continue;
    const text = readFileSync(file, 'utf8');
    const matches = [...text.matchAll(/^\s*window\.[A-Za-z_$][\w$]*\s*=/gm)];
    if (matches.length) windowAssignments.push({ file: relative(root, file), count: matches.length });
  }
}
if (windowAssignments.length) {
  const total = windowAssignments.reduce((n, x) => n + x.count, 0);
  errors.push(`legacy 层以外仍有 ${total} 处 window.* 赋值（涉及 ${windowAssignments.length} 个文件）`);
}

// 4. 组件层约束：迁移到 components/verse 之后，不再新增独立 CSS 文件
//    下面的名单是迁移完成时的存量快照（全部为迁移前遗留），仅作登记用途。
//    新页面请使用 components/verse + Tailwind 工具类；确需新增样式文件时必须在此登记，
//    以便评审时能看出「这又是一份手写 CSS」。
const LEGACY_CSS_SNAPSHOT = new Set([
  'frontend/public/css/base.css',
  'frontend/public/css/classic-nav.css',
  'frontend/public/css/file-browser.css',
  'frontend/public/css/launch.css',
  'frontend/public/css/layout.css',
  'frontend/public/css/modal.css',
  'frontend/public/css/mods.css',
  'frontend/public/css/plugins.css',
  'frontend/public/css/private-server.css',
  'frontend/public/css/redstone.css',
  'frontend/public/css/server-host.css',
  'frontend/public/css/settings.css',
  'frontend/public/css/splash.css',
  'frontend/public/css/sponsor.css',
  'frontend/public/css/themes.css',
  'frontend/public/css/versions.css',
  'frontend/public/css/components/accounts.css',
  'frontend/public/css/components/base-ui.css',
  'frontend/public/css/components/cloud.css',
  'frontend/public/css/components/dialogs.css',
  'frontend/public/css/components/downloads.css',
  'frontend/public/css/components/favorites.css',
  'frontend/public/css/components/lan.css',
  'frontend/public/css/components/mod-detail.css',
  'frontend/public/css/components/resource-detail.css',
  'frontend/public/css/components/theme.css',
  'frontend/public/css/components/toolbox.css',
  'frontend/public/css/components/version-settings.css',
  'frontend/public/css/components/version-ui.css',
  // 内置社区/皮肤站面板（PR #19 引入，按快照登记）
  'frontend/public/css/components/webpanel.css',
  'frontend/public/dino/index.css',
  'frontend/public/v-island/dynamic-island.css',
]);
for (const file of walk(join(root, 'frontend', 'public'))) {
  if (!file.endsWith('.css')) continue;
  const rel = relative(root, file).replace(/\\/g, '/');
  if (!LEGACY_CSS_SNAPSHOT.has(rel)) {
    errors.push(`${rel} 未登记在存量样式快照中，新页面请改用 components/verse 与 Tailwind 工具类`);
  }
}

// 5. 内联样式棘轮：页面模板里 ≥120 字符的 style="..." 只允许减少，不允许新增
//    迁移到 components/verse 之后，布局约束应写成语义工具类，而不是继续堆行内样式。
const INLINE_STYLE_LIMIT = 29;
{
  const pagesDir = join(root, 'frontend', 'src', 'vue', 'pages');
  let count = 0;
  for (const file of walk(pagesDir)) {
    if (!file.endsWith('.js')) continue;
    const text = readFileSync(file, 'utf8');
    count += [...text.matchAll(/style=\\?"([^"\\]{120,})\\?"/g)].length;
  }
  if (count > INLINE_STYLE_LIMIT) {
    errors.push(
      `页面模板中的超长内联 style 由 ${INLINE_STYLE_LIMIT} 处增至 ${count} 处，请改用 Tailwind 工具类或 components/verse 组件`
    );
  } else if (count < INLINE_STYLE_LIMIT) {
    warnings.push(
      `页面模板中的超长内联 style 已降至 ${count} 处（基线 ${INLINE_STYLE_LIMIT}），请把 scripts/check-architecture.mjs 里的 INLINE_STYLE_LIMIT 同步下调`
    );
  }
}

// 6. 超大业务文件报告（只警告，不阻断）
const sizeTargets = [
  [join(root, 'frontend', 'js', 'app'), 650],
  [join(root, 'frontend', 'js', 'vue'), 650],
  [join(root, 'frontend', 'src'), 650],
  [join(root, 'src-tauri', 'src'), 900],
];
for (const [dir, limit] of sizeTargets) {
  for (const file of walk(dir)) {
    if (!/\.(js|rs)$/.test(file) || vendorPattern.test(file)) continue;
    const lines = countLines(file);
    if (lines > limit) {
      warnings.push(`${relative(root, file)} ${lines} 行，超过 ${limit} 行`);
    }
  }
}

// 7. core 中立层不得反向依赖业务域或视图层
//    plugin-compat 是唯一允许引用业务组件的兼容层（供已安装插件继续调用）。
{
  const coreDir = join(root, 'frontend', 'src', 'core');
  for (const file of walk(coreDir)) {
    if (!file.endsWith('.js')) continue;
    const rel = relative(root, file).replace(/\\/g, '/');
    if (rel === 'frontend/src/core/plugin-compat.js') continue;
    const text = readFileSync(file, 'utf8');
    const matches = [...text.matchAll(/from\s+['"]\.\.\/(features|vue)\//g)];
    if (matches.length) {
      errors.push(`${rel} 反向依赖 ${matches[0][1]} 层，core 只能依赖中立模块`);
    }
  }
}

// 8. 业务域不得反向依赖页面/模板层，组件层不得依赖业务或应用层
{
  const rules = [
    {
      dir: join(root, 'frontend', 'src', 'features'),
      pattern: /from\s+['"](?:\.\.\/)+vue\/(?:pages|modals)\//g,
      message: 'features 不得反向依赖 vue/pages 或 vue/modals',
    },
    {
      dir: join(root, 'frontend', 'src', 'components'),
      pattern: /from\s+['"](?:\.\.\/)+(?:features|vue|core)\//g,
      message: 'components 只能依赖无业务语义的模块',
    },
  ];
  for (const rule of rules) {
    for (const file of walk(rule.dir)) {
      if (!/\.(js|vue)$/.test(file)) continue;
      const text = readFileSync(file, 'utf8');
      if (rule.pattern.test(text)) {
        errors.push(`${relative(root, file).replace(/\\/g, '/')}：${rule.message}`);
      }
      rule.pattern.lastIndex = 0;
    }
  }
}

// 9. 前端启动包预算：index.html 首屏加载的 JS 总量与主包体积
const distIndex = join(root, 'frontend-dist', 'index.html');
if (existsSync(distIndex)) {
  const html = readFileSync(distIndex, 'utf8');
  const jsFiles = [...html.matchAll(/<(?:script|link)[^>]+(?:src|href)="\.\/(assets\/[^"]+\.js)"/g)].map((m) => m[1]);
  const uniqueFiles = [...new Set(jsFiles)];
  let startupBytes = 0;
  for (const rel of uniqueFiles) {
    const file = join(root, 'frontend-dist', rel);
    if (!existsSync(file)) continue;
    const size = statSync(file).size;
    startupBytes += size;
    if (size > 600 * 1024) {
      errors.push(`分包 ${rel} 为 ${(size / 1024).toFixed(1)} KB，超过 600 KB 上限`);
    } else if (size > 550 * 1024) {
      warnings.push(`分包 ${rel} 为 ${(size / 1024).toFixed(1)} KB，接近 600 KB 上限`);
    }
  }
  const startupKb = startupBytes / 1024;
  if (startupKb > 800) {
    errors.push(`首屏 JS 总量 ${startupKb.toFixed(1)} KB，超过 800 KB 上限（检查新增的首屏静态导入）`);
  } else if (startupKb > 760) {
    warnings.push(`首屏 JS 总量 ${startupKb.toFixed(1)} KB，接近 800 KB 上限`);
  }
}

// 10. features 直接访问 Tauri 宿主全局的存量棘轮
//     新代码统一通过 core/tauri-bridge.js；存量按文件登记，只允许减少或持平。
const TAURI_GLOBAL_BASELINE = new Map([
  ['frontend/src/features/mods/modpack-import.js', 17],
  ['frontend/src/features/mods/install-modpack.js', 4],
  ['frontend/src/features/mods/resource-import-modal.js', 2],
  ['frontend/src/features/accounts/accounts-avatar.js', 12],
  ['frontend/src/features/accounts/accounts-list.js', 6],
  ['frontend/src/features/versions/versions-list.js', 5],
  ['frontend/src/features/versions/versions-nav.js', 5],
  ['frontend/src/features/versions/versions-install.js', 3],
  ['frontend/src/features/misc/cloud.js', 6],
  ['frontend/src/features/misc/plugin-runtime.js', 5],
  ['frontend/src/features/launch/launch-runtime.js', 5],
  ['frontend/src/features/launch/init-setup-window.js', 3],
]);
{
  const featuresDir = join(root, 'frontend', 'src', 'features');
  const seen = new Set();
  for (const file of walk(featuresDir)) {
    if (!file.endsWith('.js') || file.endsWith('.test.js')) continue;
    const rel = relative(root, file).replace(/\\/g, '/');
    const text = readFileSync(file, 'utf8');
    const count = [...text.matchAll(/window\.__TAURI(?:_INTERNALS)?__/g)].length;
    if (!count) continue;
    seen.add(rel);
    const limit = TAURI_GLOBAL_BASELINE.get(rel) || 0;
    if (count > limit) {
      errors.push(`${rel} 直接访问 Tauri 宿主全局 ${count} 处，超过存量上限 ${limit} 处；请改用 core/tauri-bridge.js`);
    } else if (count < limit) {
      warnings.push(`${rel} 直接访问 Tauri 宿主全局已降至 ${count} 处，可下调 TAURI_GLOBAL_BASELINE`);
    }
  }
  for (const [rel] of TAURI_GLOBAL_BASELINE) {
    if (!seen.has(rel)) warnings.push(`${rel} 已不再直接访问 Tauri 宿主全局，可从 TAURI_GLOBAL_BASELINE 移除`);
  }
}

// 11. features 跨域依赖存量棘轮
//     记录当前域到域的 import 边，只允许减少；新增跨域调用必须走 core 中立入口。
const FEATURE_CROSS_DOMAIN_BASELINE = new Map([
  ['accounts->misc', 6],
  // 主页下载控件与下载桥（PR #22）：主页复用下载管理与提示组件
  ['home->misc', 1],
  ['home->versions', 1],
  ['launch->accounts', 3],
  ['launch->home', 1],
  ['launch->misc', 20],
  ['launch->mods', 5],
  ['launch->settings', 13],
  ['launch->versions', 5],
  ['misc->launch', 1],
  ['misc->mods', 1],
  ['misc->settings', 1],
  ['misc->versions', 2],
  ['mods->misc', 27],
  ['mods->versions', 13],
  ['settings->accounts', 3],
  ['settings->launch', 4],
  ['settings->misc', 13],
  ['settings->versions', 3],
  ['versions->accounts', 1],
  ['versions->launch', 1],
  ['versions->misc', 22],
  ['versions->mods', 9],
  ['versions->settings', 1],
]);
{
  const featuresDir = join(root, 'frontend', 'src', 'features');
  const edges = new Map();
  for (const file of walk(featuresDir)) {
    if (!file.endsWith('.js') || file.endsWith('.test.js')) continue;
    const relFromFeatures = relative(featuresDir, file).replace(/\\/g, '/');
    const domain = relFromFeatures.split('/')[0];
    const text = readFileSync(file, 'utf8');
    const specs = [
      ...[...text.matchAll(/from\s+['"]([^'"]+)['"]/g)].map((m) => m[1]),
      ...[...text.matchAll(/import\(\s*['"]([^'"]+)['"]\s*\)/g)].map((m) => m[1]),
    ];
    for (const spec of specs) {
      if (!spec.startsWith('.')) continue;
      const resolved = posix.normalize(posix.join('features/' + posix.dirname(relFromFeatures), spec));
      const match = /^features\/([^/]+)\//.exec(resolved);
      if (!match || match[1] === domain) continue;
      const key = `${domain}->${match[1]}`;
      edges.set(key, (edges.get(key) || 0) + 1);
    }
  }
  for (const [key, count] of edges) {
    const limit = FEATURE_CROSS_DOMAIN_BASELINE.get(key) || 0;
    if (count > limit) {
      errors.push(`features 跨域依赖 ${key} 为 ${count} 处，超过存量上限 ${limit} 处；请改走 core 中立入口`);
    } else if (count < limit) {
      warnings.push(`features 跨域依赖 ${key} 已降至 ${count} 处，可下调 FEATURE_CROSS_DOMAIN_BASELINE`);
    }
  }
  for (const [key] of FEATURE_CROSS_DOMAIN_BASELINE) {
    if (!edges.has(key)) warnings.push(`features 跨域依赖 ${key} 已消失，可从 FEATURE_CROSS_DOMAIN_BASELINE 移除`);
  }
}

// 12. 顶层副作用登记：模块加载即执行 setInterval/document/window 的文件必须登记
const TOP_LEVEL_SIDE_EFFECT_BASELINE = new Map([
  ['frontend/src/features/launch/java-mgr.js', 1],
  ['frontend/src/features/launch/launch-settings.js', 1],
  ['frontend/src/features/mods/mod-browse.js', 1],
  ['frontend/src/features/mods/resource-import-modal.js', 2],
  ['frontend/src/features/mods/resource-list.js', 1],
  ['frontend/src/core/plugin-compat.js', 4],
]);
{
  const seen = new Set();
  for (const dir of [join(root, 'frontend', 'src', 'features'), join(root, 'frontend', 'src', 'core')]) {
    for (const file of walk(dir)) {
      if (!file.endsWith('.js') || file.endsWith('.test.js')) continue;
      const rel = relative(root, file).replace(/\\/g, '/');
      if (rel.startsWith('frontend/src/test-utils/')) continue;
      const text = readFileSync(file, 'utf8');
      const count = [...text.matchAll(/^(setInterval\(|document\.addEventListener\(|window\.[A-Za-z_$])/gm)].length;
      if (!count) continue;
      seen.add(rel);
      const limit = TOP_LEVEL_SIDE_EFFECT_BASELINE.get(rel) || 0;
      if (count > limit) {
        errors.push(`${rel} 顶层副作用 ${count} 处，超过登记上限 ${limit} 处；请改为显式启动函数`);
      } else if (count < limit) {
        warnings.push(`${rel} 顶层副作用已降至 ${count} 处，可下调 TOP_LEVEL_SIDE_EFFECT_BASELINE`);
      }
    }
  }
  for (const [rel] of TOP_LEVEL_SIDE_EFFECT_BASELINE) {
    if (!seen.has(rel)) warnings.push(`${rel} 已无顶层副作用，可从 TOP_LEVEL_SIDE_EFFECT_BASELINE 移除`);
  }
}

// 13. 业务文件行数硬阈值：前端 700 行、Rust 1200 行（测试文件不计）
{
  for (const file of walk(join(root, 'frontend', 'src'))) {
    if (!file.endsWith('.js') || file.endsWith('.test.js')) continue;
    const lines = countLines(file);
    if (lines > 700) {
      errors.push(`${relative(root, file).replace(/\\/g, '/')} ${lines} 行，超过前端 700 行上限`);
    }
  }
  for (const file of walk(join(root, 'src-tauri', 'src'))) {
    if (!file.endsWith('.rs')) continue;
    const lines = countLines(file);
    if (lines > 1200) {
      errors.push(`${relative(root, file).replace(/\\/g, '/')} ${lines} 行，超过 Rust 1200 行上限`);
    }
  }
}

// 输出
if (errors.length) {
  console.log('\n[架构检查] 违规项：');
  for (const e of errors) console.log(`  ✗ ${e}`);
}
if (warnings.length) {
  console.log('\n[架构检查] 提示项：');
  for (const w of warnings) console.log(`  ! ${w}`);
}
if (!errors.length && !warnings.length) {
  console.log('[架构检查] 通过，未发现问题');
}

if (strict && errors.length) process.exit(1);
