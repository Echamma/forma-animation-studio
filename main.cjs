const {app, BrowserWindow, ipcMain, dialog, protocol} = require('electron');
const fs = require('node:fs/promises');
const path = require('node:path');
const crypto = require('node:crypto');
const {createBridge} = require('./mcp/desktop-bridge.cjs');
const {createMediaStore} = require('./media-store.cjs');
protocol.registerSchemesAsPrivileged([{scheme: 'forma-asset', privileges: {standard: true, secure: true, supportFetchAPI: true, stream: true, corsEnabled: true}}]);
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
    openProjectFile(projectArgument(argv));
    if (argv.includes('--mcp') && bridge) {
      try {await bridge.enable(); window.webContents.send('forma:mcp-status', bridge.status());}
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
  try {window.webContents.send('forma:open-file', {name: path.basename(file), bytes: await fs.readFile(file)});}
  catch (error) {console.error('Forma could not open ' + file + ':', error.message);}
}
// Installed builds update themselves from GitHub Releases; the portable .exe and the .deb package are updated by hand.
function checkForUpdates() {
  if (!app.isPackaged || process.env.FORMA_TEST === '1' || process.env.PORTABLE_EXECUTABLE_DIR || (process.platform === 'linux' && !process.env.APPIMAGE)) return;
  try {require('electron-updater').autoUpdater.checkForUpdatesAndNotify().catch(error => console.error('Forma update:', error.message));}
  catch (error) {console.error('Forma update:', error.message);}
}
const recoveryPath = () => path.join(app.getPath('userData'), 'last-session.json');
const previousRecoveryPath = () => path.join(app.getPath('userData'), 'last-session.previous.json');
const defaultProjectPath = () => path.join(app.getPath('userData'), 'default-project.json');
const assetDirectory = () => path.join(app.getPath('userData'), 'assets');
const media = createMediaStore(() => path.join(app.getPath('userData'), 'media'));
const projectsDirectory = () => path.join(app.getPath('userData'), 'projects');
const assetFields = ['assetData', 'posterData', 'svgSource'];
async function readProject(file) {
  try {
    const project = JSON.parse(await fs.readFile(file, 'utf8')), media = new Map();
    for (const shape of project.shapes || []) for (const field of assetFields) {
      const token = shape[field + 'Ref'];
      if (!token) continue;
      if (!/^[0-9a-f-]{36}$/i.test(token)) throw Error('Invalid media reference.');
      if (!media.has(token)) media.set(token, await fs.readFile(path.join(assetDirectory(), token), 'utf8'));
      shape[field] = media.get(token);
    }
    return project;
  } catch {return null;}
}
async function replaceRecoveryFile(temporary) {
  for (let attempt = 0; ; attempt++) {
    try {await fs.rename(temporary, recoveryPath()); return;}
    catch (error) {
      if (!['EPERM', 'EACCES', 'EBUSY'].includes(error.code) || attempt >= 7) throw error;
      await new Promise(resolve => setTimeout(resolve, 40 * (attempt + 1)));
    }
  }
}
let saveQueue = Promise.resolve(), latestRevision = -1, backedUpPrevious = false;
function saveSnapshot({revision, projectJson, assets = []}) {
  if (!Number.isSafeInteger(revision) || revision < 0 || typeof projectJson !== 'string') throw Error('Invalid autosave data.');
  const project = JSON.parse(projectJson);
  if (project?.version !== 1 || !Array.isArray(project.shapes)) throw Error('Invalid Forma project.');
  if (!Array.isArray(assets) || assets.some(item => !/^[0-9a-f-]{36}$/i.test(item?.token) || typeof item.data !== 'string')) throw Error('Invalid media data.');
  if (revision < latestRevision) return Promise.resolve(false);
  latestRevision = revision;
  saveQueue = saveQueue.catch(() => {}).then(async () => {
    if (revision < latestRevision) return false;
    await fs.mkdir(app.getPath('userData'), {recursive: true});
    if (assets.length) await fs.mkdir(assetDirectory(), {recursive: true});
    for (const item of assets) {
      const destination = path.join(assetDirectory(), item.token);
      try {await fs.access(destination); continue;} catch {}
      const temporaryAsset = destination + '.' + crypto.randomUUID() + '.tmp';
      try {await fs.writeFile(temporaryAsset, item.data, 'utf8'); await fs.rename(temporaryAsset, destination);}
      finally {await fs.rm(temporaryAsset, {force: true});}
    }
    for (const shape of project.shapes) for (const field of assetFields) {
      const token = shape[field + 'Ref'];
      if (!token) continue;
      if (!/^[0-9a-f-]{36}$/i.test(token)) throw Error('Invalid media reference.');
      await fs.access(path.join(assetDirectory(), token));
    }
    if (!backedUpPrevious) {
      try {await fs.copyFile(recoveryPath(), previousRecoveryPath());} catch (error) {if (error.code !== 'ENOENT') throw error;}
      backedUpPrevious = true;
    }
    const temporary = recoveryPath() + '.' + crypto.randomUUID() + '.tmp';
    try {await fs.writeFile(temporary, projectJson, 'utf8'); await replaceRecoveryFile(temporary);}
    finally {await fs.rm(temporary, {force: true});}
    if (typeof project.projectId === 'string' && /^[0-9a-f-]{36}$/i.test(project.projectId)) {
      await fs.mkdir(projectsDirectory(), {recursive: true});
      const destination = path.join(projectsDirectory(), project.projectId + '.json');
      const projectTemporary = destination + '.' + crypto.randomUUID() + '.tmp';
      try {await fs.writeFile(projectTemporary, projectJson, 'utf8'); await fs.rename(projectTemporary, destination);}
      finally {await fs.rm(projectTemporary, {force: true});}
    }
    return true;
  });
  return saveQueue;
}
async function checkpoint() {
  const payload = await window.webContents.executeJavaScript('(async()=>{if(!window.FormaSession)return null;await window.FormaSession.ready;return window.FormaSession.autosavePayload()})()');
  if (payload) await saveSnapshot(payload);
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
ipcMain.handle('load-recovery', async () => await readProject(recoveryPath()) || await readProject(previousRecoveryPath()) || readProject(defaultProjectPath()));
ipcMain.handle('load-default', () => readProject(defaultProjectPath()));
ipcMain.handle('forma:list-projects', async event => {
  if (event.sender !== window?.webContents) throw Error('Unknown editor window.');
  let names; try {names = await fs.readdir(projectsDirectory());} catch (error) {if (error.code === 'ENOENT') return []; throw error;}
  const projects = [];
  for (const name of names) {
    if (!/^[0-9a-f-]{36}\.json$/i.test(name)) continue;
    try {
      const file = path.join(projectsDirectory(), name);
      const data = JSON.parse(await fs.readFile(file, 'utf8'));
      if (data.version === 1 && Array.isArray(data.shapes)) projects.push({id:name.slice(0,-5),name:typeof data.projectName === 'string' ? data.projectName : 'Untitled project',savedAt:(await fs.stat(file)).mtimeMs,layerCount:data.shapes.length});
    } catch {}
  }
  return projects.sort((a,b)=>b.savedAt-a.savedAt);
});
ipcMain.handle('forma:open-local-project', async (event,id) => {
  if (event.sender !== window?.webContents || typeof id !== 'string' || !/^[0-9a-f-]{36}$/i.test(id)) throw Error('Invalid project ID.');
  await saveQueue;
  return readProject(path.join(projectsDirectory(), id + '.json'));
});
ipcMain.handle('forma:store-media-file', (event, source, type) => {
  if (event.sender !== window?.webContents || typeof source !== 'string' || !path.isAbsolute(source)) throw Error('Invalid media file.');
  return media.storeFile(source, type);
});
ipcMain.handle('forma:store-media', (event, bytes, type) => {
  if (event.sender !== window?.webContents || !(bytes instanceof ArrayBuffer || ArrayBuffer.isView(bytes))) throw Error('Invalid media data.');
  return media.storeBytes(bytes, type);
});
ipcMain.handle('forma:autosave', (event, payload) => {
  if (event.sender !== window?.webContents) throw Error('Unknown editor window.');
  return saveSnapshot(payload);
});
app.whenReady().then(() => {
  window = new BrowserWindow({width: 1500, height: 980, minWidth: 1050, minHeight: 760,
    show: process.env.FORMA_TEST !== '1', backgroundColor: '#1b1e1f', autoHideMenuBar: true,
    title: 'Forma — Animation Studio',
    webPreferences: {preload: path.join(__dirname, 'preload.cjs'), contextIsolation: true, nodeIntegration: false, sandbox: true, backgroundThrottling: false,
      additionalArguments: process.env.FORMA_TEST === '1' ? ['--forma-test'] : []}});
  bridge = createBridge({
    discoveryFile: process.env.FORMA_BRIDGE_FILE || path.join(app.getPath('userData'), 'mcp-bridge.json'),
    exportDirectory: process.env.FORMA_EXPORT_DIR || path.join(app.getPath('documents'), 'Forma Exports'), dispatch, checkpoint,
    storeMedia: media.storeBytes
  });
  protocol.handle('forma-asset', request => media.respond(request));
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
      const result = await dialog.showMessageBox(window, {type: 'warning', title: 'Autosave failed', message: 'The current animation could not be saved automatically.', detail: 'Choose Cancel to keep Forma open and retry.\n\n' + error.message, buttons: ['Cancel', 'Close without saving'], defaultId: 0, cancelId: 0});
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
    openProjectFile(queuedProjectFile || projectArgument(process.argv));
    queuedProjectFile = null;
    checkForUpdates();
    if (smokeTest) {
      try {
        const pixel = await media.storeBytes(Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScL/nwAAAABJRU5ErkJggg==', 'base64'), 'image/png');
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
app.on('before-quit', () => {if (bridge) bridge.disable().catch(error => console.error('Forma MCP:', error.message));});
const smokeTimeout = smokeTest ? setTimeout(() => {console.error('Packaged Forma startup timed out'); app.exit(1);}, 20000) : undefined;
