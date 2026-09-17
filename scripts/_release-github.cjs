// _release-github.cjs — 创建 GitHub tag + release 并上传资产（Node https，避免 curl 命令行超长问题）
// 用法: node scripts/_release-github.cjs <token> <tag> <release-title> <release-notes-file> <asset1> <asset2> ...
// 资产参数为本地绝对路径，上传后文件名取 basename
const { readFileSync, existsSync } = require('node:fs');
const { basename } = require('node:path');
const https = require('node:https');

const TOKEN = process.argv[2] || '';
const TAG = process.argv[3] || '';
const TITLE = process.argv[4] || '';
const NOTES_FILE = process.argv[5] || '';
const ASSETS = process.argv.slice(6);
const REPO = 'doujie081231/VersePc2';

function request(hostname, method, path, headers, body, rawBody) {
  return new Promise((resolvePromise) => {
    const req = https.request({ hostname, path, method, headers }, (res) => {
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
    else if (body !== undefined) req.write(JSON.stringify(body));
    req.end();
  });
}

const api = (method, path, body) => request('api.github.com', method, path, {
  'Authorization': 'Bearer ' + TOKEN,
  'Accept': 'application/vnd.github+json',
  'User-Agent': 'VersePC',
  ...(body !== undefined ? { 'Content-Type': 'application/json', 'Content-Length': Buffer.byteLength(JSON.stringify(body)) } : {})
}, body);

(async () => {
  if (!TOKEN || !TAG) { console.log('缺少 token/tag 参数'); process.exit(1); }
  const notes = readFileSync(NOTES_FILE, 'utf8');

  // 1. main 分支 HEAD sha
  const ref = await api('GET', '/repos/' + REPO + '/git/ref/heads/main');
  if (ref.status !== 200) { console.log('获取 main sha 失败:', ref.body); process.exit(1); }
  const sha = ref.body.object.sha;
  console.log('main head sha:', sha);

  // 2. 创建 tag（已存在则忽略）
  const tag = await api('POST', '/repos/' + REPO + '/git/refs', { ref: 'refs/tags/' + TAG, sha });
  if (tag.status === 201) console.log('tag 已创建:', TAG);
  else if (tag.status === 422 && /already exists/i.test(JSON.stringify(tag.body))) console.log('tag 已存在:', TAG);
  else { console.log('创建 tag 失败:', tag.body); process.exit(1); }

  // 3. 创建 release（已存在则复用）
  let releaseId = null;
  const rel = await api('POST', '/repos/' + REPO + '/releases', { tag_name: TAG, name: TITLE, body: notes, draft: false, prerelease: false });
  if (rel.status === 201) { releaseId = rel.body.id; console.log('release 已创建 id=', releaseId); }
  else if (rel.status === 422 && /already exists/i.test(JSON.stringify(rel.body))) {
    const list = await api('GET', '/repos/' + REPO + '/releases?per_page=100');
    const found = (list.body || []).find((r) => r.tag_name === TAG);
    if (found) { releaseId = found.id; console.log('release 已存在 id=', releaseId); }
    else { console.log('复用 release 失败:', rel.body); process.exit(1); }
  } else { console.log('创建 release 失败:', rel.body); process.exit(1); }

  // 4. 上传资产
  for (const assetPath of ASSETS) {
    if (!existsSync(assetPath)) { console.log('资产不存在，跳过:', assetPath); continue; }
    const buf = readFileSync(assetPath);
    const name = basename(assetPath);
    const url = '/repos/' + REPO + '/releases/' + releaseId + '/assets?name=' + encodeURIComponent(name);
    const up = await request('uploads.github.com', 'POST', url, {
      'Authorization': 'Bearer ' + TOKEN,
      'Accept': 'application/vnd.github+json',
      'User-Agent': 'VersePC',
      'Content-Type': 'application/octet-stream',
      'Content-Length': buf.length
    }, undefined, buf);
    if (up.status === 201) console.log('资产上传成功:', name, buf.length, 'bytes');
    else console.log('资产上传失败:', name, up.body ? (up.body.message || up.status) : 'no resp');
  }
  console.log('\n完成。release:', 'https://github.com/' + REPO + '/releases/tag/' + TAG);
})();
