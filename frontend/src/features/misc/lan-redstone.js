import { API as API } from '../../core/api.js';
import { featureBridge as featureBridge } from '../../core/feature-bridge.js';
import { bridge as bridge } from '../../core/tauri-bridge.js';
import { showToast as showToast } from './ui-components.js';
import { S } from './lan-state.js';
import { showLanStep } from './lan-shared.js';

function updateRedstoneStatus(text, state) {
    const dot = document.getElementById('redstone-status-dot');
    const textEl = document.getElementById('redstone-status-text');
    if (textEl) textEl.textContent = text;
    if (dot) {
        dot.className = 'lan-status-dot';
        if (state === 'connected') dot.classList.add('connected');
        else if (state === 'connecting') dot.classList.add('connecting');
        else dot.classList.add('disconnected');
    }
}

function redstoneSwitchTab(tab) {
    // 切换 tab 按钮高亮
    document.querySelectorAll('.redstone-tabs .lan-tab').forEach(b => {
        b.classList.toggle('active', b.dataset.redstoneTab === tab);
    });
    // 切换 tab 内容显示
    document.querySelectorAll('.redstone-tab-content').forEach(el => {
        el.style.display = el.id === 'redstone-tab-' + tab ? 'block' : 'none';
    });
}

async function redstoneRefreshServers() {
    const btn = document.getElementById('redstone-server-btn');
    const info = document.getElementById('redstone-server-info');
    if (btn) btn.textContent = '服务器: 加载中...';
    if (info) info.textContent = '正在加载节点列表...';
    try {
        const r = await bridge.redstoneOnline.getServers();
        if (r && r.ok && r.servers && r.servers.length > 0) {
            S._redstoneServers = r.servers;
            if (info) info.textContent = '共 ' + r.servers.length + ' 个节点';
        } else {
            if (info) info.textContent = '节点列表为空（使用默认节点）';
            S._redstoneServers = [{ name: '南京', address: 'nj.hongshi.site' }];
        }
    } catch (e) {
        if (info) info.textContent = '加载失败: ' + e.message;
        S._redstoneServers = [{ name: '南京', address: 'nj.hongshi.site' }];
    }
    S._redstoneServerIdx = 0;
    updateServerBtn();
}

function updateServerBtn() {
    const btn = document.getElementById('redstone-server-btn');
    if (!btn || S._redstoneServers.length === 0) return;
    const s = S._redstoneServers[S._redstoneServerIdx % S._redstoneServers.length];
    btn.textContent = '服务器: ' + s.name + ' (' + s.address + ')';
}

function redstoneCycleServer() {
    if (S._redstoneServers.length === 0) return;
    S._redstoneServerIdx = (S._redstoneServerIdx + 1) % S._redstoneServers.length;
    updateServerBtn();
    addRedstoneLog('切换到服务器: ' + S._redstoneServers[S._redstoneServerIdx].name);
}

async function redstoneToggle() {
    if (S._redstoneRunning) await redstoneStop();
    else await redstoneStart();
}

async function redstoneStart() {
    const btn = document.getElementById('redstone-action-btn');
    if (S._redstoneServers.length === 0) { alert('请先等待节点列表加载'); return; }
    const server = S._redstoneServers[S._redstoneServerIdx % S._redstoneServers.length];
    if (!server) { alert('请选择服务器'); return; }

    // 检查游戏是否在运行，自动检测局域网端口
    // 如果启动器能追踪到最好，否则自动扫描 java 进程的监听端口
    let gameStatus;
    let gamePort;
    try {
        gameStatus = await API.getGameStatus();
        if (!gameStatus || !gameStatus.running) {
            // 游戏不在运行——但可能游戏是自己启动（非启动器），尝试扫端口
            addRedstoneLog('未检测到启动器追踪的游戏进程，正在扫描端口...');
            updateRedstoneStatus('扫描端口...', 'connecting');
            const scanResult = await bridge.redstoneOnline.scanPort();
            if (scanResult && scanResult.ok && scanResult.port) {
                gamePort = scanResult.port;
                addRedstoneLog('端口扫描成功: ' + gamePort);
            } else {
                addRedstoneLog('错误: 未找到 Minecraft 游戏进程，请先启动游戏并进入存档');
                updateRedstoneStatus('游戏未运行', 'disconnected');
                showToast('未找到 Minecraft 游戏进程，请先启动游戏', 'error');
                return;
            }
        } else if (!gameStatus.lanPort) {
            // 游戏在运行但未开放局域网，尝试扫端口
            addRedstoneLog('未检测到局域网端口，正在扫描...');
            updateRedstoneStatus('扫描端口...', 'connecting');
            const scanResult = await bridge.redstoneOnline.scanPort();
            if (scanResult && scanResult.ok && scanResult.port) {
                gamePort = scanResult.port;
                addRedstoneLog('端口扫描成功: ' + gamePort);
            } else {
                addRedstoneLog('错误: 未检测到局域网端口，请在游戏内按 Esc → 对局域网开放');
                updateRedstoneStatus('未开放局域网', 'disconnected');
                showToast('请在游戏内按 Esc → 对局域网开放，然后才能开启隧道', 'error');
                return;
            }
        } else {
            // 正常路径：启动器追踪到游戏进程且有 lanPort
            gamePort = gameStatus.lanPort;
            addRedstoneLog('检测到局域网端口: ' + gamePort);
        }
    } catch (e) {
        addRedstoneLog('检查游戏状态失败: ' + e.message);
        return;
    }

    const maxPlayersInput = document.getElementById('redstone-max-players');
    const maxPlayers = maxPlayersInput ? maxPlayersInput.value : '';

    // 读取当前选中的游戏版本，作为房间描述上传到联机大厅
    let versionDesc = '';
    try {
        const ctx = await API.getCurrentContext();
        if (ctx && ctx.selectedVersion) {
            const mcVer = ctx.selectedVersion;
            const loader = ctx.loader || '';
            const loaderVer = ctx.loaderVersion || '';
            if (loader && loaderVer) {
                let shortLoaderVer = loaderVer;
                const m = loaderVer.match(/(\d+\.\d+(?:\.\d+)?)/);
                if (m) shortLoaderVer = m[1];
                versionDesc = mcVer + ' / ' + loader.charAt(0).toUpperCase() + loader.slice(1) + ' ' + shortLoaderVer;
            } else if (loader) {
                versionDesc = mcVer + ' / ' + loader.charAt(0).toUpperCase() + loader.slice(1);
            } else {
                versionDesc = mcVer + ' / 原版';
            }
        }
    } catch (e) { addRedstoneLog('读取当前版本失败: ' + e.message); }
    // 更新页面显示
    const verBox = document.getElementById('redstone-current-version');
    if (verBox) {
        verBox.textContent = versionDesc || '未选择版本';
        verBox.style.color = versionDesc ? 'var(--text-primary)' : 'var(--text-muted)';
    }
    if (versionDesc) addRedstoneLog('当前版本: ' + versionDesc);

    S._redstoneRunning = true;
    if (btn) { btn.textContent = '正在开启...'; btn.disabled = true; }
    updateRedstoneStatus('正在连接...', 'connecting');
    addRedstoneLog('选择服务器: ' + server.name + ' (' + server.address + ')');
    addRedstoneLog('带宽上限: 4 Mbps，约支持 5-8 人');
    if (maxPlayers) addRedstoneLog('最大人数: ' + maxPlayers);

    try {
        const r = await bridge.redstoneOnline.start({
            serverAddress: server.address, gamePort: gamePort,
            maxPlayers: maxPlayers,
        });
        if (r && r.ok) {
            document.getElementById('redstone-connected-info').style.display = '';
            document.getElementById('redstone-room-addr').textContent = r.address;
            updateRedstoneStatus('隧道已开启 | ' + r.address, 'connected');
            if (btn) { btn.textContent = '关闭隧道'; btn.disabled = false; }
            try {
                await navigator.clipboard.writeText(r.address);
                addRedstoneLog('联机地址已复制到剪贴板: ' + r.address);
            } catch (_) {}
        } else {
            S._redstoneRunning = false;
            if (btn) { btn.textContent = '开启隧道'; btn.disabled = false; }
            updateRedstoneStatus('开启失败', 'disconnected');
            addRedstoneLog('开启失败: ' + (r && r.error ? r.error : '未知错误'));
        }
    } catch (e) {
        S._redstoneRunning = false;
        if (btn) { btn.textContent = '开启隧道'; btn.disabled = false; }
        updateRedstoneStatus('开启失败', 'disconnected');
        addRedstoneLog('开启失败: ' + e.message);
    }
}

async function redstoneStop() {
    const btn = document.getElementById('redstone-action-btn');
    if (btn) { btn.disabled = true; btn.textContent = '正在关闭...'; }
    try { await bridge.redstoneOnline.stop(); }
    catch (e) { addRedstoneLog('关闭失败: ' + e.message); }
    S._redstoneRunning = false;
    document.getElementById('redstone-connected-info').style.display = 'none';
    if (btn) { btn.textContent = '开启隧道'; btn.disabled = false; }
    updateRedstoneStatus('未连接', 'disconnected');
    addRedstoneLog('隧道已关闭');
}

function redstoneCopyAddr() {
    const addr = document.getElementById('redstone-room-addr').textContent;
    if (!addr || addr === '--') return;
    navigator.clipboard.writeText(addr).then(() => {
        addRedstoneLog('地址已复制: ' + addr);
    }).catch(() => {});
}

function addRedstoneLog(msg) {
    const logEl = document.getElementById('redstone-room-log');
    if (!logEl) return;
    const time = new Date().toLocaleTimeString();
    logEl.textContent += '[' + time + '] ' + msg + '\n';
    logEl.scrollTop = logEl.scrollHeight;
}

async function redstoneRefreshCurrentVersion() {
    const verBox = document.getElementById('redstone-current-version');
    if (!verBox) return;
    try {
        const ctx = await API.getCurrentContext();
        if (ctx && ctx.selectedVersion) {
            const mcVer = ctx.selectedVersion;
            const loader = ctx.loader || '';
            const loaderVer = ctx.loaderVersion || '';
            let versionDesc;
            if (loader && loaderVer) {
                let shortLoaderVer = loaderVer;
                const m = loaderVer.match(/(\d+\.\d+(?:\.\d+)?)/);
                if (m) shortLoaderVer = m[1];
                versionDesc = mcVer + ' / ' + loader.charAt(0).toUpperCase() + loader.slice(1) + ' ' + shortLoaderVer;
            } else if (loader) {
                versionDesc = mcVer + ' / ' + loader.charAt(0).toUpperCase() + loader.slice(1);
            } else {
                versionDesc = mcVer + ' / 原版';
            }
            verBox.textContent = versionDesc;
            verBox.style.color = 'var(--text-primary)';
        } else {
            verBox.textContent = '未选择版本';
            verBox.style.color = 'var(--text-muted)';
        }
    } catch (e) {
        verBox.textContent = '读取失败';
        verBox.style.color = 'var(--text-muted)';
    }
}

async function redstoneInitPage() {
    // 首次使用才显示引导（localStorage 标记），之后直接进入主界面
    if (!localStorage.getItem('redstone_onboarding_seen')) {
        showRedstoneOnboarding();
    } else {
        showRedstoneMain();
    }
    // 后台准备服务器节点列表（引导完成后主界面可直接使用）
    if (S._redstoneServers.length === 0) redstoneRefreshServers();
    // 同步主进程隧道状态：若已开启则直接恢复主界面
    try {
        const status = await bridge.redstoneOnline.getStatus();
        if (status && status.running && status.address) {
            showRedstoneMain();
            const addrEl = document.getElementById('redstone-room-addr');
            if (addrEl) addrEl.textContent = status.address;
        }
    } catch (e) {}
}

function showRedstoneOnboarding() {
    const ob = document.getElementById('redstone-onboarding');
    const main = document.getElementById('redstone-main');
    if (!ob || !main) return;
    main.style.display = 'none';
    ob.style.display = 'flex';
    document.getElementById('redstone-ob-dl').style.display = '';
    document.getElementById('redstone-ob-spinner').style.display = 'none';
    document.getElementById('redstone-ob-done').style.display = 'none';
    document.getElementById('redstone-ob-title').textContent = '你好！欢迎使用红石联机';
    document.getElementById('redstone-ob-desc').textContent = '基于 frp 的内网穿透，一键开启外网联机';
    ob.classList.remove('terracotta-ob--fadeout');
    void ob.offsetWidth;
    ob.classList.add('terracotta-ob--enter');
}

function showRedstoneMain() {
    const ob = document.getElementById('redstone-onboarding');
    const main = document.getElementById('redstone-main');
    if (!ob || !main) return;
    ob.style.display = 'none';
    main.style.display = '';
    redstoneBackToHome();
}

function redstoneStartOnboarding() {
    const dlBtn = document.getElementById('redstone-ob-dl');
    const spinnerBox = document.getElementById('redstone-ob-spinner');
    if (!dlBtn || !spinnerBox) return;
    dlBtn.style.display = 'none';
    spinnerBox.style.display = 'flex';
    // 红石联机无独立核心需下载，模拟初始化动画后进入主界面
    setTimeout(() => {
        spinnerBox.style.display = 'none';
        const done = document.getElementById('redstone-ob-done');
        if (done) {
            done.style.display = 'block';
            done.classList.add('terracotta-ob-done--show');
        }
        setTimeout(() => {
            const ob = document.getElementById('redstone-onboarding');
            if (ob) {
                localStorage.setItem('redstone_onboarding_seen', '1');
                ob.classList.add('terracotta-ob--fadeout');
                setTimeout(showRedstoneMain, 450);
            }
        }, 1500);
    }, 1200);
}

function redstoneHide() {
    document.getElementById('redstone-host-panel').style.display = 'none';
    const jp = document.getElementById('redstone-join-panel');
    if (jp) jp.style.display = 'none';
    document.getElementById('redstone-hint').style.display = 'none';
}

function redstoneBackToHome() {
    document.getElementById('redstone-host-panel').style.display = 'none';
    const jp = document.getElementById('redstone-join-panel');
    if (jp) jp.style.display = 'none';
    document.getElementById('redstone-hint').style.display = 'none';
    document.getElementById('redstone-home').style.display = '';
    updateRedstoneStatus('未连接', 'disconnected');
}

async function redstoneHostStep1() {
    redstoneHide();
    document.getElementById('redstone-home').style.display = 'none';
    const rjPanel = document.getElementById('redstone-join-panel');
    if (rjPanel) rjPanel.style.display = 'none';
    document.getElementById('redstone-host-panel').style.display = '';
    showLanStep('redstone', 'host', 1);
    const nextBtn = document.getElementById('redstone-host-next');
    const doCheck = async () => {
        try {
            const gs = await API.getGameStatus();
            const running = !!(gs && gs.running);
            if (nextBtn) {
                nextBtn.disabled = !running;
                nextBtn.title = running ? '下一步' : '请先启动游戏并开放局域网';
            }
        } catch (e) {}
    };
    doCheck();
    if (S.redstoneGamePoll) clearInterval(S.redstoneGamePoll);
    S.redstoneGamePoll = setInterval(doCheck, 2000);
}

async function redstoneHostNext() {
    let gs = null;
    try { gs = await API.getGameStatus(); } catch (e) {}
    if (!gs || !gs.running) {
        showToast('请先启动游戏，然后在游戏内开放局域网联机', 'error');
        return;
    }
    if (S.redstoneGamePoll) { clearInterval(S.redstoneGamePoll); S.redstoneGamePoll = null; }
    showLanStep('redstone', 'host', 2);
    try {
        let server = null;
        const vs = featureBridge;
        if (vs && vs.redstoneSelectedNode) {
          server = vs.redstoneSelectedNode;
        } else if (vs && vs.redstoneNodes && vs.redstoneNodes.length) {
          server = vs.redstoneNodes[0];
        } else {
          if (S._redstoneServers.length === 0) await redstoneRefreshServers();
          server = S._redstoneServers[S._redstoneServerIdx % S._redstoneServers.length];
        }
        if (!server) throw new Error('服务器节点不可用');
        let gamePort = gs.lanPort;
        if (!gamePort) {
            const scanResult = await bridge.redstoneOnline.scanPort();
            gamePort = scanResult && scanResult.ok && scanResult.port ? scanResult.port : 25565;
        }
        const r = await bridge.redstoneOnline.start({
            serverAddress: server.address, gamePort: gamePort, maxPlayers: ''
        });
        if (!r || !r.ok) throw new Error(r && r.error ? r.error : '开启失败');
        const addrEl = document.getElementById('redstone-room-addr');
        if (addrEl) addrEl.textContent = r.address;
        showLanStep('redstone', 'host', 3);
        const hint = document.getElementById('redstone-hint');
        if (hint) {
            hint.textContent = '联机地址已生成，发送给朋友即可加入';
            hint.style.display = '';
            hint.style.background = 'rgba(16,185,129,0.1)';
            hint.style.color = 'var(--green)';
        }
    } catch (e) {
        showToast('开启隧道失败: ' + (e.message || e), 'error');
        redstoneBackToHome();
    }
}

async function redstoneDisconnect() {
    try { await bridge.redstoneOnline.stop(); } catch (e) {}
    redstoneBackToHome();
    showToast('已断开红石联机', 'info');
}

export { updateRedstoneStatus, redstoneSwitchTab, redstoneRefreshServers, updateServerBtn, redstoneCycleServer, redstoneToggle, redstoneStart, redstoneStop, redstoneCopyAddr, addRedstoneLog, redstoneRefreshCurrentVersion, redstoneInitPage, showRedstoneOnboarding, showRedstoneMain, redstoneStartOnboarding, redstoneHide, redstoneBackToHome, redstoneHostStep1, redstoneHostNext, redstoneDisconnect };
