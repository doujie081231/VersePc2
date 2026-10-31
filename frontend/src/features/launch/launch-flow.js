import { API as API } from '../../core/api.js';
import { bridge as bridge } from '../../core/tauri-bridge.js';
import { incrementLaunchCount as incrementLaunchCount, checkSupportMilestone as checkSupportMilestone } from '../misc/announcement.js';
import { showToast as showToast } from '../misc/ui-components.js';
import { appState as appState } from '../../core/app-state.js';
import { S } from './launch-state.js';
import { refreshGameStatusAfterLaunch, _onGameRunning, startGameLogStream } from './launch-runtime.js';
import { showLaunchModal, closeLaunchModal, updateLaunchProgress, setLaunchStep, showLaunchError, hideLaunchError, pollLaunchDownload } from './launch-modal.js';

async function handleLaunch() {
  // 读取本次快速进世界目标并立即清空，避免下次普通启动误进上一次的世界
  const _quickPlayWorld = appState.quickPlayWorld;
  appState.quickPlayWorld = '';

  if (S.launching) {
    if (typeof showToast === 'function') showToast('正在启动中，请稍候...', 'info');
    return;
  }
  S.launching = true;
  setTimeout(() => { S.launching = false; }, 300000);

  const versionId = (typeof appState.currentLaunchVersionId !== 'undefined' && appState.currentLaunchVersionId) || (appState.launchVersionCustomSelect ? appState.launchVersionCustomSelect.getValue() : '');
  if (!versionId) { showToast('请选择游戏版本', 'error'); S.launching = false; return; }

  appState._cachedLastLaunchVersion = versionId;
  try { await bridge.store.set('versepc_last_launch_version', versionId); } catch (_) {}

  // 基岩版：不走账户/Java/文件校验，直接启动本地 exe
  const _bedrockVersion = (appState.installedVersions || []).find(v => v.id === versionId && v.isBedrock);
  if (_bedrockVersion) {
    try {
      const result = await API.launchGame(versionId);
      if (result.success) showToast(result.message || '基岩版已启动', 'success');
      else showToast(result.error || '基岩版启动失败', 'error');
    } catch (e) {
      showToast('启动请求失败：' + (e.message || e), 'error');
    } finally {
      S.launching = false;
    }
    return;
  }

  const launchBtn = document.getElementById('launch-btn');
  const homeLaunchBtn = document.getElementById('home-launch-btn');

  launchBtn.disabled = true;
  homeLaunchBtn.disabled = true;

  appState._launchCounted = false;

  showLaunchModal();
  hideLaunchError();

  try {
    setLaunchStep('auth', 'running', '正在验证登录状态...');
    const accountsResult = await API.getAccounts();
    const accounts = Array.isArray(accountsResult) ? accountsResult : (accountsResult.accounts || []);
    if (accounts.length === 0) {
      setLaunchStep('auth', 'failed', '未登录，请先添加账户');
      showLaunchError('未登录，请先在账户管理中添加账户后再启动游戏。');
      S.launching = false;
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
    let depCheck = await API.launchCheck(versionId, externalVersionDir);
    const requiredJava = (depCheck.java && depCheck.java.required) || 21;

    if (!depCheck.java || !depCheck.java.ok) {
      const requiredVer = requiredJava;
      const maxVer = depCheck.java.maxVersion;
      const rangeDesc = maxVer && maxVer < 999 ? `${requiredVer}~${maxVer}` : `${requiredVer}+`;
      const serverMsg = depCheck.java.message || '';

      // ===== 自动补 Java（对齐 Electron 版 autoInstallJava）=====
      setLaunchStep('java-check', 'running', `未找到 Java ${rangeDesc}，正在自动下载...`);
      try {
        const ai = await API.autoInstallJava(requiredVer);
        if (ai && ai.success && ai.javaPath) {
          setLaunchStep('java-check', 'success', `Java ${ai.version || requiredVer} 已就绪 ✓`);
        } else if (ai && ai.success && ai.sessionId) {
          const ok = await waitJavaAutoInstall(ai.sessionId, requiredVer, rangeDesc);
          if (!ok) {
            setLaunchStep('java-check', 'error', `Java ${rangeDesc} 自动下载失败`);
            showLaunchError(`Java 自动下载失败，请检查网络后重试，或前往 Java 管理页面手动安装。<br><a href="javascript:void(0)" id="launch-nav-java" style="color:var(--accent);text-decoration:underline;cursor:pointer;">前往 Java 管理页面 →</a>`);
            launchBtn.disabled = false;
            homeLaunchBtn.disabled = false;
            S.launching = false;
            return;
          }
          setLaunchStep('java-check', 'success', `Java ${requiredVer} 自动安装成功 ✓`);
        } else {
          throw new Error((ai && ai.message) || 'Java 自动安装请求失败');
        }
      } catch (e) {
        setLaunchStep('java-check', 'error', `Java ${rangeDesc} 自动下载失败`);
        showLaunchError(`${e.message || 'Java 自动下载失败'}<br><a href="javascript:void(0)" id="launch-nav-java" style="color:var(--accent);text-decoration:underline;cursor:pointer;">前往 Java 管理页面 →</a>`);
        launchBtn.disabled = false;
        homeLaunchBtn.disabled = false;
        S.launching = false;
        return;
      }

      // 自动安装完成后重新检查
      try {
        depCheck = await API.launchCheck(versionId, externalVersionDir);
      } catch (_) {}
      if (!depCheck.java || !depCheck.java.ok) {
        setLaunchStep('java-check', 'error', `Java ${rangeDesc} 仍未就绪`);
        showLaunchError(`${serverMsg || `未找到合适的Java运行环境（需要 Java ${rangeDesc}）`}<br><a href="javascript:void(0)" id="launch-nav-java" style="color:var(--accent);text-decoration:underline;cursor:pointer;">前往 Java 管理页面 →</a>`);
        launchBtn.disabled = false;
        homeLaunchBtn.disabled = false;
        S.launching = false;
        return;
      }
      setLaunchStep('java-check', 'success', depCheck.java.message || `Java ${depCheck.java.version} ✓`);
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
        // 主 JAR 缺失但有可下载补全项时，继续走后续的自动补全流程（与库/Forge核心一致），而不是直接报错
        const mainJarDownloadable = (depCheck.missingFiles || []).some(f => f.kind === 'main_jar' && f.url);
        if (mainJarDownloadable) {
          setLaunchStep('files-check', 'warning', `${depCheck.mainJar.message}，正在自动补全...`);
        } else {
          setLaunchStep('files-check', 'error', depCheck.mainJar.message);
          showLaunchError(depCheck.mainJar.message);
          launchBtn.disabled = false;
          homeLaunchBtn.disabled = false;
          S.launching = false;
          return;
        }
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
          S.launching = false;
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
        S.launching = false;
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
        S.launching = false;
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
        S.launching = false;
        return;
      }
      setLaunchStep('download', 'success', '缺失文件已补全');
    }

    setLaunchStep('build-args', 'running', '正在构建启动参数...');
    await new Promise(r => setTimeout(r, 200));
    setLaunchStep('build-args', 'success', '启动参数构建完成');

    setLaunchStep('S.launching', 'running', '正在启动 Minecraft...');

    const result = await API.launchGame(versionId, {
      resolution: getCurrentWindowSize(),
      ...(_quickPlayWorld ? { quickPlayWorld: _quickPlayWorld } : {})
    }, 300000);

    if (result.needDownload && result.sessionId) {
      pollLaunchDownload(result.sessionId, versionId, requiredJava);
      S.launching = false;
      return;
    }

    if (result.success) {
      setLaunchStep('S.launching', 'success', '游戏进程已创建');
      updateLaunchProgress(100);
      document.getElementById('launch-log-section').style.display = '';
      launchBtn.classList.add('running');
      launchBtn.querySelector('span').textContent = '启动游戏';
      document.getElementById('status-indicator')?.classList.add('running');
      const _st1 = document.getElementById('status-text'); if (_st1) _st1.textContent = '游戏运行中';
      startGameLogStream();
      refreshGameStatusAfterLaunch();
      incrementLaunchCount();
      checkSupportMilestone();
      _onGameRunning();
      setTimeout(() => {
        closeLaunchModal('fade');
        launchBtn.disabled = false;
        homeLaunchBtn.disabled = false;
        S.launching = false;
      }, 2000);
    } else {
      setLaunchStep('S.launching', 'error', result.error || '启动失败');
      showLaunchError(result.error || '启动失败', result.details || result);
      launchBtn.disabled = false;
      homeLaunchBtn.disabled = false;
      S.launching = false;
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
    S.launching = false;
    // 超时后游戏可能已在后台启动，延迟关闭模态框让用户看到错误提示
    setTimeout(() => {
      if (typeof closeLaunchModal === 'function') closeLaunchModal();
    }, 5000);
  }
}

function waitJavaAutoInstall(sessionId, requiredVer, rangeDesc) {
  return new Promise((resolve) => {
    let tries = 0;
    const maxTries = 3600; // 500ms 轮询，最长约 30 分钟
    const timer = setInterval(async () => {
      tries++;
      if (tries > maxTries) { clearInterval(timer); resolve(false); return; }
      try {
        const st = await API.getJavaInstallStatus(sessionId);
        if (!st) { return; }
        if (st.status === 'completed') {
          clearInterval(timer);
          resolve(true);
          return;
        }
        if (st.status === 'error' || st.status === 'failed' || st.status === 'cancelled') {
          clearInterval(timer);
          resolve(false);
          return;
        }
        // 进行中：更新进度文案
        const pct = st.progress || 0;
        if (typeof setLaunchStep === 'function') {
          setLaunchStep('java-check', 'running', `正在自动下载 Java ${rangeDesc}... ${pct}%`);
        }
      } catch (e) {
        // 轮询出错忽略，继续重试
      }
    }, 500);
  });
}

function getCurrentWindowSize() {
  let ws = document.getElementById('window-size')?.value || 'default';
  if (ws === 'default') ws = '854x480';
  else if (ws === 'custom') {
    const w = document.getElementById('custom-width')?.value;
    const h = document.getElementById('custom-height')?.value;
    ws = (w && h) ? `${w}x${h}` : '1920x1080';
  }
  return ws;
}

function cancelLaunchFlow() {
  if (S.launchDlPollInterval) {
    clearInterval(S.launchDlPollInterval);
    S.launchDlPollInterval = null;
  }
  S.launching = false;
  if (typeof API !== 'undefined' && API.cancelLaunch) {
    API.cancelLaunch().catch(() => {});
  }
  closeLaunchModal();
  const launchBtn = document.getElementById('launch-btn');
  const homeLaunchBtn = document.getElementById('home-launch-btn');
  if (launchBtn) launchBtn.disabled = false;
  if (homeLaunchBtn) homeLaunchBtn.disabled = false;
}

export { handleLaunch, waitJavaAutoInstall, getCurrentWindowSize, cancelLaunchFlow };