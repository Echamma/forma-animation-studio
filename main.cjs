const {app, BrowserWindow, ipcMain, dialog} = require('electron');
const fs = require('node:fs/promises');
const path = require('node:path');
const crypto = require('node:crypto');
const {createBridge} = require('./mcp/desktop-bridge.cjs');
let window, bridge, closing = false, allowClose = false;
const pending = new Map();
const smokeTest = process.argv.includes('--smoke-test');
if (smokeTest) {
  process.env.FORMA_TEST = '1';
  app.setPath('userData', path.join(app.getPath('temp'), 'forma-animation-studio-smoke-' + process.pid));
} else if (process.env.FORMA_TEST !== '1') {
  app.setPath('userData', path.join(app.getPath('appData'), 'forma-animation-studio'));
  if (!app.requestSingleInstanceLock()) {app.quit(); return;}
  app.on('second-instance', async (_, argv) => {
    if (window) {if (window.isMinimized()) window.restore(); window.show(); window.focus();}
    if (argv.includes('--mcp') && bridge) {
      try {await bridge.enable(); window.webContents.send('forma:mcp-status', bridge.status());}
      catch (error) {console.error('Forma MCP:', error.message);}
    }
  });
}
const recoveryPath = () => path.join(app.getPath('userData'), 'last-session.json');
async function checkpoint() {
  const snapshot = await window.webContents.executeJavaScript('(async()=>{if(!window.FormaSession)return null;await window.FormaSession.ready;return window.FormaSession.snapshot()})()');
  if (snapshot) {
    await fs.mkdir(app.getPath('userData'), {recursive: true});
    const temporary = recoveryPath() + '.' + crypto.randomUUID() + '.tmp';
    try {await fs.writeFile(temporary, JSON.stringify(snapshot), 'utf8'); await fs.rename(temporary, recoveryPath());}
    finally {await fs.rm(temporary, {force: true});}
  }
}
function dispatch(name, args) {
  if (!window || window.isDestroyed() || closing) return Promise.reject(Object.assign(new Error('The Forma editor is closing.'), {code: 'APP_UNAVAILABLE'}));
  return new Promise((resolve, reject) => {
    const requestId = crypto.randomUUID();
    const timer = setTimeout(() => {pending.delete(requestId); reject(Object.assign(new Error('The editor did not respond; inspect the scene before retrying.'), {code: 'EDITOR_TIMEOUT'}));}, 85000);
    pending.set(requestId, {resolve, reject, timer}); window.webContents.send('forma:command', {requestId, name, args});
  });
}
ipcMain.on('forma:reply', (event, response) => {
  if (event.sender !== window?.webContents || !response || typeof response.requestId !== 'string') return;
  const task = pending.get(response.requestId); if (!task) return;
  clearTimeout(task.timer); pending.delete(response.requestId);
  if (response.error) task.reject(Object.assign(new Error(response.error.message), {code: response.error.code})); else task.resolve(response.result);
});
ipcMain.handle('forma:mcp-status', () => bridge?.status() || {enabled: false});
ipcMain.handle('forma:mcp-toggle', async (event, enabled) => {
  if (event.sender !== window?.webContents || typeof enabled !== 'boolean') throw Error('Invalid bridge toggle.');
  return enabled ? bridge.enable() : bridge.disable();
});
ipcMain.handle('load-recovery', async () => {
  try {
    const stat = await fs.stat(recoveryPath());
    if (stat.size > 150 * 1024 * 1024) return null;
    return JSON.parse(await fs.readFile(recoveryPath(), 'utf8'));
  } catch {return null;}
});
app.whenReady().then(() => {
  window = new BrowserWindow({width: 1500, height: 980, minWidth: 1050, minHeight: 760,
    show: process.env.FORMA_TEST !== '1', backgroundColor: '#1b1e1f', autoHideMenuBar: true,
    title: 'Forma — Animation Studio',
    webPreferences: {preload: path.join(__dirname, 'preload.cjs'), contextIsolation: true, nodeIntegration: false, sandbox: true, backgroundThrottling: false}});
  bridge = createBridge({
    discoveryFile: process.env.FORMA_BRIDGE_FILE || path.join(app.getPath('userData'), 'mcp-bridge.json'),
    exportDirectory: process.env.FORMA_EXPORT_DIR || path.join(app.getPath('documents'), 'Forma Exports'), dispatch, checkpoint
  });
  window.webContents.setWindowOpenHandler(() => ({action: 'deny'}));
  window.webContents.on('will-navigate', event => event.preventDefault());
  window.webContents.session.on('will-download', (_, item) => {
    item.setSaveDialogOptions({title: 'Save Forma export', defaultPath: path.join(app.getPath('downloads'), path.basename(item.getFilename()))});
  });
  window.on('close', async event => {
    if (allowClose) return;
    event.preventDefault(); if (closing) return; closing = true;
    try {
      let timer;
      await Promise.race([checkpoint(), new Promise((_, reject) => {timer = setTimeout(() => reject(new Error('The editor did not respond.')), 5000);})]).finally(() => clearTimeout(timer));
      await bridge.disable();
      allowClose = true; window.close();
    } catch (error) {
      const result = await dialog.showMessageBox(window, {type: 'warning', title: 'Save before closing', message: 'The current animation could not be saved automatically.', detail: 'Choose Cancel to use Save project before closing.\n\n' + error.message, buttons: ['Cancel', 'Close without saving'], defaultId: 0, cancelId: 0});
      if (result.response === 1) {await bridge.disable(); allowClose = true; window.close();}
    } finally {closing = false;}
  });
  window.on('closed', () => {
    bridge.disable().catch(error => console.error('Forma MCP cleanup:', error.message));
    for (const task of pending.values()) {clearTimeout(task.timer); task.reject(Object.assign(new Error('Forma closed.'), {code: 'APP_UNAVAILABLE'}));}
    pending.clear();
  });
  window.webContents.once('did-finish-load', async () => {
    if (process.argv.includes('--mcp')) {
      try {await bridge.enable(); window.webContents.send('forma:mcp-status', bridge.status());}
      catch (error) {console.error('Forma MCP:', error.message);}
    }
    if (smokeTest) {
      try {
        const ok = await window.webContents.executeJavaScript('(async()=>{await window.FormaSession.ready;const snapshot=window.FormaSession.snapshot();return !!(snapshot&&snapshot.shapes.length>0&&typeof window.FormaSession.dispatch==="function"&&document.getElementById("mcp-control"))})()');
        if (!ok) throw Error('Packaged editor or MCP integration did not initialize');
        console.log('PASS: Packaged Forma desktop app starts and loads its editor and MCP controls');
        clearTimeout(smokeTimeout); app.exit(0);
      } catch (error) {console.error(error); clearTimeout(smokeTimeout); app.exit(1);}
    }
  });
  window.loadFile(path.join(__dirname, 'index.html'));
});
app.on('window-all-closed', () => app.quit());
app.on('before-quit', () => {if (bridge) bridge.disable().catch(error => console.error('Forma MCP:', error.message));});
const smokeTimeout = smokeTest ? setTimeout(() => {console.error('Packaged Forma startup timed out'); app.exit(1);}, 20000) : undefined;
