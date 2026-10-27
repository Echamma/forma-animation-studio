const { contextBridge, ipcRenderer, webUtils } = require('electron');
contextBridge.exposeInMainWorld('formaDesktop', {
  loadRecovery: () => ipcRenderer.invoke('load-recovery'),
  loadDefault: () => ipcRenderer.invoke('load-default'),
  autosave: payload => ipcRenderer.invoke('forma:autosave', payload),
  listProjects: () => ipcRenderer.invoke('forma:list-projects'),
  openLocalProject: id => ipcRenderer.invoke('forma:open-local-project', id),
  // Files picked or dropped by the user are copied by path; anything else (a generated poster, a file from a .forma
  // project) is sent as bytes. Both return a forma-asset:// URL.
  storeMedia: async (file, type) => {
    const source = webUtils.getPathForFile(file);
    return source ? ipcRenderer.invoke('forma:store-media-file', source, type) : ipcRenderer.invoke('forma:store-media', await file.arrayBuffer(), type);
  },
  onOpenFile: callback => ipcRenderer.on('forma:open-file', (_, file) => callback(file)),
  mcpStatus: () => ipcRenderer.invoke('forma:mcp-status'),
  mcpToggle: enabled => ipcRenderer.invoke('forma:mcp-toggle', enabled),
  onMcpStatus: callback => ipcRenderer.on('forma:mcp-status', (_, status) => callback(status)),
  onCommand: callback => ipcRenderer.on('forma:command', (_, request) => callback(request)),
  reply: response => ipcRenderer.send('forma:reply', response),
  testMode: process.argv.includes('--forma-test')
});
