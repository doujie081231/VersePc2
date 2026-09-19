// _push-main.cjs — 全量同步脚本：遍历本地源码目录，与 GitHub main 逐文件比对（git blob SHA），只推送差异/新增
// 用法: node scripts/_push-main.cjs <token> [commit-message-prefix]
// 排除项：构建产物、工具链、运行时数据、构建日志、本地配置等（见 EXCLUDE_*）
const { readFileSync, readdirSync, statSync } = require('node:fs');
const { join, resolve, sep } = require('node:path');
const https = require('node:https');
const crypto = require('node:crypto');

const TOKEN = process.argv[2] || '';
const MSG = process.argv[3] || 'sync';
const REPO = 'doujie081231/VersePc2';
const ROOT = resolve(__dirname, '..');

// 排除的目录名（任意层级命中即跳过）
const EXCLUDE_DIRS = new Set([
  '.git', '.tools', 'dist', 'node_modules', '.verse-target', '.trae', '.vscode',
  'docs-gh', 'target', 'data', 'gen', '.cargo', 'installer-app', 'winstubs',
]);
// 排除的散文件（按文件名）
const EXCLUDE_FILES = new Set([
  'config.toml.bak', '.gitconfig',
]);
// 构建日志 / 临时文件（正则，匹配文件名）
const JUNK_RE = /^(build_err.*\.txt|build_out\.txt|build_release\.txt|err_.*\.txt|err_short\.txt|errors\.txt|warn\.txt|.*\.log)$/i;

function isExcludedFile(name) {
  return EXCLUDE_FILES.has(name) || JUNK_RE.test(name);
}

// 递归收集相对路径（/ 分隔）
function walk(dir, rel) {
  const out = [];
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const r = rel ? rel + '/' + e.name : e.name;
    if (e.isDirectory()) {
      if (EXCLUDE_DIRS.has(e.name)) continue;
      out.push(...walk(join(dir, e.name), r));
    } else if (!isExcludedFile(e.name)) {
      out.push(r);
    }
  }
  return out;
}

// git blob SHA（GitHub Contents API 的 sha 即 blob sha）
function blobSha(buf) {
  const h = crypto.createHash('sha1');
  h.update(`blob ${buf.length}\0`);
  h.update(buf);
  return h.digest('hex');
}

function request(method, path, body, rawBody) {
  return new Promise((resolvePromise) => {
    const payload = body === undefined ? null : JSON.stringify(body);
    const req = https.request({
      hostname: 'api.github.com',
      path,
      method,
      headers: {
        'Authorization': 'Bearer ' + TOKEN,
        'Accept': 'application/vnd.github+json',
        'User-Agent': 'VersePC',
        ...(rawBody ? { 'Content-Type': 'application/octet-stream', 'Content-Length': rawBody.length }
          : payload ? { 'Content-Type': 'application/json', 'Content-Length': Buffer.byteLength(payload) } : {}),
      },
    }, (res) => {
      const chunks = [];
      res.on('data', (c) => chunks.push(c));
      res.on('end', () => {
        const text = Buffer.concat(chunks).toString('utf8');
        let parsed = null;
        try { parsed = text ? JSON.parse(text) : null; } catch (_) {}
        resolvePromise({ status: res.statusCode, body: parsed });
      });
    });
    req.on('error', (e) => resolvePromise({ status: 0, body: { message: String(e.message) } }));
    if (rawBody) req.write(rawBody);
    else if (payload) req.write(payload);
    req.end();
  });
}

const api = (method, path, body) => request(method, path, body);

(async () => {
  if (!TOKEN) { console.log('缺少 token'); process.exit(1); }

  // 1. 取 main 头 + 完整文件树（path -> blob sha）
  const ref = await api('GET', `/repos/${REPO}/git/ref/heads/main`);
  if (ref.status !== 200) { console.log('获取 main 失败:', ref.body); process.exit(1); }
  const headSha = ref.body.object.sha;
  const tree = await api('GET', `/repos/${REPO}/git/trees/${headSha}?recursive=1`);
  if (tree.status !== 200) { console.log('获取文件树失败:', tree.body); process.exit(1); }
  const remote = new Map();
  for (const it of (tree.body.tree || [])) {
    if (it.type === 'blob') remote.set(it.path, it.sha);
  }
  console.log('main head:', headSha, '远端文件数:', remote.size);

  // 2. 本地遍历
  const files = walk(ROOT, '');
  console.log('本地文件数:', files.length);

  // 3. 逐文件比对推送
  let pushed = 0, created = 0, same = 0, failed = 0;
  for (const rel of files) {
    const localPath = join(ROOT, rel.split('/').join(sep));
    let buf;
    try { buf = readFileSync(localPath); } catch (e) { failed++; console.log('READ FAIL:', rel, e.message); continue; }
    if (buf.length > 95 * 1024 * 1024) { failed++; console.log('SKIP(>95MB):', rel); continue; }

    const localSha = blobSha(buf);
    const remoteSha = remote.get(rel);
    if (remoteSha === localSha) { same++; continue; }

    const url = `/repos/${REPO}/contents/${rel.split('/').map(encodeURIComponent).join('/')}`;
    const body = { message: `${MSG} ${rel}`, content: buf.toString('base64'), branch: 'main' };
    if (remoteSha) body.sha = remoteSha;
    const put = await api('PUT', url, body);
    if (put.status === 200 || put.status === 201) {
      if (remoteSha) { pushed++; console.log('PUSHED  :', rel); }
      else { created++; console.log('CREATED :', rel); }
    } else {
      failed++; console.log('FAIL    :', rel, put.body ? (put.body.message || put.status) : 'no resp');
    }
  }
  console.log(`\nDone. pushed=${pushed} created=${created} same=${same} failed=${failed}`);
})();
