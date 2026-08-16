// build-portable.mjs — 便携版打包脚本
// 流程：准备前端资源 → tauri build → 复制 exe 到便携版目录
import { execSync } from 'node:child_process';
import { copyFileSync, mkdirSync, existsSync, readFileSync } from 'node:fs';
import { join, dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const projectRoot = resolve(__dirname, '..');

function run(cmd) {
  console.log(`\n> ${cmd}`);
  execSync(cmd, { stdio: 'inherit', cwd: projectRoot, shell: true });
}

// 1. 读取版本号
const tauriConf = JSON.parse(readFileSync(join(projectRoot, 'src-tauri', 'tauri.conf.json'), 'utf8'));
const version = tauriConf.version || '0.0.0';
console.log(`[build-portable] 版本号: ${version}`);

// 2. 编译 Tauri（只生成 exe，不打包安装包）
//    tauri.conf.json 的 beforeBuildCommand 会自动执行 prepare-frontend.mjs
run('npx tauri build');

// 3. 复制 exe 到便携版目录
const exeSrc = join(projectRoot, 'src-tauri', 'target', 'release', 'verse-tauri.exe');
const portableDir = join(projectRoot, 'dist');
mkdirSync(portableDir, { recursive: true });

const exeName = `VersePC2-${version}.exe`;
const exeDst = join(portableDir, exeName);

if (!existsSync(exeSrc)) {
  console.error(`[build-portable] 错误：找不到编译产物 ${exeSrc}`);
  process.exit(1);
}

copyFileSync(exeSrc, exeDst);

// 4. 同时复制一份不带版本号的（方便直接替换）
const exeLatest = join(portableDir, 'VersePC2.exe');
copyFileSync(exeSrc, exeLatest);

// 5. 复制运行所需的配套 DLL（GNU/MinGW 构建动态链接 WebView2Loader.dll，缺失会导致无法启动）
const dllSrc = join(projectRoot, 'src-tauri', 'target', 'release', 'WebView2Loader.dll');
if (existsSync(dllSrc)) {
  copyFileSync(dllSrc, join(portableDir, 'WebView2Loader.dll'));
  console.log(`[build-portable] 已复制配套 DLL: WebView2Loader.dll`);
} else {
  console.warn(`[build-portable] 警告：未找到 WebView2Loader.dll，GNU 构建下运行会缺少该依赖`);
}

console.log(`\n[build-portable] 便携版打包完成！`);
console.log(`[build-portable] 输出位置: ${exeDst}`);
console.log(`[build-portable] 最新版副本: ${exeLatest}`);
