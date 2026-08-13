function switchLanTab(page, tab, btnEl) {
    const tabsContainer = btnEl.closest('.lan-tabs');
    tabsContainer.querySelectorAll('.lan-tab').forEach(t => t.classList.remove('active'));
    btnEl.classList.add('active');

    if (page === 'terracotta') {
        const hostPanel = document.getElementById('terracotta-host-panel');
        const joinPanel = document.getElementById('terracotta-join-panel');
        const connected = document.getElementById('terracotta-connected');
        if (connected.style.display !== 'none') return;
        hostPanel.style.display = tab === 'host' ? '' : 'none';
        joinPanel.style.display = tab === 'join' ? '' : 'none';
        if (tab === 'host') {
            terracottaHost();
        } else {
            updateTerracottaStatus('陶瓦联机 - 加入房间', '输入房间码加入', 'disconnected');
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

async function terracottaHost() {
    document.getElementById('terracotta-host-panel').style.display = '';
    document.getElementById('terracotta-join-panel').style.display = 'none';
    document.getElementById('terracotta-connected').style.display = 'none';
    document.getElementById('terracotta-tabs').style.display = '';
    updateTerracottaStatus('陶瓦联机 - 创建房间', '准备创建房间', 'disconnected');
    try {
        const lanResult = await fetch('/api/lan/port');
        if (lanResult.ok) {
            const data = await lanResult.json();
            if (data.port) {
                document.getElementById('terracotta-host-port').value = data.port;
            }
        }
    } catch (e) {}
}

async function terracottaJoin() {
    document.getElementById('terracotta-join-panel').style.display = '';
    document.getElementById('terracotta-host-panel').style.display = 'none';
    document.getElementById('terracotta-connected').style.display = 'none';
    document.getElementById('terracotta-tabs').style.display = '';
    updateTerracottaStatus('陶瓦联机 - 加入房间', '输入房间码加入', 'disconnected');
}

function terracottaBackToActions() {
    document.getElementById('terracotta-host-panel').style.display = '';
    document.getElementById('terracotta-join-panel').style.display = 'none';
    document.getElementById('terracotta-connected').style.display = 'none';
    document.getElementById('terracotta-tabs').style.display = '';
    const tabs = document.getElementById('terracotta-tabs');
    tabs.querySelectorAll('.lan-tab').forEach((t, i) => t.classList.toggle('active', i === 0));
    updateTerracottaStatus('未连接', '创建房间或输入房间码加入', 'disconnected');
}

function terracottaHide() {
    document.getElementById('terracotta-host-panel').style.display = 'none';
    document.getElementById('terracotta-join-panel').style.display = 'none';
    document.getElementById('terracotta-connected').style.display = 'none';
    document.getElementById('terracotta-tabs').style.display = 'none';
    if (terracottaPollTimer) { clearInterval(terracottaPollTimer); terracottaPollTimer = null; }
    if (_terracottaPollRefresher) { clearInterval(_terracottaPollRefresher); _terracottaPollRefresher = null; }
}

async function terracottaStartHost() {
    try {
        const agreed = await terracottaShowAgreement();
        if (!agreed) return;

        const gameStatus = await API.getGameStatus();
        if (!gameStatus.running) {
            showToast('请先启动游戏，然后在游戏内开放局域网联机', 'error');
            return;
        }
        
        let gamePort = gameStatus.lanPort;
        if (!gamePort) {
            const manualPort = parseInt(document.getElementById('terracotta-host-port').value, 10);
            if (manualPort > 0 && manualPort < 65536) {
                gamePort = manualPort;
            }
        }
        if (!gamePort) {
            showToast('请在游戏内先开放局域网联机（按Esc → 对局域网开放）', 'error');
            return;
        }
        
        document.getElementById('terracotta-host-port').value = gamePort;
        
        const playerName = localStorage.getItem('cachedPlayerName') || 'Player';
        showToast('正在初始化陶瓦联机...', 'info');
        
        const result = await API.easytierHost(gamePort, playerName);
        if (result.success) {
            terracottaState = { mode: 'host', connected: true };
            
            document.getElementById('terracotta-host-panel').style.display = 'none';
            document.getElementById('terracotta-connected').style.display = '';
            document.getElementById('terracotta-addr-field').style.display = 'none';
            document.getElementById('terracotta-roomcode').textContent = '等待分配房间码...';
            document.getElementById('terracotta-conn-status').textContent = '正在创建房间...';
            document.getElementById('terracotta-hint').textContent = `已检测到局域网端口 ${gamePort}，房间创建中...`;
            document.getElementById('terracotta-hint').style.background = 'rgba(59,130,246,0.1)';
            document.getElementById('terracotta-hint').style.color = 'var(--blue)';
            
            updateTerracottaStatus('陶瓦联机 - 主机', '正在创建房间...', 'connecting');
            
            terracottaStartPolling();
        }
    } catch (e) {
        showToast('创建联机失败: ' + e.message, 'error');
    }
}

async function terracottaJoinRoom() {
    const codeText = document.getElementById('terracotta-join-code').value.trim();
    if (!codeText) {
        showToast('请输入房间码', 'error');
        return;
    }
    
    try {
        const agreed = await terracottaShowAgreement();
        if (!agreed) return;

        showToast('正在初始化陶瓦联机...', 'info');
        
        const playerName = localStorage.getItem('cachedPlayerName') || 'Player';
        const result = await API.easytierGuest(codeText, playerName);
        if (result.success) {
            terracottaState = { mode: 'guest', connected: true };
            
            document.getElementById('terracotta-join-panel').style.display = 'none';
            document.getElementById('terracotta-connected').style.display = '';
            document.getElementById('terracotta-addr-field').style.display = '';
            document.getElementById('terracotta-roomcode').textContent = '--';
            document.getElementById('terracotta-connect-addr').textContent = '等待分配...';
            document.getElementById('terracotta-conn-status').textContent = '正在连接...';
            document.getElementById('terracotta-hint').textContent = '正在连接到主机...';
            document.getElementById('terracotta-hint').style.background = 'rgba(59,130,246,0.1)';
            document.getElementById('terracotta-hint').style.color = 'var(--blue)';
            
            updateTerracottaStatus('陶瓦联机 - 客户端', '正在连接...', 'connecting');
            
            terracottaStartPolling();
        }
    } catch (e) {
        showToast('加入联机失败: ' + e.message, 'error');
    }
}

async function terracottaDisconnect() {
    try {
        await API.easytierStop();
    } catch (e) {}
    
    terracottaState = { mode: null, connected: false };
    if (terracottaPollTimer) { clearInterval(terracottaPollTimer); terracottaPollTimer = null; }
    
    terracottaBackToActions();
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
                document.getElementById('terracotta-conn-status').textContent = '已断开';
                document.getElementById('terracotta-conn-status').style.color = 'var(--red)';
                clearInterval(terracottaPollTimer);
                terracottaPollTimer = null;
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
                if (stateType === 'host-scanning') {
                    document.getElementById('terracotta-conn-status').textContent = '正在扫描局域网游戏...';
                    document.getElementById('terracotta-conn-status').style.color = 'var(--blue)';
                } else if (stateType === 'host-starting') {
                    document.getElementById('terracotta-conn-status').textContent = '正在启动房间...';
                    document.getElementById('terracotta-conn-status').style.color = 'var(--blue)';
                } else if (stateType === 'host-ok') {
                    const roomObj = state.room;
                    const roomCode = (typeof roomObj === 'object' && roomObj !== null) ? (roomObj.code || '') : (roomObj || result.roomCode || '');
                    document.getElementById('terracotta-roomcode').textContent = roomCode;
                    const profileText = profiles.length > 0 ? ` (${profiles.length}人已连接)` : '';
                    document.getElementById('terracotta-conn-status').textContent = '房间已创建 (P2P)' + profileText;
                    document.getElementById('terracotta-conn-status').style.color = 'var(--green)';
                    document.getElementById('terracotta-hint').textContent = '将房间码发送给朋友即可联机';
                    document.getElementById('terracotta-hint').style.background = 'rgba(16,185,129,0.1)';
                    document.getElementById('terracotta-hint').style.color = 'var(--green)';
                    updateTerracottaStatus('陶瓦联机 - 主机', `房间码: ${roomCode}`, 'connected');
                } else if (stateType === 'exception') {
                    const errMsg = errorMessage || '连接异常';
                    document.getElementById('terracotta-conn-status').textContent = errMsg;
                    document.getElementById('terracotta-conn-status').style.color = 'var(--red)';
                    document.getElementById('terracotta-hint').textContent = errorType ? `错误类型: ${errorType}` : '';
                    document.getElementById('terracotta-hint').style.background = 'rgba(239,68,68,0.1)';
                    document.getElementById('terracotta-hint').style.color = 'var(--red)';
                }
            } else if (terracottaState.mode === 'guest') {
                if (stateType === 'guest-connecting') {
                    document.getElementById('terracotta-conn-status').textContent = '正在连接...';
                    document.getElementById('terracotta-conn-status').style.color = 'var(--blue)';
                } else if (stateType === 'guest-starting') {
                    const diffMap = { 'EASIEST': '和平', 'SIMPLE': '简单', 'MEDIUM': '普通', 'TOUGH': '困难' };
                    const diffText = difficulty && difficulty !== 'UNKNOWN' ? ` | 难度: ${diffMap[difficulty] || difficulty}` : '';
                    document.getElementById('terracotta-conn-status').textContent = '正在建立P2P连接...' + diffText;
                    document.getElementById('terracotta-conn-status').style.color = 'var(--blue)';
                } else if (stateType === 'guest-ok') {
                    const rawUrl = state.url || result.virtualIP || '';
                    const connectUrl = rawUrl.startsWith('127.0.0.1') ? rawUrl : `127.0.0.1${rawUrl.includes(':') ? ':' + rawUrl.split(':').pop() : ''}`;
                    document.getElementById('terracotta-roomcode').textContent = connectUrl;
                    document.getElementById('terracotta-connect-addr').textContent = connectUrl;
                    const profileText = profiles.length > 0 ? ` (${profiles.length}人在线)` : '';
                    document.getElementById('terracotta-conn-status').textContent = '已连接 (P2P)' + profileText;
                    document.getElementById('terracotta-conn-status').style.color = 'var(--green)';
                    document.getElementById('terracotta-hint').textContent = `在Minecraft多人游戏中添加服务器地址: ${connectUrl}`;
                    document.getElementById('terracotta-hint').style.background = 'rgba(16,185,129,0.1)';
                    document.getElementById('terracotta-hint').style.color = 'var(--green)';
                    updateTerracottaStatus('陶瓦联机 - 客户端', `连接地址: ${connectUrl}`, 'connected');
                } else if (stateType === 'exception') {
                    const errMsg = errorMessage || '连接异常';
                    document.getElementById('terracotta-conn-status').textContent = errMsg;
                    document.getElementById('terracotta-conn-status').style.color = 'var(--red)';
                    document.getElementById('terracotta-hint').textContent = errorType ? `错误类型: ${errorType}` : '';
                    document.getElementById('terracotta-hint').style.background = 'rgba(239,68,68,0.1)';
                    document.getElementById('terracotta-hint').style.color = 'var(--red)';
                }
            }
        } catch (e) {
            _terracottaPollFailCount++;
            console.warn(`[Terracotta] 状态轮询失败 (连续${_terracottaPollFailCount}次):`, e.message || e);
            if (_terracottaPollFailCount >= 8) {
                document.getElementById('terracotta-conn-status').textContent = '网络连接异常，请检查网络';
                document.getElementById('terracotta-conn-status').style.color = 'var(--red)';
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

function updateRedstoneTitleCount(input) {
    var c = input.value.length || 0;
    var e = document.getElementById('redstone-title-count');
    if (!e) return;
    e.textContent = c + '/8';
    e.classList.remove('warn', 'full');
    if (c >= 8) e.classList.add('full');
    else if (c >= 6) e.classList.add('warn');
}

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
    if (!btn) return;
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

/** 加载本地 API Key 到输入框 */
async function redstoneLoadApikey() {
    const input = document.getElementById('redstone-apikey');
    if (!input) return;
    try {
        const r = await window.electronAPI.redstoneOnline.getApikey();
        if (r && r.ok && r.apikey) input.value = r.apikey;
    } catch (e) {
        console.warn('[Redstone] loadApikey failed:', e.message);
    }
}

/** 复制 API Key */
function redstoneCopyApikey() {
    const input = document.getElementById('redstone-apikey');
    if (!input || !input.value) return;
    navigator.clipboard.writeText(input.value).then(() => {
        const btn = event.target;
        if (btn) { const old = btn.textContent; btn.textContent = '已复制!'; setTimeout(() => { btn.textContent = old; }, 1500); }
    }).catch(() => {});
}

/** 重置 API Key */
async function redstoneResetApikey() {
    if (!confirm('重置 API Key 后旧密钥将失效，确定继续吗？')) return;
    try {
        const r = await window.electronAPI.redstoneOnline.resetApikey();
        if (r && r.ok && r.apikey) {
            document.getElementById('redstone-apikey').value = r.apikey;
            addRedstoneLog('API Key 已重置');
        } else {
            alert('重置失败: ' + (r && r.error ? r.error : '未知错误'));
        }
    } catch (e) {
        alert('重置失败: ' + e.message);
    }
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

    const titleInput = document.getElementById('redstone-room-title');
    const title = (titleInput ? titleInput.value : '').trim().slice(0, 8);
    const publicAccess = document.getElementById('redstone-is-open') ? document.getElementById('redstone-is-open').checked : true;
    const allowOffline = document.getElementById('redstone-allow-offline') ? document.getElementById('redstone-allow-offline').checked : false;

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
    if (title) addRedstoneLog('房间标题: ' + title);

    try {
        const r = await window.electronAPI.redstoneOnline.start({
            serverAddress: server.address, gamePort: gamePort,
            title: title, description: versionDesc, publicAccess: publicAccess, allowOffline: allowOffline,
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
    // 同步主进程状态：每次进入页面都检查真实隧道状态
    // 防止页面切换后 _redstoneRunning 与主进程不一致
    try {
        const status = await window.electronAPI.redstoneOnline.getStatus();
        if (status.reconnecting) {
            // 正在自动重连中
            _redstoneRunning = true;
            const btn = document.getElementById('redstone-action-btn');
            if (btn) { btn.textContent = '关闭隧道'; btn.disabled = false; }
            updateRedstoneStatus('正在自动重连...', 'connecting');
        } else if (!status.running) {
            _redstoneRunning = false;
            const btn = document.getElementById('redstone-action-btn');
            if (btn) { btn.textContent = '开启隧道'; btn.disabled = false; }
            const info = document.getElementById('redstone-connected-info');
            if (info) info.style.display = 'none';
            updateRedstoneStatus('未连接', 'disconnected');
        } else {
            _redstoneRunning = true;
            const btn = document.getElementById('redstone-action-btn');
            if (btn) { btn.textContent = '关闭隧道'; btn.disabled = false; }
            const info = document.getElementById('redstone-connected-info');
            if (info) info.style.display = '';
            if (status.address) {
                document.getElementById('redstone-room-addr').textContent = status.address;
                updateRedstoneStatus('隧道已开启 | ' + status.address, 'connected');
            }
            // 恢复房间设置显示
            const titleInput = document.getElementById('redstone-room-title');
            if (titleInput && status.title !== undefined) {
                titleInput.value = status.title;
                updateRedstoneTitleCount(titleInput);
            }
            const isOpenInput = document.getElementById('redstone-is-open');
            const isOpenSwitch = document.getElementById('redstone-is-open-switch');
            const isOpenValue = status.publicAccess !== undefined ? status.publicAccess : status.isOpen;
            if (isOpenInput && isOpenValue !== undefined) {
                isOpenInput.checked = isOpenValue;
                if (isOpenSwitch) isOpenSwitch.classList.toggle('active', isOpenValue);
            }
            const allowOfflineInput = document.getElementById('redstone-allow-offline');
            const allowOfflineSwitch = document.getElementById('redstone-allow-offline-switch');
            if (allowOfflineInput && status.allowOffline !== undefined) {
                allowOfflineInput.checked = status.allowOffline;
                if (allowOfflineSwitch) allowOfflineSwitch.classList.toggle('active', status.allowOffline);
            }
        }
    } catch (e) {
        // 查不到状态时按断开处理
        _redstoneRunning = false;
    }

    redstoneSwitchTab('connect');
    redstoneRefreshServers();
    redstoneLoadApikey();
    // 主动读取当前版本显示在页面上（不阻塞，后台执行）
    redstoneRefreshCurrentVersion();
    // 监听主进程日志
    if (!window._redstoneLogListener) {
        window._redstoneLogListener = true;
        try { window.electronAPI.redstoneOnline.onLog((msg) => addRedstoneLog(msg)); } catch (_) {}
    }
    // 监听自动重连中通知
    if (!window._redstoneReconnectingListener) {
        window._redstoneReconnectingListener = true;
        try {
            window.electronAPI.redstoneOnline.onReconnecting((info) => {
                _redstoneRunning = true;
                const btn = document.getElementById('redstone-action-btn');
                if (btn) { btn.textContent = '关闭隧道'; btn.disabled = false; }
                updateRedstoneStatus(
                    '正在自动重连 (' + (info.attempt || 0) + '/' + (info.maxAttempts || 5) + ')',
                    'connecting'
                );
                addRedstoneLog('隧道异常断开，' + (info.delay || 0) / 1000 + ' 秒后自动重连');
                showToast('隧道断开，正在自动重连...', 'info');
            });
        } catch (_) {}
    }
    // 监听自动重连成功通知
    if (!window._redstoneReconnectedListener) {
        window._redstoneReconnectedListener = true;
        try {
            window.electronAPI.redstoneOnline.onReconnected((info) => {
                _redstoneRunning = true;
                const btn = document.getElementById('redstone-action-btn');
                if (btn) { btn.textContent = '关闭隧道'; btn.disabled = false; }
                const info2 = document.getElementById('redstone-connected-info');
                if (info2) info2.style.display = '';
                if (info && info.address) {
                    document.getElementById('redstone-room-addr').textContent = info.address;
                    updateRedstoneStatus('隧道已开启 | ' + info.address, 'connected');
                } else {
                    updateRedstoneStatus('隧道已开启', 'connected');
                }
                addRedstoneLog('自动重连成功');
                showToast('隧道已自动重连', 'success');
            });
        } catch (_) {}
    }
    // 监听彻底断开通知（超出最大重连次数）
    if (!window._redstoneDisconnectListener) {
        window._redstoneDisconnectListener = true;
        try {
            window.electronAPI.redstoneOnline.onDisconnected(() => {
                if (!_redstoneRunning) return;
                _redstoneRunning = false;
                const btn = document.getElementById('redstone-action-btn');
                if (btn) { btn.textContent = '开启隧道'; btn.disabled = false; }
                const info = document.getElementById('redstone-connected-info');
                if (info) info.style.display = 'none';
                updateRedstoneStatus('连接已断开', 'disconnected');
                addRedstoneLog('隧道连接已彻底断开（自动重连已达最大次数）');
                showToast('红石联机隧道已断开，请重新开启', 'error');
            });
        } catch (_) {}
    }
}
