#!/usr/bin/env node
// publish-quark-releases.mjs — 发布流程：把三平台安装包上传到夸克网盘 VERSE 目录
//
// 目录结构（夸克「夸克上传文件/VersePC」）：
//   VersePC/            ← 根目录
//   ├── VersePC2/         Windows 安装包
//   ├── VersePc2-Linux/   Linux 安装包
//   └── VersePc2-macOS/   macOS 安装包（x86_64 + aarch64）
//
// 行为：上传新包到对应子目录，成功后把该目录原有的旧包移动到
//       VersePC/旧版本-待删除/（夸克 Skill/OpenAPI 未提供删除接口）。
//
// 用法（先完成 npm run build，并已安装、授权夸克网盘 Skill）：
//   node scripts/publish-quark-releases.mjs --version 1.9.2 \
//     --win dist/VersePC2.exe \
//     --linux dist/VersePC2-linux-x86_64 \
//     --mac-x64 dist/VersePC2-macos-x86_64.zip \
//     --mac-arm64 dist/VersePC2-macos-aarch64.zip
//
// 可用环境变量覆盖：
//   QUARK_CLI          夸克 Skill CLI 路径
//   QUARK_VERSE_FID    VersePC 根目录 FID
//   QUARK_WIN_FID / QUARK_LINUX_FID / QUARK_MAC_FID  三个子目录 FID

import { spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, basename, resolve } from 'node:path';

const DEFAULT_CLI = 'C:/Users/huang/.trae-cn/skills/quarkclouddrive/scripts/quark-drive.cjs';
const DEFAULT_VERSE_FID = '~1LMmS_Tdwo1Mf8IYOoCpiino4biYJLZlhT0OPwd9ZQrOEk_AwzBeUW7FxoN_obqnZX9cVpWS6E302EYf-wksYaQ|Xuix1PGk8IU';
const DEFAULT_WIN_FID = '~1YYQwIvy_nd08zfiolIbu0-pbEssYmDTAJA51_hdGd4pb1nDSeWn3CO-WpD17gpyPFm2D4nVGKS5x_rOUF33wn8|a2h5IwgPzCM';
const DEFAULT_LINUX_FID = '~1Y2PTFahrXQAxlXAvf7SAG0rXP6xLm6_hfwINTQfxNYS4DgakwEDuK2sB12xR0I-i16xpByLGE2H7_vLOmVVszw|YIr-XYzwCPw';
const DEFAULT_MAC_FID = '~1VcCFn8JdtZEyMOgH2PCZfTTlD-cprBLZzYcDb59MKRJRS42BhN8_BECjvReep-qrE4UXseYP02YN0aQ7tWxqgc|CJ73ch3M6Z4';

const CLI = process.env.QUARK_CLI || DEFAULT_CLI;
const SESSION_ID = `${Math.floor(Date.now() / 1000)}-${Math.random().toString(36).slice(2, 8)}`;
const SESSION_INPUT = '将 Windows、Linux 和 macOS 的安装包上传到夸克 VERSE 目录，替换旧版本';

function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i++) {
    const key = argv[i];
    if (!key.startsWith('--')) continue;
    out[key.slice(2)] = argv[i + 1];
    i++;
  }
  return out;
}

function runCli(args) {
  const env = { ...process.env, TRAE_RUNTIME: '1', TRAE_PRODUCT_REGION: 'cn' };
  const res = spawnSync(process.execPath, [CLI, ...args, '--session-input', SESSION_INPUT, '--session-id', SESSION_ID], {
    encoding: 'utf8',
    env,
    maxBuffer: 128 * 1024 * 1024,
    timeout: 30 * 60 * 1000,
  });
  const lines = String(res.stdout || '').split(/\r?\n/).filter(Boolean);
  const results = [];
  for (const line of lines) {
    try {
      const obj = JSON.parse(line);
      if (obj && obj.type === 'result') results.push(obj);
    } catch {
      // 忽略非 JSON 调试输出
    }
  }
  const final = results[results.length - 1];
  if (!final) {
    throw new Error(`夸克 CLI 无有效输出: ${String(res.stderr || res.error || '').slice(0, 500)}`);
  }
  if (res.status !== 0 || final.code !== 0) {
    throw new Error(final.msg || `夸克 CLI 退出码 ${res.status}`);
  }
  return final;
}

function listFolder(fid) {
  const taskDir = mkdtempSync(join(tmpdir(), 'verse-quark-'));
  try {
    const result = runCli(['browse', '--parent-fid', fid, '--all', '--task-dir', taskDir]);
    const filePath = result?.data?.file_path;
    if (!filePath || !existsSync(filePath)) return [];
    return readFileSync(filePath, 'utf8')
      .split(/\r?\n/)
      .filter(Boolean)
      .map((line) => JSON.parse(line));
  } finally {
    rmSync(taskDir, { recursive: true, force: true });
  }
}

function ensureTrashFid() {
  const root = process.env.QUARK_VERSE_FID || DEFAULT_VERSE_FID;
  const existing = listFolder(root).find((f) => f.file_type === '0' && f.filename === '旧版本-待删除');
  if (existing) return existing.fid;
  const created = runCli(['create-folder', '--dir-path', '旧版本-待删除', '--parent-fid', root]);
  return created?.data?.fid;
}

function uploadAndArchive(label, fid, files) {
  const oldFiles = listFolder(fid);
  const newPaths = files.map((p) => resolve(p));
  for (const p of newPaths) {
    if (!existsSync(p)) throw new Error(`[${label}] 文件不存在: ${p}`);
  }

  // 先移走旧文件再上传：夸克对同名文件会自动改名（VersePC2(1).exe），
  // 先清空目录才能保证新包保留正确的文件名。上传失败时把旧文件移回来。
  let trashFid = null;
  if (oldFiles.length > 0) {
    trashFid = ensureTrashFid();
    if (!trashFid) throw new Error(`[${label}] 无法创建旧版本目录`);
    console.log(`[${label}] 移动 ${oldFiles.length} 个旧文件到「旧版本-待删除」`);
    runCli(['move', ...oldFiles.map((f) => f.fid), '--target-fid', trashFid]);
  }

  console.log(`[${label}] 上传 ${newPaths.map((p) => basename(p)).join(', ')}`);
  let uploaded;
  try {
    uploaded = runCli(['upload', ...newPaths, '--parent-fid', fid]);
    console.log(`[${label}] 上传完成: ${uploaded.msg}`);
  } catch (e) {
    if (trashFid && oldFiles.length > 0) {
      console.warn(`[${label}] 上传失败，回滚旧文件`);
      try {
        runCli(['move', ...oldFiles.map((f) => f.fid), '--target-fid', fid]);
      } catch (rollbackError) {
        console.warn(`[${label}] 回滚失败: ${rollbackError.message}`);
      }
    }
    throw e;
  }
  return uploaded;
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  const version = args.version || '';
  const win = args.win || '';
  const linux = args.linux || '';
  const macX64 = args['mac-x64'] || '';
  const macArm64 = args['mac-arm64'] || '';

  if (!version || !win) {
    console.error('用法: node scripts/publish-quark-releases.mjs --version 1.9.2 --win <exe> [--linux <file>] [--mac-x64 <zip>] [--mac-arm64 <zip>]');
    process.exit(1);
  }

  uploadAndArchive('Windows', process.env.QUARK_WIN_FID || DEFAULT_WIN_FID, [win]);
  if (linux) uploadAndArchive('Linux', process.env.QUARK_LINUX_FID || DEFAULT_LINUX_FID, [linux]);
  const macFiles = [macX64, macArm64].filter(Boolean);
  if (macFiles.length > 0) uploadAndArchive('macOS', process.env.QUARK_MAC_FID || DEFAULT_MAC_FID, macFiles);

  console.log(`\n[quark] v${version} 三平台安装包已更新到夸克 VERSE 目录`);
}

main();
