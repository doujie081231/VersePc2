#!/usr/bin/env node
// sign-update.mjs — 为 update.json 生成 Ed25519 发布签名
//
// 用法:
//   VERSE_UPDATE_KEY=<私钥 pem 路径> node scripts/sign-update.mjs [update.json]
//
// 私钥只保存在发布者本机，禁止提交到仓库。验签公钥硬编码在
// src-tauri/src/updater.rs（UPDATE_PUBLIC_KEY_B64），本脚本的规范化文本
// 必须与 Rust 侧 canonical_update_payload 保持一致。

import { createPrivateKey, sign } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

function escapeField(value) {
  return String(value == null ? '' : value)
    .replace(/\\/g, '\\\\')
    .replace(/\n/g, '\\n')
    .replace(/\r/g, '\\r')
    .replace(/\t/g, '\\t');
}

export function canonicalUpdatePayload(manifest) {
  const m = manifest && typeof manifest === 'object' ? manifest : {};
  const field = (k) => (typeof m[k] === 'string' ? m[k] : '');
  const files = m.files && typeof m.files === 'object' && !Array.isArray(m.files) ? m.files : {};
  const platforms = Object.keys(files).sort();

  let out = 'verse-update-v1\n';
  out += `version=${escapeField(field('version'))}\n`;
  out += `releaseName=${escapeField(field('releaseName'))}\n`;
  out += `releaseDate=${escapeField(field('releaseDate'))}\n`;
  out += `releaseNotes=${escapeField(field('releaseNotes'))}\n`;
  for (const p of platforms) {
    const f = files[p] && typeof files[p] === 'object' ? files[p] : {};
    const url = typeof f.url === 'string' ? f.url : '';
    const sha = typeof f.sha256 === 'string' ? f.sha256.trim().toLowerCase() : '';
    const size =
      typeof f.size === 'number' && Number.isFinite(f.size) && Number.isInteger(f.size) && f.size >= 0
        ? f.size
        : 0;
    out += `file\t${escapeField(p)}\t${escapeField(url)}\t${escapeField(sha)}\t${size}\n`;
  }
  return out;
}

function main() {
  const file = resolve(process.argv[2] || 'update.json');
  const keyPath = process.env.VERSE_UPDATE_KEY;
  if (!keyPath) {
    console.error('缺少 VERSE_UPDATE_KEY 环境变量（指向 Ed25519 私钥 pem 文件）');
    process.exit(1);
  }

  const manifest = JSON.parse(readFileSync(file, 'utf8'));
  delete manifest.signature;
  const payload = canonicalUpdatePayload(manifest);
  const signature = sign(null, Buffer.from(payload, 'utf8'), createPrivateKey(readFileSync(keyPath)));

  manifest.signature = signature.toString('base64');
  writeFileSync(file, `${JSON.stringify(manifest, null, 2)}\n`);
  console.log(`已签名 ${file}: ${manifest.signature}`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main();
}
