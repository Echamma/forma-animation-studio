const { contextBridge, ipcRenderer } = require('electron');
contextBridge.exposeInMainWorld('formaDesktop', { loadRecovery: () => ipcRenderer.invoke('load-recovery') });
