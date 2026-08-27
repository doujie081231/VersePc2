const fs = require('fs');
const path = require('path');
const https = require('https');
const http = require('http');
const { spawn } = require('child_process');

const args = process.argv.slice(2);
const params = {};
for (let i = 0; i < args.length; i += 2) {
    params[args[i].replace('--', '')] = args[i + 1];
}

const ROOT = params.root;
const LIBS = params.libs;
const VER_DIR = params.verdir;
const FORGE_VER = params.forgever;
const GAME_VER = params.gamever || '';
const CONFIG = params.config;
const APP_DIR = params.appdir || '';

let AdmZip;
try { AdmZip = require('adm-zip'); } catch (_) {
    try { AdmZip = require(path.join(APP_DIR, 'node_modules', 'adm-zip')); } catch (_) {}
}
const LOG_FILE = path.join(ROOT, 'temp', 'forge-installer.log');

const log = (msg) => {
    const line = `[${new Date().toISOString()}] ${msg}\n`;
    try { fs.appendFileSync(LOG_FILE, line); } catch (_) {}
};

const send = (obj) => {
    process.stdout.write(JSON.stringify(obj) + '\n');
};

try { fs.mkdirSync(path.dirname(LOG_FILE), { recursive: true }); } catch (_) {}
fs.writeFileSync(LOG_FILE, '');

log(`ROOT=${ROOT}`);
log(`LIBS=${LIBS}`);
log(`VER_DIR=${VER_DIR}`);
log(`FORGE_VER=${FORGE_VER}`);
log(`CONFIG=${CONFIG}`);

if (!CONFIG || !fs.existsSync(CONFIG)) {
    log('ERROR: Config file not found');
    send({ type: 'done', success: false, error: 'Config file not found' });
    process.exit(1);
}

const config = JSON.parse(fs.readFileSync(CONFIG, 'utf8'));
const ip = config.installProfile;
const vj = config.versionJson;
const processorsInfo = config.processors;

log(`\n=== Step 1: Verify files ===`);
log(`Processors: ${processorsInfo.length}`);

const versionDir = VER_DIR;
if (!fs.existsSync(versionDir)) fs.mkdirSync(versionDir, { recursive: true });

const vjId = path.basename(versionDir);
vj.id = vjId;
// [CRITICAL - 2026-06-20] inheritsFrom 必须指向纯净的 MC 版本号（如 "1.21.1"），不能是 Forge/NeoForge 版本号。
// Forge 1.20.6+ 的 version.json 里 inheritsFrom 可能指向 "neoforge-X.X.X" 等非原版版本，
// 导致合并时找不到正确的原版 JSON，或者合并了错误的父版本参数。
// 必须强制覆盖为 GAME_VER（原版 MC 版本号），确保合并正确的原版内容。
if (GAME_VER) {
    vj.inheritsFrom = GAME_VER;
} else if (!vj.inheritsFrom) {
    vj.inheritsFrom = FORGE_VER;
}

const vjPath = path.join(versionDir, `${vjId}.json`);
fs.writeFileSync(vjPath, JSON.stringify(vj, null, 2));
log(`Written version.json: ${vjPath}`);

const ipPath = path.join(versionDir, 'install_profile.json');
fs.writeFileSync(ipPath, JSON.stringify(ip, null, 2));
log(`Written install_profile.json: ${ipPath}`);

send({ type: 'progress', percent: 0.3, message: 'Files verified, running processors...' });

log(`\n=== Step 2: Run processors ===`);

let javaPath = 'java';
try {
    const javaHome = process.env.JAVA_HOME;
    if (javaHome) {
        const candidate = path.join(javaHome, 'bin', 'java.exe');
        if (fs.existsSync(candidate)) javaPath = candidate;
    }
} catch (_) {}
log(`Java: ${javaPath} exists=${fs.existsSync(javaPath)}`);

function resolveMavenPath(name) {
    const parts = name.split(':');
    if (parts.length < 3) return null;
    const groupPath = parts[0].replace(/\./g, '/');
    const artifact = parts[1];
    let version = parts[2];
    let classifier = '';
    let ext = 'jar';
    let atIdx = version.indexOf('@');
    if (atIdx >= 0) {
        ext = version.substring(atIdx + 1);
        version = version.substring(0, atIdx);
    }
    if (parts[3]) {
        const atIdx3 = parts[3].indexOf('@');
        if (atIdx3 >= 0) {
            classifier = parts[3].substring(0, atIdx3);
            ext = parts[3].substring(atIdx3 + 1);
        } else {
            classifier = parts[3];
        }
    }
    if (parts[4]) {
        const atIdx4 = parts[4].indexOf('@');
        ext = atIdx4 >= 0 ? parts[4].substring(atIdx4 + 1) : parts[4];
    }
    const fileName = classifier ? `${artifact}-${version}-${classifier}.${ext}` : `${artifact}-${version}.${ext}`;
    return path.join(LIBS, groupPath, artifact, version, fileName);
}

function normalizePath(val) {
    if (val && val.match(/^\[.+\]$/g)) {
        const name = val.substring(1, val.length - 1);
        return resolveMavenPath(name);
    }
    return val;
}

function normalizeVariable(val, variables, side) {
    if (!val) return val;
    let result = val.replace(/{([A-Za-z0-9_-]+)}/g, (_, key) => {
        if (key === 'SIDE') return side;
        if (variables[key]) return normalizePath(variables[key][side] || '');
        return '';
    });
    if (result.match(/^\[.+\]$/g)) {
        result = resolveMavenPath(result.substring(1, result.length - 1)) || result;
    }
    return result;
}

const side = 'client';
const variables = {
    SIDE: { client: 'client', server: 'server' },
    MINECRAFT_JAR: {
        client: path.join(ROOT, 'versions', ip.minecraft, `${ip.minecraft}.jar`),
        server: path.join(ROOT, 'versions', ip.minecraft, `${ip.minecraft}-server.jar`),
    },
    ROOT: { client: ROOT, server: ROOT },
    MINECRAFT_VERSION: { client: ip.minecraft, server: ip.minecraft },
    LIBRARY_DIR: { client: LIBS, server: LIBS },
};

if (ip.data) {
    for (const key in ip.data) {
        const val = ip.data[key];
        variables[key] = {
            client: normalizePath(typeof val === 'object' ? (val.client || '') : val),
            server: normalizePath(typeof val === 'object' ? (val.server || '') : val),
        };
    }
}

function extractZipEntry(buf, entryName) {
    const eocd = buf.lastIndexOf(Buffer.from([0x50, 0x4b, 0x05, 0x06]));
    if (eocd < 0) return null;
    const centralDirOffset = buf.readUInt32LE(eocd + 16);
    let offset = centralDirOffset;
    while (offset < eocd) {
        if (buf.readUInt32LE(offset) !== 0x02014b50) break;
        const compMethod = buf.readUInt16LE(offset + 10);
        const compSize = buf.readUInt32LE(offset + 20);
        const uncompSize = buf.readUInt32LE(offset + 24);
        const nameLen = buf.readUInt16LE(offset + 28);
        const extraLen = buf.readUInt16LE(offset + 30);
        const commentLen = buf.readUInt16LE(offset + 32);
        const localHeaderOffset = buf.readUInt32LE(offset + 42);
        const name = buf.toString('utf8', offset + 46, offset + 46 + nameLen);
        if (name === entryName) {
            const lNameLen = buf.readUInt16LE(localHeaderOffset + 26);
            const dataStart = localHeaderOffset + 30 + lNameLen;
            const data = buf.slice(dataStart, dataStart + compSize);
            if (compMethod === 0) return data.toString('utf8');
            if (compMethod === 8) {
                const zlib = require('zlib');
                return zlib.inflateRawSync(data).toString('utf8');
            }
            return null;
        }
        offset += 46 + nameLen + extraLen + commentLen;
    }
    return null;
}

function downloadFile(url, dest, timeoutMs = 60000) {
    return new Promise((resolve, reject) => {
        try {
            const dir = path.dirname(dest);
            if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
            const mod = url.startsWith('https') ? https : http;
            const req = mod.get(url, { timeout: timeoutMs }, (res) => {
                if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
                    return downloadFile(res.headers.location, dest, timeoutMs).then(resolve, reject);
                }
                if (res.statusCode !== 200) { res.resume(); reject(new Error(`HTTP ${res.statusCode}`)); return; }
                const ws = fs.createWriteStream(dest);
                res.pipe(ws);
                ws.on('finish', () => { ws.close(); resolve(); });
                ws.on('error', (e) => { try { fs.unlinkSync(dest); } catch(_){} reject(e); });
            });
            req.on('error', reject);
            req.on('timeout', () => { req.destroy(); reject(new Error('timeout')); });
        } catch(e) { reject(e); }
    });
}

// 带重试的下载：同一 URL 失败时重试 retryTimes 次，每次间隔递增
async function downloadFileWithRetry(url, dest, timeoutMs = 60000, retryTimes = 2) {
    let lastErr = null;
    for (let attempt = 0; attempt <= retryTimes; attempt++) {
        try {
            if (attempt > 0) {
                const delay = 1000 * attempt; // 1s, 2s, 3s...
                await new Promise(r => setTimeout(r, delay));
                log(`Retry ${attempt}/${retryTimes}: ${url}`);
            }
            await downloadFile(url, dest, timeoutMs);
            if (fs.existsSync(dest) && fs.statSync(dest).size > 100) return true;
            lastErr = new Error('file too small or missing');
        } catch(e) { lastErr = e; log(`Attempt ${attempt+1} failed: ${e.message}`); }
    }
    if (lastErr) throw lastErr;
    return false;
}

async function downloadMavenArtifact(mavenCoord) {
    const parts = mavenCoord.split(':');
    if (parts.length < 3) return false;
    const groupPath = parts[0].replace(/\./g, '/');
    const artifact = parts[1];
    let version = parts[2];
    let classifier = '';
    let ext = 'jar';
    const atIdx = version.indexOf('@');
    if (atIdx >= 0) { ext = version.substring(atIdx + 1); version = version.substring(0, atIdx); }
    if (parts[3]) {
        const atIdx3 = parts[3].indexOf('@');
        if (atIdx3 >= 0) { classifier = parts[3].substring(0, atIdx3); ext = parts[3].substring(atIdx3 + 1); }
        else classifier = parts[3];
    }
    const fileName = classifier ? `${artifact}-${version}-${classifier}.${ext}` : `${artifact}-${version}.${ext}`;
    const relativePath = `${groupPath}/${artifact}/${version}/${fileName}`;
    const dest = path.join(LIBS, relativePath);
    if (fs.existsSync(dest) && fs.statSync(dest).size > 100) return true;

    // For modloader-related libraries, use BMCLAPI first, then official sources as fallback
    // For other libraries, use BMCLAPI first, then aliyun, then official sources
    const isModloaderLib = ['minecraftforge', 'neoforged', 'fabricmc', 'quiltmc'].some(
        k => parts[0].toLowerCase().includes(k)
    );
    const isForgeLib = parts[0].toLowerCase().includes('minecraftforge');
    const mirrors = isModloaderLib
        ? [
              `https://bmclapi2.bangbang93.com/maven/${relativePath}`,
              ...(isForgeLib ? [`https://maven.minecraftforge.net/${relativePath}`] : []),
              `https://maven.aliyun.com/repository/public/${relativePath}`,
          ]
        : [
              `https://bmclapi2.bangbang93.com/maven/${relativePath}`,
              `https://maven.aliyun.com/repository/public/${relativePath}`,
              `https://libraries.minecraft.net/${relativePath}`,
              `https://maven.minecraftforge.net/${relativePath}`,
              `https://mirrors.cloud.tencent.com/nexus/repository/maven-public/${relativePath}`,
          ];
    for (const url of mirrors) {
        try {
            log(`Downloading: ${url}`);
            // 每个源最多重试 2 次，超时 60 秒（比原 15 秒更宽松，解决大文件慢速下载）
            await downloadFileWithRetry(url, dest, 60000, 2);
            if (fs.existsSync(dest) && fs.statSync(dest).size > 100) {
                log(`Downloaded: ${dest} (${fs.statSync(dest).size} bytes)`);
                return true;
            }
        } catch(e) { log(`Download failed ${url}: ${e.message}`); }
    }
    return false;
}

async function runProcessor(procInfo, index) {
    const { jar, mainClass, classpath: cpNames, args: procArgs } = procInfo;
    log(`\n--- Processor ${index + 1}/${processorsInfo.length}: ${jar} ---`);
    log(`Main-Class: ${mainClass}`);

    send({ type: 'progress', percent: 0.4 + (index / processorsInfo.length) * 0.5, message: `准备处理器 ${index + 1}/${processorsInfo.length}...` });

    let jarPath = resolveMavenPath(jar);
    if (!jarPath || !fs.existsSync(jarPath)) {
        log(`[P${index+1}] 处理器JAR不存在，下载: ${jar}`);
        send({ type: 'progress', percent: 0.4 + (index / processorsInfo.length) * 0.5, message: `下载处理器 ${index + 1}...` });
        const ok = await downloadMavenArtifact(jar);
        if (!ok) { log(`[P${index+1}] ERROR: 下载处理器JAR失败: ${jar}`); return false; }
        jarPath = resolveMavenPath(jar);
        if (!jarPath || !fs.existsSync(jarPath)) { log(`[P${index+1}] ERROR: 下载后JAR仍不存在: ${jarPath}`); return false; }
    }
    log(`[P${index+1}] 处理器JAR: ${jarPath} (${fs.existsSync(jarPath) ? fs.statSync(jarPath).size : 'missing'} bytes)`);

    let resolvedMainClass = mainClass;
    if (!resolvedMainClass && jarPath && fs.existsSync(jarPath)) {
        if (AdmZip) {
            try {
                const jarZip = new AdmZip(jarPath);
                const mf = jarZip.getEntry('META-INF/MANIFEST.MF');
                if (mf) {
                    for (const line of mf.getData().toString('utf8').split(/\r?\n/)) {
                        const t = line.trim();
                        if (t.startsWith('Main-Class:')) { resolvedMainClass = t.substring('Main-Class:'.length).trim(); break; }
                    }
                }
            } catch(e) { log(`Failed to read Main-Class (adm-zip): ${e.message}`); }
        }
        if (!resolvedMainClass) {
            try {
                const buf = fs.readFileSync(jarPath);
                const manifest = extractZipEntry(buf, 'META-INF/MANIFEST.MF');
                if (manifest) {
                    for (const line of manifest.split(/\r?\n/)) {
                        const t = line.trim();
                        if (t.startsWith('Main-Class:')) { resolvedMainClass = t.substring('Main-Class:'.length).trim(); break; }
                    }
                }
            } catch(e) { log(`Failed to read Main-Class (native-zip): ${e.message}`); }
        }
        if (!resolvedMainClass) {
            try {
                const psScript = `$z=[System.IO.File]::ReadAllBytes('${jarPath.replace(/'/g,"''")}'); $ms=New-Object System.IO.MemoryStream(,$z); $za=New-Object System.IO.Compression.ZipArchive($ms); $e=$za.GetEntry('META-INF/MANIFEST.MF'); if($e){$sr=New-Object System.IO.StreamReader($e.Open()); $c=$sr.ReadToEnd(); $sr.Close(); $c}else{''}; $za.Dispose(); $ms.Dispose()`;
                const result = require('child_process').execSync(`powershell -NoProfile -Command "${psScript.replace(/"/g, '\\"')}"`, { timeout: 10000, windowsHide: true, encoding: 'utf8' });
                if (result) {
                    for (const line of result.split(/\r?\n/)) {
                        const t = line.trim();
                        if (t.startsWith('Main-Class:')) { resolvedMainClass = t.substring('Main-Class:'.length).trim(); break; }
                    }
                }
            } catch(e) { log(`Failed to read Main-Class (powershell): ${e.message}`); }
        }
    }
    if (!resolvedMainClass) { log(`ERROR: No Main-Class for processor ${jar}`); return false; }

    // Collect all missing maven artifacts to download them in parallel
    const pendingDownloads = new Set();
    const collectMissing = (coord) => {
        if (!coord) return;
        const p = resolveMavenPath(coord);
        if (!p || !fs.existsSync(p)) pendingDownloads.add(coord);
    };

    for (const name of cpNames) collectMissing(name);

    const resolvedArgs = procArgs
        .map(a => normalizeVariable(a, variables, side));

    for (const rawArg of procArgs) {
        const m = rawArg.match(/^\[([^\]]+)\]$/);
        if (m) collectMissing(m[1]);
    }

    for (const arg of resolvedArgs) {
        if (arg && typeof arg === 'string' && path.isAbsolute(arg) && !fs.existsSync(arg) && /\.(zip|jar|lzma|txt|gz)$/.test(arg)) {
            const mavenFromArg = procArgs.find(raw => {
                const p = normalizeVariable(raw, variables, side);
                return p === arg && raw.startsWith('[');
            });
            if (mavenFromArg) {
                const cm = mavenFromArg.match(/^\[([^\]]+)\]$/);
                if (cm) collectMissing(cm[1]);
            }
        }
    }

    if (pendingDownloads.size > 0) {
        const list = Array.from(pendingDownloads);
        log(`[P${index+1}] 并行下载 ${list.length} 个依赖: ${list.join(', ')}`);
        send({ type: 'progress', percent: 0.4 + (index / processorsInfo.length) * 0.5, message: `并行下载 ${list.length} 个依赖...` });
        const results = await Promise.all(list.map(c => downloadMavenArtifact(c)));
        const failed = list.filter((_, i) => !results[i]);
        if (failed.length) log(`[P${index+1}] WARNING: 下载失败的依赖: ${failed.join(', ')}`);
    }

    const classpath = [];
    for (const name of cpNames) {
        let p = resolveMavenPath(name);
        if (p && fs.existsSync(p)) classpath.push(p);
        else log(`[P${index+1}] WARNING: 类路径JAR仍不存在: ${name}`);
    }
    log(`[P${index+1}] 类路径: ${classpath.length} 个JAR`);
    classpath.push(jarPath);
    const cpStr = classpath.join(';');

    log(`[P${index+1}] 命令: java -cp <${classpath.length} jars> ${resolvedMainClass} <${resolvedArgs.length} args>`);
    send({ type: 'progress', percent: 0.4 + (index / processorsInfo.length) * 0.5, message: `运行处理器 ${index + 1}/${processorsInfo.length}...` });

    return new Promise((resolve) => {
        log(`[P${index+1}] 启动Java进程...`);
        const child = spawn(javaPath, ['-cp', cpStr, resolvedMainClass, ...resolvedArgs], {
            timeout: 120000,
            encoding: 'utf8',
            windowsHide: true,
            stdio: ['ignore', 'pipe', 'pipe'],
        });

        let stdout = '';
        let stderr = '';
        let lastOutputTime = Date.now();
        let killed = false;
        const heartbeatTimer = setInterval(() => {
            const elapsed = Math.round((Date.now() - lastOutputTime) / 1000);
            if (elapsed > 10) {
                log(`[P${index+1}] 等待中... (${elapsed}s 无输出, stdout=${stdout.length}B, stderr=${stderr.length}B)`);
            }
            // Auto-kill if no output for 60 seconds (likely hung)
            if (elapsed > 60 && !killed) {
                killed = true;
                log(`[P${index+1}] 60秒无输出，自动终止Java进程`);
                try { child.kill('SIGKILL'); } catch(_) {}
            }
        }, 15000);

        child.stdout.on('data', (data) => { stdout += data; lastOutputTime = Date.now(); });
        child.stderr.on('data', (data) => { stderr += data; lastOutputTime = Date.now(); });

        child.on('close', (code) => {
            clearInterval(heartbeatTimer);
            log(`[P${index+1}] 退出码: ${code}`);
            if (stdout) log(`[P${index+1}] stdout: ${stdout.substring(0, 1000)}`);
            if (stderr) log(`[P${index+1}] stderr: ${stderr.substring(0, 1000)}`);
            if (code !== 0) {
                log(`Processor FAILED with code ${code}`);
                resolve(false);
            } else {
                log(`Processor completed successfully`);
                resolve(true);
            }
        });

        child.on('error', (err) => {
            clearInterval(heartbeatTimer);
            log(`ERROR: ${err.message}`);
            resolve(false);
        });
    });
}

async function main() {
    // Pre-download ALL dependencies for ALL processors before running any of them.
    // This avoids repeated network round-trips between processor runs.
    send({ type: 'progress', percent: 0.35, message: `预下载所有依赖 (${processorsInfo.length} 个处理器)...` });
    log(`\n=== Pre-download phase: scanning all processors ===`);

    const allPendingDownloads = new Set();
    for (let i = 0; i < processorsInfo.length; i++) {
        const proc = processorsInfo[i];
        // Collect processor jar itself
        if (proc.jar) {
            const p = resolveMavenPath(proc.jar);
            if (!p || !fs.existsSync(p)) allPendingDownloads.add(proc.jar);
        }
        // Collect classpath jars
        if (proc.classpath) {
            for (const name of proc.classpath) {
                const p = resolveMavenPath(name);
                if (!p || !fs.existsSync(p)) allPendingDownloads.add(name);
            }
        }
        // Collect maven args [group:artifact:version]
        if (proc.args) {
            for (const rawArg of proc.args) {
                const m = rawArg.match(/^\[([^\]]+)\]$/);
                if (m) {
                    const p = resolveMavenPath(m[1]);
                    if (!p || !fs.existsSync(p)) allPendingDownloads.add(m[1]);
                }
            }
        }
    }

    if (allPendingDownloads.size > 0) {
        const list = Array.from(allPendingDownloads);
        log(`Pre-downloading ${list.length} artifacts: ${list.join(', ')}`);
        send({ type: 'progress', percent: 0.38, message: `并行预下载 ${list.length} 个依赖...` });
        const results = await Promise.all(list.map(c => downloadMavenArtifact(c)));
        const failed = list.filter((_, i) => !results[i]);
        if (failed.length) {
            log(`WARNING: Pre-download failed for: ${failed.join(', ')}`);
        } else {
            log(`All ${list.length} artifacts pre-downloaded successfully`);
        }
    } else {
        log(`All dependencies already present, skipping pre-download`);
    }

    let allOk = true;
    for (let i = 0; i < processorsInfo.length; i++) {
        send({ type: 'progress', percent: 0.4 + (i / processorsInfo.length) * 0.5, message: `Running processor ${i + 1}/${processorsInfo.length}...` });
        const ok = await runProcessor(processorsInfo[i], i);
        if (!ok) {
            log(`\nProcessor ${i + 1} failed, stopping.`);
            allOk = false;
            break;
        }
    }

    if (allOk) {
        log(`\n=== All processors completed successfully ===`);

        const patchedOutputs = [];
        for (const proc of processorsInfo) {
            if (proc.outputs) {
                for (const [file, checksum] of Object.entries(proc.outputs)) {
                    const normalized = normalizeVariable(file, variables, side);
                    if (normalized && fs.existsSync(normalized)) {
                        patchedOutputs.push(normalized);
                    }
                }
            }
        }
        log(`Patched files: ${patchedOutputs.length}`);

        if (patchedOutputs.length > 0) {
            const versionJar = path.join(versionDir, `${vj.id}.jar`);
            let patchedJar = patchedOutputs.find(f => f.endsWith('-client.jar') || f.endsWith('.jar'));

            if (patchedJar) {
                fs.copyFileSync(patchedJar, versionJar);
                log(`Copied patched jar to: ${versionJar}`);
            }
        }

        const mergedForgeLibs = vj.libraries || [];
        const seenNames = new Set(mergedForgeLibs.map(l => l.name).filter(Boolean));
        if (vj.inheritsFrom) {
            const vanillaJsonPath = path.join(ROOT, 'versions', vj.inheritsFrom, `${vj.inheritsFrom}.json`);
            log(`Vanilla JSON path: ${vanillaJsonPath} exists=${fs.existsSync(vanillaJsonPath)}`);
            if (fs.existsSync(vanillaJsonPath)) {
                try {
                    const vanillaJson = JSON.parse(fs.readFileSync(vanillaJsonPath, 'utf8'));
                    const vanillaLibs = vanillaJson.libraries || [];
                    for (const vl of vanillaLibs) {
                        if (vl.name && !seenNames.has(vl.name)) {
                            mergedForgeLibs.push(vl);
                            seenNames.add(vl.name);
                        }
                    }
                    if (vanillaJson.arguments) {
                        if (!vj.arguments) {
                            vj.arguments = vanillaJson.arguments;
                        } else {
                            const mergedGame = [...(vanillaJson.arguments.game || [])];
                            for (const fg of (vj.arguments.game || [])) {
                                const fgStr = typeof fg === 'string' ? fg : JSON.stringify(fg);
                                if (!mergedGame.some(mg => (typeof mg === 'string' ? mg : JSON.stringify(mg)) === fgStr)) {
                                    mergedGame.push(fg);
                                }
                            }
                            vj.arguments.game = mergedGame;
                            const expandedJvm = [];
                            const fjArr = vj.arguments.jvm || [];
                            for (let fji = 0; fji < fjArr.length; fji++) {
                                const fj = fjArr[fji];
                                if (typeof fj === 'string' && (fj === '--add-opens' || fj === '--add-exports' || fj === '--add-reads' || fj === '--add-modules')) {
                                    const vals = [];
                                    while (fji + 1 < fjArr.length && typeof fjArr[fji + 1] === 'string' && !fjArr[fji + 1].startsWith('-')) {
                                        fji++;
                                        vals.push(fjArr[fji]);
                                    }
                                    if (vals.length === 0) {
                                        expandedJvm.push(fj);
                                    } else {
                                        for (const v of vals) { expandedJvm.push(fj, v); }
                                    }
                                } else {
                                    expandedJvm.push(fj);
                                }
                            }
                            const mergedJvm = [...(vanillaJson.arguments.jvm || [])];
                            for (const fj of expandedJvm) {
                                const fjStr = typeof fj === 'string' ? fj : JSON.stringify(fj);
                                if (!mergedJvm.some(mj => (typeof mj === 'string' ? mj : JSON.stringify(mj)) === fjStr)) {
                                    mergedJvm.push(fj);
                                }
                            }
                            vj.arguments.jvm = mergedJvm;
                        }
                    }
                    log(`Merged ${vanillaLibs.length} vanilla libraries, arguments merged`);
                } catch (e) {
                    log(`Warning: Failed to merge vanilla JSON: ${e.message}`);
                }
            }
        }
        // [CRITICAL - 2026-06-20] 最终写入版本 JSON 时必须移除 inheritsFrom 并合并所有 vanilla 库。
        // 这样 launcher 不需要再去查找前置版本的 JSON 文件，避免因前置版本不存在而导致启动失败。
        // 同时将 vanilla 的 libraries 和 arguments 合并进来，确保启动时所有依赖都齐全。
        const outputJson = {};
        for (const key of Object.keys(vj)) {
            if (key !== 'inheritsFrom') outputJson[key] = vj[key];
        }
        outputJson.libraries = mergedForgeLibs;
        const vjPath2 = path.join(versionDir, `${vj.id}.json`);
        fs.writeFileSync(vjPath2, JSON.stringify(outputJson, null, 2));
        try { fs.fsyncSync(fs.openSync(vjPath2, 'r')); } catch (_) {}
        log(`Wrote version JSON (inheritsFrom removed): ${vjPath2}`);

        send({ type: 'progress', percent: 1.0, message: 'Forge install complete' });
        send({ type: 'done', success: true, versionId: vj.id });
        log(`\n=== SUCCESS ===`);
        process.exit(0);
    } else {
        send({ type: 'done', success: false, error: 'Processor execution failed' });
        log(`\n=== FAILED ===`);
        process.exit(1);
    }
}

main().catch(err => {
    log(`FATAL: ${err.message}`);
    send({ type: 'done', success: false, error: err.message });
    process.exit(1);
});
