import { API as API } from '../../core/api.js';
import { bridge as bridge } from '../../core/tauri-bridge.js';
import { showToast as showToast } from './ui-components.js';
import { S } from './lan-state.js';
import { detectLanPort } from './lan-port.js';

function updateTerracottaStatus(title, desc, state) {
    const titleEl = document.getElementById('terracotta-status-title');
    const descEl = document.getElementById('terracotta-status-desc');
    const dot = document.getElementById('terracotta-status-dot');
    if (titleEl) titleEl.textContent = title;
    if (descEl) descEl.textContent = desc;
    if (dot) {
        dot.className = 'lan-status-dot';
        if (state === 'connected') dot.classList.add('connected');
        else if (state === 'connecting') dot.classList.add('connecting');
        else dot.classList.add('disconnected');
    }
}

async function terracottaHostStep1() {
    terracottaHide();
    document.getElementById('terracotta-home').style.display = 'none';
    document.getElementById('terracotta-join-panel').style.display = 'none';
    const panel = document.getElementById('terracotta-host-panel');
    panel.style.display = '';
    showTerracottaStep('host', 1);
    updateTerracottaStatus('陶瓦联机 - 创建房间', '请先启动游戏并开放局域网', 'disconnected');
    // 启动游戏状态监听：游戏运行后启用"下一步"，并探测已开放的局域网端口
    const nextBtn = document.getElementById('terracotta-host-next');
    const portHint = document.getElementById('terracotta-port-hint');
    let detectedPort = null;
    const doCheck = async () => {
        try {
            const gs = await API.getGameStatus();
            const running = !!(gs && gs.running);
            if (nextBtn) {
                nextBtn.disabled = !running;
                nextBtn.title = running ? '下一步' : '请先启动游戏并开放局域网';
            }
            if (running && !detectedPort) {
                detectedPort = await detectLanPort();
                if (portHint && detectedPort) portHint.textContent = '已检测到端口：' + detectedPort;
            }
        } catch (e) {}
    };
    doCheck();
    if (S.terracottaGamePoll) clearInterval(S.terracottaGamePoll);
    S.terracottaGamePoll = setInterval(doCheck, 2000);
}

async function terracottaHostNext() {
    // 未启动游戏则无法下一步
    let gs = null;
    try { gs = await API.getGameStatus(); } catch (e) {}
    if (!gs || !gs.running) {
        showToast('请先启动游戏，然后在游戏内开放局域网联机', 'error');
        return;
    }
    if (S.terracottaGamePoll) { clearInterval(S.terracottaGamePoll); S.terracottaGamePoll = null; }
    const agreed = await terracottaShowAgreement();
    if (!agreed) return;

    let gamePort = gs.lanPort;
    if (!gamePort) gamePort = await detectLanPort();
    if (!gamePort) gamePort = 25565;
    const playerName = localStorage.getItem('cachedPlayerName') || 'Player';
    // 进入"正在创建隧道连接中"
    showTerracottaStep('host', 2);
    updateTerracottaStatus('陶瓦联机 - 主机', '正在创建隧道连接中...', 'connecting');
    try {
        const result = await API.easytierHost(gamePort, playerName);
        if (!result.success) throw new Error(result.error || '创建失败');
        S.terracottaState = { mode: 'host', connected: true };
        document.getElementById('terracotta-hint').textContent = '隧道创建中，请稍候...';
        document.getElementById('terracotta-hint').style.display = '';
        document.getElementById('terracotta-hint').style.background = 'rgba(59,130,246,0.1)';
        document.getElementById('terracotta-hint').style.color = 'var(--blue)';
        terracottaStartPolling();
    } catch (e) {
        showToast('创建联机失败: ' + (e.message || e), 'error');
        terracottaBackToHome();
    }
}

function terracottaJoinStep1() {
    terracottaHide();
    document.getElementById('terracotta-home').style.display = 'none';
    document.getElementById('terracotta-host-panel').style.display = 'none';
    document.getElementById('terracotta-join-panel').style.display = '';
    showTerracottaStep('join', 1);
    updateTerracottaStatus('陶瓦联机 - 加入房间', '请输入房主发来的房间码', 'disconnected');
    const codeBox = document.getElementById('terracotta-join-code');
    if (codeBox) codeBox.value = '';
    const nextBtn = document.getElementById('terracotta-join-next');
    if (nextBtn) nextBtn.disabled = true;
}

function terracottaJoinCodeInput() {
    const code = (document.getElementById('terracotta-join-code').value || '').trim();
    const nextBtn = document.getElementById('terracotta-join-next');
    if (nextBtn) {
        nextBtn.disabled = !code;
        nextBtn.title = code ? '下一步' : '请先输入房间码';
    }
}

async function terracottaJoinNext() {
    const codeText = (document.getElementById('terracotta-join-code').value || '').trim();
    if (!codeText) {
        showToast('请输入房间码', 'error');
        return;
    }
    const agreed = await terracottaShowAgreement();
    if (!agreed) return;

    const playerName = localStorage.getItem('cachedPlayerName') || 'Player';
    showTerracottaStep('join', 2);
    updateTerracottaStatus('陶瓦联机 - 客户端', '正在连接房间...', 'connecting');
    try {
        const result = await API.easytierGuest(codeText, playerName);
        if (!result.success) throw new Error(result.error || '加入失败');
        S.terracottaState = { mode: 'guest', connected: true };
        document.getElementById('terracotta-hint').textContent = '正在建立 P2P 连接，请稍候...';
        document.getElementById('terracotta-hint').style.display = '';
        document.getElementById('terracotta-hint').style.background = 'rgba(59,130,246,0.1)';
        document.getElementById('terracotta-hint').style.color = 'var(--blue)';
        terracottaStartPolling();
    } catch (e) {
        showToast('加入联机失败: ' + (e.message || e), 'error');
        terracottaBackToHome();
    }
}

function showTerracottaStep(flow, step) {
    const flowEl = document.getElementById('terracotta-' + flow + '-panel');
    for (let i = 1; i <= 3; i++) {
        const st = flowEl.querySelector('#terracotta-' + flow + '-step' + i);
        if (st) st.style.display = i === step ? '' : 'none';
    }
    const stepEl = flowEl.querySelector('#terracotta-' + flow + '-step' + step);
    if (stepEl) {
        stepEl.classList.remove('terracotta-step--enter');
        void stepEl.offsetWidth;
        stepEl.classList.add('terracotta-step--enter');
    }
}

function terracottaBackToHome() {
    if (S.terracottaGamePoll) { clearInterval(S.terracottaGamePoll); S.terracottaGamePoll = null; }
    if (S.terracottaPollTimer) { clearInterval(S.terracottaPollTimer); S.terracottaPollTimer = null; }
    if (S._terracottaPollRefresher) { clearInterval(S._terracottaPollRefresher); S._terracottaPollRefresher = null; }
    document.getElementById('terracotta-host-panel').style.display = 'none';
    document.getElementById('terracotta-join-panel').style.display = 'none';
    document.getElementById('terracotta-hint').style.display = 'none';
    document.getElementById('terracotta-home').style.display = '';
    const connectRow = document.getElementById('terracotta-connect-row');
    const connectAddr = document.getElementById('terracotta-connect-addr');
    if (connectRow) connectRow.style.display = 'none';
    if (connectAddr) connectAddr.textContent = '--';
    updateTerracottaStatus('未连接', '启动游戏后可创建房间或加入房间', 'disconnected');
    S.terracottaState = { mode: null, connected: false };
}

function terracottaHide() {
    document.getElementById('terracotta-host-panel').style.display = 'none';
    document.getElementById('terracotta-join-panel').style.display = 'none';
    document.getElementById('terracotta-hint').style.display = 'none';
    if (S.terracottaPollTimer) { clearInterval(S.terracottaPollTimer); S.terracottaPollTimer = null; }
    if (S._terracottaPollRefresher) { clearInterval(S._terracottaPollRefresher); S._terracottaPollRefresher = null; }
}

async function terracottaDisconnect() {
    try {
        await API.easytierStop();
    } catch (e) {}
    terracottaBackToHome();
    showToast('已断开陶瓦联机', 'info');
}

function terracottaCopyRoomCode() {
    const code = document.getElementById('terracotta-roomcode').textContent;
    if (!code || code === '--' || code === '等待分配房间码...') return;
    bridge.clipboard.writeText(code).then(() => {
        showToast('房间码已复制！发送给朋友即可加入', 'success');
    });
}

function terracottaCopyAddr() {
    const addr = document.getElementById('terracotta-connect-addr').textContent;
    if (!addr || addr === '等待分配...') return;
    bridge.clipboard.writeText(addr).then(() => {
        showToast('连接地址已复制', 'success');
    });
}

function terracottaShowAgreement() {
    const agreementSeen = localStorage.getItem('terracotta_agreement_v2');
    if (agreementSeen) return Promise.resolve(true);
    const modal = document.createElement('div');
    modal.style.cssText = 'position:fixed;inset:0;z-index:10000;display:flex;align-items:center;justify-content:center;background:rgba(0,0,0,0.5);';
    modal.innerHTML = `<div style="background:var(--bg-primary);border-radius:12px;padding:24px;max-width:500px;width:90%;box-shadow:0 20px 60px rgba(0,0,0,0.3);">
        <h3 style="margin-bottom:12px">陶瓦联机使用须知</h3>
        <div style="color:var(--text-secondary);font-size:13px;line-height:1.7;margin-bottom:16px">
            <p>陶瓦联机基于 <a href="https://github.com/EasyTier/EasyTier" style="color: var(--accent-ink)">EasyTier</a> 开源项目，由第三方提供公共节点。</p>
            <p style="margin-top:8px">• 联机质量取决于网络环境，可能有延迟</p>
            <p>• 公共节点由社区维护，不保证100%可用</p>
            <p>• 游戏数据通过P2P加密传输，不经过服务器</p>
        </div>
        <div style="display:flex;gap:8px;justify-content:flex-end">
            <button class="btn btn-primary" id="terracotta-agree-btn">我已了解，开始使用</button>
        </div>
    </div>`;
    document.body.appendChild(modal);
    return new Promise(resolve => {
        document.getElementById('terracotta-agree-btn').onclick = () => {
            localStorage.setItem('terracotta_agreement_v2', '1');
            modal.remove();
            resolve(true);
        };
        modal.onclick = (e) => { if (e.target === modal) { modal.remove(); resolve(false); } };
    });
}

async function terracottaExportLog() {
    try {
        const result = await API.easytierLog();
        if (result && result.log) {
            const blob = new Blob([result.log], { type: 'text/plain' });
            const url = URL.createObjectURL(blob);
            const a = document.createElement('a');
            a.href = url;
            a.download = `terracotta-log-${new Date().toISOString().slice(0,10)}.txt`;
            a.click();
            URL.revokeObjectURL(url);
            showToast('日志已导出', 'success');
        } else {
            showToast('暂无日志', 'info');
        }
    } catch (e) {
        showToast('导出日志失败: ' + e.message, 'error');
    }
}

function initTerracottaPage() {
    const ob = document.getElementById('terracotta-onboarding');
    const main = document.getElementById('terracotta-main');
    if (!ob || !main) return;
    // 核心已安装 → 直接显示主界面；未安装 → 首次使用引导
    API.easytierStatus().then(st => {
        if (st && st.installed) {
            showTerracottaMain();
        } else {
            showTerracottaOnboarding();
        }
    }).catch(() => showTerracottaOnboarding());
}

function showTerracottaOnboarding() {
    const ob = document.getElementById('terracotta-onboarding');
    const main = document.getElementById('terracotta-main');
    if (!ob || !main) return;
    terracottaHide();
    main.style.display = 'none';
    ob.style.display = 'flex';
    // 重置到初始阶段（图标+欢迎语+下载按钮），触发入场动画
    document.getElementById('terracotta-ob-dl').style.display = '';
    document.getElementById('terracotta-ob-spinner').style.display = 'none';
    document.getElementById('terracotta-ob-done').style.display = 'none';
    document.getElementById('terracotta-ob-title').textContent = '你好！欢迎使用陶瓦联机';
    document.getElementById('terracotta-ob-desc').textContent = '在首次使用前，请先下载核心';
    ob.classList.remove('terracotta-ob--fadeout');
    void ob.offsetWidth;
    ob.classList.add('terracotta-ob--enter');
}

function showTerracottaMain() {
    const ob = document.getElementById('terracotta-onboarding');
    const main = document.getElementById('terracotta-main');
    if (!ob || !main) return;
    ob.style.display = 'none';
    main.style.display = '';
    terracottaBackToHome();
}

async function terracottaDownloadCore() {
    const dlBtn = document.getElementById('terracotta-ob-dl');
    const spinnerBox = document.getElementById('terracotta-ob-spinner');
    const progText = document.getElementById('terracotta-ob-progress-text');
    if (!dlBtn || !spinnerBox) return;
    // 下载图标 → 转圈加载动画
    dlBtn.style.display = 'none';
    spinnerBox.style.display = 'flex';
    if (progText) progText.textContent = '正在下载核心...';

    try {
        const res = await API.easytierDownload();
        if (res && res.error) throw new Error(res.error);
        // 确认安装状态（后端下载为同步完成，轮询一次即可）
        let st = null;
        for (let i = 0; i < 10; i++) {
            st = await API.easytierDownloadStatus('easytier');
            if (st && (st.status === 'completed' || st.status === 'error')) break;
            await new Promise(r => setTimeout(r, 400));
        }
        if (st && st.status === 'completed') {
            completeTerracottaOnboarding();
        } else {
            failTerracottaDownload();
        }
    } catch (e) {
        failTerracottaDownload();
    }
}

function completeTerracottaOnboarding() {
    const spinnerBox = document.getElementById('terracotta-ob-spinner');
    const done = document.getElementById('terracotta-ob-done');
    if (!spinnerBox || !done) return;
    // 转圈结束 → 打钩矢量动画 + "祝你使用愉快！"
    spinnerBox.style.display = 'none';
    done.style.display = 'block';
    done.classList.add('terracotta-ob-done--show');
    setTimeout(() => {
        const ob = document.getElementById('terracotta-onboarding');
        if (ob) {
            ob.classList.add('terracotta-ob--fadeout');
            setTimeout(showTerracottaMain, 450);
        }
    }, 1600);
}

function failTerracottaDownload() {
    const dlBtn = document.getElementById('terracotta-ob-dl');
    const spinnerBox = document.getElementById('terracotta-ob-spinner');
    if (dlBtn) dlBtn.style.display = '';
    if (spinnerBox) spinnerBox.style.display = 'none';
    document.getElementById('terracotta-ob-title').textContent = '下载失败，请检查网络后重试';
    document.getElementById('terracotta-ob-desc').textContent = '点击下载图标重新下载核心';
}

function terracottaStartPolling() {
    if (S.terracottaPollTimer) clearInterval(S.terracottaPollTimer);
    if (S._terracottaPollRefresher) { clearInterval(S._terracottaPollRefresher); S._terracottaPollRefresher = null; }
    S._lastTerracottaStateIndex = -1;
    S._terracottaPollFailCount = 0;
    let pollInterval = 3000;
    let idleCount = 0;

    const doPoll = async () => {
        try {
            const result = await API.easytierStatus();
            S._terracottaPollFailCount = 0;
            if (!result.running) {
                S._terracottaPollFailCount++;
                if (S._terracottaPollFailCount < 5) return;
                if (typeof showToast === 'function') showToast('陶瓦联机已断开', 'info');
                clearInterval(S.terracottaPollTimer);
                S.terracottaPollTimer = null;
                terracottaBackToHome();
                return;
            }
            if (!result.state) return;

            const state = result.state;
            const stateType = state.state;
            const stateIndex = result.stateIndex || state.index || -1;

            if (stateIndex > 0 && stateIndex === S._lastTerracottaStateIndex) {
                idleCount++;
                if (idleCount > 5) pollInterval = 5000;
                return;
            }
            S._lastTerracottaStateIndex = stateIndex;
            idleCount = 0;
            pollInterval = 1500;
            if (S.terracottaPollTimer) { clearInterval(S.terracottaPollTimer); S.terracottaPollTimer = setInterval(doPoll, pollInterval); }

            const profiles = result.profiles || state.profiles || [];
            const errorType = result.errorType || null;
            const errorMessage = result.errorMessage || null;

            if (S.terracottaState.mode === 'host') {
                if (stateType === 'host-ok') {
                    const roomObj = state.room;
                    const roomCode = (typeof roomObj === 'object' && roomObj !== null) ? (roomObj.code || '') : (roomObj || result.roomCode || '');
                    document.getElementById('terracotta-roomcode').textContent = roomCode;

                    // Terracotta 主机的“连接地址”来自内核下发的虚拟 IP + 游戏端口；
                    // 只有拿到地址时才显示这一行，避免一直显示“--”造成“连接地址不可用”的误解。
                    const virtualIp = result.virtualIP || state.virtualIP || '';
                    const gamePort = result.gamePort || state.gamePort || '';
                    const connectAddr = virtualIp && gamePort ? `${virtualIp}:${gamePort}` : '';
                    const connectRow = document.getElementById('terracotta-connect-row');
                    const connectAddrEl = document.getElementById('terracotta-connect-addr');
                    if (connectAddrEl) connectAddrEl.textContent = connectAddr || '--';
                    if (connectRow) connectRow.style.display = connectAddr ? '' : 'none';

                    const profileText = profiles.length > 0 ? ` (${profiles.length}人已连接)` : '';
                    document.getElementById('terracotta-hint').textContent = '将房间码发送给朋友即可联机' + profileText;
                    document.getElementById('terracotta-hint').style.display = '';
                    document.getElementById('terracotta-hint').style.background = 'rgba(16,185,129,0.1)';
                    document.getElementById('terracotta-hint').style.color = 'var(--green)';
                    updateTerracottaStatus('陶瓦联机 - 主机', `房间码: ${roomCode}`, 'connected');
                    showTerracottaStep('host', 3);
                } else if (stateType === 'exception') {
                    const errMsg = errorMessage || '连接异常';
                    document.getElementById('terracotta-hint').textContent = errMsg + (errorType ? ` (${errorType})` : '');
                    document.getElementById('terracotta-hint').style.display = '';
                    document.getElementById('terracotta-hint').style.background = 'rgba(239,68,68,0.1)';
                    document.getElementById('terracotta-hint').style.color = 'var(--red)';
                    if (typeof showToast === 'function') showToast('创建房间失败: ' + errMsg, 'error');
                }
            } else if (S.terracottaState.mode === 'guest') {
                if (stateType === 'guest-ok') {
                    const rawUrl = state.url || result.virtualIP || '';
                    const connectUrl = rawUrl.startsWith('127.0.0.1') ? rawUrl : `127.0.0.1${rawUrl.includes(':') ? ':' + rawUrl.split(':').pop() : ''}`;
                    document.getElementById('terracotta-connect-addr').textContent = connectUrl;
                    const profileText = profiles.length > 0 ? ` (${profiles.length}人在线)` : '';
                    document.getElementById('terracotta-hint').textContent = `在Minecraft多人游戏中添加服务器地址: ${connectUrl}` + profileText;
                    document.getElementById('terracotta-hint').style.display = '';
                    document.getElementById('terracotta-hint').style.background = 'rgba(16,185,129,0.1)';
                    document.getElementById('terracotta-hint').style.color = 'var(--green)';
                    updateTerracottaStatus('陶瓦联机 - 客户端', `连接地址: ${connectUrl}`, 'connected');
                    showTerracottaStep('join', 3);
                } else if (stateType === 'exception') {
                    const errMsg = errorMessage || '连接异常';
                    document.getElementById('terracotta-hint').textContent = errMsg + (errorType ? ` (${errorType})` : '');
                    document.getElementById('terracotta-hint').style.display = '';
                    document.getElementById('terracotta-hint').style.background = 'rgba(239,68,68,0.1)';
                    document.getElementById('terracotta-hint').style.color = 'var(--red)';
                    if (typeof showToast === 'function') showToast('加入房间失败: ' + errMsg, 'error');
                }
            }
        } catch (e) {
            S._terracottaPollFailCount++;
            console.warn(`[Terracotta] 状态轮询失败 (连续${S._terracottaPollFailCount}次):`, e.message || e);
            if (S._terracottaPollFailCount >= 8) {
                document.getElementById('terracotta-hint').textContent = '网络连接异常，请检查网络';
                document.getElementById('terracotta-hint').style.display = '';
                document.getElementById('terracotta-hint').style.background = 'rgba(239,68,68,0.1)';
                document.getElementById('terracotta-hint').style.color = 'var(--red)';
            }
        }
    };

    doPoll();
    S.terracottaPollTimer = setInterval(doPoll, pollInterval);

    S._terracottaPollRefresher = setInterval(() => {
        if (S.terracottaPollTimer) {
            clearInterval(S.terracottaPollTimer);
            S.terracottaPollTimer = setInterval(doPoll, pollInterval);
        }
    }, 30000);
}

export { updateTerracottaStatus, terracottaHostStep1, terracottaHostNext, terracottaJoinStep1, terracottaJoinCodeInput, terracottaJoinNext, showTerracottaStep, terracottaBackToHome, terracottaHide, terracottaDisconnect, terracottaCopyRoomCode, terracottaCopyAddr, terracottaShowAgreement, terracottaExportLog, initTerracottaPage, showTerracottaOnboarding, showTerracottaMain, terracottaDownloadCore, completeTerracottaOnboarding, failTerracottaDownload, terracottaStartPolling };
