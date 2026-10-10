// _release-github.cjs — 创建 GitHub tag + release 并上传资产（通过 curl 走本地代理）
// 用法: node scripts/_release-github.cjs <token> <tag> <release-title> <release-notes-file> <asset1> <asset2> ...
// 资产参数为本地绝对路径，上传后文件名取 basename
// 代理：环境变量 VERSE_PROXY（默认 127.0.0.1:21081），设为 none 则直连
const { readFileSync, writeFileSync, existsSync, unlinkSync } = require('node:fs');
const { basename, join } = require('node:path');
const { execFileSync } = require('node:child_process');
const os = require('node:os');

const TOKEN = process.argv[2] || '';
const TAG = process.argv[3] || '';
const TITLE = process.argv[4] || '';
const NOTES_FILE = process.argv[5] || '';
const ASSETS = process.argv.slice(6);
const REPO = 'doujie081231/VersePc2';
const PROXY = (process.env.VERSE_PROXY || '127.0.0.1:21081') === 'none' ? '' : ['-x', 'http://' + (process.env.VERSE_PROXY || '127.0.0.1:21081')];
const TMP = os.tmpdir();

// 返回 { status, body }；body 为 JSON 或原始文本
function curl(method, url, extraArgs) {
  const respFile = join(TMP, 'verse-gh-resp-' + Date.now() + Math.random().toString(36).slice(2) + '.json');
  const args = [
    '-s', ...PROXY,
    '-X', method,
    '-H', 'Authorization: Bearer ' + TOKEN,
    '-H', 'Accept: application/vnd.github+json',
    '-H', 'User-Agent: VersePC',
    '-o', respFile,
    '-w', '%{http_code}',
    '--max-time', '120',
    url,
  ].concat(extraArgs || []);
  let code = '';
  try {
    code = execFileSync('curl.exe', args, { encoding: 'utf8' }).trim();
  } catch (e) {
    return { status: 0, body: 'curl error: ' + String(e.message).slice(0, 300) };
  }
  let text = '';
  try { text = readFileSync(respFile, 'utf8'); } catch (_) {}
  try { unlinkSync(respFile); } catch (_) {}
  let parsed = null;
  try { parsed = text ? JSON.parse(text) : null; } catch (_) { parsed = text.slice(0, 300); }
  return { status: Number(code) || 0, body: parsed };
}

function jsonFile(obj) {
  const f = join(TMP, 'verse-gh-body-' + Date.now() + Math.random().toString(36).slice(2) + '.json');
  writeFileSync(f, JSON.stringify(obj));
  return f;
}

(async () => {
  if (!TOKEN || !TAG) { console.log('缺少 token/tag 参数'); process.exit(1); }
  const notes = readFileSync(NOTES_FILE, 'utf8');

  // 1. main 分支 HEAD sha
  const ref = await curl('GET', 'https://api.github.com/repos/' + REPO + '/git/ref/heads/main');
  if (ref.status !== 200) { console.log('获取 main sha 失败:', ref.status, JSON.stringify(ref.body)); process.exit(1); }
  const sha = ref.body.object.sha;
  console.log('main head sha:', sha);

  // 2. 创建 tag（已存在则忽略）
  const tagBody = jsonFile({ ref: 'refs/tags/' + TAG, sha });
  const tag = await curl('POST', 'https://api.github.com/repos/' + REPO + '/git/refs', ['--data-binary', '@' + tagBody]);
  if (tag.status === 201) console.log('tag 已创建:', TAG);
  else if (tag.status === 422 && /already exists/i.test(JSON.stringify(tag.body))) console.log('tag 已存在:', TAG);
  else { console.log('创建 tag 失败:', tag.status, JSON.stringify(tag.body)); process.exit(1); }

  // 3. 创建 release（已存在则复用）
  let releaseId = null;
  const relBody = jsonFile({ tag_name: TAG, name: TITLE, body: notes, draft: false, prerelease: false });
  const rel = await curl('POST', 'https://api.github.com/repos/' + REPO + '/releases', ['--data-binary', '@' + relBody]);
  if (rel.status === 201) { releaseId = rel.body.id; console.log('release 已创建 id=', releaseId); }
  else if (rel.status === 422 && /already exists/i.test(JSON.stringify(rel.body))) {
    const list = await curl('GET', 'https://api.github.com/repos/' + REPO + '/releases?per_page=100');
    const found = (list.body || []).find((r) => r.tag_name === TAG);
    if (found) { releaseId = found.id; console.log('release 已存在 id=', releaseId); }
    else { console.log('复用 release 失败:', JSON.stringify(list.body)); process.exit(1); }
  } else { console.log('创建 release 失败:', rel.status, JSON.stringify(rel.body)); process.exit(1); }

  // 4. 上传资产（大文件，不限 120s 超时）
  for (const assetPath of ASSETS) {
    if (!existsSync(assetPath)) { console.log('资产不存在，跳过:', assetPath); continue; }
    const buf = readFileSync(assetPath);
    const name = basename(assetPath);
    const url = 'https://uploads.github.com/repos/' + REPO + '/releases/' + releaseId + '/assets?name=' + encodeURIComponent(name);
    console.log('上传资产中:', name, buf.length, 'bytes ...');
    const respFile = join(TMP, 'verse-gh-asset-' + Date.now() + '.json');
    const code = execFileSync('curl.exe', [
      '-s', ...PROXY,
      '-X', 'POST',
      '-H', 'Authorization: Bearer ' + TOKEN,
      '-H', 'Accept: application/vnd.github+json',
      '-H', 'User-Agent: VersePC',
      '-H', 'Content-Type: application/octet-stream',
      '--data-binary', '@' + assetPath,
      '-o', respFile,
      '-w', '%{http_code}',
      url,
    ], { encoding: 'utf8', maxBuffer: 1024 * 1024 }).trim();
    let text = '';
    try { text = readFileSync(respFile, 'utf8'); } catch (_) {}
    try { unlinkSync(respFile); } catch (_) {}
    if (code === '201') console.log('资产上传成功:', name, buf.length, 'bytes');
    else console.log('资产上传失败:', name, 'HTTP', code, text.slice(0, 300));
  }
  console.log('\n完成。release:', 'https://github.com/' + REPO + '/releases/tag/' + TAG);
})();
