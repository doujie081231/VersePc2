import { speak, stopSpeak } from './dynamic-island-tts.js';
import { _startVoiceAssistant, _stopWake } from './dynamic-island-voice.js';
import { AI_PROVIDERS, _getAIConfig, _saveAIConfig } from './dynamic-island-config.js';
import { _ensureHoverZone, dismiss, preview } from './dynamic-island-view.js';

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
            card.innerHTML = '<img src="img/providers/' + p.icon + '.png" alt="" data-img-fallback="visibility"><span>' + p.name + '</span>';
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

export { _isOnboardingComplete, _completeOnboarding, _resetOnboarding, _showOnboarding, _hideOnboarding, _runGreetingPhase, _switchPhase, _runProviderPhase, _showProviderConfig, _saveProviderSelection, _initToggle };
