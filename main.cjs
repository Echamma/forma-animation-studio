const { app, BrowserWindow, ipcMain, dialog } = require('electron');
const fs = require('node:fs/promises');
const path = require('node:path');
let window;
let closing = false;
let allowClose = false;
const recoveryPath = () => path.join(app.getPath('userData'), 'last-session.json');
app.whenReady().then(() => {
  window = new BrowserWindow({ width: 1500, height: 980, minWidth: 1050, minHeight: 760, show: process.env.FORMA_TEST !== '1', backgroundColor: '#1b1e1f', autoHideMenuBar: true, title: 'Forma — Animation Studio', webPreferences: { preload: path.join(__dirname, 'preload.cjs'), contextIsolation: true, nodeIntegration: false, sandbox: true } });
  window.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
  window.webContents.on('will-navigate', event => event.preventDefault());
  window.webContents.session.on('will-download', (_, item) => {
    item.setSaveDialogOptions({ title: 'Save Forma export', defaultPath: path.join(app.getPath('downloads'), path.basename(item.getFilename())) });
  });
  window.on('close', async event => {
    if (allowClose) return;
    event.preventDefault();
    if (closing) return;
    closing = true;
    try {
      let timer;
      const snapshot = await Promise.race([
        window.webContents.executeJavaScript('window.FormaSession ? window.FormaSession.snapshot() : null'),
        new Promise((_, reject) => { timer = setTimeout(() => reject(new Error('The editor did not respond.')), 5000); })
      ]).finally(() => clearTimeout(timer));
      if (snapshot) {
        const temporary = recoveryPath() + '.' + process.pid + '.tmp';
        await fs.writeFile(temporary, JSON.stringify(snapshot), 'utf8');
        await fs.rename(temporary, recoveryPath());
      }
      allowClose = true; window.close();
    } catch (error) {
      const result = await dialog.showMessageBox(window, { type: 'warning', title: 'Save before closing', message: 'The current animation could not be saved automatically.', detail: 'Choose Cancel to use Save project before closing.\n\n' + error.message, buttons: ['Cancel', 'Close without saving'], defaultId: 0, cancelId: 0 });
      if (result.response === 1) { allowClose = true; window.close(); }
    } finally { closing = false; }
  });
  window.loadFile(path.join(__dirname, 'index.html'));
});
app.on('window-all-closed', () => app.quit());
if (process.argv.includes('--smoke-test')) {
  process.env.FORMA_TEST = '1';
  app.setPath('userData', path.join(app.getPath('temp'), 'forma-animation-studio-smoke-' + process.pid));
  const timeout = setTimeout(() => { console.error('Packaged Forma startup timed out'); app.exit(1); }, 20000);
  app.whenReady().then(() => window.webContents.once('did-finish-load', async () => {
    try {
      const ok = await window.webContents.executeJavaScript('(() => { const snapshot = window.FormaSession && window.FormaSession.snapshot(); return snapshot && snapshot.shapes.length > 0; })()');
      if (!ok) throw new Error('Packaged editor did not initialize');
      console.log('PASS: Packaged Forma desktop app starts and loads its editor');
      clearTimeout(timeout); app.exit(0);
    } catch (error) { console.error(error); clearTimeout(timeout); app.exit(1); }
  }));
}
ipcMain.handle('load-recovery', async () => {
  try {
    const stat = await fs.stat(recoveryPath());
    if (stat.size > 150 * 1024 * 1024) return null;
    return JSON.parse(await fs.readFile(recoveryPath(), 'utf8'));
  } catch (_) { return null; }
});
