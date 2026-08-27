/**
 * dynamic-island.js - V岛 (灵动岛) 组件
 * 左侧：圆环+数字（任务数），圆环显示总进度
 * 右侧：文字描述
 * 展开：每个任务的进度条
 */

(function () {
    'use strict';

    var _el = null;
    var _hoverZone = null;
    var _expanded = false;
    var _visible = false;
    var _state = 'idle';
    var _autoDismissTimer = null;
    var _hoverTimer = null;
    var _leaveTimer = null;
    var _hideTimer = null;
    var _replyHideTimer = null;
    var _tasks = {};  // 任务列表，key=任务名
    var _currentName = null;  // 当前活跃任务名

    function isEnabled() {
        try {
            var el = document.getElementById('vIsland');
            if (el) return !!el.checked;
            return document.body.classList.contains('v-island-enabled');
        } catch (_) { return false; }
    }

    function _clearHideTimer() {
        if (_hideTimer) { clearTimeout(_hideTimer); _hideTimer = null; }
    }

    function _startHideTimer(delay) {
        _clearHideTimer();
        _hideTimer = setTimeout(function () {
            if (_state === 'idle' && !_expanded && _visible) _hide();
        }, delay || 1200);
    }

    function _ensureHoverZone() {
        if (_hoverZone) return _hoverZone;
        _hoverZone = document.createElement('div');
        _hoverZone.className = 'v-island-hover-zone';
        document.body.appendChild(_hoverZone);

        _hoverZone.addEventListener('mouseenter', function () {
            if (!isEnabled()) return;
            _clearHideTimer();
            if (_hoverTimer) clearTimeout(_hoverTimer);
            _hoverTimer = setTimeout(function () {
                if (!_visible && _state === 'idle') _showIdle();
            }, 250);
        });

        _hoverZone.addEventListener('mouseleave', function () {
            if (_hoverTimer) { clearTimeout(_hoverTimer); _hoverTimer = null; }
            if (_state === 'idle' && _visible && !_expanded) {
                _startHideTimer(800);
            }
        });

        return _hoverZone;
    }

    function _ensureEl() {
        if (_el) return _el;
        _el = document.createElement('div');
        _el.id = 'v-island';
        _el.className = 'v-island';
        _el.innerHTML =
            '<div class="v-island__pill">' +
                '<div class="v-island__ring">' +
                    '<svg viewBox="0 0 36 36">' +
                        '<circle class="v-island__ring-track" cx="18" cy="18" r="15"/>' +
                        '<circle class="v-island__ring-fill" cx="18" cy="18" r="15"/>' +
                    '</svg>' +
                    '<img class="v-island__ring-icon" src="img/icon.svg" alt="">' +
                    '<span class="v-island__ring-num">0</span>' +
                '</div>' +
                '<div class="v-island__info">' +
                    '<div class="v-island__subtitle">点击展开</div>' +
                    '<input type="text" class="v-island__input" placeholder="问我点什么..." style="display:none">' +
                '</div>' +
                '<button class="v-island__close" title="关闭" style="display:none;">×</button>' +
                '<div class="v-island__detail"></div>' +
            '</div>';

        document.body.appendChild(_el);

        var pill = _el.querySelector('.v-island__pill');
        pill.addEventListener('click', function (e) {
            if (e.target.closest('.v-island__close')) {
                e.stopPropagation();
                _hide();
                return;
            }
            // 输入框已显示时，点击 pill 不切换状态（让用户能点输入框）
            if (e.target.classList && e.target.classList.contains('v-island__input')) return;
            _toggleExpand();
        });

        // 输入框回车触发对话
        var input = _el.querySelector('.v-island__input');
        if (input) {
            input.addEventListener('keydown', function (e) {
                if (e.key === 'Enter' || e.keyCode === 13) {
                    e.preventDefault();
                    e.stopPropagation();
                    var q = input.value.trim();
                    if (q) _askQuestion(q);
                } else if (e.key === 'Escape' || e.keyCode === 27) {
                    _collapseChatInput();
                }
            });
            // 阻止输入框点击冒泡到 pill，并确保输入框获得焦点
            input.addEventListener('click', function (e) {
                e.stopPropagation();
                this.focus();
            });
        }

        _el.addEventListener('mouseenter', function () {
            _clearHideTimer();
            if (_leaveTimer) { clearTimeout(_leaveTimer); _leaveTimer = null; }
        });

        _el.addEventListener('mouseleave', function () {
            if (_state === 'idle' && _expanded) {
                _leaveTimer = setTimeout(function () {
                    if (_state === 'idle' && _expanded) _collapse();
                    if (_state === 'idle' && _visible) _startHideTimer(800);
                }, 600);
            } else if (_state === 'idle' && !_expanded && _visible) {
                _startHideTimer(800);
            }
        });

        return _el;
    }

    // 圆环周长 = 2 * PI * 15 ≈ 94.25
    var _circumference = 94.25;

    function _setRingProgress(pct) {
        if (!_el) return;
        var offset = _circumference - (Math.min(pct, 100) / 100) * _circumference;
        var fill = _el.querySelector('.v-island__ring-fill');
        if (fill) fill.style.strokeDashoffset = offset;
    }

    function _setRingNum(num) {
        if (!_el) return;
        var numEl = _el.querySelector('.v-island__ring-num');
        var iconEl = _el.querySelector('.v-island__ring-icon');
        if (numEl) {
            numEl.textContent = num;
            numEl.style.display = num > 0 ? '' : 'none';
        }
        if (iconEl) {
            iconEl.style.display = num > 0 ? 'none' : '';
        }
    }

    function _getActiveTaskCount() {
        var count = 0;
        var keys = Object.keys(_tasks);
        for (var i = 0; i < keys.length; i++) {
            var t = _tasks[keys[i]];
            if (t.status === 'downloading') count++;
        }
        return count;
    }

    function _getTotalProgress() {
        var keys = Object.keys(_tasks);
        if (keys.length === 0) return 0;
        var total = 0;
        var count = 0;
        for (var i = 0; i < keys.length; i++) {
            var t = _tasks[keys[i]];
            if (t.status === 'downloading' || t.status === 'completed') {
                total += Math.max(0, Math.min(100, t.progress || 0));
                count++;
            }
        }
        return count > 0 ? Math.round(total / count) : 0;
    }

    function _esc(s) {
        if (!s) return '';
        return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
    }

    function _fmtSpeed(speed) {
        if (!speed || speed <= 0) return '';
        if (speed > 1024 * 1024) return (speed / 1024 / 1024).toFixed(1) + ' MB/s';
        return (speed / 1024).toFixed(0) + ' KB/s';
    }

    function _buildStageHistoryHtml(stages) {
        if (!stages || stages.length === 0) return '';
        var html = '<div class="v-island__stages">';
        for (var i = 0; i < stages.length; i++) {
            var s = stages[i];
            // 后端格式: { stage, message, progress }
            var name = s.message || s.stage || s.name || '';
            var pct = Math.round(s.progress || s.percent || 0);
            var dc = 'v-island__stage-dot';
            if (pct >= 100) dc += ' v-island__stage-dot--done';
            else if (pct > 0) dc += ' v-island__stage-dot--active';
            var pctText = pct > 0 && pct < 100 ? pct + '%' : '';
            html += '<div class="v-island__stage"><span class="' + dc + '"></span><span class="v-island__stage-name">' + _esc(name) + '</span><span class="v-island__stage-pct">' + pctText + '</span></div>';
        }
        return html + '</div>';
    }

    function _buildFilesHtml(files) {
        if (!files || files.length === 0) return '';
        var shown = files.slice(0, 8);
        var html = '<div class="v-island__files">';
        for (var i = 0; i < shown.length; i++) {
            var f = shown[i];
            // 兼容两种格式: { name, status, progress } 和 { n, s, p }
            var fname = f.name || f.n || f.filename || '';
            var fstatus = f.status || f.s || 'pending';
            var fprogress = Math.round(f.progress || f.p || 0);
            var st = '';
            if (fstatus === 'downloading') st = fprogress + '%';
            else if (fstatus === 'completed' || fstatus === 'done') st = '完成';
            else if (fstatus === 'failed') st = '失败';
            html += '<div class="v-island__file"><span class="v-island__file-name">' + _esc(fname) + '</span><span class="v-island__file-status">' + st + '</span></div>';
        }
        if (files.length > 8) {
            html += '<div class="v-island__file"><span class="v-island__file-name" style="color:var(--text-muted)">... 还有 ' + (files.length - 8) + ' 个文件</span></div>';
        }
        return html + '</div>';
    }

    function _buildTasksHtml() {
        var keys = Object.keys(_tasks);
        if (keys.length === 0) {
            return '<div class="v-island__empty">暂无下载任务</div>';
        }

        var html = '<div class="v-island__tasks">';
        for (var i = 0; i < keys.length; i++) {
            var t = _tasks[keys[i]];
            var pct = Math.max(0, Math.min(100, t.progress || 0));
            var statusText = '';
            var statusClass = '';

            if (t.status === 'downloading') {
                statusText = Math.round(pct) + '%';
                statusClass = 'task--downloading';
            } else if (t.status === 'completed') {
                statusText = '完成';
                statusClass = 'task--completed';
                pct = 100;
            } else if (t.status === 'failed') {
                statusText = '失败';
                statusClass = 'task--failed';
            }

            html += '<div class="v-island__task ' + statusClass + '">' +
                '<div class="v-island__task-head">' +
                    '<span class="v-island__task-name">' + _esc(t.name) + '</span>' +
                    '<span class="v-island__task-pct">' + statusText + '</span>' +
                '</div>' +
                '<div class="v-island__task-bar">' +
                    '<div class="v-island__task-bar-fill" style="width:' + pct + '%"></div>' +
                '</div>';

            // 失败时显示错误信息
            if (t.status === 'failed' && t.message) {
                html += '<div class="v-island__task-error">' + _esc(t.message) + '</div>';
            }

            // 下载中显示阶段步骤和文件列表
            if (t.status === 'downloading') {
                if (t.stageHistory && t.stageHistory.length > 0) html += _buildStageHistoryHtml(t.stageHistory);
                if (t.files && t.files.length > 0) html += _buildFilesHtml(t.files);
                else if (t.currentFile) html += '<div class="v-island__task-current">正在下载: ' + _esc(t.currentFile) + '</div>';
            }

            html += '</div>';
        }
        return html + '</div>';
    }

    function _updateDetail() {
        if (!_expanded || !_el) return;
        var detail = _el.querySelector('.v-island__detail');
        if (!detail) return;
        detail.innerHTML = _buildTasksHtml();
    }

    function _refreshDisplay() {
        if (!_el) return;
        var active = _getActiveTaskCount();
        var totalPct = _getTotalProgress();

        _setRingNum(active);
        _setRingProgress(totalPct);

        var subEl = _el.querySelector('.v-island__subtitle');
        if (subEl) {
            if (active > 0) {
                subEl.textContent = active + ' 个任务进行中  ' + Math.round(totalPct) + '%';
            } else {
                subEl.textContent = '点击展开查看任务';
            }
        }

        if (_expanded) _updateDetail();
    }

    function _expand() {
        _expanded = true;
        _clearHideTimer();
        if (_el) {
            _el.classList.add('v-island--expanded');
            _updateDetail();
        }
    }

    function _collapse() {
        _expanded = false;
        if (_el) _el.classList.remove('v-island--expanded');
    }

    function _toggleExpand() {
        // 空闲状态：点击切换对话输入框
        if (_state === 'idle') {
            if (_expanded) {
                _collapseChatInput();
            } else {
                _expandChatInput();
            }
            return;
        }
        // 有任务时：展开/收起进度详情（原行为）
        if (_expanded) _collapse(); else _expand();
    }

    // 展开 V 岛对话输入框
    function _expandChatInput() {
        _expanded = true;
        _clearHideTimer();
        if (!_el) _ensureEl();
        _el.classList.add('v-island--expanded', 'v-island--chat');
        var input = _el.querySelector('.v-island__input');
        var sub = _el.querySelector('.v-island__subtitle');
        if (sub) sub.style.display = 'none';
        if (input) {
            input.style.display = '';
            // 多次尝试聚焦，确保在 Tauri webview 中也能可靠激活输入框
            var focusAttempts = [50, 150, 300];
            focusAttempts.forEach(function (delay) {
                setTimeout(function () { input.focus(); }, delay);
            });
        }
        // 灵动岛弹出音效（Web Audio API 生成短促"啵"声）
        _playPopSound();
    }

    // 灵动岛弹出音效：短促高频"啵"声，类似 iPhone 灵动岛
    // 复用单个 AudioContext，避免每次新建导致内存泄漏
    var _popSoundCtx = null;
    function _getPopSoundCtx() {
        if (!_popSoundCtx) {
            try {
                _popSoundCtx = new (window.AudioContext || window.webkitAudioContext)();
            } catch (e) {
                _popSoundCtx = null;
            }
        }
        // 浏览器可能因自动播放策略挂起 context，尝试恢复
        if (_popSoundCtx && _popSoundCtx.state === 'suspended') {
            _popSoundCtx.resume().catch(function () {});
        }
        return _popSoundCtx;
    }
    function _playPopSound() {
        try {
            var ctx = _getPopSoundCtx();
            if (!ctx) return;
            var osc = ctx.createOscillator();
            var gain = ctx.createGain();
            osc.type = 'sine';
            osc.frequency.setValueAtTime(880, ctx.currentTime);
            osc.frequency.exponentialRampToValueAtTime(440, ctx.currentTime + 0.15);
            gain.gain.setValueAtTime(0.15, ctx.currentTime);
            gain.gain.exponentialRampToValueAtTime(0.001, ctx.currentTime + 0.2);
            osc.connect(gain);
            gain.connect(ctx.destination);
            osc.start(ctx.currentTime);
            osc.stop(ctx.currentTime + 0.2);
        } catch (e) {}
    }

    // 收起对话输入框
    function _collapseChatInput() {
        if (!_el) return;
        _expanded = false;
        _el.classList.remove('v-island--expanded', 'v-island--chat');
        var input = _el.querySelector('.v-island__input');
        var sub = _el.querySelector('.v-island__subtitle');
        if (input) { input.style.display = 'none'; input.value = ''; }
        if (sub) sub.style.display = '';
        // 回到圆环默认状态
        _el.classList.remove('v-island--thinking');
    }

    // ========================================================================
    // 扩展模块 5：指令执行（AI 识别意图 → 调用软件功能）
    // ========================================================================

    // 工具定义：告诉 AI 有哪些操作可用
    var _tools = [
        {
            name: 'navigate',
            desc: '切换到指定页面',
            danger: false,
            params: { page: '页面名称：home(主页)/download(下载)/resource(资源)/mods(模组)/modpack(整合包)/settings(设置)/accounts(账户)/personalize(个性化)/java(Java)/experimental(实验性)' },
            run: function (args) {
                var pageMap = {
                    '主页': 'home', '首页': 'home', 'home': 'home',
                    '下载': 'download', 'download': 'download',
                    '资源': 'resource', 'resource': 'resource',
                    '模组': 'mods', 'mods': 'mods',
                    '整合包': 'modpack', 'modpack': 'modpack',
                    '设置': 'settings', 'settings': 'settings',
                    '账户': 'accounts', '账号': 'accounts', 'accounts': 'accounts',
                    '个性化': 'personalize', 'personalize': 'personalize',
                    'java': 'java', 'Java': 'java',
                    '实验': 'experimental', '实验性': 'experimental', 'experimental': 'experimental'
                };
                var p = pageMap[args.page] || args.page;
                if (typeof navigateToPage === 'function') {
                    navigateToPage(p);
                    return '已切换到 ' + args.page + ' 页面';
                }
                return '无法切换页面';
            }
        },
        {
            name: 'open_folder',
            desc: '打开文件夹',
            danger: false,
            params: { type: '文件夹类型：mods(模组文件夹)/saves(存档文件夹)/version(版本文件夹)/screenshots(截图文件夹)' },
            run: function (args) {
                var versionId = (typeof currentLaunchVersionId !== 'undefined' && currentLaunchVersionId) ||
                                (typeof launchVersionCustomSelect !== 'undefined' && launchVersionCustomSelect ? launchVersionCustomSelect.getValue() : '');
                if (!versionId) return '请先选择一个版本';
                if (typeof API !== 'undefined' && API.openVersionFolder) {
                    API.openVersionFolder(versionId, args.type || 'mods');
                    return '已打开 ' + (args.type || 'mods') + ' 文件夹';
                }
                return '无法打开文件夹';
            }
        },
        {
            name: 'launch_game',
            desc: '启动游戏',
            danger: true,
            params: { version: '要启动的版本ID（可选，不填则启动当前选中版本）' },
            run: function (args) {
                if (args.version && typeof launchVersionCustomSelect !== 'undefined' && launchVersionCustomSelect) {
                    var versions = (typeof installedVersions !== 'undefined') ? installedVersions : [];
                    var match = versions.find(function (v) {
                        return v.id === args.version || (v.customName && v.customName.indexOf(args.version) !== -1);
                    });
                    if (match) {
                        launchVersionCustomSelect.setValue(match.id);
                    }
                }
                if (typeof handleLaunch === 'function') {
                    handleLaunch();
                    return '正在启动游戏...';
                }
                return '无法启动游戏';
            }
        },
        {
            name: 'select_version',
            desc: '选择当前启动版本',
            danger: false,
            params: { version: '版本名称或ID' },
            run: function (args) {
                if (typeof launchVersionCustomSelect === 'undefined' || !launchVersionCustomSelect) return '版本选择器未初始化';
                if (typeof installedVersions === 'undefined') return '版本列表未加载';
                var match = installedVersions.find(function (v) {
                    return v.id === args.version ||
                           (v.customName && v.customName.indexOf(args.version) !== -1) ||
                           v.id.indexOf(args.version) !== -1;
                });
                if (match) {
                    launchVersionCustomSelect.setValue(match.id);
                    return '已选择版本：' + (match.customName || match.id);
                }
                return '未找到版本：' + args.version;
            }
        },
        {
            name: 'search_mods',
            desc: '搜索模组',
            danger: false,
            params: { query: '搜索关键词' },
            run: function (args) {
                if (typeof navigateToPage === 'function') navigateToPage('mods');
                setTimeout(function () {
                    var searchInput = document.querySelector('#mods-search-input, input[placeholder*="搜索"]');
                    if (searchInput) {
                        searchInput.value = args.query;
                        searchInput.dispatchEvent(new Event('input', { bubbles: true }));
                        searchInput.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
                    }
                    if (typeof searchMods === 'function') searchMods();
                }, 300);
                return '正在搜索模组：' + args.query;
            }
        },
        {
            name: 'open_version_settings',
            desc: '打开版本设置',
            danger: false,
            params: { version: '版本名称或ID（可选）' },
            run: function (args) {
                var versionId = args.version;
                if (!versionId) {
                    versionId = (typeof currentLaunchVersionId !== 'undefined' && currentLaunchVersionId) ||
                                (typeof launchVersionCustomSelect !== 'undefined' && launchVersionCustomSelect ? launchVersionCustomSelect.getValue() : '');
                }
                if (!versionId) return '请先选择一个版本';
                if (typeof installedVersions !== 'undefined') {
                    var match = installedVersions.find(function (v) {
                        return v.id === versionId || (v.customName && v.customName.indexOf(versionId) !== -1) || v.id.indexOf(versionId) !== -1;
                    });
                    if (match) versionId = match.id;
                }
                if (typeof openVersionSettings === 'function') {
                    openVersionSettings(versionId, versionId);
                    return '已打开版本设置：' + versionId;
                }
                return '无法打开版本设置';
            }
        },
        {
            name: 'get_status',
            desc: '获取当前软件状态',
            danger: false,
            params: {},
            run: function () {
                var status = [];
                if (typeof installedVersions !== 'undefined') {
                    status.push('已安装 ' + installedVersions.length + ' 个版本');
                }
                var activeTaskCount = _getActiveTaskCount();
                if (activeTaskCount > 0) {
                    status.push(activeTaskCount + ' 个下载任务进行中');
                }
                var currentVer = '';
                if (typeof launchVersionCustomSelect !== 'undefined' && launchVersionCustomSelect) {
                    currentVer = launchVersionCustomSelect.getValue();
                }
                if (currentVer) {
                    status.push('当前选中版本：' + currentVer);
                }
                return status.length > 0 ? status.join('，') : '一切正常，没有进行中的任务';
            }
        },
        {
            name: 'check_updates',
            desc: '检查模组更新',
            danger: false,
            params: {},
            run: function () {
                var versionId = (typeof currentLaunchVersionId !== 'undefined' && currentLaunchVersionId) ||
                                (typeof launchVersionCustomSelect !== 'undefined' && launchVersionCustomSelect ? launchVersionCustomSelect.getValue() : '');
                if (!versionId) return '请先选择一个版本';
                if (typeof checkModUpdatesForVersion === 'function') {
                    checkModUpdatesForVersion();
                    return '正在检查模组更新...';
                }
                return '无法检查更新';
            }
        },
        {
            name: 'install_version',
            desc: '下载安装 Minecraft 版本',
            danger: true,
            params: { version: 'Minecraft 版本号，如 1.21.1' },
            run: function (args) {
                if (!args.version) return '请指定要下载的版本号';
                if (typeof navigateToPage === 'function') navigateToPage('download');
                setTimeout(function () {
                    var searchInput = document.querySelector('#download-search-input, input[placeholder*="搜索"]');
                    if (searchInput) {
                        searchInput.value = args.version;
                        searchInput.dispatchEvent(new Event('input', { bubbles: true }));
                    }
                }, 300);
                return '已跳转到下载页面，请搜索 ' + args.version + ' 并点击安装';
            }
        }
    ];

    // 构建给 AI 的系统提示词
    function _buildToolPrompt() {
        var toolList = _tools.map(function (t) {
            var params = Object.keys(t.params).map(function (k) {
                return k + '(' + t.params[k] + ')';
            }).join(', ');
            return t.name + (t.danger ? '[确认]' : '') + ':' + params;
        }).join(',');
        return '你是V岛，启动器助手。可用操作：' + toolList + '。用户想操作时回复{{ACTION:{"name":"名","args":{}}}提示语。需确认的操作先问用户。其他正常回复。';
    }

    // 解析 AI 回复，提取操作指令
    function _parseAction(reply) {
        var marker = '{{ACTION:';
        var idx = reply.indexOf(marker);
        if (idx === -1) return null;
        var start = idx + marker.length;
        var depth = 0;
        var jsonEnd = -1;
        for (var i = start; i < reply.length; i++) {
            if (reply[i] === '{') depth++;
            else if (reply[i] === '}') {
                depth--;
                if (depth === 0) { jsonEnd = i; break; }
            }
        }
        if (jsonEnd === -1) return null;
        var jsonStr = reply.substring(start, jsonEnd + 1);
        try {
            var action = JSON.parse(jsonStr);
            // 跳过 JSON 后的 }} 标记结尾
            var textStart = jsonEnd + 1;
            if (reply[textStart] === '}') textStart++;
            if (reply[textStart] === '}') textStart++;
            var text = reply.substring(0, idx) + reply.substring(textStart).trim();
            return { name: action.name, args: action.args || {}, text: text };
        } catch (e) {
            console.warn('[V岛] 解析操作指令失败:', e.message, 'json:', jsonStr);
            return null;
        }
    }

    // 执行操作
    function _executeAction(action) {
        var tool = _tools.find(function (t) { return t.name === action.name; });
        if (!tool) return '未知操作：' + action.name;
        try {
            return tool.run(action.args || {});
        } catch (e) {
            return '执行出错：' + (e.message || e);
        }
    }

    // 显示确认对话框（危险操作）
    function _showConfirmDialog(action, question, reply) {
        if (!_el) return;
        var detail = _el.querySelector('.v-island__detail');
        if (!detail) return;

        _el.classList.remove('v-island--thinking');
        _el.classList.add('v-island--expanded');
        _expanded = true;

        var sub = _el.querySelector('.v-island__subtitle');
        if (sub) {
            sub.style.display = '';
            sub.textContent = '需要确认';
        }

        var tool = _tools.find(function (t) { return t.name === action.name; });
        var actionDesc = tool ? tool.desc : action.name;

        detail.innerHTML =
            '<div class="v-island__reply">' +
                '<div class="v-island__reply-a">' + _esc(reply || actionDesc) + '</div>' +
                '<div style="margin-top:12px;display:flex;gap:8px;">' +
                    '<button class="btn btn-primary v-island__confirm-yes" style="flex:1;padding:8px 12px;font-size:13px;">确认执行</button>' +
                    '<button class="btn btn-ghost v-island__confirm-no" style="flex:1;padding:8px 12px;font-size:13px;">取消</button>' +
                '</div>' +
            '</div>';

        var yesBtn = detail.querySelector('.v-island__confirm-yes');
        var noBtn = detail.querySelector('.v-island__confirm-no');
        if (yesBtn) {
            yesBtn.onclick = function () {
                var result = _executeAction(action);
                _showReply(question, result);
            };
        }
        if (noBtn) {
            noBtn.onclick = function () {
                _showReply(question, '已取消操作');
            };
        }

        // 朗读确认提示
        speak(reply || actionDesc);
    }

    // ========================================================================
    // 扩展模块 6：界面操控（AI 像人一样操作软件）
    // 原理：快照界面元素 → AI 判断要操作哪个 → 执行 → 重新快照
    // ========================================================================

    // 收集当前页面上所有可交互元素，返回文字清单
    function _snapshotUI() {
        var selector = 'button, a, input, [onclick], [role="button"], .version-item, .mod-card, .modpack-item, .search-result, [data-clickable], .clickable, .nav-item, .vset-nav-item, .tab-btn';
        var els = document.querySelectorAll(selector);
        var items = [];
        var visibleEls = [];
        for (var i = 0; i < els.length; i++) {
            var el = els[i];
            // 跳过不可见元素
            var rect = el.getBoundingClientRect();
            if (rect.width === 0 || rect.height === 0) continue;
            if (el.offsetParent === null && el.style.position !== 'fixed') continue;
            // 跳过 V 岛自身元素
            if (el.closest('#v-island')) continue;

            var text = (el.textContent || '').trim().substring(0, 60);
            var placeholder = el.getAttribute('placeholder') || '';
            var title = el.getAttribute('title') || '';
            var type = el.getAttribute('type') || '';
            var value = el.value || '';
            var cls = el.className || '';
            var desc = text || placeholder || title || value || type;
            if (!desc) continue;

            var tag = el.tagName.toLowerCase();
            var action = 'click';
            if (tag === 'input' || tag === 'textarea') {
                action = (type === 'checkbox' || type === 'radio') ? 'click' : 'input';
            }

            var idx = visibleEls.length;
            visibleEls.push(el);
            items.push({
                id: idx,
                tag: tag,
                action: action,
                desc: desc,
                placeholder: placeholder,
                value: value.substring(0, 30)
            });
        }
        // 把元素存到全局变量，执行时用
        _snapshotElements = visibleEls;

        // 生成文字清单
        var lines = items.map(function (it) {
            var line = '[' + it.id + '] ' + it.action + ' "' + it.desc + '"';
            if (it.placeholder) line += ' (placeholder:' + it.placeholder + ')';
            if (it.value && it.action === 'input') line += ' (current:' + it.value + ')';
            return line;
        });
        return lines.join('\n');
    }

    var _snapshotElements = [];

    // 执行 AI 指定的界面操作
    function _executeUIAction(action) {
        if (action.type === 'click') {
            var el = _snapshotElements[action.target];
            if (!el) return '元素' + action.target + '不存在';
            try {
                el.scrollIntoView({ block: 'center' });
                el.click();
                return '已点击"' + (el.textContent || '').trim().substring(0, 30) + '"';
            } catch (e) {
                return '点击失败：' + e.message;
            }
        } else if (action.type === 'input') {
            var el2 = _snapshotElements[action.target];
            if (!el2) return '元素' + action.target + '不存在';
            try {
                el2.scrollIntoView({ block: 'center' });
                el2.focus();
                el2.value = action.value;
                el2.dispatchEvent(new Event('input', { bubbles: true }));
                el2.dispatchEvent(new Event('change', { bubbles: true }));
                if (action.enter) {
                    el2.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', keyCode: 13, which: 13, bubbles: true }));
                    el2.dispatchEvent(new KeyboardEvent('keypress', { key: 'Enter', keyCode: 13, which: 13, bubbles: true }));
                    el2.dispatchEvent(new KeyboardEvent('keyup', { key: 'Enter', keyCode: 13, which: 13, bubbles: true }));
                }
                return '已输入"' + action.value + '"';
            } catch (e) {
                return '输入失败：' + e.message;
            }
        } else if (action.type === 'done') {
            return null; // 表示任务完成
        }
        return '未知操作类型：' + action.type;
    }

    // 解析 AI 返回的操控指令
    function _parseUIAction(reply) {
        var marker = '{{UI:';
        var idx = reply.indexOf(marker);
        if (idx === -1) return null;
        var start = idx + marker.length;
        var depth = 0;
        var jsonEnd = -1;
        for (var i = start; i < reply.length; i++) {
            if (reply[i] === '{') depth++;
            else if (reply[i] === '}') {
                depth--;
                if (depth === 0) { jsonEnd = i; break; }
            }
        }
        if (jsonEnd === -1) return null;
        var jsonStr = reply.substring(start, jsonEnd + 1);
        try {
            var action = JSON.parse(jsonStr);
            if (typeof action.target === 'string') action.target = parseInt(action.target);
            return action;
        } catch (e) {
            console.warn('[V岛] 解析UI指令失败:', e.message, 'json:', jsonStr);
            return null;
        }
    }

    // 操控循环：AI 像人一样一步步操作软件
    async function _runAgentTask(userRequest) {
        if (!_el) _ensureEl();
        var sub = _el.querySelector('.v-island__subtitle');
        var detail = _el.querySelector('.v-island__detail');

        var toolPrompt = '回复格式{{UI:{"type":"click/input/done","target":编号,"value":"文字","think":"用一句话说正在干什么"}}}}。click点按钮，input填输入框(可加enter:true回车)，done表示完成。元素编号是[]里的数字。think描述这一步在做什么。每次只返回一个操作。';

        var MAX_STEPS = 5;
        var history = [];
        var stepLogs = [];
        var lastAction = null;

        // 实时显示操作进度的函数
        function _renderProgress(currentStep, currentDesc, isThinking) {
            if (!detail) return;
            var stepsHtml = '';
            for (var i = 0; i < stepLogs.length; i++) {
                stepsHtml += '<div style="display:flex;align-items:center;gap:6px;padding:3px 0;font-size:12px;color:var(--text-muted);">' +
                    '<span style="width:6px;height:6px;border-radius:50%;background:#4caf50;flex-shrink:0;"></span>' +
                    '<span>' + _esc(stepLogs[i]) + '</span>' +
                '</div>';
            }
            var currentHtml = '';
            if (currentDesc) {
                var dotStyle = isThinking
                    ? 'width:6px;height:6px;border-radius:50%;background:var(--text-muted);flex-shrink:0;animation:v-island-pulse 1s ease-in-out infinite;'
                    : 'width:6px;height:6px;border-radius:50%;background:var(--accent);flex-shrink:0;';
                var color = isThinking ? 'var(--text-muted)' : 'var(--accent)';
                currentHtml = '<div style="display:flex;align-items:center;gap:6px;padding:3px 0;font-size:12px;color:' + color + ';">' +
                    '<span style="' + dotStyle + '"></span>' +
                    '<span>' + _esc(currentDesc) + '</span>' +
                '</div>';
            }
            detail.innerHTML =
                '<div class="v-island__reply">' +
                    '<div style="font-size:12px;line-height:1.6;">' + stepsHtml + currentHtml + '</div>' +
                '</div>';
            detail.scrollTop = detail.scrollHeight;
        }

        // 带重试的AI调用（最多重试3次）
        async function _chatWithRetry(messages) {
            for (var retry = 0; retry < 3; retry++) {
                try {
                    var reply = await chatWithAI(messages);
                    if (reply && reply !== '(空回复)' && reply.trim() !== '') {
                        return reply;
                    }
                } catch (e) {
                    console.warn('[V岛] AI请求失败(第' + (retry + 1) + '次):', e.message);
                }
                await new Promise(function (r) { setTimeout(r, 1200); });
            }
            return null;
        }

        // 任务完成后的结果验证：快照当前界面，让AI根据界面状态生成结果报告
        async function _verifyTaskResult(doneHint) {
            if (sub) sub.textContent = '正在检查任务结果...';
            _renderProgress(stepLogs.length, '正在检查任务结果...', true);
            try {
                var verifySnapshot = _snapshotUI();
                var verifyMsg = '用户的需求是：' + userRequest + '。你已经执行了一些操作。请根据当前界面状态，用一句话告诉用户任务的结果。如果任务成功完成，说明结果；如果没完成或有问题，说明原因。只回复结果，不要返回操作指令。';
                var verifyMessages = [{ role: 'user', content: verifyMsg + '\n\n当前界面元素：\n' + verifySnapshot }];
                var verifyReply = await _chatWithRetry(verifyMessages);
                if (verifyReply && verifyReply !== '(空回复)') {
                    return verifyReply;
                }
            } catch (e) {
                console.warn('[V岛] 任务验证失败:', e.message);
            }
            return doneHint || _generateSummary(userRequest, stepLogs, _snapshotUI());
        }

        for (var step = 0; step < MAX_STEPS; step++) {
            _el.classList.add('v-island--thinking');
            _el.classList.add('v-island--expanded');
            _el.classList.remove('v-island--chat');
            _expanded = true;
            if (sub) {
                sub.style.display = '';
                sub.textContent = '正在操作（第' + (step + 1) + '/' + MAX_STEPS + '步）';
            }
            _renderProgress(step + 1, '思考下一步操作...', true);

            // 快照当前界面
            var snapshot = _snapshotUI();
            var pageId = (typeof currentPage === 'string') ? currentPage : (document.querySelector('.nav-item.active')?.textContent || '').trim();
            var pageDesc = '当前页面：' + (pageId || '未知');

            // 把提示词放到user消息里，不用system消息（避免mimo空回复bug）
            var userMsg = '你是V岛，通过操作界面元素帮用户完成任务。' + toolPrompt + '\n\n' +
                pageDesc + '\n用户需求：' + userRequest + '\n\n界面元素：\n' + snapshot;
            if (history.length > 0) {
                userMsg = '上一步结果：' + history[history.length - 1] + '\n' + userMsg;
            }

            var messages = [{ role: 'user', content: userMsg }];

            var reply = await _chatWithRetry(messages);
            if (!reply) {
                // AI连续空回复，根据界面状态自己生成结果
                var currentSnapshot = _snapshotUI();
                var summary = _generateSummary(userRequest, stepLogs, currentSnapshot);
                _showReply(userRequest, summary);
                return;
            }

            var action = _parseUIAction(reply);
            if (!action) {
                // AI没返回操控指令，直接显示回复
                _showReply(userRequest, reply);
                return;
            }

            // 任务完成 - 验证结果后再回复
            if (action.type === 'done') {
                var doneHint = action.think || '任务完成';
                var verifiedResult = await _verifyTaskResult(doneHint);
                _showReply(userRequest, verifiedResult);
                return;
            }

            // 重复操作检测：如果连续两步操作相同，说明卡住了
            if (lastAction && lastAction.type === action.type && lastAction.target === action.target) {
                var stuckSummary = _generateSummary(userRequest, stepLogs, _snapshotUI());
                _showReply(userRequest, stuckSummary);
                return;
            }

            // 显示这一步的操作描述（实时）
            var stepDesc = action.think || ('执行' + action.type + '操作');
            _renderProgress(step + 1, stepDesc, false);

            // 执行操作
            var result = _executeUIAction(action);
            if (result === null) {
                var doneHint2 = action.think || '任务完成';
                var verifiedResult2 = await _verifyTaskResult(doneHint2);
                _showReply(userRequest, verifiedResult2);
                return;
            }

            lastAction = action;
            stepLogs.push(stepDesc);
            history.push(result + (action.think ? ' (' + action.think + ')' : ''));
            _renderProgress(step + 1, null, false);

            await new Promise(function (r) { setTimeout(r, 1200); });
        }

        // 跑满步数后，生成有用的总结而不是简单的"已执行N步"
        var finalSummary = _generateSummary(userRequest, stepLogs, _snapshotUI());
        _showReply(userRequest, finalSummary);
    }

    // 根据已完成步骤和界面状态生成结果总结（不依赖AI）
    function _generateSummary(userRequest, stepLogs, snapshot) {
        var steps = stepLogs.length;
        if (steps === 0) return '操作未能完成，请稍后重试';

        // 从快照里提取有用信息
        var lines = snapshot.split('\n');
        var resultItems = [];
        for (var i = 0; i < lines.length; i++) {
            // 找搜索结果、卡片等
            if (lines[i].match(/\d+\] click ".*"/) && lines[i].length > 30) {
                var m = lines[i].match(/\d+\] click "(.*)"/);
                if (m && m[1]) {
                    var desc = m[1].substring(0, 40);
                    if (desc.indexOf('安装') !== -1 || desc.indexOf('下载') !== -1) continue;
                    resultItems.push(desc);
                }
            }
        }

        var summary = '已完成 ' + steps + ' 步操作：' + stepLogs[steps - 1];
        if (resultItems.length > 0) {
            summary += '。当前界面显示' + resultItems.length + '个结果，如：' + resultItems.slice(0, 2).join('、');
            if (resultItems.length > 2) summary += '等';
        }
        summary += '。你可以告诉我要安装哪个，或继续其他操作。';
        return summary;
    }

    // 判断是否需要进入"操控模式"（而不是简单问答）
    function _shouldUseAgentMode(q) {
        var keywords = ['找', '搜索', '下载', '安装', '打开', '去', '点击', '帮我', '操作', '切换', '启动'];
        var lowerQ = q.toLowerCase();
        for (var i = 0; i < keywords.length; i++) {
            if (lowerQ.indexOf(keywords[i]) !== -1) return true;
        }
        return false;
    }

    var _pendingConfirm = null;

    // 提问 → V 岛变圆环旋转思考 → 展开显示回复
    async function _askQuestion(q) {
        if (!_el) _ensureEl();
        var input = _el.querySelector('.v-island__input');
        var sub = _el.querySelector('.v-island__subtitle');
        // 隐藏输入框，显示思考状态
        if (input) input.style.display = 'none';
        _el.classList.add('v-island--thinking');
        if (sub) {
            sub.style.display = '';
            sub.textContent = '思考中...';
        }

        // 检查是否是"确认执行"的回复（用户之前被要求确认）
        if (_pendingConfirm && q.match(/^(确认|确定|yes|ok|好|可以)/i)) {
            var pendingAction = _pendingConfirm;
            _pendingConfirm = null;
            var result = _executeAction(pendingAction);
            _dialogHistory.push({ role: 'assistant', content: result });
            if (_dialogHistory.length > 10) _dialogHistory = _dialogHistory.slice(-10);
            _showReply(q, result);
            return;
        }

        // 判断是否进入操控模式
        if (_shouldUseAgentMode(q)) {
            await _runAgentTask(q);
            return;
        }

        // 构建带工具提示的消息
        var messages = [{ role: 'system', content: _buildToolPrompt() }];
        _dialogHistory.push({ role: 'user', content: q });
        if (_dialogHistory.length > 10) _dialogHistory = _dialogHistory.slice(-10);
        messages = messages.concat(_dialogHistory);

        try {
            var reply = await chatWithAI(messages);

            // 解析操作指令
            var action = _parseAction(reply);
            if (action) {
                var tool = _tools.find(function (t) { return t.name === action.name; });
                if (tool && tool.danger) {
                    // 危险操作：先确认
                    _pendingConfirm = action;
                    _showConfirmDialog(action, q, action.text || tool.desc);
                    _dialogHistory.push({ role: 'assistant', content: action.text || tool.desc });
                } else {
                    // 安全操作：直接执行
                    var execResult = _executeAction(action);
                    var displayText = action.text || execResult;
                    if (execResult && execResult.indexOf('出错') === -1 && execResult.indexOf('无法') === -1) {
                        displayText = (action.text ? action.text + ' ' : '') + execResult;
                    }
                    _dialogHistory.push({ role: 'assistant', content: displayText });
                    if (_dialogHistory.length > 10) _dialogHistory = _dialogHistory.slice(-10);
                    _showReply(q, displayText);
                }
            } else {
                // 普通对话
                _dialogHistory.push({ role: 'assistant', content: reply });
                if (_dialogHistory.length > 10) _dialogHistory = _dialogHistory.slice(-10);
                _showReply(q, reply);
            }
        } catch (e) {
            _showReply(q, '出错了：' + (e.message || e));
        }
    }

    // 在 detail 里显示回复（打字机效果逐字输出），并 TTS 朗读
    function _showReply(question, reply) {
        if (!_el) return;
        _el.classList.remove('v-island--thinking');
        _el.classList.remove('v-island--chat');  // 切回展开态展示回复
        _el.classList.add('v-island--expanded');
        _expanded = true;
        var sub = _el.querySelector('.v-island__subtitle');
        var detail = _el.querySelector('.v-island__detail');
        if (sub) {
            sub.style.display = '';
            sub.textContent = 'V 岛回复';
        }
        if (detail) {
            detail.innerHTML =
                '<div class="v-island__reply">' +
                    '<div class="v-island__reply-a"><span class="v-typing-text"></span><span class="v-typing-cursor"></span></div>' +
                '</div>';
            _typewriter(detail.querySelector('.v-typing-text'), reply, 30);
        }
        // TTS 朗读回复
        speak(reply);

        // 打字机结束后 10 秒自动收起（时间按字数动态计算）
        var typingDuration = Math.min(reply.length * 30 + 500, 8000);
        var totalDelay = typingDuration + 10000;
        if (_replyHideTimer) clearTimeout(_replyHideTimer);
        _replyHideTimer = setTimeout(function () {
            if (_state === 'idle' && _expanded) _collapseChatInput();
        }, totalDelay);
    }

    // 打字机效果：逐字输出
    function _typewriter(el, text, speed) {
        if (!el) return;
        var i = 0;
        el.textContent = '';
        var timer = setInterval(function () {
            if (i >= text.length) {
                clearInterval(timer);
                // 打字结束，移除光标
                var cursor = el.parentNode.querySelector('.v-typing-cursor');
                if (cursor) cursor.remove();
                return;
            }
            el.textContent += text.charAt(i);
            i++;
        }, speed);
    }

    function _esc(s) {
        return String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
    }

    function _showIdle() {
        if (!isEnabled()) return;
        var el = _ensureEl();
        _visible = true;
        _state = 'idle';

        var subEl = el.querySelector('.v-island__subtitle');
        var closeBtn = el.querySelector('.v-island__close');
        if (subEl) {
            var active = _getActiveTaskCount();
            subEl.textContent = active > 0 ? active + ' 个任务进行中' : '点击展开查看任务';
        }
        if (closeBtn) closeBtn.style.display = 'none';

        _setRingNum(_getActiveTaskCount());
        _setRingProgress(_getTotalProgress());

        el.classList.remove('v-island--completed', 'v-island--failed', 'v-island--expanded', 'island--active');
        el.classList.add('island--idle');
        _expanded = false;
        el.classList.add('v-island--visible');
    }

    function _hide() {
        _clearHideTimer();
        if (_autoDismissTimer) { clearTimeout(_autoDismissTimer); _autoDismissTimer = null; }
        if (_leaveTimer) { clearTimeout(_leaveTimer); _leaveTimer = null; }
        _visible = false;
        _expanded = false;
        _state = 'idle';
        if (_el) {
            _el.classList.remove('v-island--visible', 'v-island--expanded', 'v-island--completed', 'v-island--failed', 'island--active', 'island--idle');
        }
    }

    // ── 公开接口 ───────────────────────────────────────────────────────────

    function show(title) {
        if (!isEnabled()) return;
        var el = _ensureEl();
        var name = title || '导入中';
        _currentName = name;

        // 添加任务
        if (!_tasks[name]) {
            _tasks[name] = { name: name, progress: 0, status: 'downloading' };
        } else {
            _tasks[name].status = 'downloading';
            _tasks[name].progress = 0;
        }

        _visible = true;
        _state = 'downloading';
        _clearHideTimer();
        if (_autoDismissTimer) { clearTimeout(_autoDismissTimer); _autoDismissTimer = null; }
        if (_leaveTimer) { clearTimeout(_leaveTimer); _leaveTimer = null; }

        el.classList.remove('v-island--completed', 'v-island--failed', 'v-island--expanded', 'island--idle');
        el.classList.add('v-island--visible', 'island--active');

        _refreshDisplay();
    }

    function update(data) {
        if (!isEnabled() || !_el) return;
        if (!data) return;

        var name = data.name || _currentName;
        if (!name || !_tasks[name]) return;

        var task = _tasks[name];
        if (data.progress != null) task.progress = Math.max(0, Math.min(100, data.progress));
        if (data.status) task.status = data.status;
        if (data.speed != null) task.speed = data.speed;
        if (data.message) task.message = data.message;
        if (data.stageHistory) task.stageHistory = data.stageHistory;
        if (data.files) task.files = data.files;
        if (data.currentFile) task.currentFile = data.currentFile;

        var el = _el;

        if (data.status === 'completed') {
            task.progress = 100;
            task.status = 'completed';
            _state = 'completed';
            el.classList.remove('v-island--failed', 'island--active', 'island--idle');
            el.classList.add('v-island--completed', 'v-island--visible');
            _visible = true;

            var subEl = el.querySelector('.v-island__subtitle');
            if (subEl) subEl.textContent = '导入完成';
            var cb = el.querySelector('.v-island__close');
            if (cb) cb.style.display = '';

            // 3秒后移除任务并隐藏（如果没有其他活跃任务）
            (function (taskName) {
                _autoDismissTimer = setTimeout(function () {
                    delete _tasks[taskName];
                    if (_getActiveTaskCount() === 0) {
                        _hide();
                    } else {
                        _state = 'downloading';
                        el.classList.remove('v-island--completed');
                        el.classList.add('island--active');
                        _refreshDisplay();
                    }
                }, 3000);
            })(name);
            _refreshDisplay();
            return;
        }

        if (data.status === 'failed') {
            task.status = 'failed';
            task.message = data.message || '未知错误';
            _state = 'failed';
            el.classList.remove('v-island--completed', 'island--active', 'island--idle');
            el.classList.add('v-island--failed', 'v-island--visible');
            _visible = true;

            var subEl2 = el.querySelector('.v-island__subtitle');
            if (subEl2) subEl2.textContent = (data.message || '导入失败');
            var cb2 = el.querySelector('.v-island__close');
            if (cb2) cb2.style.display = '';
            _refreshDisplay();
            return;
        }

        // 下载中
        _state = 'downloading';
        el.classList.remove('island--idle', 'v-island--completed', 'v-island--failed');
        el.classList.add('island--active');
        _refreshDisplay();
    }

    function dismiss() {
        _tasks = {};
        _currentName = null;
        _hide();
    }

    function preview() {
        if (!isEnabled()) return;
        var el = _ensureEl();
        _visible = true;
        el.classList.remove('v-island--visible', 'v-island--preview', 'v-island--completed', 'v-island--failed', 'island--active', 'island--idle');
        void el.offsetWidth;
        el.classList.add('v-island--preview', 'island--idle');

        _setRingNum(0);
        _setRingProgress(0);
        var subEl = el.querySelector('.v-island__subtitle');
        if (subEl) subEl.textContent = '鼠标移到顶部可唤出';

        el.addEventListener('animationend', function () {
            el.classList.remove('v-island--preview', 'island--idle');
            _visible = false;
        }, { once: true });
    }

    function setupHoverZone() { _ensureHoverZone(); }
    function isVisible() { return _visible; }

    // ========================================================================
    // 扩展模块 1：TTS 语音合成（双模式 + 自动降级）
    // 主方案：IPC 调用主进程 msedge-tts（微软晓晓女声，接近真人）
    // 降级方案：浏览器原生 SpeechSynthesis（机器感，但保证断网也有声）
    // ========================================================================
    var _tts = null;
    function _getTTS() {
        if (_tts) return _tts;
        _tts = {
            queue: [],
            playing: false,
            audio: null,
            voices: [],

            speak: function (text, onEnd) {
                if (!text || !String(text).trim()) { if (onEnd) onEnd(); return; }
                this.queue.push({ text: String(text), onEnd: onEnd });
                if (!this.playing) this._drain();
            },

            stop: function () {
                this.queue = [];
                this.playing = false;
                if (this.audio) { try { this.audio.pause(); } catch (e) {} this.audio = null; }
                try { if (window.speechSynthesis) window.speechSynthesis.cancel(); } catch (e) {}
            },

            _drain: function () {
                var self = this;
                if (this.queue.length === 0) { this.playing = false; return; }
                this.playing = true;
                var item = this.queue.shift();
                var done = false;
                var finish = function () {
                    if (done) return;
                    done = true;
                    if (item.onEnd) item.onEnd();
                    self._drain();
                };
                // 8 秒超时保护，确保引导页不会卡住
                setTimeout(finish, 8000);
                this._playWithEdge(item.text, function (ok) {
                    if (ok) {
                        finish();
                    } else {
                        // msedge-tts 失败，降级到浏览器原生语音合成
                        console.log('[V岛 TTS] 降级到浏览器原生语音合成');
                        self._playWithNative(item.text, finish);
                    }
                });
            },

            // 主方案：IPC 调用主进程 msedge-tts
            _playWithEdge: function (text, cb) {
                if (!window.electronAPI || !window.electronAPI.tts || !window.electronAPI.tts.speak) {
                    cb(false); return;
                }
                var self = this;
                window.electronAPI.tts.speak(text, 'zh-CN-XiaoxiaoNeural').then(function (res) {
                    if (!res || !res.ok || !res.data || res.data.length === 0) {
                        console.warn('[V岛 TTS] msedge-tts 返回空，降级原生');
                        cb(false);
                        return;
                    }
                    try {
                        // Tauri 返回的 Vec<u8> 会序列化为数字数组，需先转 Uint8Array 再构造 Blob
                        var u8 = res.data instanceof Uint8Array ? res.data : new Uint8Array(res.data);
                        var blob = new Blob([u8], { type: 'audio/mpeg' });
                        var url = URL.createObjectURL(blob);
                        var audio = new Audio(url);
                        self.audio = audio;
                        var done = false;
                        var finish = function (ok) {
                            if (done) return;
                            done = true;
                            URL.revokeObjectURL(url);
                            cb(ok);
                        };
                        audio.onended = function () { console.log('[V岛 TTS] msedge-tts 朗读完成'); finish(true); };
                        audio.onerror = function (e) {
                            console.warn('[V岛 TTS] 音频播放失败，降级原生:', e);
                            finish(false);
                        };
                        audio.play().then(function () {
                            console.log('[V岛 TTS] 使用 msedge-tts 朗读');
                        }).catch(function (e) {
                            console.warn('[V岛 TTS] play() 被拒，降级原生:', e);
                            finish(false);
                        });
                    } catch (e) {
                        console.error('[V岛 TTS] Edge 音频处理异常:', e);
                        cb(false);
                    }
                }).catch(function (e) {
                    console.warn('[V岛 TTS] IPC 调用失败，降级原生:', e);
                    cb(false);
                });
            },

            // 降级方案：浏览器原生 SpeechSynthesis
            _loadVoices: function () {
                var self = this;
                if (!window.speechSynthesis) return;
                var pick = function () {
                    var all = window.speechSynthesis.getVoices() || [];
                    self.voices = all;
                };
                pick();
                if (this.voices.length === 0 && window.speechSynthesis.onvoiceschanged !== undefined) {
                    window.speechSynthesis.onvoiceschanged = pick;
                }
            },

            _pickVoice: function () {
                if (!this.voices || this.voices.length === 0) return null;
                var preferred = ['zh-CN-Xiaoxiao', 'zh-CN-Xiaoyi', 'zh-CN-Xiaohan'];
                for (var i = 0; i < preferred.length; i++) {
                    for (var j = 0; j < this.voices.length; j++) {
                        if (this.voices[j].name === preferred[i]) return this.voices[j];
                    }
                }
                for (var k = 0; k < this.voices.length; k++) {
                    if (this.voices[k].lang === 'zh-CN') return this.voices[k];
                }
                for (var m = 0; m < this.voices.length; m++) {
                    if (this.voices[m].lang && this.voices[m].lang.indexOf('zh') === 0) return this.voices[m];
                }
                return this.voices[0];
            },

            _playWithNative: function (text, cb) {
                if (!window.speechSynthesis) { cb(); return; }
                var self = this;
                var utter = new SpeechSynthesisUtterance(text);
                var voice = self._pickVoice();
                if (voice) {
                    utter.voice = voice;
                    console.log('[V岛 TTS] 降级原生语音:', voice.name);
                } else {
                    utter.lang = 'zh-CN';
                    console.warn('[V岛 TTS] 降级原生，未找到中文语音');
                }
                utter.rate = 1.0;
                utter.pitch = 1.0;
                utter.volume = 1.0;
                utter.onend = function () { cb(); };
                utter.onerror = function (e) { console.error('[V岛 TTS] 原生朗读出错:', e); cb(); };
                window.speechSynthesis.speak(utter);
            }
        };
        _tts._loadVoices();
        return _tts;
    }

    function speak(text, onEnd) { _getTTS().speak(text, onEnd); }
    function stopSpeak() { if (_tts) _tts.stop(); }

    // ========================================================================
    // 扩展模块 2：AI 对话（云端供应商，支持 openai/anthropic/google 格式）
    // ========================================================================
    var AI_PROVIDERS = {
        zhipu:     { name: '智谱清言', icon: 'zhipu',     apiFormat: 'openai',     endpoint: 'https://open.bigmodel.cn/api/paas/v4/chat/completions', authHeader: 'Authorization', authPrefix: 'Bearer ', models: [{ id: 'glm-4-flash', name: 'GLM-4-Flash', free: true }, { id: 'glm-4', name: 'GLM-4' }] },
        deepseek:  { name: 'DeepSeek', icon: 'deepseek',  apiFormat: 'openai',     endpoint: 'https://api.deepseek.com/chat/completions',          authHeader: 'Authorization', authPrefix: 'Bearer ', models: [{ id: 'deepseek-chat', name: 'DeepSeek-V3' }, { id: 'deepseek-reasoner', name: 'DeepSeek-R1' }] },
        qwen:      { name: '通义千问', icon: 'qwen',      apiFormat: 'openai',     endpoint: 'https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions', authHeader: 'Authorization', authPrefix: 'Bearer ', models: [{ id: 'qwen-turbo', name: 'Qwen Turbo' }, { id: 'qwen-plus', name: 'Qwen Plus' }] },
        moonshot:  { name: 'Kimi',     icon: 'moonshot',   apiFormat: 'openai',     endpoint: 'https://api.moonshot.cn/v1/chat/completions',          authHeader: 'Authorization', authPrefix: 'Bearer ', models: [{ id: 'moonshot-v1-8k', name: 'Moonshot 8K' }] },
        yi:        { name: '零一万物', icon: 'yi',        apiFormat: 'openai',     endpoint: 'https://api.lingyiwanwu.com/v1/chat/completions',    authHeader: 'Authorization', authPrefix: 'Bearer ', models: [{ id: 'yi-large', name: 'Yi-Large' }] },
        minimax:   { name: 'MiniMax', icon: 'minimax',    apiFormat: 'openai',     endpoint: 'https://api.minimax.chat/v1/text/chatcompletion_v2', authHeader: 'Authorization', authPrefix: 'Bearer ', models: [{ id: 'MiniMax-Text-01', name: 'MiniMax-Text-01' }] },
        stepfun:   { name: '阶跃星辰', icon: 'stepfun',    apiFormat: 'openai',     endpoint: 'https://api.stepfun.com/v1/chat/completions',         authHeader: 'Authorization', authPrefix: 'Bearer ', models: [{ id: 'step-1-8k', name: 'Step-1 8K' }] },
        doubao:    { name: '豆包',     icon: 'doubao',     apiFormat: 'openai',     endpoint: 'https://ark.cn-beijing.volces.com/api/v3/chat/completions', authHeader: 'Authorization', authPrefix: 'Bearer ', models: [{ id: 'doubao-pro-4k', name: 'Doubao Pro 4K' }] },
        siliconflow: { name: '硅基流动', icon: 'siliconflow', apiFormat: 'openai',  endpoint: 'https://api.siliconflow.cn/v1/chat/completions',      authHeader: 'Authorization', authPrefix: 'Bearer ', models: [{ id: 'Qwen/Qwen2.5-7B-Instruct', name: 'Qwen2.5-7B', free: true }] },
        openrouter: { name: 'OpenRouter', icon: 'openrouter', apiFormat: 'openai',   endpoint: 'https://openrouter.ai/api/v1/chat/completions',      authHeader: 'Authorization', authPrefix: 'Bearer ', models: [{ id: 'openai/gpt-4o-mini', name: 'GPT-4o mini' }] },
        groq:      { name: 'Groq',     icon: 'groq',       apiFormat: 'openai',     endpoint: 'https://api.groq.com/openai/v1/chat/completions',     authHeader: 'Authorization', authPrefix: 'Bearer ', models: [{ id: 'llama-3.1-8b-instant', name: 'Llama 3.1 8B', free: true }] },
        openai:    { name: 'OpenAI',   icon: 'openai',     apiFormat: 'openai',     endpoint: 'https://api.openai.com/v1/chat/completions',          authHeader: 'Authorization', authPrefix: 'Bearer ', models: [{ id: 'gpt-4o-mini', name: 'GPT-4o mini' }, { id: 'gpt-4o', name: 'GPT-4o' }] },
        anthropic: { name: 'Anthropic', icon: 'anthropic', apiFormat: 'anthropic',  endpoint: 'https://api.anthropic.com/v1/messages',              authHeader: 'x-api-key',     authPrefix: '',        models: [{ id: 'claude-3-5-sonnet-20241022', name: 'Claude 3.5 Sonnet' }] },
        google:    { name: 'Google Gemini', icon: 'gemini', apiFormat: 'google',    endpoint: '',                                                   authHeader: 'url_key',      authPrefix: '',        models: [{ id: 'gemini-1.5-flash', name: 'Gemini 1.5 Flash', free: true }] },
        baichuan:  { name: '百川',     icon: 'baichuan',   apiFormat: 'openai',     endpoint: 'https://api.baichuan-ai.com/v1/chat/completions',     authHeader: 'Authorization', authPrefix: 'Bearer ', models: [{ id: 'Baichuan4', name: 'Baichuan4' }] }
    };

    function _getAIConfig() {
        try { return JSON.parse(localStorage.getItem('v-island-ai-config') || '{}'); }
        catch (_) { return {}; }
    }

    function _saveAIConfig(cfg) {
        try { localStorage.setItem('v-island-ai-config', JSON.stringify(cfg)); return true; }
        catch (_) { return false; }
    }

    function _buildBody(format, model, messages) {
        if (format === 'anthropic') return { model: model, messages: messages, max_tokens: 1024 };
        if (format === 'google') return { contents: messages.map(function (m) { return { role: m.role === 'assistant' ? 'model' : 'user', parts: [{ text: m.content }] }; }) };
        return { model: model, messages: messages, stream: false };
    }

    function _extractReply(format, data) {
        try {
            if (format === 'anthropic') return (data.content || []).map(function (c) { return c.text; }).join('') || '(空回复)';
            if (format === 'google') return (data.candidates[0].content.parts || []).map(function (p) { return p.text; }).join('') || '(空回复)';
            return data.choices[0].message.content || '(空回复)';
        } catch (e) { return '(解析回复失败)'; }
    }

    // 将 AI 服务商返回的错误转换为用户能看懂的中文提示
    function _friendlyError(status, body) {
        var raw = (body || '').substring(0, 300);
        var lower = raw.toLowerCase();
        if (status === 402 || lower.indexOf('insufficient_balance') !== -1 ||
            lower.indexOf('insufficient balance') !== -1 ||
            lower.indexOf('insufficient_quota') !== -1 ||
            lower.indexOf('insufficient quota') !== -1) {
            return 'AI 账户余额不足，请登录对应服务商官网充值后重试（错误码 ' + status + '）';
        }
        if (status === 401 || lower.indexOf('invalid api key') !== -1 ||
            lower.indexOf('invalid_api_key') !== -1 ||
            lower.indexOf('unauthorized') !== -1) {
            return 'API Key 无效或已过期，请在设置中检查并重新填写正确的 API Key（错误码 ' + status + '）';
        }
        if (status === 403 || lower.indexOf('permission_denied') !== -1 ||
            lower.indexOf('forbidden') !== -1) {
            return 'API Key 权限不足或请求内容被拒绝，请检查 Key 权限或换一种说法重试（错误码 ' + status + '）';
        }
        if (status === 429 || lower.indexOf('rate_limit') !== -1 ||
            lower.indexOf('rate limit') !== -1 ||
            lower.indexOf('too many requests') !== -1) {
            return '请求过于频繁，已触发限流，请稍等几秒后重试（错误码 ' + status + '）';
        }
        if (status === 404 || lower.indexOf('model not found') !== -1 ||
            lower.indexOf('model_not_found') !== -1) {
            return '接口地址或模型名称不正确，请检查供应商配置和所选模型是否匹配（错误码 ' + status + '）';
        }
        if (status === 413 || lower.indexOf('too large') !== -1 ||
            lower.indexOf('context length') !== -1) {
            return '对话内容过长，超出了 AI 模型的处理上限，请缩短内容后重试（错误码 ' + status + '）';
        }
        if (status === 400 || lower.indexOf('invalid_request') !== -1 ||
            lower.indexOf('invalid request') !== -1) {
            return '请求参数有误，可能是模型名称或消息格式不正确（错误码 ' + status + '）';
        }
        if (status >= 500) {
            return 'AI 服务商暂时不可用，请稍后重试（错误码 ' + status + '）';
        }
        return 'AI 请求失败（错误码 ' + status + '）：' + raw;
    }

    async function chatWithAI(messages) {
        var cfg = _getAIConfig();
        if (!cfg.provider || !cfg.apiKey) return '我还没接入 AI，请先在 V 岛设置中配置供应商。';
        if (!cfg.model) return '未选择模型。';

        // 自定义供应商需要补全 apiFormat 和 endpoint
        var reqConfig = {
            provider: cfg.provider,
            apiKey: cfg.apiKey,
            model: cfg.model,
            messages: messages
        };

        if (cfg.provider === 'custom') {
            if (!cfg.endpoint) return '自定义供应商未配置接口地址。';
            reqConfig.endpoint = cfg.endpoint;
            reqConfig.apiFormat = cfg.apiFormat || 'openai';
        } else {
            // 预设供应商：从 AI_PROVIDERS 取接口地址和格式
            var p = AI_PROVIDERS[cfg.provider];
            if (!p) return '不支持的供应商：' + cfg.provider;
            reqConfig.endpoint = p.endpoint;
            reqConfig.apiFormat = p.apiFormat;
            // Google 的 endpoint 是空的，特殊处理
            if (cfg.provider === 'google') {
                reqConfig.endpoint = 'https://generativelanguage.googleapis.com/v1beta/models/' + cfg.model + ':generateContent';
            }
        }

        // 通过 IPC 调用主进程发起请求，绕过 CORS
        if (window.electronAPI && window.electronAPI.ai && window.electronAPI.ai.chat) {
            try {
                var res = await window.electronAPI.ai.chat(reqConfig);
                if (res && res.ok) return res.reply || '(空回复)';
                return '对话失败：' + (res && res.error ? res.error : '未知错误');
            } catch (e) {
                return '对话出错：' + (e.message || e);
            }
        }

        // 降级：渲染进程直接 fetch（可能被 CORS 拦截）
        console.warn('[V岛] 主进程 AI 代理不可用，降级到渲染进程 fetch');
        try {
            var headers = { 'Content-Type': 'application/json' };
            var url;
            if (reqConfig.apiFormat === 'google') {
                url = reqConfig.endpoint + '?key=' + cfg.apiKey;
            } else if (reqConfig.apiFormat === 'anthropic') {
                url = reqConfig.endpoint;
                headers['x-api-key'] = cfg.apiKey;
                headers['anthropic-version'] = '2023-06-01';
            } else {
                url = reqConfig.endpoint;
                headers['Authorization'] = 'Bearer ' + cfg.apiKey;
            }
            var body = _buildBody(reqConfig.apiFormat, cfg.model, messages);
            var resp = await fetch(url, { method: 'POST', headers: headers, body: JSON.stringify(body) });
            if (!resp.ok) {
                var t = await resp.text().catch(function () { return ''; });
                return _friendlyError(resp.status, t);
            }
            var data = await resp.json();
            return _extractReply(reqConfig.apiFormat, data);
        } catch (e) {
            return '对话出错：' + (e.message || e);
        }
    }

    // ========================================================================
    // 扩展模块 3：语音识别与唤醒词（嘿 Verse）
    // ========================================================================
    var _wakeRec = null;
    var _wakeListening = false;
    var _inDialog = false;
    var _dialogHistory = [];

    function _startWake() {
        var SR = window.SpeechRecognition || window.webkitSpeechRecognition;
        if (!SR) { console.warn('[V岛] 浏览器不支持语音识别，唤醒词功能不可用'); return; }
        if (_wakeRec) _stopWake();
        var rec = new SR();
        rec.lang = 'zh-CN';
        rec.continuous = true;
        rec.interimResults = true;
        rec.onstart = function () { console.log('[V岛唤醒] 监听已启动'); };
        rec.onaudiostart = function () { console.log('[V岛唤醒] 音频流已开启'); };
        rec.onaudioend = function () { console.log('[V岛唤醒] 音频流已结束'); };
        rec.onspeechstart = function () { console.log('[V岛唤醒] 检测到说话'); };
        rec.onresult = function (ev) {
            var txt = '';
            for (var i = ev.resultIndex; i < ev.results.length; i++) txt += ev.results[i][0].transcript;
            if (txt) console.log('[V岛唤醒] 识别到:', txt);
            // 唤醒词匹配：放宽到各种近似音
            // 中文识别"Verse"通常返回：沃斯/佛斯/沃尔斯/弗斯/vers/维斯 等
            var lower = txt.toLowerCase();
            var matched = false;
            // 1. 中文字面匹配
            if (txt.indexOf('嘿Verse') !== -1 || txt.indexOf('嘿verse') !== -1 ||
                txt.indexOf('嘿 Verse') !== -1) {
                matched = true;
            }
            // 2. 英文匹配
            else if (lower.indexOf('hey verse') !== -1 || lower.indexOf('hey vers') !== -1 ||
                     lower.indexOf('hi verse') !== -1) {
                matched = true;
            }
            // 3. 中文近似音匹配（最常见）
            else if (txt.indexOf('嘿') !== -1) {
                // "嘿"后面的字匹配 Verse 的各种近似音
                var afterHey = txt.substring(txt.indexOf('嘿') + 1);
                if (afterHey.match(/(沃斯|佛斯|沃尔斯|弗斯|维斯|维尔|沃瑟|佛瑟|vers|verse|沃斯尔|佛斯尔)/i)) {
                    matched = true;
                }
                // "嘿 V" 也算（用户说"嘿 V"）
                else if (afterHey.match(/^v/i) || afterHey.indexOf('维') !== -1) {
                    matched = true;
                }
            }
            // 4. 只说"Verse"或近似音（没"嘿"也算）
            else if (lower.match(/(verse|vers|沃斯|佛斯|维斯)/i)) {
                matched = true;
            }
            if (matched) {
                console.log('[V岛唤醒] ✓ 唤醒词触发');
                _onWake();
            }
        };
        rec.onerror = function (e) {
            console.warn('[V岛唤醒] 识别错误:', e.error);
            if (e.error === 'not-allowed' || e.error === 'service-not-allowed') {
                console.warn('[V岛唤醒] 麦克风未授权，请检查系统麦克风权限');
            }
        };
        rec.onend = function () {
            console.log('[V岛唤醒] 监听结束，2秒后重启');
            if (_wakeListening && !_inDialog) {
                setTimeout(function () {
                    if (_wakeListening && !_inDialog) {
                        try { rec.start(); } catch (e) { console.warn('[V岛唤醒] 重启失败:', e); }
                    }
                }, 2000);
            }
        };
        try { rec.start(); _wakeListening = true; _wakeRec = rec; console.log('[V岛唤醒] 正在启动监听...'); }
        catch (e) { console.warn('[V岛唤醒] 启动失败:', e); }
    }

    function _stopWake() {
        _wakeListening = false;
        if (_wakeRec) { try { _wakeRec.stop(); } catch (e) {} _wakeRec = null; }
    }

    function _onWake() {
        if (_inDialog) return;
        _inDialog = true;
        _stopWake();
        _setStatusIndicator('正在聆听...', 'listening');
        speak('我在听，请说');
        var SR = window.SpeechRecognition || window.webkitSpeechRecognition;
        if (!SR) { _endDialog(); return; }
        var rec = new SR();
        rec.lang = 'zh-CN';
        rec.continuous = false;
        rec.interimResults = false;
        var gotReply = false;
        rec.onresult = function (ev) {
            var q = ev.results[0][0].transcript;
            _setStatusIndicator('思考中...', 'processing');
            _askAI(q);
        };
        rec.onerror = function () { _endDialog(); };
        rec.onend = function () {
            if (_inDialog && !gotReply) setTimeout(function () { if (_inDialog && !gotReply) _endDialog(); }, 3000);
        };
        setTimeout(function () { try { rec.start(); } catch (e) {} }, 600);
    }

    async function _askAI(q) {
        _dialogHistory.push({ role: 'user', content: q });
        if (_dialogHistory.length > 10) _dialogHistory = _dialogHistory.slice(-10);
        _dialogHistory.unshift({ role: 'system', content: '你是 V 岛，一个住在 Minecraft 启动器里的语音助手，回答简洁友好，不超过两句话。' });
        var reply = await chatWithAI(_dialogHistory);
        _dialogHistory.shift();
        _dialogHistory.push({ role: 'assistant', content: reply });
        _setStatusIndicator('回答中...', 'processing');
        var self = this;
        speak(reply, function () { _endDialog(); });
    }

    function _endDialog() {
        _inDialog = false;
        _setStatusIndicator('V 岛 · 待命', 'ready');
        _startWake();
    }

    function _setStatusIndicator(text, state) {
        // 在 V 岛 pill 上显示对话状态（复用 subtitle）
        if (!_el) return;
        var subEl = _el.querySelector('.v-island__subtitle');
        if (subEl) subEl.textContent = text;
        // 状态切换 class
        _el.classList.remove('island--idle', 'island--active', 'v-island--listening', 'v-island--processing');
        if (state === 'listening') _el.classList.add('v-island--listening');
        else if (state === 'processing') _el.classList.add('v-island--processing');
        _el.classList.add('v-island--visible');
    }

    // ========================================================================
    // 扩展模块 4：首次引导页（大字浮现 + TTS 朗读）
    // ========================================================================
    function _isOnboardingComplete() {
        return localStorage.getItem('v-island-onboarding-complete') === 'true';
    }

    function _completeOnboarding() {
        localStorage.setItem('v-island-onboarding-complete', 'true');
    }

    function _resetOnboarding() {
        localStorage.removeItem('v-island-onboarding-complete');
    }

    function _showOnboarding() {
        // 动态创建引导页 DOM
        var existing = document.getElementById('v-island-onboarding');
        if (existing) existing.remove();

        var overlay = document.createElement('div');
        overlay.id = 'v-island-onboarding';
        overlay.className = 'v-island-onboarding';
        overlay.innerHTML =
            '<div class="v-island-onboarding__bg"></div>' +
            '<div class="v-island-onboarding__content">' +
                '<div class="v-island-onboarding__phase" data-phase="greeting">' +
                    '<h1 class="v-island-onboarding__title" data-line="1">Hi～我是 V 岛</h1>' +
                    '<h1 class="v-island-onboarding__title" data-line="2">一个活在你启动器的助手</h1>' +
                    '<h1 class="v-island-onboarding__title" data-line="3">接下来，我将帮助你完成很多工作</h1>' +
                    '<h1 class="v-island-onboarding__title" data-line="4">在此之前，我们先完成一些配置</h1>' +
                '</div>' +
                '<div class="v-island-onboarding__phase" data-phase="provider" style="display:none">' +
                    '<h2 class="v-island-onboarding__subtitle">选择你的 AI 供应商</h2>' +
                    '<p class="v-island-onboarding__desc">接入 AI 后，你可以唤醒 V 岛进行语音对话</p>' +
                    '<div class="v-island-onboarding__providers" id="v-island-onboarding-providers"></div>' +
                    '<div class="v-island-onboarding__config" id="v-island-onboarding-config" style="display:none">' +
                        '<div class="form-group"><label>API Key</label>' +
                            '<input type="password" class="text-input" id="v-island-onboarding-key" placeholder="粘贴你的 API Key"></div>' +
                        '<div class="form-group"><label>模型</label>' +
                            '<select class="select-input" id="v-island-onboarding-model"></select></div>' +
                    '</div>' +
                    '<div class="v-island-onboarding__actions">' +
                        '<button class="btn btn-ghost" id="v-island-onboarding-skip">先不配置</button>' +
                        '<button class="btn btn-primary" id="v-island-onboarding-next" disabled>下一步</button>' +
                    '</div>' +
                '</div>' +
                '<div class="v-island-onboarding__phase" data-phase="done" style="display:none">' +
                    '<h1 class="v-island-onboarding__title">准备就绪</h1>' +
                    '<p class="v-island-onboarding__desc">点击顶部的 V 岛，输入问题，开始对话吧</p>' +
                    '<div class="v-island-onboarding__actions" style="margin-top:24px">' +
                        '<button class="btn btn-primary" id="v-island-onboarding-finish">进入 V 岛</button>' +
                    '</div>' +
                '</div>' +
            '</div>';
        document.body.appendChild(overlay);
        requestAnimationFrame(function () { overlay.classList.add('v-island-onboarding--visible'); });

        _runGreetingPhase();
    }

    function _hideOnboarding() {
        var overlay = document.getElementById('v-island-onboarding');
        if (!overlay) return;
        overlay.classList.add('v-island-onboarding--fadeout');
        setTimeout(function () {
            overlay.classList.remove('v-island-onboarding--visible', 'v-island-onboarding--fadeout');
            if (overlay.parentNode) overlay.parentNode.removeChild(overlay);
        }, 600);
    }

    function _runGreetingPhase() {
        var lines = document.querySelectorAll('.v-island-onboarding__phase[data-phase="greeting"] .v-island-onboarding__title');
        var texts = ['Hi～我是 V 岛', '一个活在你启动器的助手', '接下来，我将帮助你完成很多工作', '在此之前，我们先完成一些配置'];
        var i = 0;
        var self = this;
        var step = function () {
            if (i >= lines.length) {
                setTimeout(function () { _switchPhase('provider'); _runProviderPhase(); }, 1200);
                return;
            }
            var el = lines[i];
            if (el) el.classList.add('v-appear');
            speak(texts[i], function () { i++; setTimeout(step, 400); });
        };
        setTimeout(step, 500);
    }

    function _switchPhase(name) {
        var phases = document.querySelectorAll('.v-island-onboarding__phase');
        for (var i = 0; i < phases.length; i++) {
            phases[i].style.display = (phases[i].getAttribute('data-phase') === name) ? '' : 'none';
        }
    }

    function _runProviderPhase() {
        var grid = document.getElementById('v-island-onboarding-providers');
        var cfgBox = document.getElementById('v-island-onboarding-config');
        var nextBtn = document.getElementById('v-island-onboarding-next');
        var skipBtn = document.getElementById('v-island-onboarding-skip');
        if (!grid) return;

        grid.innerHTML = '';
        var selectedProvider = null;
        if (cfgBox) cfgBox.style.display = 'none';
        if (nextBtn) nextBtn.disabled = true;

        // 先绑定按钮事件（无论供应商是否加载成功）
        if (skipBtn) skipBtn.onclick = function () { _saveProviderSelection(null); _switchPhase('done'); };
        if (nextBtn) nextBtn.onclick = function () { _saveProviderSelection(selectedProvider); _switchPhase('done'); };

        // 渲染供应商列表
        Object.keys(AI_PROVIDERS).forEach(function (id) {
            var p = AI_PROVIDERS[id];
            var card = document.createElement('div');
            card.className = 'v-island-onboarding-provider-card';
            card.innerHTML = '<img src="img/providers/' + p.icon + '.png" alt="" onerror="this.style.visibility=\'hidden\'"><span>' + p.name + '</span>';
            card.onclick = function () {
                var all = grid.querySelectorAll('.v-island-onboarding-provider-card');
                for (var j = 0; j < all.length; j++) all[j].classList.remove('v-island-onboarding-provider-card--active');
                card.classList.add('v-island-onboarding-provider-card--active');
                selectedProvider = id;
                _showProviderConfig(id, nextBtn);
            };
            grid.appendChild(card);
        });

        // 追加"自定义供应商"卡片
        var customCard = document.createElement('div');
        customCard.className = 'v-island-onboarding-provider-card';
        customCard.innerHTML = '<div style="width:48px;height:48px;display:flex;align-items:center;justify-content:center;background:var(--bg-glass,rgba(255,255,255,0.08));border-radius:12px;font-size:24px;border:1px dashed var(--border,rgba(255,255,255,0.2));">+</div><span>自定义供应商</span>';
        customCard.onclick = function () {
            var all = grid.querySelectorAll('.v-island-onboarding-provider-card');
            for (var j = 0; j < all.length; j++) all[j].classList.remove('v-island-onboarding-provider-card--active');
            customCard.classList.add('v-island-onboarding-provider-card--active');
            selectedProvider = 'custom';
            _showProviderConfig('custom', nextBtn);
        };
        grid.appendChild(customCard);

        // 完成按钮
        var finishBtn = document.getElementById('v-island-onboarding-finish');
        if (finishBtn) finishBtn.onclick = function () {
            _completeOnboarding();
            stopSpeak();
            _hideOnboarding();
            _startVoiceAssistant();
        };
    }

    function _showProviderConfig(id, nextBtn) {
        var cfgBox = document.getElementById('v-island-onboarding-config');
        var keyInput = document.getElementById('v-island-onboarding-key');
        var modelSel = document.getElementById('v-island-onboarding-model');
        if (!cfgBox) return;

        // 自定义供应商：显示完整输入框
        if (id === 'custom') {
            cfgBox.style.display = '';
            cfgBox.innerHTML =
                '<div style="display:grid;grid-template-columns:1fr 1fr;gap:12px;">' +
                    '<div><label style="display:block;font-size:12px;color:var(--text-muted);margin-bottom:4px;">接口地址 (Endpoint)</label>' +
                    '<input type="text" class="text-input" id="v-island-onboarding-endpoint" placeholder="https://api.example.com/v1/chat/completions"></div>' +
                    '<div><label style="display:block;font-size:12px;color:var(--text-muted);margin-bottom:4px;">接口格式</label>' +
                    '<select class="select-input" id="v-island-onboarding-format">' +
                        '<option value="openai">OpenAI 格式</option>' +
                        '<option value="anthropic">Anthropic 格式</option>' +
                        '<option value="google">Google Gemini 格式</option>' +
                    '</select></div>' +
                '</div>' +
                '<div><label style="display:block;font-size:12px;color:var(--text-muted);margin-bottom:4px;">API Key</label>' +
                '<input type="password" class="text-input" id="v-island-onboarding-key" placeholder="粘贴你的 API Key"></div>' +
                '<div><label style="display:block;font-size:12px;color:var(--text-muted);margin-bottom:4px;">模型 ID</label>' +
                '<input type="text" class="text-input" id="v-island-onboarding-model" placeholder="例如：gpt-4o-mini"></div>';
            var newKey = document.getElementById('v-island-onboarding-key');
            var newEndpoint = document.getElementById('v-island-onboarding-endpoint');
            var newModel = document.getElementById('v-island-onboarding-model');
            var check = function () {
                if (nextBtn) nextBtn.disabled = !(newKey && newKey.value && newKey.value.trim() &&
                                                  newEndpoint && newEndpoint.value && newEndpoint.value.trim() &&
                                                  newModel && newModel.value && newModel.value.trim());
            };
            if (newKey) newKey.oninput = check;
            if (newEndpoint) newEndpoint.oninput = check;
            if (newModel) newModel.oninput = check;
            check();
            return;
        }

        // 预设供应商：恢复原始配置 HTML
        var p = AI_PROVIDERS[id];
        if (!p) return;
        cfgBox.style.display = '';
        cfgBox.innerHTML =
            '<div><label style="display:block;font-size:12px;color:var(--text-muted);margin-bottom:4px;">API Key</label>' +
                '<input type="password" class="text-input" id="v-island-onboarding-key" placeholder="粘贴你的 API Key"></div>' +
            '<div><label style="display:block;font-size:12px;color:var(--text-muted);margin-bottom:4px;">模型</label>' +
                '<select class="select-input" id="v-island-onboarding-model"></select></div>';
        var newKeyInput = document.getElementById('v-island-onboarding-key');
        var newModelSel = document.getElementById('v-island-onboarding-model');
        if (newModelSel) {
            newModelSel.innerHTML = '';
            (p.models || []).forEach(function (m) {
                var opt = document.createElement('option');
                opt.value = m.id; opt.textContent = m.name + (m.free ? ' (免费)' : '');
                newModelSel.appendChild(opt);
            });
        }
        var check2 = function () { if (nextBtn) nextBtn.disabled = !(newKeyInput && newKeyInput.value && newKeyInput.value.trim()); };
        if (newKeyInput) { newKeyInput.value = ''; newKeyInput.oninput = check2; }
        check2();
    }

    function _saveProviderSelection(providerId) {
        if (!providerId) return;
        var cfg = _getAIConfig();
        if (providerId === 'custom') {
            var epInput = document.getElementById('v-island-onboarding-endpoint');
            var fmtInput = document.getElementById('v-island-onboarding-format');
            var keyInput = document.getElementById('v-island-onboarding-key');
            var modelInput = document.getElementById('v-island-onboarding-model');
            cfg.provider = 'custom';
            cfg.endpoint = epInput ? epInput.value.trim() : '';
            cfg.apiFormat = fmtInput ? fmtInput.value : 'openai';
            cfg.apiKey = keyInput ? keyInput.value.trim() : '';
            cfg.model = modelInput ? modelInput.value.trim() : '';
        } else {
            var p = AI_PROVIDERS[providerId];
            var keyInput2 = document.getElementById('v-island-onboarding-key');
            var modelSel2 = document.getElementById('v-island-onboarding-model');
            cfg.provider = providerId;
            cfg.apiKey = keyInput2 ? keyInput2.value.trim() : '';
            cfg.model = modelSel2 ? modelSel2.value : '';
            // 自定义供应商的字段清空，避免残留
            delete cfg.endpoint;
            delete cfg.apiFormat;
        }
        _saveAIConfig(cfg);
    }

    // 启动语音助手（引导完成后或已启用时调用）
    // 注：Chrome SpeechRecognition API 依赖 Google 云服务，国内不可用，已放弃语音唤醒
    // 改为纯点击对话模式：鼠标移到顶部 → 点击 V 岛 → 输入问题 → 回车
    function _startVoiceAssistant() {
        if (!isEnabled()) return;
        _ensureHoverZone();
        console.log('[V岛] 语音助手已启动（点击对话模式）');
    }

    // ========================================================================
    // V 岛开关初始化（从 other-settings.js 迁移过来）
    // ========================================================================
    function _initToggle() {
        var el = document.getElementById('vIsland');
        if (!el) return false;
        if (el._vIslandBound) return true; // 已绑定过，跳过
        el._vIslandBound = true;
        // 恢复保存的状态
        var saved = localStorage.getItem('vIsland-enabled');
        if (saved === 'true') {
            el.checked = true;
            document.body.classList.add('v-island-enabled');
        }
        // 启动时：已启用且已完成引导 → 直接启动语音助手（不主动弹引导）
        if (el.checked && _isOnboardingComplete()) {
            _startVoiceAssistant();
        }
        el.addEventListener('change', function () {
            localStorage.setItem('vIsland-enabled', el.checked ? 'true' : 'false');
            if (el.checked) {
                document.body.classList.add('v-island-enabled');
                _ensureHoverZone();
                preview();
                // 用户主动勾选时：若未完成引导，才弹引导页
                if (!_isOnboardingComplete()) {
                    setTimeout(function () { _showOnboarding(); }, 800);
                } else {
                    _startVoiceAssistant();
                }
            } else {
                document.body.classList.remove('v-island-enabled');
                _stopWake();
                stopSpeak();
                dismiss();
            }
        });
        return true;
    }

    // "重新播放引导"按钮（事件委托，避免 Vue 组件渲染时机问题）
    document.addEventListener('click', function (e) {
        if (e.target && e.target.id === 'v-island-replay-onboarding') {
            _resetOnboarding();
            _showOnboarding();
        }
    });

    // Vue 组件在 DOMContentLoaded 中挂载，defer 脚本先于 DOMContentLoaded 执行，
    // 因此 _initToggle 可能拿不到 Vue 渲染的 #vIsland 元素。
    // 用重试机制：立即尝试 → DOMContentLoaded 后重试 → 200ms 后再兜底一次。
    if (_initToggle()) {
        // 元素已存在，无需重试
    } else if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', function () { _initToggle(); });
        setTimeout(function () { _initToggle(); }, 300);
    } else {
        setTimeout(function () { _initToggle(); }, 300);
    }

    // ========================================================================
    // 崩溃自动修复：崩溃卡片"用V岛帮我修"按钮入口
    // 流程：弹出任务卡片 → 用户点"交给V岛修复" → AI 操控界面执行修复 → 总结
    // ========================================================================

    // 根据崩溃类型生成给 AI 的修复指令
    function _buildFixInstruction(info) {
        var base = '游戏「' + (info.versionId || '') + '」启动失败了。失败原因：' + (info.reason || '未知') + '。';
        switch (info.fixType) {
            case 'memory':
                return base + '这是内存不足导致的。请帮我修复：进入"设置"页面，找到内存分配（最大内存）设置，把内存调大到 4096MB（如果已经是 4096 就调到 6144MB），保存后告诉我结果。';
            case 'missing-dep':
                return base + '这是缺少前置模组导致的。缺失的依赖：' + (info.modName || '见崩溃日志') + '。请帮我修复：进入"模组"或"下载"页面，搜索并安装缺失的前置模组，注意要与当前游戏版本匹配。';
            case 'mod-crash':
                return base + '问题模组：' + (info.modName || '未知') + '。请帮我修复：进入"模组管理"页面，找到这个模组，将它禁用或卸载。';
            case 'duplicate-mod':
                return base + '请帮我修复：进入模组管理页面，找到重复安装的模组' + (info.modName ? '（' + info.modName + '）' : '') + '，删除多余的副本，只保留一个。';
            case 'mod-conflict':
                return base + '请帮我修复：进入模组管理页面，找到互相冲突的模组，移除其中一个。';
            case 'file-corrupt':
            case 'file-missing':
                return base + '请帮我修复：进入版本设置页面，找到"文件修复"功能并执行修复，重新下载损坏或缺失的文件。';
            case 'loader-missing':
                return base + '请帮我修复：重新安装该版本的模组加载器（Forge/Fabric），或者重新安装这个游戏版本。';
            default:
                return base + '建议的修复方案：' + (info.solution || '无') + '。请按方案帮我操作完成修复。';
        }
    }

    // 崩溃修复任务入口
    function fixCrash(crashInfo) {
        if (!isEnabled()) return;
        if (!crashInfo || !crashInfo.fixType) return;
        if (!_el) _ensureEl();

        _visible = true;
        _state = 'idle';
        _expanded = true;
        _clearHideTimer();
        if (_autoDismissTimer) { clearTimeout(_autoDismissTimer); _autoDismissTimer = null; }
        _el.classList.add('v-island--visible', 'v-island--expanded');
        _el.classList.remove('v-island--chat', 'v-island--thinking');

        var sub = _el.querySelector('.v-island__subtitle');
        if (sub) {
            sub.style.display = '';
            sub.textContent = '检测到可自动修复的问题';
        }

        var detail = _el.querySelector('.v-island__detail');
        if (!detail) return;

        var problemText = crashInfo.reason || '游戏启动失败';
        var fixText = crashInfo.fixDesc || crashInfo.solution || '';
        detail.innerHTML =
            '<div class="v-island__reply">' +
                '<div class="v-island__reply-a" style="font-size:13px;line-height:1.7;">' +
                    '<div style="font-weight:600;margin-bottom:4px;">' + _esc(problemText) + '</div>' +
                    (fixText ? '<div style="color:var(--text-muted);font-size:12px;">' + _esc(fixText) + '</div>' : '') +
                '</div>' +
                '<div style="margin-top:12px;display:flex;gap:8px;">' +
                    '<button class="btn btn-primary v-island__fix-accept" style="flex:1;padding:8px 12px;font-size:13px;">交给V岛修复</button>' +
                    '<button class="btn btn-ghost v-island__fix-cancel" style="flex:1;padding:8px 12px;font-size:13px;">我自己处理</button>' +
                '</div>' +
            '</div>';

        speak('检测到游戏启动失败，这个问题我可以帮你自动修复，要交给我处理吗？');

        var acceptBtn = detail.querySelector('.v-island__fix-accept');
        var cancelBtn = detail.querySelector('.v-island__fix-cancel');
        if (acceptBtn) {
            acceptBtn.onclick = function () {
                _runAgentTask(_buildFixInstruction(crashInfo));
            };
        }
        if (cancelBtn) {
            cancelBtn.onclick = function () {
                _showReply('', '好的，如果需要帮助随时叫我');
            };
        }
    }

    // ========================================================================
    // 公开接口
    // ========================================================================
    window.DynamicIsland = {
        show: show,
        update: update,
        dismiss: dismiss,
        preview: preview,
        setupHoverZone: setupHoverZone,
        isEnabled: isEnabled,
        isVisible: isVisible,
        // 扩展接口
        speak: speak,
        stopSpeak: stopSpeak,
        chatWithAI: chatWithAI,
        showOnboarding: _showOnboarding,
        fixCrash: fixCrash,
        AI_PROVIDERS: AI_PROVIDERS
    };
})();
