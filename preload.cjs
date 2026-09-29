const { contextBridge, ipcRenderer } = require('electron');
contextBridge.exposeInMainWorld('formaDesktop', {
  loadRecovery: () => ipcRenderer.invoke('load-recovery'),
  mcpStatus: () => ipcRenderer.invoke('forma:mcp-status'),
  mcpToggle: enabled => ipcRenderer.invoke('forma:mcp-toggle', enabled),
  onMcpStatus: callback => ipcRenderer.on('forma:mcp-status', (_, status) => callback(status)),
  onCommand: callback => ipcRenderer.on('forma:command', (_, request) => callback(request)),
  reply: response => ipcRenderer.send('forma:reply', response)
});
