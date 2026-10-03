// build-portable.mjs — 便携版打包脚本
// 流程：准备前端资源 → tauri build → 复制 exe 到便携版目录
// 构建产物默认放项目内（src-tauri/target 与 dist）。
// 如需迁移到其它盘（如 E 盘省 C 盘空间），设置环境变量 VERSEPC2_BUILD_ROOT 指向目标目录即可。
import { execSync } from 'node:child_process';
import { copyFileSync, mkdirSync, existsSync, readFileSync, rmSync, statSync, readdirSync } from 'node:fs';
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

// 打包产物统一为固定名 VersePC2.exe，不带版本后缀；
// 升级时更新器把新包覆盖到正在运行的 exe 文件名上，使安装后的 exe 始终是 VersePC2.exe。
copyFileSync(exeSrc, join(portableDir, 'VersePC2.exe'));

// 5. 配套 WebView2Loader.dll（可选）
//    - MSVC 构建会把 WebView2Loader 静态链接进 exe（import 表不含 WebView2Loader.dll），便携包只需单个 exe；
//    - GNU/MinGW 构建是动态链接，必须随包附带 WebView2Loader.dll，否则无法启动。
//    下面探测 exe 是否真的依赖该 DLL：依赖则要求并复制副本，不依赖（已静态内嵌）则跳过，实现单文件便携包。
function exeImports(file, dllName) {
  try {
    // 定位导入表检测工具：优先 PATH 上的 dumpbin，其次 VSBuildTools/Visual Studio 下的 dumpbin，再次 MinGW objdump
    function findDumpbin() {
      try {
        const fromPath = execSync('where.exe dumpbin', { encoding: 'utf8', shell: true })
          .split(/\r?\n/).find(l => l.trim() && l.includes('dumpbin.exe'));
        if (fromPath) return fromPath.trim();
      } catch { /* PATH 上无 dumpbin */ }
      // 在常见 VS 安装根里递归查找 dumpbin.exe（Hostx64\x64 优先即最新版本之一）
      const roots = [
        process.env.VSINSTALLDIR,
        'D:\\VerseTools\\VSBuildTools',
        'C:\\Program Files\\Microsoft Visual Studio',
        'C:\\Program Files (x86)\\Microsoft Visual Studio',
      ].filter(Boolean);
      for (const root of roots) {
        const results = [];
        (function walk(dir, depth) {
          if (depth > 12) return;
          let entries;
          try { entries = readdirSync(dir, { withFileTypes: true }); } catch { return; }
          for (const e of entries) {
            if (e.name.toLowerCase() === 'dumpbin.exe') results.push(join(dir, e.name));
            if (e.isDirectory()) walk(join(dir, e.name), depth + 1);
          }
        })(root, 0);
        results.sort((a, b) => (b.includes('Hostx64\\x64') ? 1 : 0) - (a.includes('Hostx64\\x64') ? 1 : 0));
        if (results.length) return results[0];
      }
      return null;
    }
    const dumpbin = findDumpbin();
    const mingwObjdump = join(
      process.env.VERSEPC2_MINGW_BIN || join(projectRoot, '.tools', 'msys64', 'mingw64', 'bin'),
      'objdump.exe'
    );
    let out = null;
    if (dumpbin) {
      out = execSync(`"${dumpbin}" /dependents "${file}"`, {
        encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'], maxBuffer: 16 * 1024 * 1024,
      });
    } else if (existsSync(mingwObjdump)) {
      out = execSync(`"${mingwObjdump}" -p "${file}"`, {
        encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'], maxBuffer: 16 * 1024 * 1024,
      });
    } else {
      // 无可用的导入表检测工具时不硬判；由调用方决定如何处理（此处视为未依赖）。
      return false;
    }
    return new RegExp(`\\b${dllName.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}\\b`, 'i').test(out || '');
  } catch {
    return false;
  }
}

const exeNeedsLoader = exeImports(exeSrc, 'WebView2Loader.dll');
if (exeNeedsLoader) {
  // 动态链接构建：优先仓库权威副本（src-tauri/WebView2Loader.dll），再回退 cargo target、项目 target、旧便携目录；
  // 找不到则中止打包，避免产出缺 DLL 的坏包
  const dllSrc = firstExisting([
    join(projectRoot, 'src-tauri', 'WebView2Loader.dll'),
    join(cargoTargetDir, 'release', 'WebView2Loader.dll'),
    join(projectRoot, 'src-tauri', 'target', 'release', 'WebView2Loader.dll'),
    join(portableDir, 'WebView2Loader.dll'),
  ]);
  if (!dllSrc) {
    console.error('[build-portable] 错误：exe 依赖 WebView2Loader.dll 但未找到该 DLL，已中止打包。请将 WebView2Loader.dll 放到 src-tauri/ 后重试');
    process.exit(1);
  }
  copyFileSync(dllSrc, join(portableDir, 'WebView2Loader.dll'));
  console.log(`[build-portable] 已复制配套 DLL: ${dllSrc}`);
} else {
  console.log('[build-portable] WebView2Loader 已静态内嵌于 exe，便携包输出单文件（不含配套 DLL）');
}

// 6. 场景渲染器组件包（可选）：约 314MB，不进入主便携包。
//    设置 VERSEPC2_BUILD_RENDERER_PACKAGE=1 时，把渲染器闭包打包为
//    dist/scene-renderer-windows-x64.zip，作为独立 release 资产发布；
//    用户在主程序内可下载该组件包到 <数据目录>/scene-renderer/。
const rendererSrcDir = process.env.VERSEPC2_RENDERER_DIR || join(projectRoot, '.tools', 'linux-wallpaperengine', 'build-win', 'output');
const mingwBinDir = process.env.VERSEPC2_MINGW_BIN || join(projectRoot, '.tools', 'msys64', 'mingw64', 'bin');
const objdump = join(mingwBinDir, 'objdump.exe');
const rendererDestDir = join(portableDir, '.renderer-staging');

// Windows 系统 DLL（进程内建，无需携带）
const SYSTEM_DLL_RE = /^(KERNEL32|KERNELBASE|msvcrt|USER32|GDI32|ADVAPI32|SHELL32|OLE32|OLEAUT32|WS2_32|WINMM|SETUPAPI|VERSION|IMM32|ole32|oleaut32|dbghelp|ntdll|RPCRT4|USP10|DWrite|USERENV|MSIMG32|bcrypt|ncrypt|CRYPT32|IPHLPAPI|WSOCK32|bcryptprimitives|SHLWAPI|DNSAPI|gdiplus)\.dll$/i;

function isSystemDll(name) {
  return SYSTEM_DLL_RE.test(name) || /^(api-ms-|ext-ms-)/i.test(name);
}

function dllImports(dllPath) {
  try {
    const out = execSync(`"${objdump}" -p "${dllPath}"`, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'], maxBuffer: 64 * 1024 * 1024 });
    const names = [];
    for (const m of out.matchAll(/DLL Name:\s*([^\r\n]+)/g)) names.push(m[1].trim());
    return names;
  } catch (e) {
    return [];
  }
}

function copyDllsClosure(entryFiles, destDir) {
  if (!existsSync(objdump)) {
    console.warn(`[build-portable] 警告：找不到 ${objdump}，跳过渲染器依赖闭包解析`);
    return;
  }
  mkdirSync(destDir, { recursive: true });
  // 用小写名去重（Windows 文件系统大小写不敏感）
  const queue = [...entryFiles];
  const copied = new Set();
  const queued = new Set(queue.map(f => f.toLowerCase()));
  while (queue.length) {
    const file = queue.pop();
    // 渲染器目录优先，其次 MinGW 运行库目录
    let src = join(rendererSrcDir, file);
    if (!existsSync(src)) src = join(mingwBinDir, file);
    if (!existsSync(src)) {
      console.warn(`[build-portable] 警告：缺少渲染器依赖文件 ${file}（场景壁纸功能将不可用）`);
      continue;
    }
    copyFileSync(src, join(destDir, file));
    copied.add(file.toLowerCase());
    for (const imp of dllImports(src)) {
      if (isSystemDll(imp)) continue;
      const key = imp.toLowerCase();
      if (copied.has(key) || queued.has(key)) continue;
      if (existsSync(join(rendererSrcDir, imp)) || existsSync(join(mingwBinDir, imp))) {
        queued.add(key);
        queue.push(imp);
      } else {
        console.warn(`[build-portable] 警告：找不到依赖 ${imp}（${file} 需要它）`);
      }
    }
  }
  return copied;
}

if (process.env.VERSEPC2_BUILD_RENDERER_PACKAGE === '1' && existsSync(join(rendererSrcDir, 'verse-scene-renderer.exe'))) {
  // 闭包收集到 staging 目录 → tar 打包为 zip（zip 根目录即 exe 与 DLL）
  const staging = rendererDestDir;
  rmSync(staging, { recursive: true, force: true });
  mkdirSync(staging, { recursive: true });
  copyDllsClosure(
    ['verse-scene-renderer.exe', 'libverse-scene-renderer-lib.dll',
     'opengl32.dll', 'libgallium_wgl.dll', 'libLLVM-22.dll', 'libkissfft-float.dll'],
    staging
  );
  const zipPath = join(portableDir, 'scene-renderer-windows-x64.zip');
  if (existsSync(zipPath)) rmSync(zipPath, { force: true });
  run(`tar -a -cf "${zipPath}" -C "${staging}" .`);
  rmSync(staging, { recursive: true, force: true });
  const zipSize = existsSync(zipPath) ? (statSync(zipPath).size / 1024 / 1024).toFixed(1) : '0';
  console.log(`[build-portable] 场景渲染器组件包已生成: dist/scene-renderer-windows-x64.zip (${zipSize} MB)`);
} else {
  console.log('[build-portable] 跳过场景渲染器组件包（设置 VERSEPC2_BUILD_RENDERER_PACKAGE=1 时生成，组件由用户按需下载）');
}

console.log(`\n[build-portable] 便携版打包完成！`);
console.log(`[build-portable] 输出目录: ${portableDir}`);
console.log(`[build-portable] 产物: VersePC2.exe`);
