// build-verseportable.mjs — 加载 MSVC 环境 + 构建便携版（适配 D 盘）
import { execSync } from 'node:child_process';
import { existsSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';

const projectRoot = resolve('D:\\Versepc2');
const vcvars = 'D:\\vs\\VC\\Auxiliary\\Build\\vcvars64.bat';

// 1. 加载 vcvars64.bat 环境变量
console.log('[build] 加载 MSVC 环境:', vcvars);
if (!existsSync(vcvars)) {
  console.error('[build] 错误: 未找到 vcvars64.bat');
  process.exit(1);
}
const raw = execSync(`cmd /c "call "${vcvars}" >nul 2>&1 && set"`, { encoding: 'utf8' });
for (const line of raw.split('\n')) {
  const m = line.match(/^(.*?)=(.*)$/);
  if (m) process.env[m[1]] = m[2];
}
console.log('[build] MSVC 环境已加载');
console.log('[build] cl.exe:', process.env.VCToolsInstallDir ? '已定位' : '未定位');

// 2. 设置 Rust / Cargo / Node 环境
process.env.RUSTUP_HOME = 'D:\\tools\\rustup';
process.env.CARGO_HOME = 'D:\\tools\\cargo';
process.env.VERSEPC2_BUILD_ROOT = 'D:\\Versepc2';
process.env.VERSEPC2_TARGET_DIR = 'D:\\Versepc2\\.target-msvc';
process.env.CARGO_TARGET_DIR = 'D:\\Versepc2\\.target-msvc';

// 额外路径
const extraPaths = [
  'D:\\tools\\cargo\\bin',
  'D:\\tools\\rustup\\toolchains\\stable-x86_64-pc-windows-msvc\\bin',
  'C:\\Users\\Administrator\\AppData\\Roaming\\TRAE SOLO CN\\ModularData\\ai-agent\\vm\\tools\\node',
  'D:\\tools\\MinGit\\cmd',
];
process.env.PATH = extraPaths.join(';') + ';' + process.env.PATH;

// 3. 验证构建环境
console.log('\n[build] 验证环境:');
try {
  console.log('  rustc:', execSync('rustc --version', { encoding: 'utf8' }).trim());
  console.log('  cargo:', execSync('cargo --version', { encoding: 'utf8' }).trim());
  console.log('  node:', execSync('node --version', { encoding: 'utf8' }).trim());
  console.log('  npm:', execSync('npm --version', { encoding: 'utf8' }).trim());
  console.log('  npx:', execSync('npx --version', { encoding: 'utf8' }).trim());
} catch (e) {
  console.error('[build] 环境验证失败:', e.message);
  process.exit(1);
}

// 4. 运行构建脚本
console.log('\n[build] 开始执行 build-portable.mjs ...');
console.log('  VERSEPC2_BUILD_ROOT =', process.env.VERSEPC2_BUILD_ROOT);
console.log('  CARGO_TARGET_DIR =', process.env.CARGO_TARGET_DIR);
console.log('  cwd =', projectRoot);
execSync('node scripts/build-portable.mjs', {
  cwd: projectRoot,
  stdio: 'inherit',
  shell: true,
  env: process.env,
});