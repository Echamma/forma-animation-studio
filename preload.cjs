const { contextBridge, ipcRenderer } = require('electron');
contextBridge.exposeInMainWorld('formaDesktop', {
  loadRecovery: () => ipcRenderer.invoke('load-recovery'),
  loadDefault: () => ipcRenderer.invoke('load-default'),
  autosave: payload => ipcRenderer.invoke('forma:autosave', payload),
  listProjects: () => ipcRenderer.invoke('forma:list-projects'),
  openLocalProject: id => ipcRenderer.invoke('forma:open-local-project', id),
  mcpStatus: () => ipcRenderer.invoke('forma:mcp-status'),
  mcpToggle: enabled => ipcRenderer.invoke('forma:mcp-toggle', enabled),
  onMcpStatus: callback => ipcRenderer.on('forma:mcp-status', (_, status) => callback(status)),
  onCommand: callback => ipcRenderer.on('forma:command', (_, request) => callback(request)),
  reply: response => ipcRenderer.send('forma:reply', response),
  testMode: process.argv.includes('--forma-test')
});
