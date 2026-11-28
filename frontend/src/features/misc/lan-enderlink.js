import { API as API } from '../../core/api.js';
import { featureBridge as featureBridge } from '../../core/feature-bridge.js';
import { bridge as bridge } from '../../core/tauri-bridge.js';
import { showToast as showToast } from './ui-components.js';
import { actionAttrs as actionAttrs } from '../../core/actions.js';
import { S } from './lan-state.js';
import { showLanStep } from './lan-shared.js';

function updateEnderlinkStatus(text, state) {
    const dot = document.getElementById('enderlink-status-dot');
    const textEl = document.getElementById('enderlink-status-text');
    if (textEl) textEl.textContent = text;
    if (dot) {
        dot.className = 'lan-status-dot';
        if (state === 'connected') dot.classList.add('connected');
        else if (state === 'connecting') dot.classList.add('connecting');
        else dot.classList.add('disconnected');
    }
}

function enderlinkSwitchTab(tab) {
    document.querySelectorAll('.enderlink-tabs .lan-tab').forEach(b => {
        b.classList.toggle('active', b.dataset.enderlinkTab === tab);
    });
    document.querySelectorAll('.enderlink-tab-content').forEach(el => {
        el.style.display = el.id === 'enderlink-tab-' + tab ? 'block' : 'none';
    });
}

function addEnderlinkLog(msg) {
    const logEl = document.getElementById('enderlink-room-log');
    if (!logEl) return;
    const time = new Date().toLocaleTimeString();
    logEl.textContent += '[' + time + '] ' + msg + '\n';
    logEl.scrollTop = logEl.scrollHeight;
}

async function enderlinkRefreshNodes() {
    const info = document.getElementById('enderlink-node-info');
    const list = document.getElementById('enderlink-node-list');
    const btn = document.getElementById('enderlink-node-btn');
    if (btn) btn.textContent = '节点: 加载中...';
    if (info) info.textContent = '正在加载节点列表...';
    try {
        const r = await bridge.enderlinkOnline.getNodes();
        if (r && r.ok && r.nodes && r.nodes.length > 0) {
            S._enderlinkNodes = r.nodes;
            if (info) info.textContent = '共 ' + r.nodes.length + ' 个节点';
            if (list) {
                list.innerHTML = S._enderlinkNodes.map((n, i) =>
                    '<div style="padding:6px 0;border-bottom:1px dashed var(--border)"><b>' +
                    (n.name || ('节点' + (i + 1))) + '</b> &nbsp;' +
                    '<span style="color:var(--text-muted)">' + n.frpIp + ':' + n.frpPort + '</span>' +
                    '<div style="font-size:11px;color:var(--text-tertiary);margin-top:2px">ID: ' + n.id + '</div></div>'
                ).join('');
            }
        } else {
            if (info) info.textContent = '节点列表为空';
            if (list) list.innerHTML = '暂无可用节点';
            S._enderlinkNodes = [];
        }
    } catch (e) {
        if (info) info.textContent = '加载失败: ' + e.message;
        if (list) list.innerHTML = '加载失败: ' + e.message;
    }
    enderlinkUpdateNodeBtn();
}

async function enderlinkRefreshRooms() {
    const list = document.getElementById('enderlink-hall-list');
    if (!list) return;
    list.textContent = '加载中...';
    try {
        const r = await bridge.enderlinkOnline.getRooms();
        if (r && r.ok && r.rooms && r.rooms.length > 0) {
            list.innerHTML = '';
            r.rooms.forEach((room, i) => {
                const addr = (room.server_addr || '') + ':' + (room.remote_port || '');
                const row = document.createElement('div');
                row.style.cssText = 'display:flex;align-items:center;justify-content:space-between;padding:10px 16px;border-bottom:1px solid var(--border)';
                row.innerHTML =
                    '<div style="min-width:0"><div style="font-size:14px;color:var(--text-primary);font-weight:500;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">' +
                    (room.room_name || ('房间' + (i + 1))) + '</div>' +
                    '<div style="font-size:12px;color:var(--text-muted);font-family:monospace">' + addr + '</div></div>' +
                    '<button class="btn btn-secondary btn-sm"' + actionAttrs('enderlinkCopyHall', addr) + '>复制地址</button>';
                list.appendChild(row);
            });
        } else {
            list.textContent = '暂无公开房间';
        }
    } catch (e) {
        list.textContent = '加载失败: ' + e.message;
    }
}

function enderlinkCopyHall(addr) {
    navigator.clipboard.writeText(addr).then(() => {
        addEnderlinkLog('复制大厅地址: ' + addr);
        showToast('地址已复制', 'success');
    }).catch(() => {});
}

function enderlinkUpdateNodeBtn() {
    const btn = document.getElementById('enderlink-node-btn');
    if (!btn) return;
    if (!S._enderlinkNodes.length) {
        btn.textContent = '节点: 无可用节点';
        return;
    }
    const n = S._enderlinkNodes[S._enderlinkNodeIdx % S._enderlinkNodes.length];
    btn.textContent = '节点: ' + (n.name || ('节点' + (S._enderlinkNodeIdx + 1))) + ' (' + n.frpIp + ':' + n.frpPort + ')';
}

function enderlinkCycleNode() {
    if (!S._enderlinkNodes.length) return;
    S._enderlinkNodeIdx = (S._enderlinkNodeIdx + 1) % S._enderlinkNodes.length;
    enderlinkUpdateNodeBtn();
    addEnderlinkLog('切换到节点: ' + (S._enderlinkNodes[S._enderlinkNodeIdx].name || ('节点' + S._enderlinkNodeIdx)));
}

async function enderlinkToggle() {
    if (S._enderlinkRunning) await enderlinkStop();
    else await enderlinkStart();
}

async function enderlinkStart() {
    const btn = document.getElementById('enderlink-action-btn');
    if (S._enderlinkOpening) return;
    if (!S._enderlinkNodes.length) { alert('请先等待节点列表加载'); return; }
    const node = S._enderlinkNodes[S._enderlinkNodeIdx % S._enderlinkNodes.length];
    if (!node) { alert('请选择节点'); return; }

    let gamePort = 25565;
    try {
        const gameStatus = await API.getGameStatus();
        if (gameStatus && gameStatus.running && gameStatus.lanPort) {
            gamePort = gameStatus.lanPort;
            addEnderlinkLog('检测到局域网端口: ' + gamePort);
        } else {
            addEnderlinkLog('正在扫描局域网端口...');
            const scanResult = await bridge.redstoneOnline.scanPort();
            if (scanResult && scanResult.ok && scanResult.port) {
                gamePort = scanResult.port;
                addEnderlinkLog('端口扫描成功: ' + gamePort);
            } else {
                addEnderlinkLog('未检测到端口，将使用默认 25565');
            }
        }
    } catch (e) {
        addEnderlinkLog('检查游戏状态失败: ' + e.message);
    }

    const roomNameEl = document.getElementById('enderlink-room-name');
    const publicEl = document.getElementById('enderlink-public');
    const roomName = roomNameEl ? roomNameEl.value.trim() : '';
    const isPublic = publicEl ? publicEl.classList.contains('active') : true;
    let versionDesc = '';
    try {
        const ctx = await API.getCurrentContext();
        if (ctx && ctx.selectedVersion) versionDesc = ctx.selectedVersion;
    } catch (e) {}

    S._enderlinkOpening = true;
    if (btn) { btn.disabled = true; btn.textContent = '正在开启...'; }
    updateEnderlinkStatus('正在连接...', 'connecting');
    addEnderlinkLog('选择节点: ' + (node.name || node.id) + ' (' + node.frpIp + ':' + node.frpPort + ')');
    addEnderlinkLog('本机端口: ' + gamePort + (isPublic ? '，公开大厅' : '，仅端口映射'));
    if (roomName) addEnderlinkLog('房间名: ' + roomName);

    try {
        const r = await bridge.enderlinkOnline.start({
            node: node,
            localPort: gamePort,
            roomName: roomName,
            gameVersion: versionDesc || '1.12.2',
            isPublic: isPublic
        });
        if (r && r.ok) {
            S._enderlinkRunning = true;
            document.getElementById('enderlink-connected-info').style.display = '';
            document.getElementById('enderlink-room-addr').textContent = r.address;
            updateEnderlinkStatus('联机已开启 | ' + r.address, 'connected');
            if (btn) { btn.textContent = '关闭联机'; btn.disabled = false; }
            try {
                await navigator.clipboard.writeText(r.address);
                addEnderlinkLog('联机地址已复制到剪贴板: ' + r.address);
            } catch (_) {}
        } else {
            updateEnderlinkStatus('开启失败', 'disconnected');
            if (btn) { btn.textContent = '开启联机'; btn.disabled = false; }
            addEnderlinkLog('开启失败: ' + (r && r.error ? r.error : '未知错误'));
            showToast('开启失败: ' + (r && r.error ? r.error : '未知错误'), 'error');
        }
    } catch (e) {
        updateEnderlinkStatus('开启失败', 'disconnected');
        if (btn) { btn.textContent = '开启联机'; btn.disabled = false; }
        addEnderlinkLog('开启失败: ' + e.message);
        showToast('开启失败: ' + e.message, 'error');
    }
    S._enderlinkOpening = false;
}

async function enderlinkStop() {
    const btn = document.getElementById('enderlink-action-btn');
    if (btn) { btn.disabled = true; btn.textContent = '正在关闭...'; }
    try { await bridge.enderlinkOnline.stop(); }
    catch (e) { addEnderlinkLog('关闭失败: ' + e.message); }
    S._enderlinkRunning = false;
    document.getElementById('enderlink-connected-info').style.display = 'none';
    if (btn) { btn.textContent = '开启联机'; btn.disabled = false; }
    updateEnderlinkStatus('未连接', 'disconnected');
    addEnderlinkLog('联机已关闭');
}

function enderlinkCopyAddr() {
    const addr = document.getElementById('enderlink-room-addr');
    if (!addr) return;
    const t = addr.textContent;
    if (!t || t === '--') return;
    navigator.clipboard.writeText(t).then(() => addEnderlinkLog('地址已复制: ' + t)).catch(() => {});
}

async function enderlinkInitPage() {
    // 首次使用才显示引导（localStorage 标记），之后直接进入主界面
    if (!localStorage.getItem('enderlink_onboarding_seen')) {
        showEnderlinkOnboarding();
    } else {
        showEnderlinkMain();
    }
    // 后台准备节点列表（引导完成后主界面可直接使用）
    if (S._enderlinkNodes.length === 0) enderlinkRefreshNodes();
    // 同步主进程状态：若已开启则直接恢复主界面
    try {
        const status = await bridge.enderlinkOnline.getStatus();
        if (status && status.running && status.address) {
            showEnderlinkMain();
            const addrEl = document.getElementById('enderlink-room-addr');
            if (addrEl) addrEl.textContent = status.address;
        }
    } catch (e) { S._enderlinkRunning = false; }
}

function showEnderlinkOnboarding() {
    const ob = document.getElementById('enderlink-onboarding');
    const main = document.getElementById('enderlink-main');
    if (!ob || !main) return;
    main.style.display = 'none';
    ob.style.display = 'flex';
    document.getElementById('enderlink-ob-dl').style.display = '';
    document.getElementById('enderlink-ob-spinner').style.display = 'none';
    document.getElementById('enderlink-ob-done').style.display = 'none';
    document.getElementById('enderlink-ob-title').textContent = '你好！欢迎使用 EnderLink 联机';
    document.getElementById('enderlink-ob-desc').textContent = '基于 lytapi 大厅 + frp 内网穿透，一键开启外网联机';
    ob.classList.remove('terracotta-ob--fadeout');
    void ob.offsetWidth;
    ob.classList.add('terracotta-ob--enter');
}

function showEnderlinkMain() {
    const ob = document.getElementById('enderlink-onboarding');
    const main = document.getElementById('enderlink-main');
    if (!ob || !main) return;
    ob.style.display = 'none';
    main.style.display = '';
    enderlinkBackToHome();
}

function enderlinkStartOnboarding() {
    const dlBtn = document.getElementById('enderlink-ob-dl');
    const spinnerBox = document.getElementById('enderlink-ob-spinner');
    if (!dlBtn || !spinnerBox) return;
    dlBtn.style.display = 'none';
    spinnerBox.style.display = 'flex';
    // EnderLink 无独立核心需下载，模拟初始化动画后进入主界面
    setTimeout(() => {
        spinnerBox.style.display = 'none';
        const done = document.getElementById('enderlink-ob-done');
        if (done) {
            done.style.display = 'block';
            done.classList.add('terracotta-ob-done--show');
        }
        setTimeout(() => {
            const ob = document.getElementById('enderlink-onboarding');
            if (ob) {
                localStorage.setItem('enderlink_onboarding_seen', '1');
                ob.classList.add('terracotta-ob--fadeout');
                setTimeout(showEnderlinkMain, 450);
            }
        }, 1500);
    }, 1200);
}

function enderlinkHide() {
    document.getElementById('enderlink-host-panel').style.display = 'none';
    const jp = document.getElementById('enderlink-join-panel');
    if (jp) jp.style.display = 'none';
    document.getElementById('enderlink-hint').style.display = 'none';
}

function enderlinkBackToHome() {
    document.getElementById('enderlink-host-panel').style.display = 'none';
    const jp = document.getElementById('enderlink-join-panel');
    if (jp) jp.style.display = 'none';
    document.getElementById('enderlink-hint').style.display = 'none';
    document.getElementById('enderlink-home').style.display = '';
    updateEnderlinkStatus('未连接', 'disconnected');
}

async function enderlinkHostStep1() {
    enderlinkHide();
    document.getElementById('enderlink-home').style.display = 'none';
    const ejPanel = document.getElementById('enderlink-join-panel');
    if (ejPanel) ejPanel.style.display = 'none';
    document.getElementById('enderlink-host-panel').style.display = '';
    showLanStep('enderlink', 'host', 1);
    const nextBtn = document.getElementById('enderlink-host-next');
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
    if (S.enderlinkGamePoll) clearInterval(S.enderlinkGamePoll);
    S.enderlinkGamePoll = setInterval(doCheck, 2000);
}

async function enderlinkHostNext() {
    let gs = null;
    try { gs = await API.getGameStatus(); } catch (e) {}
    if (!gs || !gs.running) {
        showToast('请先启动游戏，然后在游戏内开放局域网联机', 'error');
        return;
    }
    if (S.enderlinkGamePoll) { clearInterval(S.enderlinkGamePoll); S.enderlinkGamePoll = null; }
    showLanStep('enderlink', 'host', 2);
    try {
        let node = null;
        const vs = featureBridge;
        if (vs && vs.enderlinkSelectedNode) {
          node = vs.enderlinkSelectedNode;
        } else if (vs && vs.enderlinkNodes && vs.enderlinkNodes.length) {
          node = vs.enderlinkNodes[0];
        } else {
          if (S._enderlinkNodes.length === 0) await enderlinkRefreshNodes();
          node = S._enderlinkNodes[S._enderlinkNodeIdx % S._enderlinkNodes.length];
        }
        if (!node) throw new Error('联机节点不可用');
        let gamePort = gs.lanPort;
        if (!gamePort) {
            const scanResult = await bridge.redstoneOnline.scanPort();
            gamePort = scanResult && scanResult.ok && scanResult.port ? scanResult.port : 25565;
        }
        const r = await bridge.enderlinkOnline.start({
            node: node,
            localPort: gamePort,
            roomName: '',
            gameVersion: gs.versionId || '1.12.2',
            isPublic: false
        });
        if (!r || !r.ok) throw new Error(r && r.error ? r.error : '开启失败');
        S._enderlinkRunning = true;
        const addrEl = document.getElementById('enderlink-room-addr');
        if (addrEl) addrEl.textContent = r.address;
        showLanStep('enderlink', 'host', 3);
        const hint = document.getElementById('enderlink-hint');
        if (hint) {
            hint.textContent = '联机地址已生成，发送给朋友即可加入';
            hint.style.display = '';
            hint.style.background = 'rgba(16,185,129,0.1)';
            hint.style.color = 'var(--green)';
        }
    } catch (e) {
        showToast('开启联机失败: ' + (e.message || e), 'error');
        enderlinkBackToHome();
    }
}

async function enderlinkDisconnect() {
    try { await bridge.enderlinkOnline.stop(); } catch (e) {}
    S._enderlinkRunning = false;
    enderlinkBackToHome();
    showToast('已断开 EnderLink 联机', 'info');
}

export { updateEnderlinkStatus, enderlinkSwitchTab, addEnderlinkLog, enderlinkRefreshNodes, enderlinkRefreshRooms, enderlinkCopyHall, enderlinkUpdateNodeBtn, enderlinkCycleNode, enderlinkToggle, enderlinkStart, enderlinkStop, enderlinkCopyAddr, enderlinkInitPage, showEnderlinkOnboarding, showEnderlinkMain, enderlinkStartOnboarding, enderlinkHide, enderlinkBackToHome, enderlinkHostStep1, enderlinkHostNext, enderlinkDisconnect };
