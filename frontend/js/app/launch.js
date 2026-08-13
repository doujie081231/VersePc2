/**
 * @file launch.js
 * @description 游戏启动流程 - 检查依赖、显示启动模态框、处理进度
 */
async function handleLaunch() {
  if (window._versepc_launching) {
    if (typeof showToast === 'function') showToast('正在启动中，请稍候...', 'info');
    return;
  }
  window._versepc_launching = true;
  setTimeout(() => { window._versepc_launching = false; }, 300000);

  const versionId = (typeof currentLaunchVersionId !== 'undefined' && currentLaunchVersionId) || (launchVersionCustomSelect ? launchVersionCustomSelect.getValue() : '');
  if (!versionId) { showToast('请选择游戏版本', 'error'); window._versepc_launching = false; return; }

  _cachedLastLaunchVersion = versionId;
  try { await window.electronAPI.store.set('versepc_last_launch_version', versionId); } catch (_) {}

  const launchBtn = document.getElementById('launch-btn');
  const homeLaunchBtn = document.getElementById('home-launch-btn');

  launchBtn.disabled = true;
  homeLaunchBtn.disabled = true;

  _launchCounted = false;

  showLaunchModal();
  hideLaunchError();

  try {
    setLaunchStep('auth', 'running', '正在验证登录状态...');
    const accountsResult = await API.getAccounts();
    const accounts = Array.isArray(accountsResult) ? accountsResult : (accountsResult.accounts || []);
    if (accounts.length === 0) {
      setLaunchStep('auth', 'failed', '未登录，请先添加账户');
      showLaunchError('未登录，请先在账户管理中添加账户后再启动游戏。');
      window._versepc_launching = false;
      launchBtn.disabled = false;
      homeLaunchBtn.disabled = false;
      return;
    }
    setLaunchStep('auth', 'success', '登录验证通过');

    setLaunchStep('java-check', 'running', '正在检测 Java 环境...');
    
    let externalVersionDir = null;
    try {
      const vInfo = await API.getVersions();
      if (vInfo && vInfo.versions) {
        const v = vInfo.versions.find(x => x.id === versionId);
        if (v && v.externalDir) externalVersionDir = v.externalDir;
      }
    } catch (_) {}
    const depCheck = await API.launchCheck(versionId, externalVersionDir);
    const requiredJava = (depCheck.java && depCheck.java.required) || 21;

    if (!depCheck.java || !depCheck.java.ok) {
      const requiredVer = requiredJava;
      const maxVer = depCheck.java.maxVersion;
      const rangeDesc = maxVer && maxVer < 999 ? `${requiredVer}~${maxVer}` : `${requiredVer}+`;
      const serverMsg = depCheck.java.message || '';
      setLaunchStep('java-check', 'error', `未找到 Java ${rangeDesc}`);
      const detailMsg = serverMsg || `未找到合适的Java运行环境（需要 Java ${rangeDesc}）`;
      showLaunchError(`${detailMsg}<br><a href="javascript:void(0)" id="launch-nav-java" style="color:var(--accent);text-decoration:underline;cursor:pointer;">前往 Java 管理页面 →</a>`);
      launchBtn.disabled = false;
      homeLaunchBtn.disabled = false;
      window._versepc_launching = false;
      return;
    }
    
    setLaunchStep('java-check', 'success', depCheck.java.message || `Java ${depCheck.java.version} ✓`);

    setLaunchStep('version-resolve', 'running', '正在解析版本信息...');
    await new Promise(r => setTimeout(r, 200));
    setLaunchStep('version-resolve', 'success', '版本信息解析完成');

    setLaunchStep('files-check', 'running', '正在检查文件完整性...');
    
    if (depCheck.mainJar) {
      if (depCheck.mainJar.ok) {
        setLaunchStep('files-check', 'success', depCheck.mainJar.message);
      } else {
        setLaunchStep('files-check', 'error', depCheck.mainJar.message);
        showLaunchError(depCheck.mainJar.message);
        launchBtn.disabled = false;
        homeLaunchBtn.disabled = false;
        window._versepc_launching = false;
        return;
      }
    } else {
      setLaunchStep('files-check', 'success', '游戏文件完整');
    }

    if (depCheck.forgeCore && !depCheck.forgeCore.ok && depCheck.forgeCore.missing && depCheck.forgeCore.missing.length > 0) {
      const missingNames = depCheck.forgeCore.missing.map(m => `${m.desc} (${m.name.split(':').pop()})`).join('、');
      const downloadable = depCheck.forgeCore.missing.filter(m => {
        const entry = (depCheck.missingFiles || []).find(f => f.path === m.path);
        return entry && entry.url;
      });
      if (downloadable.length > 0) {
        setLaunchStep('files-check', 'warning', `Forge核心库缺失 ${depCheck.forgeCore.missing.length} 个，正在自动修复...`);
        const dlResult = await API.launchGame(versionId, { checkOnly: true }, 120000);
        if (dlResult.needDownload && dlResult.sessionId) {
          pollLaunchDownload(dlResult.sessionId, versionId, requiredJava);
          window._versepc_launching = false;
          return;
        }
      }
      const stillMissing = depCheck.forgeCore.missing.filter(m => {
        const entry = (depCheck.missingFiles || []).find(f => f.path === m.path);
        return !entry || !entry.url;
      });
      if (stillMissing.length > 0 || downloadable.length === 0) {
        const errorMsg = `Forge核心库文件缺失 (${depCheck.forgeCore.missing.length}个): ${missingNames}`;
        setLaunchStep('files-check', 'error', errorMsg);
        showLaunchError(
          `Forge 核心库文件缺失，无法启动游戏。\n缺失文件：${missingNames}\n\n请前往"版本设置 → 文件修复"功能修复此问题，或重新安装该 Forge 版本。`,
          { forgeMissing: depCheck.forgeCore.missing, repairHint: 'forge_core_missing', versionId }
        );
        launchBtn.disabled = false;
        homeLaunchBtn.disabled = false;
        window._versepc_launching = false;
        return;
      }
    }

    setLaunchStep('natives-extract', 'running', '正在解压本地库...');
    await new Promise(r => setTimeout(r, 200));
    setLaunchStep('natives-extract', 'success', '本地库解压完成');

    setLaunchStep('assets-check', 'running', '正在检查资源文件...');
    
    if (depCheck.libraries && depCheck.libraries.missing.length > 0) {
      const libMsg = `${depCheck.libraries.missing.length}/${depCheck.libraries.total} 个库文件缺失`;
      setLaunchStep('assets-check', 'warning', libMsg);
    } else {
      setLaunchStep('assets-check', 'success', '所有资源文件完整');
    }

    const hasMissing = depCheck.missingFiles && depCheck.missingFiles.length > 0;
    const assetsMissing = depCheck.assets && depCheck.assets.missing > 0;
    if (hasMissing || assetsMissing) {
      const missingCount = (depCheck.missingFiles && depCheck.missingFiles.length) || (depCheck.assets ? depCheck.assets.missing : 0);
      setLaunchStep('download', 'running', `正在补全 ${missingCount} 个缺失文件...`);
      // 同步补全接口（自动使用国内镜像），不依赖已废弃的会话轮询
      const dlResult = await API.downloadLaunchDeps(versionId, null, 180000);
      if (dlResult.failed > 0) {
        const failedNames = (dlResult.failedFiles || []).map(f => String(f).split('/').pop() || f).join('、');
        const failMsg = `仍有 ${dlResult.failed} 个文件下载失败${failedNames ? '：' + failedNames : ''}`;
        setLaunchStep('download', 'error', failMsg);
        showLaunchError(failMsg);
        launchBtn.disabled = false;
        homeLaunchBtn.disabled = false;
        window._versepc_launching = false;
        return;
      }
      // 补全成功后重新检查，确认无缺失再继续启动
      const depCheck2 = await API.launchCheck(versionId, externalVersionDir);
      if (depCheck2.missingFiles && depCheck2.missingFiles.length > 0) {
        const stillMsg = `仍有 ${depCheck2.missingFiles.length} 个文件缺失，请再次点击补全`;
        setLaunchStep('download', 'error', stillMsg);
        showLaunchError(stillMsg);
        launchBtn.disabled = false;
        homeLaunchBtn.disabled = false;
        window._versepc_launching = false;
        return;
      }
      setLaunchStep('download', 'success', '缺失文件已补全');
    }

    setLaunchStep('build-args', 'running', '正在构建启动参数...');
    await new Promise(r => setTimeout(r, 200));
    setLaunchStep('build-args', 'success', '启动参数构建完成');

    setLaunchStep('launching', 'running', '正在启动 Minecraft...');

    const result = await API.launchGame(versionId, {}, 300000);

    if (result.needDownload && result.sessionId) {
      pollLaunchDownload(result.sessionId, versionId, requiredJava);
      window._versepc_launching = false;
      return;
    }

    if (result.success) {
      setLaunchStep('launching', 'success', '游戏进程已创建');
      updateLaunchProgress(100);
      document.getElementById('launch-log-section').style.display = '';
      launchBtn.classList.add('running');
      launchBtn.querySelector('span').textContent = '启动游戏';
      document.getElementById('status-indicator').classList.add('running');
      document.getElementById('status-text').textContent = '游戏运行中';
      startGameLogStream();
      updateGameStatus();
      incrementLaunchCount();
      checkSupportMilestone();
      _onGameRunning();
      setTimeout(() => {
        closeLaunchModal('fade');
        launchBtn.disabled = false;
        homeLaunchBtn.disabled = false;
        window._versepc_launching = false;
      }, 2000);
    } else {
      setLaunchStep('launching', 'error', result.error || '启动失败');
      showLaunchError(result.error || '启动失败', result.details || result);
      launchBtn.disabled = false;
      homeLaunchBtn.disabled = false;
      window._versepc_launching = false;
    }
  } catch (e) {
    console.error('[Launch] 启动异常:', e);
    const statusEl = document.getElementById('launch-splash-status');
    if (statusEl) {
      statusEl.textContent = e.message || '启动请求失败';
      statusEl.style.color = '#dc2626';
    }
    showLaunchError(e.message || '启动请求失败', { error: e.message, stack: e.stack });
    launchBtn.disabled = false;
    homeLaunchBtn.disabled = false;
    window._versepc_launching = false;
    // 超时后游戏可能已在后台启动，延迟关闭模态框让用户看到错误提示
    setTimeout(() => {
      if (typeof closeLaunchModal === 'function') closeLaunchModal();
    }, 5000);
  }
}

function showLaunchDepModal(versionId, sessionId, missingCount, depCheck) {
  setLaunchStep('download', 'running', `发现 ${missingCount} 个缺失文件，需要下载...`);
  updateLaunchDownloadProgress(0, `0/${missingCount} 文件`, {
    completedFiles: 0,
    totalFiles: missingCount,
    currentFile: '准备下载...',
    speed: 0,
    activeDownloads: []
  });

  startLaunchDepDownload(versionId, sessionId);
}

function closeLaunchDepModal() {
  if (launchDepPollTimer) { clearInterval(launchDepPollTimer); launchDepPollTimer = null; }
  const modal = document.getElementById('launch-dep-modal');
  if (modal) {
    modal.classList.remove('modal-visible');
    setTimeout(() => modal.remove(), 300);
  }
}

async function startLaunchDepDownload(versionId, sessionId) {
  setLaunchStep('download', 'running', '正在下载缺失文件...');

  try {
    const result = await API.downloadLaunchDeps(versionId, sessionId, 180000);

    if (result.failed > 0) {
      const failedNames = (result.failedFiles || []).map(f => String(f).split('/').pop() || f).join('、');
      const failMsg = `仍有 ${result.failed} 个文件下载失败${failedNames ? '：' + failedNames : ''}`;
      setLaunchStep('download', 'error', failMsg);
      showLaunchError(failMsg);
    } else if (result.success) {
      setLaunchStep('download', 'success', '缺失文件已补全');
      setLaunchStep('build-args', 'running', '正在构建启动参数...');
      await new Promise(r => setTimeout(r, 200));
      setLaunchStep('build-args', 'success', '启动参数构建完成');
      setLaunchStep('launching', 'running', '正在启动 Minecraft...');
      const launchBtn = document.getElementById('launch-btn');
      const homeLaunchBtn = document.getElementById('home-launch-btn');
      try {
        const launchResult = await API.launchGame(versionId);
        if (launchResult.success) {
          setLaunchStep('launching', 'success', '游戏进程已创建');
          updateLaunchProgress(100);
          showToast('游戏启动成功', 'success');
          launchBtn.classList.add('running');
          launchBtn.querySelector('span').textContent = '启动游戏';
          document.getElementById('status-indicator').classList.add('running');
          document.getElementById('status-text').textContent = '游戏运行中';
          startGameLogStream();
          updateGameStatus();
          incrementLaunchCount();
          checkSupportMilestone();
          _onGameRunning();
          setTimeout(() => {
            closeLaunchModal('fade');
            launchBtn.disabled = false;
            homeLaunchBtn.disabled = false;
          }, 2000);
        } else {
          setLaunchStep('launching', 'error', launchResult.error || '启动失败');
          showLaunchError(launchResult.error || '启动失败', launchResult.details || launchResult);
        }
      } catch (e) {
        setLaunchStep('launching', 'error', '启动失败');
        showLaunchError('启动失败', { error: e.message });
      }
      launchBtn.disabled = false;
      if (homeLaunchBtn) homeLaunchBtn.disabled = false;
    } else {
      setLaunchStep('download', 'error', '下载请求失败');
      showLaunchError('下载请求失败');
    }
  } catch (e) {
    setLaunchStep('download', 'error', '下载请求失败: ' + e.message);
    showLaunchError('下载请求失败: ' + e.message, { error: e.message });
  }
}

function pollLaunchDepProgress(sessionId, versionId) {
  if (launchDepPollTimer) clearInterval(launchDepPollTimer);
  let depSmoothPct = 0;

  launchDepPollTimer = setInterval(async () => {
    try {
      const status = await API.getLaunchSessionStatus(sessionId);

      const detailData = {
        completedFiles: status.completedFiles || 0,
        totalFiles: status.totalFiles || 0,
        currentFile: status.currentFile || '',
        speed: status.speed || 0,
        activeDownloads: status.activeDownloads || []
      };

      const rawDepPct = status.progress || 0;
      if (depSmoothPct <= 0 || rawDepPct < depSmoothPct) {
        depSmoothPct = rawDepPct;
      } else {
        depSmoothPct = depSmoothPct * 0.85 + rawDepPct * 0.15;
      }
      const smoothDepPct = Math.round(depSmoothPct);
      updateLaunchDownloadProgress(smoothDepPct, status.message || '', detailData);
      const baseProgress = 40;
      updateLaunchProgress(baseProgress + (smoothDepPct / 100) * 50);

      if (status.status === 'launched') {
        clearInterval(launchDepPollTimer);
        launchDepPollTimer = null;
        setLaunchStep('download', 'success', '缺失文件下载完成');
        setLaunchStep('build-args', 'success', '启动参数构建完成');
        setLaunchStep('launching', 'success', '游戏进程已创建');
        updateLaunchProgress(100);
        showToast('游戏启动成功', 'success');
        const launchBtn = document.getElementById('launch-btn');
        const homeLaunchBtn = document.getElementById('home-launch-btn');
        launchBtn.classList.add('running');
        launchBtn.querySelector('span').textContent = '启动游戏';
        document.getElementById('status-indicator').classList.add('running');
        document.getElementById('status-text').textContent = '游戏运行中';
        startGameLogStream();
        incrementLaunchCount();
        checkSupportMilestone();
        _onGameRunning();
        setTimeout(() => {
          closeLaunchModal('fade');
          launchBtn.disabled = false;
          if (homeLaunchBtn) homeLaunchBtn.disabled = false;
        }, 2000);
      } else if (status.status === 'launch_failed') {
        clearInterval(launchDepPollTimer);
        launchDepPollTimer = null;
        setLaunchStep('launching', 'error', status.message || '启动失败');
        showLaunchError(status.message || '启动失败', status.launchResult || status);
      } else if (status.status === 'failed') {
        clearInterval(launchDepPollTimer);
        launchDepPollTimer = null;
        setLaunchStep('download', 'error', status.message || '下载失败');
        showLaunchError(status.message || '下载失败', { failedFiles: status.failedFiles });
      } else if (status.status === 'completed' && status.failed > 0) {
        setLaunchStep('download', 'warning', `${status.failed} 个文件下载失败`);
      } else if (status.status === 'completed') {
        clearInterval(launchDepPollTimer);
        launchDepPollTimer = null;
        updateLaunchDownloadProgress(100, '下载完成', {
          completedFiles: status.totalFiles || 0,
          totalFiles: status.totalFiles || 0,
          currentFile: '',
          speed: 0,
          activeDownloads: []
        });
        setLaunchStep('download', 'success', '缺失文件下载完成');
        showToast(`下载完成: ${status.completedFiles || 0} 个文件`, 'success');
      }
    } catch (e) {
      console.error('[Launch Poll] Error:', e);
    }
  }, 200);
}

async function retryLaunchDepDownload(versionId, sessionId) {
  setLaunchStep('download', 'running', '正在重试下载...');

  try {
    const result = await API.downloadLaunchDeps(versionId, sessionId);
    if (result.success && result.sessionId) {
      pollLaunchDepProgress(result.sessionId, versionId);
    } else {
      setLaunchStep('download', 'error', '重试失败');
      showLaunchError('重试失败', result);
    }
  } catch (e) {
    setLaunchStep('download', 'error', '重试请求失败');
    showLaunchError('重试请求失败', { error: e.message });
  }
}

let _gameStatusTimer = null;
let _gameWasRunning = false;
let _gameStatusErrorCount = 0;
const MAX_STATUS_ERRORS = 5;

async function updateGameStatus() {
  try {
    const status = await API.getGameStatus();
    const indicator = document.getElementById('status-indicator');
    const statusText = document.getElementById('status-text');
    const launchBtn = document.getElementById('launch-btn');

    if (status.running) {
      indicator.classList.add('running');
      const count = status.instances ? status.instances.length : 1;
      if (count > 1) {
        statusText.textContent = `${count} 个游戏运行中`;
      } else {
        statusText.textContent = '游戏运行中';
      }
      launchBtn.classList.add('running');
      launchBtn.querySelector('span').textContent = '启动游戏';

      updateGameInstanceList(status.instances || []);
      _gameWasRunning = true;
    } else {
      // 用 DOM 状态判断（indicator 是否有 running 类），比 _gameWasRunning 变量更可靠
      // 场景：游戏启动成功后秒崩，_gameWasRunning 还没设为 true，但 indicator 已有 running 类
      const wasRunning = indicator.classList.contains('running');
      indicator.classList.remove('running');
      statusText.textContent = '就绪';
      launchBtn.classList.remove('running');
      launchBtn.querySelector('span').textContent = '启动游戏';

      updateGameInstanceList([]);
      _gameWasRunning = false;

      if (wasRunning) {
        // 后端 gameProcess.on('exit') 设置 lastGameExitAnalysis 可能有延迟，重试几次
        let analysis = null;
        for (let i = 0; i < 4; i++) {
          try {
            const analysisResult = await API.getExitAnalysis();
            analysis = analysisResult.analysis;
            if (analysis) break;
          } catch (e) {
            console.warn(`[Launch] 退出分析失败(尝试 ${i+1}/4):`, e);
          }
          await new Promise(r => setTimeout(r, 600));
        }

        if (analysis && analysis.isCrash) {
          // versionId 存在 launchInfo 里，不是 analysis 顶层
          const crashVid = analysis.launchInfo?.versionId || analysis.launchInfo?.fullVersionId || status.lastVersionId;
          if (crashVid) {
            showCrashAnalysis(crashVid);
          } else {
            showToast(`游戏崩溃: ${analysis.reason}`, 'error');
          }
        }
      }
    }
  } catch (e) {
    _gameStatusErrorCount++;
    if (_gameStatusErrorCount <= MAX_STATUS_ERRORS) {
      console.warn(`[Launch] 更新游戏状态失败(${_gameStatusErrorCount}/${MAX_STATUS_ERRORS}):`, e.message);
    } else {
      console.error('[Launch] 游戏状态轮询已停止（连续失败超过限制）');
    }
  } finally {
    // 动态调整轮询间隔：游戏运行时 3 秒，空闲时 15 秒
    if (_gameStatusTimer) clearTimeout(_gameStatusTimer);
    // 连续报错超过上限时停止轮询，防止后台忙时无限刷屏
    if (_gameStatusErrorCount <= MAX_STATUS_ERRORS) {
      _gameStatusTimer = setTimeout(updateGameStatus, _gameWasRunning ? 3000 : 15000);
    } else {
      _gameStatusTimer = null;
      _gameStatusErrorCount = 0;
    }
  }
}

async function showCrashAnalysis(versionId) {
  try {
    const result = await API.analyzeCrash(versionId);
    if (result.found) {
      showCrashAnalysisDialog(result, versionId);
    } else {
      showToast('游戏已退出，未检测到崩溃日志', 'info');
    }
  } catch (e) {
    showToast('崩溃分析失败: ' + (e.message || e), 'error');
  }
}

function showCrashAnalysisDialog(result, versionId) {
  const existing = document.getElementById('crash-analysis-dialog');
  if (existing) existing.remove();

  const overlay = document.createElement('div');
  overlay.id = 'crash-analysis-dialog';
  overlay.style.cssText = 'position:fixed;top:0;left:0;right:0;bottom:0;z-index:10001;display:flex;align-items:center;justify-content:center;background:rgba(0,0,0,0.6);backdrop-filter:blur(4px);animation:crashFadeIn 0.2s ease';

  const severityLabels = { high: '严重', medium: '中等', low: '轻微' };
  const sevLabel = severityLabels[result.severity] || '中等';

  const logExcerptHtml = (result.logExcerpt && result.logExcerpt.length > 0)
    ? result.logExcerpt.map(item => {
        const sourceLabel = item.source === 'crash-report' ? '崩溃报告' : item.source === 'hs_err' ? 'JVM崩溃' : item.source === 'launcher' ? '启动器' : '游戏日志';
        return `<div style="display:flex;gap:8px;padding:4px 0;border-bottom:1px solid var(--border);font-family:Consolas,Monaco,monospace;font-size:12px;line-height:1.5">
          <span style="font-size:10px;flex-shrink:0;margin-top:2px;padding:1px 4px;border-radius:3px;border:1px solid var(--border);color:var(--text-muted);white-space:nowrap">${sourceLabel}</span>
          <span style="color:var(--text-secondary);word-break:break-all">${escapeHtml(item.line)}</span>
        </div>`;
      }).join('')
    : '<div style="color:var(--text-muted);font-size:12px;text-align:center;padding:12px">未提取到关键日志信息</div>';

  const dialog = document.createElement('div');
  dialog.style.cssText = `width:92%;max-width:680px;max-height:88vh;background:var(--bg-secondary);border-radius:14px;box-shadow:0 24px 70px rgba(0,0,0,0.6);overflow:hidden;display:flex;flex-direction:column;animation:crashSlideUp 0.25s ease`;

  dialog.innerHTML = `
    <style>
      @keyframes crashFadeIn { from { opacity:0 } to { opacity:1 } }
      @keyframes crashSlideUp { from { opacity:0; transform:translateY(20px) } to { opacity:1; transform:translateY(0) } }
      #crash-dialog-body::-webkit-scrollbar { width:6px }
      #crash-dialog-body::-webkit-scrollbar-thumb { background:var(--border);border-radius:3px }
      #crash-dialog-body::-webkit-scrollbar-track { background:transparent }
      .crash-btn { transition:all 0.15s ease }
      .crash-btn:hover { transform:translateY(-1px) }
      .crash-btn:active { transform:translateY(0) }
    </style>
    <div style="padding:18px 24px;display:flex;align-items:flex-start;gap:8px;border-bottom:1px solid var(--border)">
      <div style="flex:1;min-width:0">
        <div style="display:flex;align-items:center;gap:6px;margin-bottom:4px">
          <h3 style="margin:0;font-size:15px;font-weight:700;color:var(--text-primary)">游戏崩溃</h3>
          <span style="font-size:10px;padding:1px 6px;border-radius:8px;border:1px solid var(--border);color:var(--text-muted);font-weight:500">${sevLabel}</span>
        </div>
        <div style="font-size:13px;color:var(--text-secondary);overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${escapeHtml(result.reason)}</div>
      </div>
      <button id="crash-dialog-close" class="crash-btn" style="width:28px;height:28px;border:none;background:transparent;color:var(--text-muted);font-size:16px;cursor:pointer;border-radius:6px;flex-shrink:0;display:flex;align-items:center;justify-content:center">×</button>
    </div>
    <div id="crash-dialog-body" style="flex:1;overflow-y:auto;padding:20px 24px">
      ${result.modName ? `
      <div style="margin-bottom:14px">
        <div style="font-size:10px;font-weight:600;color:var(--text-muted);margin-bottom:4px;text-transform:uppercase;letter-spacing:0.4px">相关模组</div>
        <div style="padding:8px 12px;background:var(--bg-primary);border-radius:6px;font-size:12px;color:var(--text-primary);font-family:Consolas,Monaco,monospace">${escapeHtml(result.modName)}</div>
      </div>` : ''}
      ${result.crashDescription ? `
      <div style="margin-bottom:14px">
        <div style="font-size:10px;font-weight:600;color:var(--text-muted);margin-bottom:4px;text-transform:uppercase;letter-spacing:0.4px">崩溃描述</div>
        <div style="padding:10px 12px;background:var(--bg-primary);border-radius:6px;font-size:12px;color:var(--text-secondary);font-family:Consolas,Monaco,monospace;line-height:1.5;white-space:pre-wrap;word-break:break-all">${escapeHtml(result.crashDescription)}</div>
      </div>` : ''}
      <div style="margin-bottom:14px">
        <div style="font-size:10px;font-weight:600;color:var(--text-muted);margin-bottom:4px;text-transform:uppercase;letter-spacing:0.4px">解决方案</div>
        <div style="padding:10px 12px;background:var(--bg-primary);border-radius:6px;font-size:12px;color:var(--text-primary);line-height:1.7">${escapeHtml(result.solution)}</div>
      </div>
      <div style="margin-bottom:14px;padding:10px 12px;background:var(--bg-primary);border-radius:6px;border:1px solid var(--border)">
        <div style="font-size:11px;color:var(--text-muted);line-height:1.6">
          请勿截图,建议导出完整日志文件以便获得更准确的分析结果。
        </div>
      </div>
      <div>
        <div style="font-size:10px;font-weight:600;color:var(--text-muted);margin-bottom:4px;text-transform:uppercase;letter-spacing:0.4px;display:flex;align-items:center;justify-content:space-between">
          <span>错误日志摘要 (${result.logExcerpt ? result.logExcerpt.length : 0} 条)</span>
          ${result.logFile ? `<span style="font-size:10px;color:var(--text-muted);font-weight:400;text-transform:none;letter-spacing:0">${escapeHtml(result.logFile)}</span>` : ''}
        </div>
        <div style="padding:10px 12px;background:var(--bg-primary);border-radius:6px;max-height:240px;overflow-y:auto">
          ${logExcerptHtml}
        </div>
      </div>
    </div>
    <div style="padding:12px 24px;border-top:1px solid var(--border);display:flex;justify-content:flex-end;gap:8px;background:var(--bg-secondary);flex-wrap:wrap">
      ${result.fixType ? `
      <button id="crash-dialog-vfix" class="crash-btn" style="padding:7px 14px;border:none;background:var(--text-primary);color:var(--bg-primary);border-radius:6px;font-size:12px;cursor:pointer;font-weight:600">
        用V岛帮我修
      </button>` : ''}
      <button id="crash-dialog-export" class="crash-btn" style="padding:7px 14px;border:1px solid var(--border);background:var(--bg-primary);color:var(--text-primary);border-radius:6px;font-size:12px;cursor:pointer">
        导出完整日志
      </button>
      <button id="crash-dialog-copy" class="crash-btn" style="padding:7px 14px;border:1px solid var(--border);background:transparent;color:var(--text-primary);border-radius:6px;font-size:12px;cursor:pointer">
        复制报告
      </button>
      <button id="crash-dialog-repair" class="crash-btn" style="padding:7px 14px;border:1px solid var(--border);background:transparent;color:var(--text-primary);border-radius:6px;font-size:12px;cursor:pointer">
        文件修复
      </button>
      <button id="crash-dialog-ok" class="crash-btn" style="padding:7px 20px;border:none;background:var(--text-primary);color:var(--bg-primary);border-radius:6px;font-size:12px;cursor:pointer;font-weight:600">知道了</button>
    </div>
  `;

  overlay.appendChild(dialog);
  document.body.appendChild(overlay);

  const closeDialog = () => { overlay.remove(); };
  overlay.addEventListener('click', (e) => { if (e.target === overlay) closeDialog(); });
  dialog.querySelector('#crash-dialog-close').addEventListener('click', closeDialog);
  dialog.querySelector('#crash-dialog-ok').addEventListener('click', closeDialog);

  dialog.querySelector('#crash-dialog-export').addEventListener('click', () => {
    if (typeof API !== 'undefined' && API.exportCrashLog) {
      API.exportCrashLog(versionId);
      showToast('正在导出日志文件...', 'info');
    } else {
      showToast('导出功能暂不可用', 'error');
    }
  });

  dialog.querySelector('#crash-dialog-copy').addEventListener('click', () => {
    const report = `=== VersePC2 崩溃报告 ===\n崩溃原因: ${result.reason}\n严重程度: ${sevLabel}\n${result.modName ? '相关模组: ' + result.modName + '\n' : ''}解决方案: ${result.solution}\n${result.logFile ? '日志文件: ' + result.logFile + '\n' : ''}${result.crashDescription ? '\n崩溃描述:\n' + result.crashDescription + '\n' : ''}${result.logExcerpt && result.logExcerpt.length > 0 ? '\n错误日志摘要:\n' + result.logExcerpt.map(i => '[' + i.source + '] ' + i.line).join('\n') : ''}`;
    if (navigator.clipboard) {
      navigator.clipboard.writeText(report).then(() => {
        showToast('崩溃报告已复制到剪贴板', 'success');
      }).catch(() => {
        showToast('复制失败，请手动选择文本', 'error');
      });
    } else {
      showToast('复制失败，请手动选择文本', 'error');
    }
  });

  dialog.querySelector('#crash-dialog-repair').addEventListener('click', () => {
    closeDialog();
    if (versionId) {
      openVersionSettings(versionId);
      document.querySelectorAll('.vset-nav-item[data-tab="overview"]').forEach(b => b.click());
      setTimeout(() => { if (typeof repairFiles === 'function') repairFiles(); }, 500);
    }
  });

  // "用V岛帮我修"按钮：把崩溃信息交给V岛，让V岛通过CDP操控启动器自动修复
  const vfixBtn = dialog.querySelector('#crash-dialog-vfix');
  if (vfixBtn) {
    vfixBtn.addEventListener('click', () => {
      closeDialog();
      if (window.DynamicIsland && typeof window.DynamicIsland.fixCrash === 'function') {
        window.DynamicIsland.fixCrash({
          versionId: versionId,
          reason: result.reason,
          solution: result.solution,
          fixType: result.fixType,
          fixDesc: result.fixDesc,
          modName: result.modName,
          logExcerpt: result.logExcerpt ? result.logExcerpt.slice(0, 10).map(i => i.line).join('\n') : ''
        });
      } else {
        showToast('V岛功能未启用，请先在设置中开启', 'warning');
      }
    });
  }

  setTimeout(() => { dialog.querySelector('#crash-dialog-ok').focus(); }, 100);
}

function showLaunchModal() {
  const overlay = document.getElementById('game-launch-overlay');
  if (!overlay) {
    console.error('[Launch] game-launch-overlay element not found');
    return;
  }

  overlay.style.display = 'flex';

  const progressBar = document.getElementById('launch-splash-progress');
  if (progressBar) progressBar.style.width = '0%';

  const statusEl = document.getElementById('launch-splash-status');
  if (statusEl) {
    statusEl.textContent = '正在验证登录状态...';
    statusEl.style.color = '';
  }

  const logo = document.getElementById('launch-splash-logo');
  if (logo) {
    logo.style.animation = 'none';
    void logo.offsetWidth;
    logo.style.animation = '';
  }

  const errorSection = document.getElementById('launch-error-section');
  if (errorSection) errorSection.style.display = 'none';

  const logSection = document.getElementById('launch-log-section');
  if (logSection) logSection.style.display = 'none';

  const repairGuide = document.getElementById('launch-repair-guide');
  if (repairGuide) repairGuide.style.display = 'none';
}

function closeLaunchModal(name_fade) {
  const overlay = document.getElementById('game-launch-overlay');
  if (!overlay) return;

  if (name_fade) {
    overlay.style.transition = 'opacity 0.5s ease-out';
    overlay.style.opacity = '0';
    setTimeout(() => {
      overlay.style.display = 'none';
      overlay.style.opacity = '1';
      overlay.style.transition = '';
      navigateToPage('home');
    }, 500);
  } else {
    overlay.style.display = 'none';
  }

  hideLaunchError();
}

function updateLaunchProgress(pct) {
  const bar = document.getElementById('launch-splash-progress');
  if (bar) bar.style.width = pct + '%';
}

const LAUNCH_STEP_PROGRESS = {
  'auth': 5, 'java-check': 15, 'version-resolve': 25,
  'files-check': 40, 'natives-extract': 55, 'assets-check': 65,
  'download': 75, 'build-args': 85, 'launching': 95
};

function setLaunchStep(stepName, status, desc) {
  const statusEl = document.getElementById('launch-splash-status');
  if (statusEl && desc) statusEl.textContent = desc;

  const progress = LAUNCH_STEP_PROGRESS[stepName] || 0;
  updateLaunchProgress(progress);

  if (statusEl) {
    if (status === 'error') {
      statusEl.style.color = '#dc2626';
    } else if (status === 'success' && stepName === 'launching') {
      updateLaunchProgress(100);
      statusEl.textContent = '启动成功！';
      statusEl.style.color = '#4ade80';
    } else {
      statusEl.style.color = '';
    }
  }
}

function completeAllPreviousSteps(currentStepName) {
}

function showLaunchError(msg, details = null) {
  const errorSection = document.getElementById('launch-error-section');
  const errorMsg = document.getElementById('launch-error-msg');
  const repairGuide = document.getElementById('launch-repair-guide');
  if (errorSection) errorSection.style.display = 'flex';
  if (repairGuide) {
    repairGuide.style.display = 'flex';
    repairGuide.dataset.versionId = (details && details.versionId) || currentSettingsVersionId || (launchVersionCustomSelect ? launchVersionCustomSelect.getValue() : '');
  }

  let fullMsg = msg || '未知错误';
  if (details) {
    console.error('[Launch] 详细错误信息:', details);
    try { console.error('[Launch] 详细错误信息(JSON):', JSON.stringify(details, null, 2)); } catch(_) {}
    if (details.versionId) fullMsg += `\n版本: ${details.versionId}`;
    if (details.mainClass) fullMsg += `\n主类: ${details.mainClass}`;
    if (details.externalVersionDir) fullMsg += `\n外部目录: ${details.externalVersionDir}`;
    if (details.error) fullMsg += `\n错误: ${details.error}`;
  }

  if (errorMsg) {
    errorMsg.innerHTML = (msg || '未知错误').replace(/\n/g, '<br>');
    errorMsg.title = fullMsg;
    // 绑定"前往 Java 管理页面"链接（避免依赖内联 onclick，兼容 CSP）
    const navLink = errorMsg.querySelector('#launch-nav-java');
    if (navLink) {
      navLink.addEventListener('click', (e) => {
        e.preventDefault();
        closeLaunchModal();
        navigateToPage('java');
      });
    }
  }

  const statusEl = document.getElementById('launch-splash-status');
  if (statusEl) {
    // 状态栏用纯文本，避免把 HTML 标签原样显示出来
    const cleanTmp = document.createElement('div');
    cleanTmp.innerHTML = msg || '启动失败';
    statusEl.textContent = cleanTmp.textContent || '启动失败';
    statusEl.style.color = '#dc2626';
  }
}

function hideLaunchError() {
  const errorSection = document.getElementById('launch-error-section');
  const repairGuide = document.getElementById('launch-repair-guide');
  if (errorSection) errorSection.style.display = 'none';
  if (repairGuide) repairGuide.style.display = 'none';

  const statusEl = document.getElementById('launch-splash-status');
  if (statusEl) statusEl.style.color = '';
}

function openVersionSettingsForRepair() {
  const repairGuide = document.getElementById('launch-repair-guide');
  const versionId = (repairGuide && repairGuide.dataset.versionId) || (launchVersionCustomSelect ? launchVersionCustomSelect.getValue() : '');
  if (versionId) {
    openVersionSettings(versionId);
  }
  closeLaunchModal();
}

function updateLaunchDownloadProgress(pct, msg, detailData) {
  const statusEl = document.getElementById('launch-splash-status');
  if (!statusEl) return;

  if (detailData) {
    const parts = [];
    if (detailData.completedFiles !== undefined && detailData.totalFiles !== undefined) {
      parts.push(detailData.completedFiles + '/' + detailData.totalFiles);
    }
    if (detailData.speed > 0) {
      const spd = detailData.speed;
      if (spd < 1024) parts.push(spd.toFixed(0) + ' B/s');
      else if (spd < 1024 * 1024) parts.push((spd / 1024).toFixed(1) + ' KB/s');
      else parts.push((spd / (1024 * 1024)).toFixed(1) + ' MB/s');
    }
    statusEl.textContent = (parts.length ? parts.join('  ') + ' - ' : '') + Math.round(pct) + '%';
  } else if (msg) {
    statusEl.textContent = msg;
  }
}

function cancelLaunchFlow() {
  if (window._launchDlPollInterval) {
    clearInterval(window._launchDlPollInterval);
    window._launchDlPollInterval = null;
  }
  window._versepc_launching = false;
  if (typeof API !== 'undefined' && API.cancelLaunch) {
    API.cancelLaunch().catch(() => {});
  }
  closeLaunchModal();
  const launchBtn = document.getElementById('launch-btn');
  const homeLaunchBtn = document.getElementById('home-launch-btn');
  if (launchBtn) launchBtn.disabled = false;
  if (homeLaunchBtn) homeLaunchBtn.disabled = false;
}

function toggleLaunchLog() {
  const content = document.getElementById('launch-log-content');
  if (content.style.maxHeight === '0px') {
    content.style.maxHeight = '150px';
  } else {
    content.style.maxHeight = '0px';
  }
}

async function pollLaunchDownload(sessionId, versionId, requiredJava) {
  try {
    let lastPct = 0;
    let smoothPct = 0;
    
    const pollInterval = setInterval(async () => {
      window._launchDlPollInterval = pollInterval;
      try {
        const dlStatus = await API.getLaunchSessionStatus(sessionId);
        
        if (!dlStatus || dlStatus.status === 'error') {
          clearInterval(pollInterval);
          setLaunchStep('download', 'error', dlStatus?.message || '下载失败');
          showLaunchError(dlStatus?.message || '下载失败');
          const launchBtn = document.getElementById('launch-btn');
          const homeLaunchBtn = document.getElementById('home-launch-btn');
          if (launchBtn) launchBtn.disabled = false;
          if (homeLaunchBtn) homeLaunchBtn.disabled = false;
          return;
        }
        
        const rawPct = dlStatus.progress || 0;
        if (smoothPct <= 0 || rawPct < smoothPct) {
          smoothPct = rawPct;
        } else {
          smoothPct = smoothPct * 0.85 + rawPct * 0.15;
        }
        const pct = Math.min(95, Math.round(smoothPct));
        if (pct !== lastPct) {
          lastPct = pct;
          updateLaunchDownloadProgress(pct, `下载文件 (${dlStatus.completedFiles || 0}/${dlStatus.totalFiles || 0}): ${dlStatus.currentFile || ''}`, {
            completedFiles: dlStatus.completedFiles || 0,
            totalFiles: dlStatus.totalFiles || 0,
            currentFile: dlStatus.currentFile || '',
            speed: dlStatus.speed || 0,
            activeDownloads: dlStatus.activeDownloads || []
          });
          const baseProgress = 40;
          updateLaunchProgress(baseProgress + (pct / 100) * 50);
        }
        
        if (dlStatus.status === 'completed') {
          clearInterval(pollInterval);
          updateLaunchDownloadProgress(100, '下载完成', {
            completedFiles: dlStatus.totalFiles || 0,
            totalFiles: dlStatus.totalFiles || 0,
            currentFile: '',
            speed: 0,
            activeDownloads: []
          });
          setLaunchStep('download', 'success', '缺失文件下载完成');
          
          setTimeout(async () => {
            setLaunchStep('build-args', 'running', '正在构建启动参数...');
            await new Promise(r => setTimeout(r, 200));
            setLaunchStep('build-args', 'success', '启动参数构建完成');
            
            setLaunchStep('launching', 'running', '正在启动 Minecraft...');
            
            const result = await API.launchGame(versionId);
            
            if (result.success) {
              setLaunchStep('launching', 'success', '游戏进程已创建');
              updateLaunchProgress(100);
              document.getElementById('launch-log-section').style.display = '';
              const launchBtn = document.getElementById('launch-btn');
              const homeLaunchBtn = document.getElementById('home-launch-btn');
              launchBtn.classList.add('running');
              launchBtn.querySelector('span').textContent = '启动游戏';
              document.getElementById('status-indicator').classList.add('running');
              document.getElementById('status-text').textContent = '游戏运行中';
              startGameLogStream();
              updateGameStatus();
              incrementLaunchCount();
              checkSupportMilestone();
              _onGameRunning();
              setTimeout(() => {
                closeLaunchModal('fade');
                launchBtn.disabled = false;
                homeLaunchBtn.disabled = false;
              }, 2000);
            } else {
              setLaunchStep('launching', 'error', result.error || '启动失败');
              showLaunchError(result.error || '启动失败');
              const launchBtn = document.getElementById('launch-btn');
              const homeLaunchBtn = document.getElementById('home-launch-btn');
              if (launchBtn) launchBtn.disabled = false;
              if (homeLaunchBtn) homeLaunchBtn.disabled = false;
            }
          }, 500);
        }
      } catch (e) {
        console.warn('[Launch] 启动轮询回调异常:', e);
      }
    }, 800);
  } catch (e) {
    console.error('[Launch] 轮询失败:', e);
  }
}

function updateGameInstanceList(instances) {
  let container = document.getElementById('game-instance-list');
  if (!container) {
    const sidebar = document.querySelector('.launch-bar') || document.querySelector('.sidebar');
    if (!sidebar) return;
    container = document.createElement('div');
    container.id = 'game-instance-list';
    container.style.cssText = 'position:fixed;bottom:60px;right:16px;z-index:1000;display:flex;flex-direction:column;gap:6px;max-width:280px;';
    document.body.appendChild(container);
  }

  if (instances.length === 0) {
    container.innerHTML = '';
    container.style.display = 'none';
    return;
  }

  container.style.display = 'flex';
  container.innerHTML = instances.map(inst => {
    const elapsed = Math.floor((Date.now() - inst.startTime) / 1000);
    const mins = Math.floor(elapsed / 60);
    const secs = elapsed % 60;
    const timeStr = mins > 0 ? `${mins}分${secs}秒` : `${secs}秒`;
    return `
      <div class="game-instance-card" data-session="${inst.sessionId}" style="
        background:var(--card-bg);border:1px solid var(--border);border-radius:8px;
        padding:8px 12px;display:flex;align-items:center;gap:8px;font-size:12px;
        box-shadow:0 2px 8px rgba(0,0,0,0.3);cursor:default;
      ">
        <div style="width:8px;height:8px;border-radius:50%;background:var(--green);flex-shrink:0;"></div>
        <div style="flex:1;min-width:0;">
          <div style="font-weight:600;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;">${inst.versionId}</div>
          <div style="color:var(--text-secondary);font-size:11px;">PID: ${inst.pid} · ${timeStr}${inst.lanPort ? ' · LAN:' + inst.lanPort : ''}</div>
        </div>
        <button onclick="stopGameInstance('${inst.sessionId}')" style="
          background:var(--red);color:white;border:none;border-radius:4px;
          padding:2px 8px;cursor:pointer;font-size:11px;flex-shrink:0;
        ">停止</button>
      </div>
    `;
  }).join('');
}

async function stopGameInstance(sessionId) {
  try {
    const result = await API.stopGameInstance(sessionId);
    if (result.success) {
      showToast('游戏实例已停止', 'info');
      updateGameStatus();
    } else {
      showToast(result.error || '停止失败', 'error');
    }
  } catch (e) {
    showToast('停止请求失败', 'error');
  }
}

// 游戏运行低调模式 - 游戏启动成功后的统一处理：
// 1. 标记 body.game-running  2. 挂起壁纸引擎  3. 5秒后按设置最小化启动器
function _onGameRunning() {
  try { document.body.classList.add('game-running'); } catch (e) {}
  try {
    if (typeof wallpaperEngine !== 'undefined' && wallpaperEngine && typeof wallpaperEngine.suspend === 'function') {
      wallpaperEngine.suspend();
    }
  } catch (e) {}
  try {
    if (window.electronAPI && window.electronAPI.store && window.electronAPI.windowMinimize) {
      window.electronAPI.store.get('minimizeOnGameRun').then((val) => {
        // 默认开启：undefined/null 视为 true
        if (val === undefined || val === null || val === true) {
          setTimeout(() => {
            try { window.electronAPI.windowMinimize(); } catch (e) {}
          }, 5000);
        }
      }).catch(() => {});
    }
  } catch (e) {}
}

// 游戏运行低调模式 - 游戏退出后的统一处理：
// 1. 移除 body.game-running  2. 恢复壁纸引擎  3. 恢复启动器窗口
function _onGameExited() {
  try { document.body.classList.remove('game-running'); } catch (e) {}
  try {
    if (typeof wallpaperEngine !== 'undefined' && wallpaperEngine && typeof wallpaperEngine.resume === 'function') {
      wallpaperEngine.resume();
    }
  } catch (e) {}
  try {
    if (window.electronAPI && window.electronAPI.windowRestore) {
      window.electronAPI.windowRestore();
    }
  } catch (e) {}
}

function startGameLogStream() {
  if (gameLogEventSource) { try { gameLogEventSource.close(); } catch(_) {} gameLogEventSource = null; }
  // 清理之前的 Tauri 事件监听器
  if (window._versepcGameLogUnlisten) { try { window._versepcGameLogUnlisten(); } catch(_) {} window._versepcGameLogUnlisten = null; }
  if (window._versepcGameExitUnlisten) { try { window._versepcGameExitUnlisten(); } catch(_) {} window._versepcGameExitUnlisten = null; }

  const consoleOutput = document.getElementById('console-output');
  consoleOutput.innerHTML = '';

  // 处理单行日志的通用函数
  function handleLogLine(line) {
    let type = '';
    if (line.includes('ERROR') || line.includes('FATAL') || line.includes('Exception')) type = 'error';
    else if (line.includes('WARN')) type = 'warn';
    else if (line.includes('[VersePC]')) type = 'info';
    appendConsoleLine(line, type);
  }

  // Tauri 环境：用事件监听替代 SSE
  if (window.__TAURI__ || window.__TAURI_INTERNALS__) {
    try {
      var evt = (window.__TAURI__ && window.__TAURI__.event) || window.__TAURI_INTERNALS__;
      if (evt && evt.listen) {
        // 监听 game-log 事件
        window._versepcGameLogUnlisten = (function() {
          var disposed = false;
          var unlisten = null;
          evt.listen('game-log', function(event) {
            if (disposed) return;
            try {
              var payload = event.payload || {};
              if (payload.line) {
                handleLogLine(payload.line);
              } else if (payload.batch && Array.isArray(payload.batch)) {
                for (var i = 0; i < payload.batch.length; i++) {
                  handleLogLine(payload.batch[i]);
                }
              }
            } catch (e) { console.warn('[GameLog] 解析日志失败:', e); }
          }).then(function(fn) {
            if (disposed) { try { fn(); } catch(_) {} } else { unlisten = fn; }
          }).catch(function(e) { console.warn('[GameLog] listen game-log failed:', e); });
          return function() { disposed = true; if (unlisten) { try { unlisten(); } catch(_) {} unlisten = null; } };
        })();

        // 监听 game-exit 事件
        window._versepcGameExitUnlisten = (function() {
          var disposed = false;
          var unlisten = null;
          evt.listen('game-exit', function(event) {
            if (disposed) return;
            try {
              var payload = event.payload || {};
              appendConsoleLine('[VersePC] 游戏进程已退出 (code=' + (payload.code != null ? payload.code : '?') + ')', 'warn');
              if (payload.isCrash) {
                appendConsoleLine('[VersePC] 检测到崩溃: ' + (payload.reason || '未知原因'), 'error');
                if (payload.suggestion) {
                  appendConsoleLine('[VersePC] 建议: ' + payload.suggestion, 'info');
                }
              }
              _onGameExited();
            } catch (e) { console.warn('[GameLog] 处理退出事件失败:', e); }
          }).then(function(fn) {
            if (disposed) { try { fn(); } catch(_) {} } else { unlisten = fn; }
          }).catch(function(e) { console.warn('[GameLog] listen game-exit failed:', e); });
          return function() { disposed = true; if (unlisten) { try { unlisten(); } catch(_) {} unlisten = null; } };
        })();

        console.log('[GameLog] 使用 Tauri 事件监听游戏日志');
        return;
      }
    } catch (e) {
      console.warn('[GameLog] Tauri 事件监听初始化失败，回退到 SSE:', e);
    }
  }

  // 非 Tauri 环境：用 SSE
  try {
    gameLogEventSource = new EventSource('/api/game/log/stream');
    gameLogEventSource.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data);
        if (data.event === 'exited') {
          appendConsoleLine('[VersePC] 游戏进程已退出', 'warn');
          _onGameExited();
          gameLogEventSource.close();
          gameLogEventSource = null;
          return;
        }
        if (data.batch && Array.isArray(data.batch)) {
          const frag = document.createDocumentFragment();
          const batchLen = data.batch.length;
          for (let i = 0; i < batchLen; i++) {
            const line = data.batch[i];
            let type = '';
            if (line.includes('ERROR') || line.includes('FATAL') || line.includes('Exception')) type = 'error';
            else if (line.includes('WARN')) type = 'warn';
            else if (line.includes('[VersePC]')) type = 'info';
            const el = document.createElement('div');
            el.className = 'console-line' + (type ? ' ' + type : '');
            el.textContent = line;
            frag.appendChild(el);
          }
          consoleOutput.appendChild(frag);
          while (consoleOutput.children.length > 500) consoleOutput.removeChild(consoleOutput.firstChild);
          consoleOutput.scrollTop = consoleOutput.scrollHeight;
        } else if (data.line) {
          handleLogLine(data.line);
        }
      } catch (e) {
        console.warn('[GameLog] 解析日志行失败:', e);
      }
    };
    gameLogEventSource.onerror = () => { gameLogEventSource.close(); gameLogEventSource = null; };
  } catch (e) {
    console.warn('[GameLog] 创建日志流失败:', e);
  }
}

function appendConsoleLine(text, type = '') {
  const consoleOutput = document.getElementById('console-output');
  const line = document.createElement('div');
  line.className = `console-line ${type}`;
  line.textContent = text;
  consoleOutput.appendChild(line);
  consoleOutput.scrollTop = consoleOutput.scrollHeight;
  while (consoleOutput.children.length > 500) consoleOutput.removeChild(consoleOutput.firstChild);
}

async function detectJava() {
  const hint = document.getElementById('java-detect-result');
  if (hint) hint.textContent = '检测中...';
  try {
    const result = await API.detectJava();
    if (result.javaList && result.javaList.length > 0) {
      const best = result.javaList.find(j => j.majorVersion >= 17) || result.javaList[0];
      const javaPathInput = document.getElementById('setting-java-path');
      if (javaPathInput) javaPathInput.value = best.path;
      if (hint) hint.textContent = `找到 Java ${best.version} (${best.is64Bit ? '64位' : '32位'})`;
      const statJava = document.getElementById('stat-java');
      if (statJava) statJava.textContent = best.majorVersion;
    } else {
      if (hint) hint.textContent = '未检测到Java，请手动配置或安装';
    }
  } catch (e) { if (hint) hint.textContent = '检测失败'; }
}

let javaInstallPollTimer = null;

async function checkJavaOnStartup() {
  try {
    const result = await API.detectJava();
    if (result.javaList && result.javaList.length > 0) {
      const best = result.javaList.find(j => j.majorVersion >= 17) || result.javaList[0];
      const statJava = document.getElementById('stat-java');
      if (statJava) statJava.textContent = best.majorVersion;
    }
  } catch (e) {
    console.error('Java startup check failed:', e);
  }
}

async function triggerJvmPreheat() {
  try {
    const saved = await window.electronAPI.store.get('versepc_launch_settings');
    if (!saved) return;
    const settings = JSON.parse(saved);
    if (!settings.jvmPreheat) return;

    const result = await API.detectJava();
    if (result && result.javaList && result.javaList.length > 0) {
      const bestJava = result.javaList.find(j => j.majorVersion >= 17) || result.javaList[0];
      const memInfo = await API.getSystemMemory();
      const totalMB = memInfo.totalMB || 8192;
      const preheatMem = Math.min(2048, Math.floor(totalMB * 0.3));
      await fetch('/api/jvm/preheat', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ javaPath: bestJava.path, maxMemMB: preheatMem })
      });
    }
  } catch(e) {}
}

async function generateCdsArchive() {
  const versionId = document.getElementById('launch-version-select')?.value;
  if (!versionId) {
    showToast('请先选择一个游戏版本', 'error');
    return;
  }
  const statusText = document.getElementById('cds-status-text');
  if (statusText) statusText.textContent = '正在生成...';
  showToast('正在生成 CDS 归档，请稍候...', 'info');
  try {
    const result = await API.generateCds(versionId);
    if (result.success) {
      const sizeInfo = result.sizeKB ? ` (${result.sizeKB}KB)` : '';
      showToast(`CDS 归档生成成功${sizeInfo}，下次启动将自动加速`, 'success');
      if (statusText) statusText.textContent = `✓ 已生成${sizeInfo}`;
    } else {
      showToast('CDS 归档生成失败: ' + (result.error || '未知错误'), 'error');
      if (statusText) statusText.textContent = '✗ 生成失败';
    }
  } catch (e) {
    showToast('CDS 归档生成失败: ' + e.message, 'error');
    if (statusText) statusText.textContent = '✗ 生成失败';
  }
}

async function checkCdsStatus() {
  const versionId = document.getElementById('launch-version-select')?.value;
  if (!versionId) return;
  const statusText = document.getElementById('cds-status-text');
  if (!statusText) return;
  try {
    const result = await API.getCdsStatus(versionId);
    if (result.available) {
      statusText.textContent = `✓ 归档已就绪 (${result.sizeKB}KB)`;
    } else {
      statusText.textContent = '未生成归档';
    }
  } catch (e) {
    statusText.textContent = '';
  }
}
