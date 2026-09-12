function switchLanTab(page, tab, btnEl) {
    const tabsContainer = btnEl.closest('.lan-tabs');
    if (tabsContainer) tabsContainer.querySelectorAll('.lan-tab').forEach(t => t.classList.remove('active'));
    btnEl.classList.add('active');

    if (page === 'terracotta') {
        const connected = document.getElementById('terracotta-connected');
        if (connected && connected.style.display !== 'none') return;
        if (tab === 'host') {
            terracottaHostStep1();
        } else {
            terracottaJoinStep1();
        }
    }
}

let terracottaPollTimer = null;
let _terracottaPollRefresher = null;
let terracottaState = { mode: null, connected: false };

function updateTerracottaStatus(title, desc, state) {
    document.getElementById('terracotta-status-title').textContent = title;
    document.getElementById('terracotta-status-desc').textContent = desc;
    const dot = document.getElementById('terracotta-status-dot');
    dot.className = 'lan-status-dot';
    if (state === 'connected') dot.classList.add('connected');
    else if (state === 'connecting') dot.classList.add('connecting');
    else dot.classList.add('disconnected');
}

async function terracottaHostStep1() {
    terracottaHide();
    document.getElementById('terracotta-home').style.display = 'none';
    document.getElementById('terracotta-join-panel').style.display = 'none';
    const panel = document.getElementById('terracotta-host-panel');
    panel.style.display = '';
    showTerracottaStep('host', 1);
    updateTerracottaStatus('陶瓦联机 - 创建房间', '请先启动游戏并开放局域网', 'disconnected');
    // 启动游戏状态监听：游戏运行后启用"下一步"
    const nextBtn = document.getElementById('terracotta-host-next');
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
    if (window._terracottaGamePoll) clearInterval(window._terracottaGamePoll);
    window._terracottaGamePoll = setInterval(doCheck, 2000);
}

async function terracottaHostNext() {
    // 未启动游戏则无法下一步
    let gs = null;
    try { gs = await API.getGameStatus(); } catch (e) {}
    if (!gs || !gs.running) {
        showToast('请先启动游戏，然后在游戏内开放局域网联机', 'error');
        return;
    }
    if (window._terracottaGamePoll) { clearInterval(window._terracottaGamePoll); window._terracottaGamePoll = null; }
    const agreed = await terracottaShowAgreement();
    if (!agreed) return;

    let gamePort = gs.lanPort || 25565;
    const playerName = localStorage.getItem('cachedPlayerName') || 'Player';
    // 进入"正在创建隧道连接中"
    showTerracottaStep('host', 2);
    updateTerracottaStatus('陶瓦联机 - 主机', '正在创建隧道连接中...', 'connecting');
    try {
        const result = await API.easytierHost(gamePort, playerName);
        if (!result.success) throw new Error(result.error || '创建失败');
        terracottaState = { mode: 'host', connected: true };
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
        terracottaState = { mode: 'guest', connected: true };
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

// 流程步骤切换：flow='host'|'join'
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
    if (window._terracottaGamePoll) { clearInterval(window._terracottaGamePoll); window._terracottaGamePoll = null; }
    if (terracottaPollTimer) { clearInterval(terracottaPollTimer); terracottaPollTimer = null; }
    if (_terracottaPollRefresher) { clearInterval(_terracottaPollRefresher); _terracottaPollRefresher = null; }
    document.getElementById('terracotta-host-panel').style.display = 'none';
    document.getElementById('terracotta-join-panel').style.display = 'none';
    document.getElementById('terracotta-hint').style.display = 'none';
    document.getElementById('terracotta-home').style.display = '';
    updateTerracottaStatus('未连接', '启动游戏后可创建房间或加入房间', 'disconnected');
    terracottaState = { mode: null, connected: false };
}

function terracottaHide() {
    document.getElementById('terracotta-host-panel').style.display = 'none';
    document.getElementById('terracotta-join-panel').style.display = 'none';
    document.getElementById('terracotta-hint').style.display = 'none';
    if (terracottaPollTimer) { clearInterval(terracottaPollTimer); terracottaPollTimer = null; }
    if (_terracottaPollRefresher) { clearInterval(_terracottaPollRefresher); _terracottaPollRefresher = null; }
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
    window.electronAPI.clipboard.writeText(code).then(() => {
        showToast('房间码已复制！发送给朋友即可加入', 'success');
    });
}

function terracottaCopyAddr() {
    const addr = document.getElementById('terracotta-connect-addr').textContent;
    if (!addr || addr === '等待分配...') return;
    window.electronAPI.clipboard.writeText(addr).then(() => {
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
            <p>陶瓦联机基于 <a href="https://github.com/EasyTier/EasyTier" style="color:var(--primary)">EasyTier</a> 开源项目，由第三方提供公共节点。</p>
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

/* ============================================================================
   陶瓦联机 - 首次使用引导（核心未安装时显示，微软风格引导动画）
   流程：图标居中渐入 → 欢迎语 → 点击下载图标 → 变进度条 → 完成显示"祝你使用愉快！"
   ============================================================================ */
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

let _lastTerracottaStateIndex = -1;
let _terracottaPollFailCount = 0;

function terracottaStartPolling() {
    if (terracottaPollTimer) clearInterval(terracottaPollTimer);
    if (_terracottaPollRefresher) { clearInterval(_terracottaPollRefresher); _terracottaPollRefresher = null; }
    _lastTerracottaStateIndex = -1;
    _terracottaPollFailCount = 0;
    let pollInterval = 3000;
    let idleCount = 0;

    const doPoll = async () => {
        try {
            const result = await API.easytierStatus();
            _terracottaPollFailCount = 0;
            if (!result.running) {
                _terracottaPollFailCount++;
                if (_terracottaPollFailCount < 5) return;
                if (typeof showToast === 'function') showToast('陶瓦联机已断开', 'info');
                clearInterval(terracottaPollTimer);
                terracottaPollTimer = null;
                terracottaBackToHome();
                return;
            }
            if (!result.state) return;

            const state = result.state;
            const stateType = state.state;
            const stateIndex = result.stateIndex || state.index || -1;

            if (stateIndex > 0 && stateIndex === _lastTerracottaStateIndex) {
                idleCount++;
                if (idleCount > 5) pollInterval = 5000;
                return;
            }
            _lastTerracottaStateIndex = stateIndex;
            idleCount = 0;
            pollInterval = 1500;
            if (terracottaPollTimer) { clearInterval(terracottaPollTimer); terracottaPollTimer = setInterval(doPoll, pollInterval); }

            const profiles = result.profiles || state.profiles || [];
            const difficulty = result.difficulty || state.difficulty || null;
            const errorType = result.errorType || null;
            const errorMessage = result.errorMessage || null;

            if (terracottaState.mode === 'host') {
                if (stateType === 'host-ok') {
                    const roomObj = state.room;
                    const roomCode = (typeof roomObj === 'object' && roomObj !== null) ? (roomObj.code || '') : (roomObj || result.roomCode || '');
                    document.getElementById('terracotta-roomcode').textContent = roomCode;
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
            } else if (terracottaState.mode === 'guest') {
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
            _terracottaPollFailCount++;
            console.warn(`[Terracotta] 状态轮询失败 (连续${_terracottaPollFailCount}次):`, e.message || e);
            if (_terracottaPollFailCount >= 8) {
                document.getElementById('terracotta-hint').textContent = '网络连接异常，请检查网络';
                document.getElementById('terracotta-hint').style.display = '';
                document.getElementById('terracotta-hint').style.background = 'rgba(239,68,68,0.1)';
                document.getElementById('terracotta-hint').style.color = 'var(--red)';
            }
        }
    };

    doPoll();
    terracottaPollTimer = setInterval(doPoll, pollInterval);

    _terracottaPollRefresher = setInterval(() => {
        if (terracottaPollTimer) {
            clearInterval(terracottaPollTimer);
            terracottaPollTimer = setInterval(doPoll, pollInterval);
        }
    }, 30000);
}

/** 更新红石联机状态指示（对齐模组：简洁文本状态） */
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

// ===== 红石联机：标签页 / 服务器 / API Key / 隧道开闭 =====

let _redstoneServers = [];
let _redstoneRunning = false;
let _redstoneServerIdx = 0;

/** 三级标签页切换 */
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

/** 拉取服务器节点列表，填充按钮文本 */
async function redstoneRefreshServers() {
    const btn = document.getElementById('redstone-server-btn');
    const info = document.getElementById('redstone-server-info');
    if (btn) btn.textContent = '服务器: 加载中...';
    if (info) info.textContent = '正在加载节点列表...';
    try {
        const r = await window.electronAPI.redstoneOnline.getServers();
        if (r && r.ok && r.servers && r.servers.length > 0) {
            _redstoneServers = r.servers;
            if (info) info.textContent = '共 ' + r.servers.length + ' 个节点';
        } else {
            if (info) info.textContent = '节点列表为空（使用默认节点）';
            _redstoneServers = [{ name: '上海', address: '122.51.108.96' }];
        }
    } catch (e) {
        if (info) info.textContent = '加载失败: ' + e.message;
        _redstoneServers = [{ name: '上海', address: '122.51.108.96' }];
    }
    _redstoneServerIdx = 0;
    updateServerBtn();
}

/** 更新服务器循环按钮文本 */
function updateServerBtn() {
    const btn = document.getElementById('redstone-server-btn');
    if (!btn || _redstoneServers.length === 0) return;
    const s = _redstoneServers[_redstoneServerIdx % _redstoneServers.length];
    btn.textContent = '服务器: ' + s.name + ' (' + s.address + ')';
}

/** 循环选择下一个服务器（对齐模组 RedstoneScreen.java：点击切换） */
function redstoneCycleServer() {
    if (_redstoneServers.length === 0) return;
    _redstoneServerIdx = (_redstoneServerIdx + 1) % _redstoneServers.length;
    updateServerBtn();
    addRedstoneLog('切换到服务器: ' + _redstoneServers[_redstoneServerIdx].name);
}

/** 开/关隧道 */
async function redstoneToggle() {
    if (_redstoneRunning) await redstoneStop();
    else await redstoneStart();
}

/** 开启隧道 */
async function redstoneStart() {
    const btn = document.getElementById('redstone-action-btn');
    if (_redstoneServers.length === 0) { alert('请先等待节点列表加载'); return; }
    const server = _redstoneServers[_redstoneServerIdx % _redstoneServers.length];
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
            const scanResult = await window.electronAPI.redstoneOnline.scanPort();
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
            const scanResult = await window.electronAPI.redstoneOnline.scanPort();
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
        const ctxRes = await fetch('/api/current-context');
        if (ctxRes.ok) {
            const ctx = await ctxRes.json();
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
        }
    } catch (e) { addRedstoneLog('读取当前版本失败: ' + e.message); }
    // 更新页面显示
    const verBox = document.getElementById('redstone-current-version');
    if (verBox) {
        verBox.textContent = versionDesc || '未选择版本';
        verBox.style.color = versionDesc ? 'var(--text-primary)' : 'var(--text-muted)';
    }
    if (versionDesc) addRedstoneLog('当前版本: ' + versionDesc);

    _redstoneRunning = true;
    if (btn) { btn.textContent = '正在开启...'; btn.disabled = true; }
    updateRedstoneStatus('正在连接...', 'connecting');
    addRedstoneLog('选择服务器: ' + server.name + ' (' + server.address + ')');
    addRedstoneLog('带宽上限: 4 Mbps，约支持 5-8 人');
    if (maxPlayers) addRedstoneLog('最大人数: ' + maxPlayers);

    try {
        const r = await window.electronAPI.redstoneOnline.start({
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
            _redstoneRunning = false;
            if (btn) { btn.textContent = '开启隧道'; btn.disabled = false; }
            updateRedstoneStatus('开启失败', 'disconnected');
            addRedstoneLog('开启失败: ' + (r && r.error ? r.error : '未知错误'));
        }
    } catch (e) {
        _redstoneRunning = false;
        if (btn) { btn.textContent = '开启隧道'; btn.disabled = false; }
        updateRedstoneStatus('开启失败', 'disconnected');
        addRedstoneLog('开启失败: ' + e.message);
    }
}

/** 关闭隧道 */
async function redstoneStop() {
    const btn = document.getElementById('redstone-action-btn');
    if (btn) { btn.disabled = true; btn.textContent = '正在关闭...'; }
    try { await window.electronAPI.redstoneOnline.stop(); }
    catch (e) { addRedstoneLog('关闭失败: ' + e.message); }
    _redstoneRunning = false;
    document.getElementById('redstone-connected-info').style.display = 'none';
    if (btn) { btn.textContent = '开启隧道'; btn.disabled = false; }
    updateRedstoneStatus('未连接', 'disconnected');
    addRedstoneLog('隧道已关闭');
}

/** 复制联机地址 */
function redstoneCopyAddr() {
    const addr = document.getElementById('redstone-room-addr').textContent;
    if (!addr || addr === '--') return;
    navigator.clipboard.writeText(addr).then(() => {
        addRedstoneLog('地址已复制: ' + addr);
    }).catch(() => {});
}

/** 追加日志 */
function addRedstoneLog(msg) {
    const logEl = document.getElementById('redstone-room-log');
    if (!logEl) return;
    const time = new Date().toLocaleTimeString();
    logEl.textContent += '[' + time + '] ' + msg + '\n';
    logEl.scrollTop = logEl.scrollHeight;
}

/** 读取当前选中的游戏版本并显示在页面上（用于红石联机页面顶部"当前版本"区域） */
async function redstoneRefreshCurrentVersion() {
    const verBox = document.getElementById('redstone-current-version');
    if (!verBox) return;
    try {
        const res = await fetch('/api/current-context');
        if (!res.ok) throw new Error('HTTP ' + res.status);
        const ctx = await res.json();
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

/** 红石联机页面初始化（由导航跳转触发） */
async function redstoneInitPage() {
    // 首次使用才显示引导（localStorage 标记），之后直接进入主界面
    if (!localStorage.getItem('redstone_onboarding_seen')) {
        showRedstoneOnboarding();
    } else {
        showRedstoneMain();
    }
    // 后台准备服务器节点列表（引导完成后主界面可直接使用）
    if (_redstoneServers.length === 0) redstoneRefreshServers();
    // 同步主进程隧道状态：若已开启则直接恢复主界面
    try {
        const status = await window.electronAPI.redstoneOnline.getStatus();
        if (status && status.running && status.address) {
            showRedstoneMain();
            const addrEl = document.getElementById('redstone-room-addr');
            if (addrEl) addrEl.textContent = status.address;
        }
    } catch (e) {}
}

/* ============================================================================
   红石联机 - 与陶瓦联机一致的流程（首次引导 / 实例卡片主界面 / 创建 / 加入）
   ============================================================================ */
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

function showLanStep(prefix, flow, step) {
    const flowEl = document.getElementById(prefix + '-' + flow + '-panel');
    if (!flowEl) return;
    for (let i = 1; i <= 3; i++) {
        const st = flowEl.querySelector('#' + prefix + '-' + flow + '-step' + i);
        if (st) st.style.display = i === step ? '' : 'none';
    }
    const stepEl = flowEl.querySelector('#' + prefix + '-' + flow + '-step' + step);
    if (stepEl) {
        stepEl.classList.remove('terracotta-step--enter');
        void stepEl.offsetWidth;
        stepEl.classList.add('terracotta-step--enter');
    }
}

async function redstoneHostStep1() {
    redstoneHide();
    document.getElementById('redstone-home').style.display = 'none';
    document.getElementById('redstone-join-panel').style.display = 'none';
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
    if (window._redstoneGamePoll) clearInterval(window._redstoneGamePoll);
    window._redstoneGamePoll = setInterval(doCheck, 2000);
}

async function redstoneHostNext() {
    let gs = null;
    try { gs = await API.getGameStatus(); } catch (e) {}
    if (!gs || !gs.running) {
        showToast('请先启动游戏，然后在游戏内开放局域网联机', 'error');
        return;
    }
    if (window._redstoneGamePoll) { clearInterval(window._redstoneGamePoll); window._redstoneGamePoll = null; }
    showLanStep('redstone', 'host', 2);
    try {
        if (_redstoneServers.length === 0) await redstoneRefreshServers();
        const server = _redstoneServers[_redstoneServerIdx % _redstoneServers.length];
        if (!server) throw new Error('服务器节点不可用');
        let gamePort = gs.lanPort;
        if (!gamePort) {
            const scanResult = await window.electronAPI.redstoneOnline.scanPort();
            gamePort = scanResult && scanResult.ok && scanResult.port ? scanResult.port : 25565;
        }
        const r = await window.electronAPI.redstoneOnline.start({
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
    try { await window.electronAPI.redstoneOnline.stop(); } catch (e) {}
    redstoneBackToHome();
    showToast('已断开红石联机', 'info');
}

// ===== EnderLink 联机：标签页 / 节点 / 大厅 / 隧道开闭 =====

let _enderlinkNodes = [];
let _enderlinkRunning = false;
let _enderlinkOpening = false;
let _enderlinkNodeIdx = 0;

/** 更新 EnderLink 状态指示 */
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

/** EnderLink 标签页切换 */
function enderlinkSwitchTab(tab) {
    document.querySelectorAll('.enderlink-tabs .lan-tab').forEach(b => {
        b.classList.toggle('active', b.dataset.enderlinkTab === tab);
    });
    document.querySelectorAll('.enderlink-tab-content').forEach(el => {
        el.style.display = el.id === 'enderlink-tab-' + tab ? 'block' : 'none';
    });
}

/** 追加 EnderLink 日志 */
function addEnderlinkLog(msg) {
    const logEl = document.getElementById('enderlink-room-log');
    if (!logEl) return;
    const time = new Date().toLocaleTimeString();
    logEl.textContent += '[' + time + '] ' + msg + '\n';
    logEl.scrollTop = logEl.scrollHeight;
}

/** 刷新 frp 节点列表 */
async function enderlinkRefreshNodes() {
    const info = document.getElementById('enderlink-node-info');
    const list = document.getElementById('enderlink-node-list');
    const btn = document.getElementById('enderlink-node-btn');
    if (btn) btn.textContent = '节点: 加载中...';
    if (info) info.textContent = '正在加载节点列表...';
    try {
        const r = await window.electronAPI.enderlinkOnline.getNodes();
        if (r && r.ok && r.nodes && r.nodes.length > 0) {
            _enderlinkNodes = r.nodes;
            if (info) info.textContent = '共 ' + r.nodes.length + ' 个节点';
            if (list) {
                list.innerHTML = _enderlinkNodes.map((n, i) =>
                    '<div style="padding:6px 0;border-bottom:1px dashed var(--border)"><b>' +
                    (n.name || ('节点' + (i + 1))) + '</b> &nbsp;' +
                    '<span style="color:var(--text-muted)">' + n.frpIp + ':' + n.frpPort + '</span>' +
                    '<div style="font-size:11px;color:var(--text-tertiary);margin-top:2px">ID: ' + n.id + '</div></div>'
                ).join('');
            }
        } else {
            if (info) info.textContent = '节点列表为空';
            if (list) list.innerHTML = '暂无可用节点';
            _enderlinkNodes = [];
        }
    } catch (e) {
        if (info) info.textContent = '加载失败: ' + e.message;
        if (list) list.innerHTML = '加载失败: ' + e.message;
    }
    enderlinkUpdateNodeBtn();
}

/** 刷新联机大厅房间列表 */
async function enderlinkRefreshRooms() {
    const list = document.getElementById('enderlink-hall-list');
    if (!list) return;
    list.textContent = '加载中...';
    try {
        const r = await window.electronAPI.enderlinkOnline.getRooms();
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
                    '<button class="btn btn-secondary btn-sm" onclick="enderlinkCopyHall(\'' + addr + '\')">复制地址</button>';
                list.appendChild(row);
            });
        } else {
            list.textContent = '暂无公开房间';
        }
    } catch (e) {
        list.textContent = '加载失败: ' + e.message;
    }
}

/** 复制大厅房间地址 */
function enderlinkCopyHall(addr) {
    navigator.clipboard.writeText(addr).then(() => {
        addEnderlinkLog('复制大厅地址: ' + addr);
        showToast('地址已复制', 'success');
    }).catch(() => {});
}

/** 更新节点按钮文本 */
function enderlinkUpdateNodeBtn() {
    const btn = document.getElementById('enderlink-node-btn');
    if (!btn) return;
    if (!_enderlinkNodes.length) {
        btn.textContent = '节点: 无可用节点';
        return;
    }
    const n = _enderlinkNodes[_enderlinkNodeIdx % _enderlinkNodes.length];
    btn.textContent = '节点: ' + (n.name || ('节点' + (_enderlinkNodeIdx + 1))) + ' (' + n.frpIp + ':' + n.frpPort + ')';
}

/** 循环选择节点 */
function enderlinkCycleNode() {
    if (!_enderlinkNodes.length) return;
    _enderlinkNodeIdx = (_enderlinkNodeIdx + 1) % _enderlinkNodes.length;
    enderlinkUpdateNodeBtn();
    addEnderlinkLog('切换到节点: ' + (_enderlinkNodes[_enderlinkNodeIdx].name || ('节点' + _enderlinkNodeIdx)));
}

/** 开/关联机 */
async function enderlinkToggle() {
    if (_enderlinkRunning) await enderlinkStop();
    else await enderlinkStart();
}

/** 开启联机 */
async function enderlinkStart() {
    const btn = document.getElementById('enderlink-action-btn');
    if (_enderlinkOpening) return;
    if (!_enderlinkNodes.length) { alert('请先等待节点列表加载'); return; }
    const node = _enderlinkNodes[_enderlinkNodeIdx % _enderlinkNodes.length];
    if (!node) { alert('请选择节点'); return; }

    let gamePort = 25565;
    try {
        const gameStatus = await API.getGameStatus();
        if (gameStatus && gameStatus.running && gameStatus.lanPort) {
            gamePort = gameStatus.lanPort;
            addEnderlinkLog('检测到局域网端口: ' + gamePort);
        } else {
            addEnderlinkLog('正在扫描局域网端口...');
            const scanResult = await window.electronAPI.redstoneOnline.scanPort();
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
        const ctxRes = await fetch('/api/current-context');
        if (ctxRes.ok) {
            const ctx = await ctxRes.json();
            if (ctx && ctx.selectedVersion) versionDesc = ctx.selectedVersion;
        }
    } catch (e) {}

    _enderlinkOpening = true;
    if (btn) { btn.disabled = true; btn.textContent = '正在开启...'; }
    updateEnderlinkStatus('正在连接...', 'connecting');
    addEnderlinkLog('选择节点: ' + (node.name || node.id) + ' (' + node.frpIp + ':' + node.frpPort + ')');
    addEnderlinkLog('本机端口: ' + gamePort + (isPublic ? '，公开大厅' : '，仅端口映射'));
    if (roomName) addEnderlinkLog('房间名: ' + roomName);

    try {
        const r = await window.electronAPI.enderlinkOnline.start({
            node: node,
            localPort: gamePort,
            roomName: roomName,
            gameVersion: versionDesc || '1.12.2',
            isPublic: isPublic
        });
        if (r && r.ok) {
            _enderlinkRunning = true;
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
    _enderlinkOpening = false;
}

/** 关闭联机 */
async function enderlinkStop() {
    const btn = document.getElementById('enderlink-action-btn');
    if (btn) { btn.disabled = true; btn.textContent = '正在关闭...'; }
    try { await window.electronAPI.enderlinkOnline.stop(); }
    catch (e) { addEnderlinkLog('关闭失败: ' + e.message); }
    _enderlinkRunning = false;
    document.getElementById('enderlink-connected-info').style.display = 'none';
    if (btn) { btn.textContent = '开启联机'; btn.disabled = false; }
    updateEnderlinkStatus('未连接', 'disconnected');
    addEnderlinkLog('联机已关闭');
}

/** 复制联机地址 */
function enderlinkCopyAddr() {
    const addr = document.getElementById('enderlink-room-addr');
    if (!addr) return;
    const t = addr.textContent;
    if (!t || t === '--') return;
    navigator.clipboard.writeText(t).then(() => addEnderlinkLog('地址已复制: ' + t)).catch(() => {});
}

/** EnderLink 页面初始化（由导航跳转触发） */
async function enderlinkInitPage() {
    // 首次使用才显示引导（localStorage 标记），之后直接进入主界面
    if (!localStorage.getItem('enderlink_onboarding_seen')) {
        showEnderlinkOnboarding();
    } else {
        showEnderlinkMain();
    }
    // 后台准备节点列表（引导完成后主界面可直接使用）
    if (_enderlinkNodes.length === 0) enderlinkRefreshNodes();
    // 同步主进程状态：若已开启则直接恢复主界面
    try {
        const status = await window.electronAPI.enderlinkOnline.getStatus();
        if (status && status.running && status.address) {
            showEnderlinkMain();
            const addrEl = document.getElementById('enderlink-room-addr');
            if (addrEl) addrEl.textContent = status.address;
        }
    } catch (e) { _enderlinkRunning = false; }
}

/* ============================================================================
   EnderLink 联机 - 与陶瓦/红石联机一致的流程
   ============================================================================ */
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
    document.getElementById('enderlink-join-panel').style.display = 'none';
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
    if (window._enderlinkGamePoll) clearInterval(window._enderlinkGamePoll);
    window._enderlinkGamePoll = setInterval(doCheck, 2000);
}

async function enderlinkHostNext() {
    let gs = null;
    try { gs = await API.getGameStatus(); } catch (e) {}
    if (!gs || !gs.running) {
        showToast('请先启动游戏，然后在游戏内开放局域网联机', 'error');
        return;
    }
    if (window._enderlinkGamePoll) { clearInterval(window._enderlinkGamePoll); window._enderlinkGamePoll = null; }
    showLanStep('enderlink', 'host', 2);
    try {
        if (_enderlinkNodes.length === 0) await enderlinkRefreshNodes();
        const node = _enderlinkNodes[_enderlinkNodeIdx % _enderlinkNodes.length];
        if (!node) throw new Error('联机节点不可用');
        let gamePort = gs.lanPort;
        if (!gamePort) {
            const scanResult = await window.electronAPI.redstoneOnline.scanPort();
            gamePort = scanResult && scanResult.ok && scanResult.port ? scanResult.port : 25565;
        }
        const r = await window.electronAPI.enderlinkOnline.start({
            node: node,
            localPort: gamePort,
            roomName: '',
            gameVersion: gs.versionId || '1.12.2',
            isPublic: false
        });
        if (!r || !r.ok) throw new Error(r && r.error ? r.error : '开启失败');
        _enderlinkRunning = true;
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
    try { await window.electronAPI.enderlinkOnline.stop(); } catch (e) {}
    _enderlinkRunning = false;
    enderlinkBackToHome();
    showToast('已断开 EnderLink 联机', 'info');
}
