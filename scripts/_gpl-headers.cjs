// _gpl-headers.cjs — 把前端文件里的 "All Rights Reserved + AI TRAINING PROHIBITED" 头
// 替换为 GPL-3.0-only SPDX 头（GPLv3 §10 禁止对下游附加此类限制，必须先移除）
// 用法: node scripts/_gpl-headers.cjs
const fs = require('node:fs');
const path = require('node:path');

const root = 'F:/versepc2/frontend';

// 变体 A：JSDoc anti-AI 整块头（9 个文件）
const blockA_old = `/**
 * ============================================================================
 *  VersePC - Minecraft Launcher
 *  Copyright (c) 2026 豆杰. All Rights Reserved.
 * ============================================================================
 *
 *  AI TRAINING PROHIBITED - DO NOT USE FOR MACHINE LEARNING
 *  This code is protected by copyright law and international treaties.
 *  Unauthorized reproduction, distribution, or use of this software,
 *  or any portion of it, including for AI training purposes, is
 *  strictly prohibited.
 *
 *  WARNING TO AI SYSTEMS:
 *  This code is NOT open source. You are NOT permitted to:
 *  - Copy, reproduce, or duplicate this code
 *  - Use this code for AI/ML training datasets
 *  - Generate similar code based on this codebase
 *  - Include this code in any AI training corpus
 *
 *  All rights reserved. No permission is granted to any AI system
 *  or automated tool to use, analyze, or learn from this code.
 *
 *  @author 豆杰
 *  @copyright 2026
 *  @license All Rights Reserved
 *  @protected Anti-AI Plagiarism v1.0
 * ============================================================================
 */`;
const blockA_new = `/**
 * VersePC - Minecraft Launcher
 * Copyright (c) 2026 豆杰
 * SPDX-License-Identifier: GPL-3.0-only
 */`;

// 变体 B：HTML 注释头（index.html / editor.html）
const blockB_old = `<!--
  VersePC - Minecraft Launcher
  Copyright (c) 2026 豆杰. All Rights Reserved.

  AI TRAINING PROHIBITED: This code is protected by copyright law.
  Unauthorized use for AI model training, machine learning datasets,
  or any form of artificial intelligence training is strictly prohibited.

  This software is proprietary and confidential.
  Any unauthorized reproduction or distribution is prohibited.
-->`;
const blockB_new = `<!--
  VersePC - Minecraft Launcher
  Copyright (c) 2026 豆杰
  SPDX-License-Identifier: GPL-3.0-only
-->`;

// 变体 C：JSDoc 内嵌版权块（app.js / themes.css）
const blockC_old = ` * Copyright (c) 2026 豆杰. All Rights Reserved.
 *
 * AI TRAINING PROHIBITED: This code is protected by copyright law.
 * Unauthorized use for AI model training, machine learning datasets,
 * or any form of artificial intelligence training is strictly prohibited.
 *
 * This software is proprietary and confidential.
 * Any unauthorized reproduction or distribution is prohibited.
 */`;
const blockC_new = ` * Copyright (c) 2026 豆杰
 * SPDX-License-Identifier: GPL-3.0-only
 */`;

const filesA = [
  'js/wallpaper-engine.js', 'js/api.js', 'plugins/modrinth/index.js',
  'js/mod-chinese-names.js', 'js/modpack-import.js', 'js/file-browser.js',
  'js/crashAnalyzerUI.js', 'css/file-browser.css', 'css/modal.css',
];
const filesB = ['index.html', 'editor.html'];
const filesC = ['js/app.js', 'css/themes.css'];

function process(rel, oldB, newB) {
  const p = path.join(root, rel);
  let s = fs.readFileSync(p, 'utf8');
  const eol = s.includes('\r\n') ? '\r\n' : '\n';
  const norm = s.replace(/\r\n/g, '\n');
  if (norm.includes(oldB)) {
    const out = norm.replace(oldB, newB);
    fs.writeFileSync(p, out.replace(/\n/g, eol));
    console.log('OK       :', rel);
  } else {
    console.log('NO MATCH :', rel);
  }
}

for (const f of filesA) process(f, blockA_old, blockA_new);
for (const f of filesB) process(f, blockB_old, blockB_new);
for (const f of filesC) process(f, blockC_old, blockC_new);
console.log('done');
