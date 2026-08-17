// build-portable.mjs — 便携版打包脚本
// 流程：准备前端资源 → tauri build → 复制 exe 到便携版目录
// 构建产物默认放项目内（src-tauri/target 与 dist）。
// 如需迁移到其它盘（如 E 盘省 C 盘空间），设置环境变量 VERSEPC2_BUILD_ROOT 指向目标目录即可。
import { execSync } from 'node:child_process';
import { copyFileSync, mkdirSync, existsSync, readFileSync } from 'node:fs';
import { join, dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const projectRoot = resolve(__dirname, '..');

// 构建根目录：便携版输出目录 dist 的根；默认项目内，设了 VERSEPC2_BUILD_ROOT 则重定向
const buildRoot = process.env.VERSEPC2_BUILD_ROOT || projectRoot;
// cargo 构建产物（target）目录：默认放 E 盘（E:\VerseTools\.verse-target），避免打包占满 C 盘；
// 工具链（.rustup/.cargo）也物理地位于 E 盘。可用 VERSEPC2_TARGET_DIR 覆盖。
const cargoTargetDir =
  process.env.VERSEPC2_TARGET_DIR ||
  join('E:', 'VerseTools', '.verse-target');
const portableDir = join(buildRoot, 'dist');

function run(cmd) {
  console.log(`\n> ${cmd}`);
  execSync(cmd, { stdio: 'inherit', cwd: projectRoot, shell: true });
}

function firstExisting(paths) {
  for (const p of paths) {
    if (existsSync(p)) return p;
  }
  return null;
}

// 1. 读取版本号
const tauriConf = JSON.parse(readFileSync(join(projectRoot, 'src-tauri', 'tauri.conf.json'), 'utf8'));
const version = tauriConf.version || '0.0.0';
console.log(`[build-portable] 版本号: ${version}`);
console.log(`[build-portable] 构建目录: ${buildRoot}`);

// 2. 重定向 Cargo 构建产物到 E 盘（避免 C 盘 target 目录不断膨胀）
process.env.CARGO_TARGET_DIR = cargoTargetDir;

// 3. 编译 Tauri（只生成 exe，不打包安装包）
//    tauri.conf.json 的 beforeBuildCommand 会自动执行 prepare-frontend.mjs
run('npx tauri build');

// 4. 定位编译产物 exe（CARGO_TARGET_DIR 生效时在 E 盘，否则回退到项目 target）
const exeSrc = firstExisting([
  join(cargoTargetDir, 'release', 'verse-tauri.exe'),
  join(projectRoot, 'src-tauri', 'target', 'release', 'verse-tauri.exe'),
]);
if (!exeSrc) {
  console.error('[build-portable] 错误：找不到编译产物 verse-tauri.exe');
  process.exit(1);
}
console.log(`[build-portable] 编译产物: ${exeSrc}`);

mkdirSync(portableDir, { recursive: true });

const exeName = `VersePC2-${version}.exe`;
copyFileSync(exeSrc, join(portableDir, exeName));
copyFileSync(exeSrc, join(portableDir, 'VersePC2.exe'));

// 5. 复制运行所需的配套 DLL（GNU/MinGW 构建动态链接 WebView2Loader.dll，缺失会导致无法启动）
//    优先使用仓库权威副本（src-tauri/WebView2Loader.dll），再依次回退 cargo target、项目 target、旧便携目录；
//    全部找不到则中止打包，避免产出缺 DLL 的坏包
const dllSrc = firstExisting([
  join(projectRoot, 'src-tauri', 'WebView2Loader.dll'),
  join(cargoTargetDir, 'release', 'WebView2Loader.dll'),
  join(projectRoot, 'src-tauri', 'target', 'release', 'WebView2Loader.dll'),
  join(portableDir, 'WebView2Loader.dll'),
]);
if (!dllSrc) {
  console.error('[build-portable] 错误：未找到 WebView2Loader.dll（GNU 构建必需），已中止打包。请将 WebView2Loader.dll 放到 src-tauri/ 后重试');
  process.exit(1);
}
copyFileSync(dllSrc, join(portableDir, 'WebView2Loader.dll'));
console.log(`[build-portable] 已复制配套 DLL: ${dllSrc}`);

console.log(`\n[build-portable] 便携版打包完成！`);
console.log(`[build-portable] 输出目录: ${portableDir}`);
console.log(`[build-portable] 版本文件: ${exeName}`);
console.log(`[build-portable] 最新版副本: VersePC2.exe`);
