// The editor's main-process side: autosave, the project list, the media library, and the MCP bridge. The standalone
// app (main.cjs) and Canopy, which shows Forma in one of its panes, both create it; everything that depends on where
// Forma runs (data folder, session, window, IPC) comes in through `create`.
const fs = require('node:fs/promises');
const path = require('node:path');
const crypto = require('node:crypto');
const {createBridge} = require('./mcp/desktop-bridge.cjs');
const {createMediaStore, MEDIA_TYPES} = require('./media-store.cjs');

const schemes = [{scheme: 'forma-asset', privileges: {standard: true, secure: true, supportFetchAPI: true, stream: true, corsEnabled: true}}];
const assetFields = ['assetData', 'posterData', 'svgSource'];
const ID = /^[0-9a-f-]{36}$/i;
const TYPE_BY_EXTENSION = {...Object.fromEntries(Object.entries(MEDIA_TYPES)), jpeg: 'image/jpeg', svg: 'image/svg+xml'};

// The projects autosaved in a data folder, newest first.
async function listProjects(dataDir) {
  const directory = path.join(dataDir, 'projects');
  let names; try {names = await fs.readdir(directory);} catch (error) {if (error.code === 'ENOENT') return []; throw error;}
  const projects = [];
  for (const name of names) {
    if (!/^[0-9a-f-]{36}\.json$/i.test(name)) continue;
    try {
      const file = path.join(directory, name);
      const data = JSON.parse(await fs.readFile(file, 'utf8'));
      if (data.version === 1 && Array.isArray(data.shapes)) projects.push({id:name.slice(0,-5),name:typeof data.projectName === 'string' ? data.projectName : 'Untitled project',savedAt:(await fs.stat(file)).mtimeMs,layerCount:data.shapes.length});
    } catch {}
  }
  return projects.sort((a,b)=>b.savedAt-a.savedAt);
}
// For Canopy's sidebar, which lists a space's projects before Forma is opened.
async function documents({dataDir}) {
  return (await listProjects(dataDir)).map(({id, name, savedAt, layerCount}) => ({id, name, savedAt, detail: `${layerCount} layer${layerCount === 1 ? '' : 's'}`}));
}
// The editor is bundled from src/ by npm run bundle; a checkout that was never bundled can't be opened.
const check = () => require('node:fs').existsSync(path.join(__dirname, 'build', 'renderer.js')) ? null : `Forma's editor isn't built yet: run "npm run bundle" in ${__dirname}.`;

// ipc: ipcMain or Canopy's per-app router. session: the editor's session (for forma-asset://). webContents: a getter
// for the editor page. canopy: present only inside Canopy ({space, saveFile}).
function create({ipc, dataDir, session, webContents, exportDirectory, discoveryFile, canopy = null}) {
  const recoveryPath = () => path.join(dataDir, 'last-session.json');
  const previousRecoveryPath = () => path.join(dataDir, 'last-session.previous.json');
  const defaultProjectPath = () => path.join(dataDir, 'default-project.json');
  const assetDirectory = () => path.join(dataDir, 'assets');
  const projectsDirectory = () => path.join(dataDir, 'projects');
  const media = createMediaStore(() => path.join(dataDir, 'media'));
  const pending = new Map();
  const editor = () => {const contents = webContents(); return contents && !contents.isDestroyed() ? contents : null;};
  const fromEditor = event => event.sender === editor();
  let closing = false;

  async function readProject(file) {
    try {
      const project = JSON.parse(await fs.readFile(file, 'utf8')), media = new Map();
      for (const shape of project.shapes || []) for (const field of assetFields) {
        const token = shape[field + 'Ref'];
        if (!token) continue;
        if (!ID.test(token)) throw Error('Invalid media reference.');
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
    if (!Array.isArray(assets) || assets.some(item => !ID.test(item?.token) || typeof item.data !== 'string')) throw Error('Invalid media data.');
    if (revision < latestRevision) return Promise.resolve(false);
    latestRevision = revision;
    saveQueue = saveQueue.catch(() => {}).then(async () => {
      if (revision < latestRevision) return false;
      await fs.mkdir(dataDir, {recursive: true});
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
        if (!ID.test(token)) throw Error('Invalid media reference.');
        await fs.access(path.join(assetDirectory(), token));
      }
      if (!backedUpPrevious) {
        try {await fs.copyFile(recoveryPath(), previousRecoveryPath());} catch (error) {if (error.code !== 'ENOENT') throw error;}
        backedUpPrevious = true;
      }
      const temporary = recoveryPath() + '.' + crypto.randomUUID() + '.tmp';
      try {await fs.writeFile(temporary, projectJson, 'utf8'); await replaceRecoveryFile(temporary);}
      finally {await fs.rm(temporary, {force: true});}
      if (typeof project.projectId === 'string' && ID.test(project.projectId)) {
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
    const contents = editor();
    if (!contents) return;
    const payload = await contents.executeJavaScript('(async()=>{if(!window.FormaSession)return null;await window.FormaSession.ready;return window.FormaSession.autosavePayload()})()');
    if (payload) await saveSnapshot(payload);
  }
  function dispatch(name, args) {
    const contents = editor();
    if (!contents || closing) return Promise.reject(Object.assign(new Error('The Forma editor is closing.'), {code: 'APP_UNAVAILABLE'}));
    return new Promise((resolve, reject) => {
      const requestId = crypto.randomUUID();
      const timer = setTimeout(() => {pending.delete(requestId); reject(Object.assign(new Error('The editor did not respond; inspect the scene before retrying.'), {code: 'EDITOR_TIMEOUT'}));}, 85000);
      pending.set(requestId, {resolve, reject, timer}); contents.send('forma:command', {requestId, name, args});
    });
  }
  const bridge = createBridge({discoveryFile, exportDirectory, dispatch, checkpoint, storeMedia: media.storeBytes});

  ipc.on('forma:reply', (event, response) => {
    if (!fromEditor(event) || !response || typeof response.requestId !== 'string') return;
    const task = pending.get(response.requestId); if (!task) return;
    clearTimeout(task.timer); pending.delete(response.requestId);
    if (response.error) task.reject(Object.assign(new Error(response.error.message), {code: response.error.code})); else task.resolve(response.result);
  });
  ipc.handle('forma:mcp-status', () => bridge.status());
  ipc.handle('forma:mcp-toggle', async (event, enabled) => {
    if (!fromEditor(event) || typeof enabled !== 'boolean') throw Error('Invalid bridge toggle.');
    return enabled ? bridge.enable() : bridge.disable();
  });
  ipc.handle('load-recovery', async () => await readProject(recoveryPath()) || await readProject(previousRecoveryPath()) || readProject(defaultProjectPath()));
  ipc.handle('load-default', () => readProject(defaultProjectPath()));
  ipc.handle('forma:list-projects', event => {
    if (!fromEditor(event)) throw Error('Unknown editor window.');
    return listProjects(dataDir);
  });
  ipc.handle('forma:open-local-project', async (event,id) => {
    if (!fromEditor(event) || typeof id !== 'string' || !ID.test(id)) throw Error('Invalid project ID.');
    await saveQueue;
    return readProject(path.join(projectsDirectory(), id + '.json'));
  });
  ipc.handle('forma:store-media-file', (event, source, type) => {
    if (!fromEditor(event) || typeof source !== 'string' || !path.isAbsolute(source)) throw Error('Invalid media file.');
    return media.storeFile(source, type);
  });
  ipc.handle('forma:store-media', (event, bytes, type) => {
    if (!fromEditor(event) || !(bytes instanceof ArrayBuffer || ArrayBuffer.isView(bytes))) throw Error('Invalid media data.');
    return media.storeBytes(bytes, type);
  });
  ipc.handle('forma:autosave', (event, payload) => {
    if (!fromEditor(event)) throw Error('Unknown editor window.');
    return saveSnapshot(payload);
  });
  session.protocol.handle('forma-asset', request => media.respond(request));

  // Opens a .forma or .json project from disk in the editor (the file manager, the command line, or Canopy).
  async function openProjectFile(file) {
    const contents = editor();
    if (!contents) return false;
    contents.send('forma:open-file', {name: path.basename(file), bytes: await fs.readFile(file)});
    return true;
  }
  // Files handed over by Canopy: projects open, pictures and videos are copied into the media library by path (so
  // large videos never pass through the editor's memory) and arrive in the editor as layers.
  async function receive(files) {
    const items = [], skipped = [];
    for (const file of files) {
      const extension = path.extname(file.path).slice(1).toLowerCase(), type = TYPE_BY_EXTENSION[extension];
      try {
        if (extension === 'forma' || extension === 'json') await openProjectFile(file.path);
        else if (type === 'image/svg+xml') items.push({name: file.name, type, text: await fs.readFile(file.path, 'utf8')});
        else if (type) items.push({name: file.name, type, url: await media.storeFile(file.path, type)});
        else skipped.push(file.name);
      } catch {skipped.push(file.name);}
    }
    if (items.length) editor()?.send('forma:canopy-import', items);
    return {accepted: files.length - skipped.length, skipped};
  }
  async function openDocument(id) {
    if (typeof id !== 'string' || !ID.test(id)) throw Error('Invalid project ID.');
    editor()?.send('forma:canopy-open-project', id);
  }
  function rejectPending() {
    for (const task of pending.values()) {clearTimeout(task.timer); task.reject(Object.assign(new Error('Forma closed.'), {code: 'APP_UNAVAILABLE'}));}
    pending.clear();
  }
  async function dispose() {
    rejectPending();
    for (const channel of ['forma:mcp-status', 'forma:mcp-toggle', 'load-recovery', 'load-default', 'forma:list-projects', 'forma:open-local-project', 'forma:store-media-file', 'forma:store-media', 'forma:autosave']) ipc.removeHandler(channel);
    ipc.removeAllListeners('forma:reply');
    if (session.protocol.isProtocolHandled('forma-asset')) session.protocol.unhandle('forma-asset');
    await bridge.disable();
  }

  return {
    preload: path.join(__dirname, 'preload.cjs'),
    entry: path.join(__dirname, 'index.html'),
    additionalArguments: canopy ? ['--canopy=' + JSON.stringify({space: canopy.space.name})] : [],
    bridge, media, checkpoint, saveSnapshot, openProjectFile, rejectPending,
    setClosing: value => {closing = value;},
    receive, listDocuments: () => documents({dataDir}), openDocument, dispose
  };
}

module.exports = {id: 'forma', name: 'Forma', accent: 'clay', schemes, check, documents, create};
