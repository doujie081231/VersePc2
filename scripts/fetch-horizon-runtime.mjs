// scripts/fetch-horizon-runtime.mjs
//
// 下载 PC2 内置的 Horizon 运行时包（官方便携 zip，约 26.9MB）。
//
// 为什么单独一个脚本：这个 zip 被 `include_bytes!` 编进 exe，让用户不必再等 27MB 下载，
// 也不让我们的站点出带宽。它是二进制产物、会随上游更新，所以不进 git；
// 构建前若缺失就由本脚本拉一份（优先国内可达的加速镜像）。
//
// 用法：
//   node scripts/fetch-horizon-runtime.mjs           # 缺失才下载
//   node scripts/fetch-horizon-runtime.mjs --force   # 强制重下
//
// 可用环境变量覆盖来源：
//   HORIZON_RUNTIME_URL      首选地址
//   HORIZON_RUNTIME_SIZE     期望字节数（校验用，0 = 不校验）

import { createWriteStream, existsSync, mkdirSync, renameSync, statSync, unlinkSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Readable } from 'node:stream';
import { pipeline } from 'node:stream/promises';

const __dirname = dirname(fileURLToPath(import.meta.url));
const projectRoot = resolve(__dirname, '..');
const target = join(projectRoot, 'src-tauri', 'assets', 'horizon-launcher-windows-x64.zip');

const UPSTREAM =
  'https://github.com/horizon-rust/horizon/releases/download/launcher%400.2.105/horizon-launcher-windows-x64.zip';
const EXPECTED_SIZE = Number(process.env.HORIZON_RUNTIME_SIZE || 26933819);
const MIRRORS = ['https://gh-proxy.com/', 'https://ghfast.top/', 'https://ghproxy.net/'];

const force = process.argv.includes('--force');

if (!force && existsSync(target)) {
  const size = statSync(target).size;
  if (!EXPECTED_SIZE || size === EXPECTED_SIZE) {
    console.log(`[fetch-runtime] 已存在（${(size / 1048576).toFixed(1)} MB），跳过`);
    process.exit(0);
  }
  console.log(`[fetch-runtime] 现有文件大小 ${size} 不符（期望 ${EXPECTED_SIZE}），重新下载`);
}

const candidates = [
  process.env.HORIZON_RUNTIME_URL,
  ...MIRRORS.map((m) => m + UPSTREAM),
  UPSTREAM,
].filter(Boolean);

mkdirSync(dirname(target), { recursive: true });

for (const url of candidates) {
  const tmp = `${target}.part`;
  try {
    console.log(`[fetch-runtime] 下载 ${url}`);
    const res = await fetch(url, { redirect: 'follow' });
    if (!res.ok || !res.body) throw new Error(`HTTP ${res.status}`);
    await pipeline(Readable.fromWeb(res.body), createWriteStream(tmp));
    const size = statSync(tmp).size;
    if (EXPECTED_SIZE && size !== EXPECTED_SIZE) {
      throw new Error(`大小不符：${size} ≠ ${EXPECTED_SIZE}`);
    }
    renameSync(tmp, target);
    console.log(`[fetch-runtime] 完成：${(size / 1048576).toFixed(1)} MB → ${target}`);
    process.exit(0);
  } catch (err) {
    console.warn(`[fetch-runtime] 失败（${err.message}），换下一个通道`);
    try {
      if (existsSync(tmp)) unlinkSync(tmp);
    } catch {
      /* 清理失败不影响主流程 */
    }
  }
}

console.error('[fetch-runtime] 所有通道都失败。可手动放一份到 src-tauri/assets/ 后再构建。');
process.exit(1);
