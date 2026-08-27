let currentStep = 'welcome';
let installPath = '';
let basePath = '';
let autoVerseFolder = false;
let installedExePath = '';
let isAlreadyInstalled = false;

const steps = ['welcome', 'path', 'installing', 'finish'];

function showStep(stepId) {
    document.querySelectorAll('.step').forEach(s => s.classList.remove('active'));
    const stepEl = document.getElementById(`step-${stepId}`);
    if (stepEl) {
        stepEl.classList.add('active');
        stepEl.style.animation = 'none';
        stepEl.offsetHeight;
        stepEl.style.animation = '';
    }
    currentStep = stepId;
    updateNavButtons();
}

function updateNavButtons() {
    const btnBack = document.getElementById('btn-back');
    const btnCancel = document.getElementById('btn-cancel');
    const btnNext = document.getElementById('btn-next');

    const stepIndex = steps.indexOf(currentStep);

    btnBack.style.display = stepIndex > 0 && currentStep !== 'installing' ? 'flex' : 'none';
    btnCancel.style.display = currentStep !== 'finish' ? 'flex' : 'none';

    if (currentStep === 'welcome') {
        btnNext.innerHTML = '下一步 <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M5 12h14"/><path d="M12 5l7 7-7 7"/></svg>';
        btnNext.disabled = false;
    } else if (currentStep === 'path') {
        if (isAlreadyInstalled) {
            btnNext.innerHTML = '重新安装 <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 2v6h-6"/><path d="M3 12a9 9 0 0 1 15-6.7L21 8"/><path d="M21 22v-6h-6"/><path d="M3 12a9 9 0 0 0 15 6.7L21 16"/></svg>';
        } else {
            btnNext.innerHTML = '安装 <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>';
        }
        btnNext.disabled = !installPath;
    } else if (currentStep === 'installing') {
        btnNext.style.display = 'none';
        btnCancel.disabled = true;
    } else if (currentStep === 'finish') {
        btnNext.innerHTML = '完成 <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"/><polyline points="22 4 12 14.01 9 11.01"/></svg>';
        btnNext.disabled = false;
        btnNext.style.display = 'flex';
        btnCancel.style.display = 'none';
    }

    if (currentStep !== 'installing') {
        btnCancel.disabled = false;
    }
}

async function goNext() {
    if (currentStep === 'welcome') {
        showStep('path');
        if (!installPath) {
            const defaultPath = await installerAPI.getDefaultInstallPath();
            basePath = defaultPath;
            installPath = defaultPath;
            document.getElementById('install-path').value = installPath;
            await checkPathFolder(installPath);
        }
    } else if (currentStep === 'path') {
        if (!installPath) return;
        showStep('installing');
        startInstallation(installPath);
    } else if (currentStep === 'finish') {
        const launchChecked = document.getElementById('launch-after-install').checked;
        if (launchChecked && installedExePath) {
            await installerAPI.launchApp(installedExePath);
        } else {
            installerAPI.closeWindow();
        }
    }
}

function goBack() {
    const stepIndex = steps.indexOf(currentStep);
    if (stepIndex > 0) {
        showStep(steps[stepIndex - 1]);
    }
}

async function browsePath() {
    const selected = await installerAPI.selectFolder();
    if (selected) {
        basePath = selected;
        installPath = selected;
        document.getElementById('install-path').value = installPath;
        await checkPathFolder(installPath);
        updateNavButtons();
    }
}

async function checkPathFolder(folderPath) {
    const verseHint = document.getElementById('verse-hint');
    const diskSpaceText = document.getElementById('disk-space-text');
    const pathInput = document.getElementById('install-path');
    const installedBadge = document.getElementById('installed-badge');
    const installedInfo = document.getElementById('installed-info');

    try {
        const result = await installerAPI.checkFolderContents(folderPath);
        if (result.exists && result.count > 3) {
            autoVerseFolder = true;
            verseHint.style.display = 'flex';
            if (!basePath.endsWith('\\Verse') && !basePath.endsWith('/Verse')) {
                installPath = basePath + '\\Verse';
            } else {
                installPath = basePath;
            }
            pathInput.value = installPath;
        } else {
            autoVerseFolder = false;
            verseHint.style.display = 'none';
            installPath = basePath;
            pathInput.value = installPath;
        }
    } catch (e) {
        autoVerseFolder = false;
        verseHint.style.display = 'none';
        installPath = basePath;
        pathInput.value = installPath;
    }

    try {
        const installedResult = await installerAPI.checkInstalled(basePath);
        if (installedResult.installed) {
            isAlreadyInstalled = true;
            installedBadge.style.display = 'flex';
            const sizeMB = (installedResult.installSize / (1024 * 1024)).toFixed(1);
            const installDate = new Date(installedResult.installTime);
            const dateStr = `${installDate.getFullYear()}-${String(installDate.getMonth() + 1).padStart(2, '0')}-${String(installDate.getDate()).padStart(2, '0')}`;
            installedInfo.textContent = `${sizeMB} MB · ${dateStr}`;

            if (installedResult.installDir && installedResult.installDir !== basePath) {
                installPath = installedResult.installDir;
                pathInput.value = installPath;
            }
        } else {
            isAlreadyInstalled = false;
            installedBadge.style.display = 'none';
        }
    } catch (e) {
        isAlreadyInstalled = false;
        installedBadge.style.display = 'none';
    }

    try {
        const diskInfo = await installerAPI.getDiskSpace(folderPath);
        if (diskInfo.available > 0) {
            const availableGB = (diskInfo.available / (1024 * 1024 * 1024)).toFixed(1);
            const totalGB = (diskInfo.total / (1024 * 1024 * 1024)).toFixed(1);
            diskSpaceText.textContent = `可用空间: ${availableGB} GB / 共 ${totalGB} GB`;
        } else {
            diskSpaceText.textContent = '可用空间: 未知';
        }
    } catch (e) {
        diskSpaceText.textContent = '可用空间: 未知';
    }

    updateNavButtons();
}

async function startInstallation(finalPath) {
    const progressFill = document.getElementById('progress-fill');
    const progressPercent = document.getElementById('progress-percent');
    const installStatus = document.getElementById('install-status');
    const currentFile = document.getElementById('current-file');
    const installingTitle = document.getElementById('installing-title');
    const finishTitle = document.getElementById('finish-title');
    const finishDesc = document.getElementById('finish-desc');

    if (isAlreadyInstalled) {
        installingTitle.textContent = '正在重新安装';
        finishTitle.textContent = '重新安装完成!';
        finishDesc.textContent = 'VersePC 已成功覆盖安装到您的计算机';
    } else {
        installingTitle.textContent = '正在安装';
        finishTitle.textContent = '安装完成!';
        finishDesc.textContent = 'VersePC 已成功安装到您的计算机';
    }

    installerAPI.onInstallProgress((data) => {
        progressFill.style.width = data.progress + '%';
        progressPercent.textContent = data.progress + '%';
        if (data.currentFile) {
            currentFile.textContent = data.currentFile;
        }
        if (data.progress < 30) {
            installStatus.textContent = '正在复制应用文件...';
        } else if (data.progress < 90) {
            installStatus.textContent = '正在安装运行时组件...';
        } else if (data.progress < 100) {
            installStatus.textContent = '正在创建快捷方式...';
        } else {
            installStatus.textContent = '安装完成!';
        }
    });

    try {
        const result = await installerAPI.installFiles(finalPath);
        if (result.success) {
            installedExePath = result.exePath;
            setTimeout(() => {
                showStep('finish');
            }, 500);
        } else {
            installStatus.textContent = '安装失败: ' + (result.error || '未知错误');
            progressFill.style.background = 'var(--red)';
        }
    } catch (e) {
        installStatus.textContent = '安装失败: ' + e.message;
        progressFill.style.background = 'var(--red)';
    }
}

document.addEventListener('DOMContentLoaded', () => {
    showStep('welcome');
});
