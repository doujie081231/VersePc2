const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('installerAPI', {
    selectFolder: () => ipcRenderer.invoke('select-folder'),
    checkFolderContents: (folderPath) => ipcRenderer.invoke('check-folder-contents', folderPath),
    checkInstalled: (folderPath) => ipcRenderer.invoke('check-installed', folderPath),
    getDefaultInstallPath: () => ipcRenderer.invoke('get-default-install-path'),
    getDiskSpace: (folderPath) => ipcRenderer.invoke('get-disk-space', folderPath),
    installFiles: (installPath) => ipcRenderer.invoke('install-files', installPath),
    launchApp: (exePath) => ipcRenderer.invoke('launch-app', exePath),
    closeWindow: () => ipcRenderer.invoke('close-window'),
    minimizeWindow: () => ipcRenderer.invoke('minimize-window'),
    onInstallProgress: (callback) => {
        ipcRenderer.on('install-progress', (event, data) => callback(data));
    }
});
