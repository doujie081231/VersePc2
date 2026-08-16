// prepare-frontend.mjs — Tauri 构建前准备前端资源目录
// 把 verse框架替换项目/frontend 下的静态资源复制到 frontend-tauri 目录，排除 node_modules
// 使用 robocopy 避免 Node.js cpSync 在 Junction + 中文路径下的 bug
import { execSync } from 'node:child_process';
import { rmSync, existsSync, mkdirSync, copyFileSync, statSync, readdirSync } from 'node:fs';
import { join, dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout } from 'node:timers/promises';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const projectRoot = resolve(__dirname, '..');
const frontendSrcDir = resolve(projectRoot, 'frontend');
const frontendDir = resolve(projectRoot, 'frontend-tauri');

console.log('[prepare-frontend] 前端源目录:', frontendSrcDir);
console.log('[prepare-frontend] 前端资源输出目录:', frontendDir);

if (existsSync(frontendDir)) {
  const keepName = 'installer-app';
  for (const entry of readdirSync(frontendDir)) {
    if (entry === keepName) continue;
    const p = join(frontendDir, entry);
    try {
      rmSync(p, { recursive: true, force: true, maxRetries: 3, retryDelay: 200 });
    } catch (e) {
      console.warn(`[prepare-frontend] 清理 ${entry} 遇到占用文件，跳过: ${e.message}`);
    }
  }
}
mkdirSync(frontendDir, { recursive: true });

// 需要复制的前端资源（文件和目录）
const resources = [
  'index.html',
  'editor.html',
  'file-browser.html',
  'forge-installer.js',
  'integrity.json',
  'js',
  'css',
  'assets',
  'img',
  'images',
  'fonts',
  'v-island',
  'resources',
  'plugins',
  'installer-app',
  'dino',
];

let copied = 0;
let skipped = 0;

for (const res of resources) {
  const src = join(frontendSrcDir, res);
  const dst = join(frontendDir, res);
  if (!existsSync(src)) {
    skipped++;
    console.log(`[prepare-frontend] 跳过(不存在): ${res}`);
    continue;
  }
  try {
    const isDir = statSync(src).isDirectory();
    if (isDir) {
      // robocopy 返回 0-7 为成功，8+ 为失败
      // /E 包含空目录  /XD 排除 node_modules  /NFL /NDL /NJH /NJS 减少输出
      execSync(`robocopy "${src}" "${dst}" /E /XD node_modules /NFL /NDL /NJH /NJS`, { stdio: 'pipe' });
    } else {
      execSync(`copy /Y "${src}" "${dst}" >nul`, { stdio: 'pipe' });
    }
    copied++;
    console.log(`[prepare-frontend] 已复制: ${res}`);
  } catch (e) {
    // robocopy 退出码 0-7 都是成功的
    if (e.status !== undefined && e.status >= 0 && e.status <= 7) {
      copied++;
      console.log(`[prepare-frontend] 已复制: ${res}`);
    } else {
      console.warn(`[prepare-frontend] 复制失败: ${res} - ${e.message}`);
      skipped++;
    }
  }
}

// 确保 Vue 运行时存在于 frontend-tauri/js/vue.global.prod.js
const vueSrc = join(projectRoot, 'node_modules', 'vue', 'dist', 'vue.global.prod.js');
const vueDst = join(frontendDir, 'js', 'vue.global.prod.js');
if (existsSync(vueDst)) {
  console.log(`[prepare-frontend] Vue 运行时已存在(随 js 目录复制)，跳过`);
} else if (existsSync(vueSrc)) {
  try {
    copyFileSync(vueSrc, vueDst);
    copied++;
    console.log(`[prepare-frontend] 已复制: js/vue.global.prod.js`);
  } catch (e) {
    console.warn(`[prepare-frontend] Vue 运行时复制失败: ${e.message}`);
    skipped++;
  }
} else {
  console.warn(`[prepare-frontend] 警告: Vue 运行时未找到，请执行 npm install vue`);
  skipped++;
}

// 清理 frontend-tauri 中的 node_modules 残留
const nmPath = join(frontendDir, 'node_modules');
for (let i = 0; i < 5; i++) {
  if (!existsSync(nmPath)) break;
  try {
    rmSync(nmPath, { recursive: true, force: true, maxRetries: 3, retryDelay: 200 });
    console.log(`[prepare-frontend] 已清理残留: node_modules`);
  } catch (e) {
    console.warn(`[prepare-frontend] 清理残留失败: ${e.message}`);
  }
  await setTimeout(200);
}

console.log(`[prepare-frontend] 完成！复制 ${copied} 项，跳过 ${skipped} 项`);
