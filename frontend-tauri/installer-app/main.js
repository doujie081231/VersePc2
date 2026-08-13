const { app, BrowserWindow, ipcMain, dialog, shell } = require('electron');
const path = require('path');
const fs = require('fs');
const os = require('os');

let mainWindow = null;
let isInstalling = false;

function createWindow() {
    mainWindow = new BrowserWindow({
        width: 660,
        height: 480,
        frame: false,
        resizable: false,
        backgroundColor: '#1a1b2e',
        webPreferences: {
            nodeIntegration: false,
            contextIsolation: true,
            preload: path.join(__dirname, 'preload.cjs')
        },
        icon: path.join(__dirname, 'icon.ico'),
        center: true,
        show: false
    });

    mainWindow.loadFile('index.html');
    mainWindow.once('ready-to-show', () => {
        mainWindow.show();
    });
}

app.whenReady().then(createWindow);

app.on('window-all-closed', () => {
    if (!isInstalling) app.quit();
});

function findAppDataDir() {
    const exeDir = path.dirname(process.execPath);
    const candidates = [
        path.join(exeDir, 'resources', 'app', 'data'),
        path.join(exeDir, 'resources', 'data'),
        path.join(__dirname, 'data'),
        path.join(__dirname, '..', 'data'),
    ];
    for (const c of candidates) {
        try {
            if (fs.existsSync(c) && fs.statSync(c).isDirectory()) {
                const mainJs = path.join(c, 'main.js');
                const serverJs = path.join(c, 'server.js');
                if (fs.existsSync(mainJs) && fs.existsSync(serverJs)) {
                    return c;
                }
            }
        } catch (e) {}
    }
    return null;
}

ipcMain.handle('select-folder', async () => {
    const result = await dialog.showOpenDialog(mainWindow, {
        properties: ['openDirectory'],
        title: '选择安装位置'
    });
    if (result.canceled) return null;
    return result.filePaths[0];
});

ipcMain.handle('check-folder-contents', async (event, folderPath) => {
    try {
        if (!fs.existsSync(folderPath)) return { count: 0, items: [], exists: false };
        const items = fs.readdirSync(folderPath).filter(i => i !== '.' && i !== '..');
        return { count: items.length, items: items.slice(0, 20), exists: true };
    } catch (e) {
        return { count: 0, items: [], exists: false };
    }
});

ipcMain.handle('check-installed', async (event, folderPath) => {
    const checkDirs = [folderPath];
    const verseSubdir = path.join(folderPath, 'Verse');
    if (fs.existsSync(verseSubdir)) checkDirs.push(verseSubdir);

    for (const dir of checkDirs) {
        try {
            const exePath = path.join(dir, 'VersePC.exe');
            const appDir = path.join(dir, 'resources', 'app');
            const appAsar = path.join(dir, 'resources', 'app.asar');
            if (fs.existsSync(exePath) && (fs.existsSync(appDir) || fs.existsSync(appAsar))) {
                const stat = fs.statSync(exePath);
                return {
                    installed: true, installDir: dir, exePath: exePath,
                    installTime: stat.mtime.toISOString(),
                    installSize: getFolderSize(dir)
                };
            }
        } catch (e) {}
    }
    return { installed: false };
});

function getFolderSize(dirPath, depth = 0) {
    if (depth > 20) return 0;
    let size = 0;
    try {
        for (const item of fs.readdirSync(dirPath)) {
            if (item === 'node_modules' || item === '.git') continue;
            const p = path.join(dirPath, item);
            try {
                const s = fs.lstatSync(p);
                if (s.isSymbolicLink()) continue;
                if (s.isDirectory()) {
                    size += getFolderSize(p, depth + 1);
                } else {
                    size += s.size;
                }
            } catch (e) {}
        }
    } catch (e) {}
    return size;
}

ipcMain.handle('get-default-install-path', async () => {
    return path.join(process.env.LOCALAPPDATA || path.join(os.homedir(), 'AppData', 'Local'), 'Verse');
});

ipcMain.handle('get-disk-space', async (event, folderPath) => {
    try {
        const { execSync } = require('child_process');
        const drive = path.resolve(folderPath).charAt(0);
        const output = execSync(`wmic logicaldisk where "DeviceID='${drive}:'" get FreeSpace,Size /format:value`, { encoding: 'utf8' });
        const lines = output.trim().split('\n').filter(l => l.trim());
        let free = 0, total = 0;
        for (const line of lines) {
            const [k, v] = line.split('=').map(s => s.trim());
            if (k === 'FreeSpace') free = parseInt(v) || 0;
            if (k === 'Size') total = parseInt(v) || 0;
        }
        return { available: free, total: total };
    } catch (e) {
        return { available: 0, total: 0 };
    }
});

ipcMain.handle('install-files', async (event, installPath) => {
    isInstalling = true;

    try {
        if (!fs.existsSync(installPath)) fs.mkdirSync(installPath, { recursive: true });

        const appDir = path.dirname(process.execPath);
        const dataDir = findAppDataDir();

        mainWindow.webContents.send('install-progress', {
            progress: 2, currentFile: '正在准备安装...', bytesCopied: 0, totalBytes: 0
        });

        if (!dataDir) {
            return { success: false, error: '找不到应用数据目录，安装包可能已损坏' };
        }

        // 只复制运行时文件，不复制 resources 目录（安装器的 UI 代码）
        const skipItems = new Set(['resources', 'LICENSE.electron.txt', 'LICENSES.chromium.html']);
        const items = fs.readdirSync(appDir);
        const filesToCopy = [];
        const dirsToCopy = [];

        for (const item of items) {
            if (skipItems.has(item)) continue;
            const ip = path.join(appDir, item);
            try {
                const st = fs.statSync(ip);
                if (st.isFile()) filesToCopy.push({ src: ip, name: item, size: st.size });
                else if (st.isDirectory()) dirsToCopy.push({ src: ip, name: item });
            } catch (e) {}
        }

        const totalFiles = filesToCopy.length;
        let copiedFiles = 0;

        for (const file of filesToCopy) {
            let destName = file.name;
            const lowerName = file.name.toLowerCase();
            if (lowerName === 'versepc setup.exe' || lowerName.startsWith('versepc')) {
                destName = 'VersePC.exe';
            }
            fs.copyFileSync(file.src, path.join(installPath, destName));
            copiedFiles++;
            const progress = 2 + Math.round((copiedFiles / totalFiles) * 45);
            mainWindow.webContents.send('install-progress', {
                progress, currentFile: file.name, bytesCopied: copiedFiles, totalBytes: totalFiles
            });
            await new Promise(r => setTimeout(r, 5));
        }

        // 复制 locales 等目录（不复制 resources）
        for (const dir of dirsToCopy) copyDirSync(dir.src, path.join(installPath, dir.name));

        mainWindow.webContents.send('install-progress', {
            progress: 50, currentFile: '正在复制应用代码...', bytesCopied: 0, totalBytes: 0
        });

        // 创建 resources/app/ 目录并复制主应用代码
        const resDir = path.join(installPath, 'resources');
        if (!fs.existsSync(resDir)) fs.mkdirSync(resDir, { recursive: true });

        const destAppDir = path.join(resDir, 'app');
        if (fs.existsSync(destAppDir)) {
            try { fs.rmSync(destAppDir, { recursive: true, force: true }); } catch (e) {}
        }
        fs.mkdirSync(destAppDir, { recursive: true });

        // 从 data/ 复制主应用代码到 resources/app/
        const dataFiles = [];
        function collectFiles(dir, base) {
            for (const item of fs.readdirSync(dir)) {
                const fp = path.join(dir, item);
                const rp = base ? base + '/' + item : item;
                try {
                    const st = fs.statSync(fp);
                    if (st.isFile()) dataFiles.push({ src: fp, rel: rp, size: st.size });
                    else if (st.isDirectory()) collectFiles(fp, rp);
                } catch (e) {}
            }
        }
        collectFiles(dataDir, '');

        const totalDataFiles = dataFiles.length;
        let copiedDataFiles = 0;

        for (const df of dataFiles) {
            const destFile = path.join(destAppDir, ...df.rel.split('/'));
            const destFileDir = path.dirname(destFile);
            if (!fs.existsSync(destFileDir)) fs.mkdirSync(destFileDir, { recursive: true });
            fs.copyFileSync(df.src, destFile);
            copiedDataFiles++;
            const progress = 50 + Math.round((copiedDataFiles / totalDataFiles) * 38);
            mainWindow.webContents.send('install-progress', {
                progress, currentFile: df.rel, bytesCopied: copiedDataFiles, totalBytes: totalDataFiles
            });
        }

        mainWindow.webContents.send('install-progress', {
            progress: 90, currentFile: '创建快捷方式...', bytesCopied: 0, totalBytes: 0
        });

        const exePath = path.join(installPath, 'VersePC.exe');
        try {
            shell.writeShortcutLink(path.join(os.homedir(), 'Desktop', 'VersePC.lnk'), { target: exePath, icon: exePath, description: 'VersePC - Minecraft Launcher' });
        } catch (e) {}

        try {
            const smd = path.join(process.env.APPDATA, 'Microsoft', 'Windows', 'Start Menu', 'Programs');
            if (!fs.existsSync(smd)) fs.mkdirSync(smd, { recursive: true });
            shell.writeShortcutLink(path.join(smd, 'VersePC.lnk'), { target: exePath, icon: exePath, description: 'VersePC - Minecraft Launcher' });
        } catch (e) {}

        try {
            const safePath = installPath.replace(/"/g, '""').replace(/\//g, '\\');
            fs.writeFileSync(path.join(installPath, 'uninstall.bat'), '@echo off\necho 正在卸载 VersePC...\ntaskkill /f /im VersePC.exe 2>nul\ntimeout /t 2 /nobreak >nul\nrd /s /q "' + safePath + '"\ndel "%APPDATA%\\Microsoft\\Windows\\Start Menu\\Programs\\VersePC.lnk" 2>nul\ndel "%USERPROFILE%\\Desktop\\VersePC.lnk" 2>nul\necho 卸载完成!\npause\ndel "%~f0"\n', 'utf8');
        } catch (e) {}

        const dataConfigPath = path.join(installPath, 'data-config.json');
        if (!fs.existsSync(dataConfigPath)) {
            const defaultDataDir = path.join(installPath, 'data');
            fs.mkdirSync(defaultDataDir, { recursive: true });
            fs.writeFileSync(dataConfigPath, JSON.stringify({ dataDir: defaultDataDir }, null, 2));
        }

        mainWindow.webContents.send('install-progress', {
            progress: 100, currentFile: '安装完成', bytesCopied: 0, totalBytes: 0
        });

        isInstalling = false;
        return { success: true, exePath };
    } catch (e) {
        isInstalling = false;
        return { success: false, error: e.message };
    }
});

function copyDirSync(s, d) {
    if (!fs.existsSync(d)) fs.mkdirSync(d, { recursive: true });
    for (const it of fs.readdirSync(s)) {
        const sp = path.join(s, it), dp = path.join(d, it);
        try { const st = fs.statSync(sp); if (st.isFile()) fs.copyFileSync(sp, dp); else if (st.isDirectory()) copyDirSync(sp, dp); } catch (e) {}
    }
}

ipcMain.handle('launch-app', async (e, p) => { try { shell.openPath(p); app.quit(); return { success: true }; } catch (ex) { return { success: false, error: ex.message }; } });
ipcMain.handle('close-window', async () => { if (!isInstalling) app.quit(); });
ipcMain.handle('minimize-window', async () => { if (mainWindow) mainWindow.minimize(); });
