/* page-transfer.js - 互传页 Vue 组件（游戏数据互传）
 * 流程（免费使用，无需激活码）：
 *   发送：选择类型（版本/模组/存档）→ 选版本（模组再选模组文件、存档再选存档）
 *         → 生成 8 位识别码（打包 zip，信令注册）
 *         → 对端连接后自动直传（SHA-256 校验）
 *   接收：输入识别码 → 预览落地信息（版本/模组/存档自动落到对应文件夹，可选目标版本）
 *         → 接收 zip 并解压到对应目录（同名自动加"(互传)"后缀）
 * 文件数据在两台设备间 TCP 点对点直连，服务器只做信令不中转。
 * 安全使用：仅传输自己有权分享的内容，请勿传播违规文件。
 */
const PageTransfer = {
  name: 'PageTransfer',
  data() {
    return {
      view: 'home', // home | send | recv
      // 发送流程
      sendStep: 'type', // type | pick | code
      sendKind: 1,
      versions: [],
      versionsLoading: false,
      sendVersion: '',
      mods: [],
      modsLoading: false,
      sendMods: [],
      saves: [],
      savesLoading: false,
      sendSave: '',
      sendCode: '',
      sending: false,
      sendStatus: '',
      sendProgress: 0,
      sendDone: false,
      sendError: '',
      sendTimer: null,
      // 接收流程
      recvCode: '',
      recvMeta: null,
      recvStep: 'input', // input | confirm | busy | done
      recvVersions: [],
      recvTarget: '',
      recvSaving: '',
      recvProgress: 0,
      recvResult: '',
      recvError: '',
      receiving: false
    };
  },
  computed: {
    kindText() {
      return this.sendKind === 1 ? '版本' : this.sendKind === 2 ? '模组' : '存档';
    },
    recvVersionOptions() {
      const s = this.recvVersions.slice();
      const vid = this.recvMeta && this.recvMeta.versionId;
      if (vid && s.indexOf(vid) < 0) s.unshift(vid);
      return s;
    }
  },
  methods: {
    isTauri() {
      return !!(window.__TAURI__ || window.__TAURI_INTERNALS__);
    },
    invoke() {
      return window.__TAURI__.core.invoke;
    },
    fmtSize(n) {
      if (!n && n !== 0) return '';
      if (n > 1024 * 1024 * 1024) return (n / 1024 / 1024 / 1024).toFixed(2) + ' GB';
      if (n > 1024 * 1024) return (n / 1024 / 1024).toFixed(1) + ' MB';
      if (n > 1024) return (n / 1024).toFixed(0) + ' KB';
      return n + ' B';
    },
    async loadVersions() {
      this.versionsLoading = true;
      try {
        this.versions = await this.invoke()('transfer_list_versions');
      } catch (e) {
        this.versions = [];
      }
      this.versionsLoading = false;
    },
    // ===== 发送流程 =====
    async startSend() {
      this.view = 'send';
      this.sendStep = 'type';
      this.resetSendState();
      this.loadVersions();
    },
    resetSendState() {
      if (this.sendTimer) { clearInterval(this.sendTimer); this.sendTimer = null; }
      this.sendKind = 1;
      this.sendVersion = '';
      this.mods = []; this.modsLoading = false; this.sendMods = [];
      this.saves = []; this.savesLoading = false; this.sendSave = '';
      this.sendCode = ''; this.sending = false; this.sendStatus = '';
      this.sendProgress = 0; this.sendDone = false; this.sendError = '';
    },
    backHome() {
      this.view = 'home';
      this.resetSendState();
      this.recvStep = 'input'; this.recvCode = ''; this.recvMeta = null;
      this.recvProgress = 0; this.recvResult = ''; this.recvError = '';
    },
    pickKind(k) {
      this.sendKind = k;
      this.sendStep = 'pick';
      this.sendVersion = ''; this.sendMods = []; this.sendSave = '';
      if (k === 2) { this.mods = []; }
      if (k === 3) { this.saves = []; }
    },
    async chooseVersion(v) {
      this.sendVersion = v;
      this.sendMods = []; this.sendSave = '';
      if (this.sendKind === 2) {
        this.modsLoading = true; this.mods = [];
        try {
          this.mods = await this.invoke()('transfer_list_mods', { versionId: v });
        } catch (e) {
          this.mods = [];
        }
        this.modsLoading = false;
      }
      if (this.sendKind === 3) {
        this.savesLoading = true; this.saves = [];
        try {
          this.saves = await this.invoke()('transfer_list_saves', { versionId: v });
        } catch (e) {
          this.saves = [];
        }
        this.savesLoading = false;
      }
    },
    toggleMod(name) {
      const i = this.sendMods.indexOf(name);
      if (i >= 0) this.sendMods.splice(i, 1);
      else this.sendMods.push(name);
    },
    sendReady() {
      if (this.sendKind === 1) return !!this.sendVersion;
      if (this.sendKind === 2) return !!this.sendVersion && this.sendMods.length > 0;
      return !!this.sendVersion && !!this.sendSave;
    },
    async createCode() {
      if (!this.sendReady() || this.sending) return;
      this.sending = true;
      this.sendError = '';
      this.sendDone = false;
      this.sendStatus = '正在打包内容.';
      try {
        const session = await this.invoke()('transfer_start_send_game', {
          kind: this.sendKind,
          versionId: this.sendVersion,
          itemName: this.sendKind === 3 ? this.sendSave : this.sendVersion,
          selection: this.sendKind === 2 ? this.sendMods : [],
          activationKey: ''
        });
        this.sendCode = session.code;
        this.sendStep = 'code';
        this.sendStatus = '等待对方连接（识别码 10 分钟内有效）';
        const invoke = this.invoke();
        this.sendTimer = setInterval(async () => {
          try {
            const st = await invoke('transfer_send_status', { code: this.sendCode });
            if (st.totalBytes > 0 && st.connected) {
              this.sendStatus = '对方已连接，传输中';
              this.sendProgress = Math.min(100, Math.round((st.sentBytes / Math.max(st.totalBytes, 1)) * 100));
            }
            if (st.transfers > 0 && !st.connected) {
              this.sendDone = true;
              this.sendStatus = '';
              clearInterval(this.sendTimer);
              this.sendTimer = null;
            }
            if (st.error) {
              this.sendError = st.error;
              clearInterval(this.sendTimer);
              this.sendTimer = null;
            }
          } catch (e) { /* 轮询失败忽略 */ }
        }, 1000);
      } catch (e) {
        this.sendError = String(e);
        this.sendStatus = '';
      }
      this.sending = false;
    },
    // ===== 接收流程 =====
    startRecv() {
      this.view = 'recv';
      this.recvStep = 'input';
      this.recvCode = ''; this.recvMeta = null; this.recvTarget = '';
      this.recvSaving = ''; this.recvProgress = 0; this.recvResult = ''; this.recvError = '';
    },
    async queryMeta() {
      if (!this.recvCode || this.receiving) return;
      this.recvError = '';
      this.recvMeta = null;
      try {
        const meta = await this.invoke()('transfer_lookup_meta', { code: this.recvCode.trim() });
        this.recvMeta = meta;
        if (meta.kind === 1 || meta.kind === 2 || meta.kind === 3) {
          this.versionsLoading = true;
          try {
            this.recvVersions = await this.invoke()('transfer_list_versions');
          } catch (e) { this.recvVersions = []; }
          this.versionsLoading = false;
          // 模组/存档：默认落到发送端所属版本；版本类型无需选择
          this.recvTarget = meta.versionId || meta.itemName || (this.recvVersions[0] || '');
          this.recvStep = 'confirm';
        } else {
          this.recvSaving = '';
          this.recvStep = 'confirm';
        }
      } catch (e) {
        this.recvError = String(e);
      }
    },
    async pickSaveDir() {
      if (!this.isTauri()) return;
      try {
        const picked = await window.__TAURI__.dialog.open({ directory: true, title: '选择保存位置' });
        if (typeof picked === 'string' && picked) this.recvSaving = picked;
      } catch (e) { /* 用户取消 */ }
    },
    async confirmRecv() {
      if (this.receiving) return;
      const meta = this.recvMeta;
      if (!meta) return;
      if (meta.kind === 0 && !this.recvSaving) {
        this.recvError = '请先选择保存位置';
        return;
      }
      if ((meta.kind === 2 || meta.kind === 3) && !this.recvTarget) {
        this.recvError = '请选择要放入的版本';
        return;
      }
      this.receiving = true;
      this.recvError = '';
      this.recvResult = '';
      this.recvProgress = 0;
      this.recvStep = 'busy';
      try {
        const r = await this.invoke()('transfer_receive_game', {
          code: this.recvCode.trim(),
          targetVersion: this.recvTarget || '',
          fallbackDir: this.recvSaving || ''
        });
        this.recvResult = r;
        this.recvStep = 'done';
      } catch (e) {
        this.recvError = String(e);
        this.recvStep = 'confirm';
      }
      this.receiving = false;
    }
  },
  mounted() {
    // 进度事件监听（接收方由 Rust emit）
    if (this.isTauri() && window.__TAURI__.event && window.__TAURI__.event.listen) {
      const listen = window.__TAURI__.event.listen;
      listen('transfer-receive-progress', (ev) => {
        const p = ev && ev.payload ? ev.payload : null;
        if (p && p.total > 0) {
          this.recvProgress = Math.min(100, Math.round((p.received / p.total) * 100));
        }
      });
    }
    // 样式注入（用标准 DOM API 确保生效，只注入一次）
    if (!document.getElementById('transfer-page-style')) {
      var st = document.createElement('style');
      st.id = 'transfer-page-style';
      st.textContent = [
        '.transfer-wrap { max-width: 760px; margin: 0 auto; padding: 22px 24px 30px; }',
        '.transfer-header { text-align: center; margin-bottom: 20px; }',
        '.transfer-header h2 { font-size: 24px; font-weight: 800; color: var(--text-primary); margin: 0 0 6px; }',
        '.transfer-header p { font-size: 13px; color: var(--text-muted); margin: 0; }',
        '.transfer-card { background: var(--bg-card); border: 1px solid var(--border); border-radius: var(--radius-lg, 16px); padding: 18px 20px 20px; margin-bottom: 16px; }',
        '.transfer-card-title { font-size: 17px; font-weight: 700; color: var(--text-primary); }',
        '.transfer-card-sub { font-size: 12.5px; color: var(--text-muted); }',
        '.transfer-card-body { display: flex; flex-direction: column; gap: 10px; }',
        '.transfer-label { font-size: 13px; font-weight: 600; color: var(--text-secondary); }',
        '.transfer-input { height: 40px; border-radius: var(--radius-md, 10px); border: 1px solid var(--border); background: var(--bg-tertiary); color: var(--text-primary); padding: 0 12px; font-size: 14px; outline: none; }',
        '.transfer-input:focus { border-color: var(--accent); }',
        '.transfer-btn { height: 40px; border-radius: var(--radius-md, 10px); border: 1px solid var(--border); background: var(--bg-tertiary); color: var(--text-primary); font-size: 14px; font-weight: 600; cursor: pointer; transition: background var(--transition, .15s); }',
        '.transfer-btn:hover:not(:disabled) { background: var(--bg-hover, var(--bg-tertiary)); }',
        '.transfer-btn:disabled { opacity: .45; cursor: not-allowed; }',
        '.transfer-btn.primary { background: var(--accent); border: none; color: var(--accent-text); font-size: 15px; }',
        '.transfer-btn.primary:hover:not(:disabled) { background: var(--accent-hover); }',
        '.transfer-code { font-size: 30px; font-weight: 800; letter-spacing: 6px; color: var(--accent); font-family: Consolas, "Courier New", monospace; text-align: center; padding: 10px 0 4px; }',
        '.transfer-hint { font-size: 12.5px; color: var(--text-muted); }',
        '.transfer-ok { font-size: 13px; color: var(--green); }',
        '.transfer-err { font-size: 13px; color: var(--red); }',
        '.transfer-note { font-size: 12px; color: var(--text-muted); text-align: center; margin-top: 4px; }',
        // 首页双大卡片（参照联机页 创建房间/加入房间）
        '.transfer-actions { display: flex; gap: 16px; }',
        '.transfer-bigcard { flex: 1; display: flex; flex-direction: column; align-items: center; gap: 10px; padding: 30px 20px; border-radius: var(--radius-lg, 16px); border: 1px solid var(--border); background: var(--bg-card); color: var(--text-primary); cursor: pointer; transition: transform .15s, border-color .15s, box-shadow .15s; }',
        '.transfer-bigcard:hover { transform: translateY(-3px); border-color: var(--accent); box-shadow: 0 8px 24px rgba(0,0,0,.12); }',
        '.transfer-bigcard svg { width: 36px; height: 36px; color: var(--accent); }',
        '.transfer-bigcard .t { font-size: 17px; font-weight: 700; }',
        '.transfer-bigcard .d { font-size: 12.5px; color: var(--text-muted); text-align: center; }',
        // 类型三选卡片
        '.transfer-typegrid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 12px; }',
        '.transfer-typecard { display: flex; flex-direction: column; align-items: center; gap: 8px; padding: 22px 12px; border-radius: var(--radius-lg, 14px); border: 1.5px solid var(--border); background: var(--bg-card); color: var(--text-primary); cursor: pointer; transition: border-color .15s, transform .15s; }',
        '.transfer-typecard:hover { transform: translateY(-2px); border-color: var(--accent); }',
        '.transfer-typecard svg { width: 30px; height: 30px; color: var(--accent); }',
        '.transfer-typecard .t { font-size: 15px; font-weight: 700; }',
        '.transfer-typecard .d { font-size: 12px; color: var(--text-muted); text-align: center; }',
        // 选择列表
        '.transfer-list { max-height: 300px; overflow-y: auto; display: flex; flex-direction: column; gap: 6px; border: 1px solid var(--border); border-radius: var(--radius-md, 10px); padding: 8px; background: var(--bg-tertiary); }',
        '.transfer-list-empty { padding: 18px; text-align: center; font-size: 13px; color: var(--text-muted); }',
        '.transfer-item { display: flex; align-items: center; gap: 10px; padding: 10px 12px; border-radius: var(--radius-md, 10px); border: 1px solid transparent; background: var(--bg-card); cursor: pointer; transition: border-color .12s; text-align: left; width: 100%; }',
        '.transfer-item:hover { border-color: var(--accent); }',
        '.transfer-item.sel { border-color: var(--accent); background: color-mix(in srgb, var(--accent) 10%, var(--bg-card)); }',
        '.transfer-item .n { flex: 1; font-size: 14px; font-weight: 600; color: var(--text-primary); word-break: break-all; }',
        '.transfer-item .s { font-size: 12px; color: var(--text-muted); flex-shrink: 0; }',
        '.transfer-item .cb { width: 18px; height: 18px; flex-shrink: 0; accent-color: var(--accent); }',
        // 进度条
        '.transfer-progress { height: 8px; border-radius: 4px; background: var(--bg-tertiary); overflow: hidden; }',
        '.transfer-progress .fill { height: 100%; background: var(--accent); border-radius: 4px; transition: width .3s; }',
        // 流程头（返回按钮）
        '.transfer-flowhead { display: flex; align-items: center; gap: 10px; margin-bottom: 14px; }',
        '.transfer-back { width: 34px; height: 34px; border-radius: 10px; border: 1px solid var(--border); background: var(--bg-card); color: var(--text-secondary); cursor: pointer; display: flex; align-items: center; justify-content: center; }',
        '.transfer-back:hover { border-color: var(--accent); color: var(--accent); }',
        '.transfer-back svg { width: 18px; height: 18px; }',
        '.transfer-flowtitle { font-size: 17px; font-weight: 700; color: var(--text-primary); }',
        '.transfer-divider { height: 1px; background: var(--border); margin: 14px 0; }'
      ].join('\n');
      document.head.appendChild(st);
    }
  },
  template: `
    <div class="transfer-wrap">
      <!-- ===== 首页：双大卡片 ===== -->
      <div v-if="view === 'home'">
          <div class="transfer-header">
            <h2>互传</h2>
            <p>版本、模组、存档，跨设备点对点直传（免费使用）</p>
          </div>
          <div class="transfer-actions">
            <button class="transfer-bigcard" @click="startSend">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19V5"/><path d="M5 12l7 7 7-7"/></svg>
              <span class="t">我要发送</span>
              <span class="d">选择版本 / 模组 / 存档，生成识别码给对方</span>
            </button>
            <button class="transfer-bigcard" @click="startRecv">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 5v14"/><path d="M19 12l-7 7-7-7"/></svg>
              <span class="t">我要接收</span>
              <span class="d">输入对方的识别码，自动放入对应文件夹</span>
            </button>
          </div>
          <div class="transfer-note" style="margin-top:18px;">文件在两台设备间点对点直连，服务器只负责信令不中转数据，免费使用</div>
          <div class="transfer-note" style="margin-top:6px;">安全提示：请仅传输自己有权分享的内容，勿传播违规文件；建议在可信网络环境下使用</div>
      </div>

        <!-- 发送流程 -->
        <div v-if="view === 'send'">
          <div class="transfer-flowhead">
            <button class="transfer-back" @click="backHome" title="返回">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>
            </button>
            <span class="transfer-flowtitle">我要发送</span>
          </div>

          <!-- 步骤1：选类型 -->
          <div v-if="sendStep === 'type'" class="transfer-card">
            <div class="transfer-card-body">
              <span class="transfer-card-sub">选择要传输的内容类型</span>
              <div class="transfer-typegrid">
                <button class="transfer-typecard" @click="pickKind(1)">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"><path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z"/><polyline points="3.27 6.96 12 12.01 20.73 6.96"/><line x1="12" y1="22.08" x2="12" y2="12"/></svg>
                  <span class="t">版本</span>
                  <span class="d">传输一个完整的游戏版本</span>
                </button>
                <button class="transfer-typecard" @click="pickKind(2)">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z"/></svg>
                  <span class="t">模组</span>
                  <span class="d">选择版本里的模组传输</span>
                </button>
                <button class="transfer-typecard" @click="pickKind(3)">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
                  <span class="t">存档</span>
                  <span class="d">选择版本里的存档传输</span>
                </button>
              </div>
            </div>
          </div>

          <!-- 步骤2：选版本/模组/存档 -->
          <div v-if="sendStep === 'pick'" class="transfer-card">
            <div class="transfer-card-body">
              <span class="transfer-card-sub">{{ sendKind === 1 ? '选择要传输的版本' : '先选择版本' }}</span>
              <div class="transfer-list">
                <div v-if="versionsLoading" class="transfer-list-empty">加载版本列表中…</div>
                <div v-else-if="versions.length === 0" class="transfer-list-empty">还没有任何版本，请先在版本管理安装</div>
                <button v-for="v in versions" :key="v" class="transfer-item" :class="{ sel: sendVersion === v }" @click="chooseVersion(v)">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" style="width:18px;height:18px;color:var(--accent);flex-shrink:0"><path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z"/></svg>
                  <span class="n">{{ v }}</span>
                </button>
              </div>

              <template v-if="sendKind === 2 && sendVersion">
                <div class="transfer-divider"></div>
                <span class="transfer-label">选择要传输的模组（{{ sendVersion }}）</span>
                <div class="transfer-list">
                  <div v-if="modsLoading" class="transfer-list-empty">加载模组列表中…</div>
                  <div v-else-if="mods.length === 0" class="transfer-list-empty">该版本没有模组</div>
                  <label v-for="m in mods" :key="m.name" class="transfer-item">
                    <input type="checkbox" class="cb" :checked="sendMods.indexOf(m.name) >= 0" @change="toggleMod(m.name)" />
                    <span class="n">{{ m.name }}</span>
                    <span class="s">{{ fmtSize(m.size) }}</span>
                  </label>
                </div>
                <span class="transfer-hint">已选 {{ sendMods.length }} 个模组</span>
              </template>

              <template v-if="sendKind === 3 && sendVersion">
                <div class="transfer-divider"></div>
                <span class="transfer-label">选择要传输的存档（{{ sendVersion }}）</span>
                <div class="transfer-list">
                  <div v-if="savesLoading" class="transfer-list-empty">加载存档列表中…</div>
                  <div v-else-if="saves.length === 0" class="transfer-list-empty">该版本没有存档</div>
                  <button v-for="s in saves" :key="s" class="transfer-item" :class="{ sel: sendSave === s }" @click="sendSave = s">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" style="width:18px;height:18px;color:var(--accent);flex-shrink:0"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
                    <span class="n">{{ s }}</span>
                  </button>
                </div>
              </template>

              <button class="transfer-btn primary" :disabled="!sendReady() || sending" @click="createCode">
                {{ sending ? '打包中…' : '生成识别码并等待连接' }}
              </button>
              <div v-if="sendError" class="transfer-err">{{ sendError }}</div>
            </div>
          </div>

          <!-- 步骤3：识别码 + 等待 -->
          <div v-if="sendStep === 'code'" class="transfer-card">
            <div class="transfer-card-body">
              <div class="transfer-code">{{ sendCode }}</div>
              <div class="transfer-hint" style="text-align:center;">把识别码告诉对方（10 分钟内有效） · 内容：{{ kindText }}「{{ sendKind === 3 ? sendSave : sendVersion }}」</div>
              <div v-if="sendStatus" class="transfer-hint" style="text-align:center;">{{ sendStatus }}</div>
              <div v-if="sendProgress > 0 && !sendDone" class="transfer-progress"><div class="fill" :style="{ width: sendProgress + '%' }"></div></div>
              <div v-if="sendDone" class="transfer-ok" style="text-align:center;">传输完成，校验通过 ✓</div>
              <div v-if="sendError" class="transfer-err" style="text-align:center;">{{ sendError }}</div>
              <button class="transfer-btn" :disabled="sending" @click="backHome">完成</button>
            </div>
          </div>
        </div>

        <!-- 接收流程 -->
        <div v-if="view === 'recv'">
          <div class="transfer-flowhead">
            <button class="transfer-back" @click="backHome" title="返回">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>
            </button>
            <span class="transfer-flowtitle">我要接收</span>
          </div>

          <div class="transfer-card">
            <div class="transfer-card-body">
              <label class="transfer-label">对方的识别码</label>
              <input class="transfer-input" v-model="recvCode" placeholder="输入 8 位识别码" maxlength="8" @keyup.enter="queryMeta" :disabled="receiving" />
              <button v-if="recvStep === 'input'" class="transfer-btn primary" :disabled="!recvCode.trim()" @click="queryMeta">查询传输内容</button>

              <template v-if="recvStep === 'confirm' && recvMeta">
                <div class="transfer-divider"></div>
                <span class="transfer-ok">
                  {{ recvMeta.kind === 1 ? ('对方要传输版本「' + recvMeta.itemName + '」，接收后将放入版本列表')
                    : recvMeta.kind === 2 ? ('对方要传输版本「' + recvMeta.versionId + '」里的模组，请选择放入哪个版本')
                    : recvMeta.kind === 3 ? ('对方要传输存档「' + recvMeta.itemName + '」，请选择放入哪个版本')
                    : ('对方要传输文件：' + recvMeta.fileName + '（' + fmtSize(recvMeta.fileSize) + '）') }}
                </span>

                <template v-if="recvMeta.kind === 2 || recvMeta.kind === 3">
                  <label class="transfer-label">放入的版本</label>
                  <select class="transfer-input" v-model="recvTarget" :disabled="receiving">
                    <option v-for="v in recvVersionOptions" :key="v" :value="v">{{ v }}</option>
                  </select>
                  <span class="transfer-hint">没有想要的版本？先在版本管理安装同名版本，或直接落到上方默认版本（接收后自动创建）</span>
                </template>

                <template v-if="recvMeta.kind === 0">
                  <button class="transfer-btn" :disabled="receiving" @click="pickSaveDir">选择保存位置</button>
                  <span v-if="recvSaving" class="transfer-hint">{{ recvSaving }}</span>
                </template>

                <button class="transfer-btn primary" :disabled="receiving" @click="confirmRecv">开始接收</button>
              </template>

              <template v-if="recvStep === 'busy'">
                <div class="transfer-progress"><div class="fill" :style="{ width: recvProgress + '%' }"></div></div>
                <div class="transfer-hint" style="text-align:center;">接收中 {{ recvProgress }}%</div>
              </template>

              <template v-if="recvStep === 'done' && recvResult">
                <div class="transfer-ok" style="text-align:center;">
                  接收完成，校验通过 ✓<br>
                  {{ recvResult.kindName }}「{{ recvResult.itemName }}」已放入：<br>
                  <span style="word-break:break-all;font-size:12px;color:var(--text-muted)">{{ recvResult.dest }}</span>
                </div>
                <button class="transfer-btn" @click="backHome">完成</button>
              </template>

              <div v-if="recvError" class="transfer-err">{{ recvError }}</div>
            </div>
          </div>
        </div>
    </div>
  `
};

// 注册到全局组件表（index.html 的 _mountBatch 从 window.VersePC 取组件挂载）
window.VersePC = window.VersePC || {};
window.VersePC.PageTransfer = PageTransfer;
