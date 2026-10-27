const http = require('node:http');
const fs = require('node:fs/promises');
const path = require('node:path');
const crypto = require('node:crypto');
const {manifest, validate, fail} = require('./validation.cjs');

function createBridge({discoveryFile, exportDirectory, dispatch, checkpoint, storeMedia}) {
  let server, token, generation = 0, queue = Promise.resolve();
  const exportRoot = path.resolve(exportDirectory);
  const status = () => ({enabled: !!server?.listening, exportDirectory: exportRoot});
  async function localFile(file, extensions) {
    if (!path.isAbsolute(file)) throw fail('INVALID_PATH', 'Use an absolute local path.');
    if (!extensions.includes(path.extname(file).toLowerCase())) throw fail('UNSUPPORTED_FILE', 'Supported file extensions: ' + extensions.join(', '));
    const handle = await fs.open(file, 'r');
    try {
      const stat = await handle.stat();
      if (!stat.isFile()) throw fail('INVALID_FILE', 'Path must point to a regular file.');
      return await handle.readFile();
    } finally { await handle.close(); }
  }
  async function invoke(name, args) {
    validate(name, args);
    let payload = {...args};
    if (name === 'forma_open_project') {
      const bytes = await localFile(args.file, ['.json']);
      try { payload.project = JSON.parse(bytes.toString('utf8')); }
      catch { throw fail('INVALID_PROJECT', 'This file is not valid JSON.'); }
    }
    if (name === 'forma_import_asset') {
      const bytes = await localFile(args.file, ['.png', '.jpg', '.jpeg', '.webp', '.svg']);
      const ext = path.extname(args.file).toLowerCase();
      payload.name ||= path.basename(args.file).slice(0, 99);
      payload.assetType = ext === '.svg' ? 'svg' : 'image';
      if (ext === '.svg') {
        payload.svgSource = bytes.toString('utf8');
      } else {
        const png = bytes.subarray(0, 8).equals(Buffer.from([137,80,78,71,13,10,26,10]));
        const jpeg = bytes[0] === 255 && bytes[1] === 216 && bytes[2] === 255;
        const webp = bytes.toString('ascii', 0, 4) === 'RIFF' && bytes.toString('ascii', 8, 12) === 'WEBP';
        if (!(ext === '.png' ? png : ext === '.webp' ? webp : jpeg)) throw fail('INVALID_ASSET', 'File content does not match its image extension.');
        const mime = ext === '.png' ? 'image/png' : ext === '.webp' ? 'image/webp' : 'image/jpeg';
        payload.assetData = storeMedia ? await storeMedia(bytes, mime) : 'data:' + mime + ';base64,' + bytes.toString('base64');
      }
    }
    let destination;
    if (name === 'forma_export') {
      const extension = {png: '.png', svg: '.svg', animated_svg: '.svg', mp4: '.mp4', webm: '.webm', project: '.json'}[args.format];
      if (path.basename(args.filename) !== args.filename || args.filename.endsWith(' ') || args.filename.endsWith('.') || /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(args.filename) || path.extname(args.filename).toLowerCase() !== extension) {
        throw fail('INVALID_FILENAME', 'Use a simple filename ending in ' + extension + '.');
      }
      await fs.mkdir(exportRoot, {recursive: true});
      destination = path.join(exportRoot, args.filename);
      if (!args.overwrite) {
        try { await fs.lstat(destination); throw fail('FILE_EXISTS', 'Export already exists; choose a new name or set overwrite=true.'); }
        catch (error) { if (error.code !== 'ENOENT') throw error; }
      }
    }
    const result = await dispatch(name, payload);
    if (!manifest.tools.find(t => t.name === name).annotations.readOnlyHint && name !== 'forma_export' && name !== 'forma_set_playback') await checkpoint();
    if (destination) {
      const bytes = Buffer.from(result.data, 'base64');
      if (args.overwrite) {
        const temporary = path.join(exportRoot, '.forma-' + crypto.randomUUID() + '.tmp');
        try { await fs.writeFile(temporary, bytes, {flag: 'wx'}); await fs.rename(temporary, destination); }
        finally { await fs.rm(temporary, {force: true}); }
      } else await fs.writeFile(destination, bytes, {flag: 'wx'});
      return {path: destination, bytes: bytes.length, mimeType: result.mimeType};
    }
    if (name === 'forma_get_scene') result.connection = status();
    return result;
  }
  async function enable() {
    if (server) return status();
    token = crypto.randomBytes(32).toString('hex');
    const activeGeneration = ++generation;
    const activeServer = http.createServer(async (req, res) => {
      const send = (code, body) => { if (!res.destroyed) { res.writeHead(code, {'Content-Type': 'application/json', 'Cache-Control': 'no-store'}); res.end(JSON.stringify(body)); } };
      try {
        const host = '127.0.0.1:' + activeServer.address()?.port;
        if (req.headers.host !== host || req.headers.origin) throw fail('FORBIDDEN', 'Only authenticated local MCP clients are accepted.');
        const supplied = Buffer.from(req.headers.authorization || '');
        const expected = Buffer.from('Bearer ' + token);
        if (supplied.length !== expected.length || !crypto.timingSafeEqual(supplied, expected)) throw fail('UNAUTHORIZED', 'Local bridge token is missing or invalid.');
        if (req.method !== 'POST' || !req.url.startsWith('/tools/') || req.headers['content-type'] !== 'application/json') throw fail('INVALID_REQUEST', 'Expected a JSON tool request.');
        let size = 0; const chunks = [];
        for await (const chunk of req) { size += chunk.length; if (size > 1024 * 1024) throw fail('REQUEST_TOO_LARGE', 'Tool arguments exceed 1 MB.'); chunks.push(chunk); }
        let args;
        try { args = JSON.parse(Buffer.concat(chunks).toString('utf8')); } catch { throw fail('INVALID_ARGUMENTS', 'Expected valid JSON arguments.'); }
        const name = req.url.slice('/tools/'.length);
        validate(name, args);
        const task = queue.then(() => {
          if (activeGeneration !== generation || !server) throw fail('DISABLED', 'MCP control has been turned off.');
          return invoke(name, args);
        });
        queue = task.catch(() => {});
        send(200, {result: await task});
      } catch (error) {
        send(error.code === 'UNAUTHORIZED' ? 401 : error.code === 'FORBIDDEN' ? 403 : 400, {error: {code: typeof error.code === 'string' ? error.code : 'TOOL_ERROR', message: error.message}});
      }
    });
    activeServer.requestTimeout = 100000;
    activeServer.headersTimeout = 10000;
    try {
      await new Promise((resolve, reject) => { activeServer.once('error', reject); activeServer.listen(0, '127.0.0.1', resolve); });
      server = activeServer;
      await fs.mkdir(path.dirname(discoveryFile), {recursive: true});
      const temporary = discoveryFile + '.' + process.pid + '.tmp';
      await fs.writeFile(temporary, JSON.stringify({version: 1, pid: process.pid, port: server.address().port, token}), {mode: 0o600});
      await fs.rename(temporary, discoveryFile);
      await fs.chmod(discoveryFile, 0o600);
      return status();
    } catch (error) { await disable(); activeServer.close(); throw error; }
  }
  async function disable() {
    generation++; const previous = server; server = undefined;
    if (previous) { previous.close(); previous.closeIdleConnections(); }
    try {
      const discovery = JSON.parse(await fs.readFile(discoveryFile, 'utf8'));
      if (discovery.pid === process.pid && discovery.token === token) await fs.rm(discoveryFile, {force: true});
    } catch (error) { if (error.code !== 'ENOENT') console.error('Could not remove Forma discovery file:', error.message); }
    return status();
  }
  return {enable, disable, status};
}
module.exports = {createBridge};
