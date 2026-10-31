const {app, BrowserWindow, ipcMain, dialog, protocol, session} = require('electron');
const path = require('node:path');
const formaHost = require('./app-host.cjs');
protocol.registerSchemesAsPrivileged(formaHost.schemes);
let window, host, allowClose = false, closing = false;
const smokeTest = process.argv.includes('--smoke-test');
if (smokeTest) {
  process.env.FORMA_TEST = '1';
  app.setPath('userData', path.join(app.getPath('temp'), 'forma-animation-studio-smoke-' + process.pid));
} else if (process.env.FORMA_TEST !== '1') {
  app.setPath('userData', path.join(app.getPath('appData'), 'forma-animation-studio'));
  if (!app.requestSingleInstanceLock()) {app.quit(); return;}
  app.on('second-instance', async (_, argv) => {
    if (window) {if (window.isMinimized()) window.restore(); window.show(); window.focus();}
    openProjectFile(projectArgument(argv));
    if (argv.includes('--mcp') && host) {
      try {await host.bridge.enable(); window.webContents.send('forma:mcp-status', host.bridge.status());}
      catch (error) {console.error('Forma MCP:', error.message);}
    }
  });
  app.on('open-file', (event, file) => {event.preventDefault(); openProjectFile(file);});
}
// A .forma or .json project opened from the file manager arrives as a command-line path (or open-file on macOS).
let queuedProjectFile = null;
function projectArgument(argv) {return argv.slice(1).find(arg => !arg.startsWith('-') && /\.(forma|json)$/i.test(arg));}
async function openProjectFile(file) {
  if (!file) return;
  if (!window || window.webContents.isLoading()) {queuedProjectFile = file; return;}
  try {await host.openProjectFile(file);}
  catch (error) {console.error('Forma could not open ' + file + ':', error.message);}
}
// Installed builds update themselves from GitHub Releases; the portable .exe and the .deb package are updated by hand.
function checkForUpdates() {
  if (!app.isPackaged || process.env.FORMA_TEST === '1' || process.env.PORTABLE_EXECUTABLE_DIR || (process.platform === 'linux' && !process.env.APPIMAGE)) return;
  try {require('electron-updater').autoUpdater.checkForUpdatesAndNotify().catch(error => console.error('Forma update:', error.message));}
  catch (error) {console.error('Forma update:', error.message);}
}
app.whenReady().then(() => {
  window = new BrowserWindow({width: 1500, height: 980, minWidth: 1050, minHeight: 760,
    show: process.env.FORMA_TEST !== '1', backgroundColor: '#1b1e1f', autoHideMenuBar: true,
    title: 'Forma — Animation Studio',
    webPreferences: {preload: path.join(__dirname, 'preload.cjs'), contextIsolation: true, nodeIntegration: false, sandbox: true, backgroundThrottling: false,
      additionalArguments: process.env.FORMA_TEST === '1' ? ['--forma-test'] : []}});
  host = formaHost.create({
    ipc: ipcMain, dataDir: app.getPath('userData'), session: session.defaultSession, webContents: () => window?.webContents,
    discoveryFile: process.env.FORMA_BRIDGE_FILE || path.join(app.getPath('userData'), 'mcp-bridge.json'),
    exportDirectory: process.env.FORMA_EXPORT_DIR || path.join(app.getPath('documents'), 'Forma Exports')
  });
  window.webContents.setWindowOpenHandler(() => ({action: 'deny'}));
  window.webContents.on('will-navigate', event => event.preventDefault());
  window.webContents.session.on('will-download', (_, item) => {
    item.setSaveDialogOptions({title: 'Save Forma export', defaultPath: path.join(app.getPath('downloads'), path.basename(item.getFilename()))});
  });
  window.on('close', async event => {
    if (allowClose) return;
    event.preventDefault(); if (closing) return; closing = true; host.setClosing(true);
    try {
      let timer;
      await Promise.race([host.checkpoint(), new Promise((_, reject) => {timer = setTimeout(() => reject(new Error('The editor did not respond.')), 5000);})]).finally(() => clearTimeout(timer));
      await host.bridge.disable();
      allowClose = true; window.close();
    } catch (error) {
      const result = await dialog.showMessageBox(window, {type: 'warning', title: 'Autosave failed', message: 'The current animation could not be saved automatically.', detail: 'Choose Cancel to keep Forma open and retry.\n\n' + error.message, buttons: ['Cancel', 'Close without saving'], defaultId: 0, cancelId: 0});
      if (result.response === 1) {await host.bridge.disable(); allowClose = true; window.close();}
    } finally {closing = false; host.setClosing(false);}
  });
  window.on('closed', () => {
    host.bridge.disable().catch(error => console.error('Forma MCP cleanup:', error.message));
    host.rejectPending();
  });
  window.webContents.once('did-finish-load', async () => {
    if (process.argv.includes('--mcp')) {
      try {await host.bridge.enable(); window.webContents.send('forma:mcp-status', host.bridge.status());}
      catch (error) {console.error('Forma MCP:', error.message);}
    }
    openProjectFile(queuedProjectFile || projectArgument(process.argv));
    queuedProjectFile = null;
    checkForUpdates();
    if (smokeTest) {
      try {
        const pixel = await host.media.storeBytes(Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScL/nwAAAABJRU5ErkJggg==', 'base64'), 'image/png');
        const ok = await window.webContents.executeJavaScript('(async()=>{await window.FormaSession.ready;const snapshot=window.FormaSession.snapshot();const media=await fetch(' + JSON.stringify(pixel) + ');return !!(snapshot&&snapshot.shapes.length>0&&typeof window.FormaSession.dispatch==="function"&&document.getElementById("mcp-control")&&media.ok)})()');
        if (!ok) throw Error('Packaged editor, media library, or MCP integration did not initialize');
        console.log('PASS: Packaged Forma desktop app starts and loads its editor, media library, and MCP controls');
        clearTimeout(smokeTimeout); app.exit(0);
      } catch (error) {console.error(error); clearTimeout(smokeTimeout); app.exit(1);}
    }
  });
  window.loadFile(path.join(__dirname, 'index.html'));
});
app.on('window-all-closed', () => app.quit());
app.on('before-quit', () => {if (host) host.bridge.disable().catch(error => console.error('Forma MCP:', error.message));});
const smokeTimeout = smokeTest ? setTimeout(() => {console.error('Packaged Forma startup timed out'); app.exit(1);}, 20000) : undefined;
