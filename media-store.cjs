// Content-addressed media library. Imported images and videos are stored once as files named by their SHA-256 and
// served to the editor through forma-asset://media/<name>, with byte ranges so video can seek.
const fs = require('node:fs/promises');
const {createReadStream} = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const {Readable} = require('node:stream');

const MEDIA_TYPES = {png: 'image/png', jpg: 'image/jpeg', webp: 'image/webp', mp4: 'video/mp4', webm: 'video/webm', ogv: 'video/ogg'};
const EXTENSIONS = Object.fromEntries(Object.entries(MEDIA_TYPES).map(([extension, type]) => [type, extension]));
const NAME = /^[0-9a-f]{64}\.(png|jpg|webp|mp4|webm|ogv)$/;

function createMediaStore(directory) {
  const extension = type => {
    if (!EXTENSIONS[type]) throw Object.assign(Error('Unsupported media type: ' + type), {code: 'UNSUPPORTED_FILE'});
    return EXTENSIONS[type];
  };
  const urlFor = name => 'forma-asset://media/' + name;
  async function commit(name, write) {
    const file = path.join(directory(), name);
    try {await fs.access(file); return;} catch {}
    await fs.mkdir(directory(), {recursive: true});
    const temporary = file + '.' + crypto.randomUUID() + '.tmp';
    try {await write(temporary); await fs.rename(temporary, file);}
    finally {await fs.rm(temporary, {force: true});}
  }
  async function storeBytes(bytes, type) {
    const buffer = Buffer.from(bytes), name = crypto.createHash('sha256').update(buffer).digest('hex') + '.' + extension(type);
    await commit(name, temporary => fs.writeFile(temporary, buffer));
    return urlFor(name);
  }
  // Copies a file the user picked without loading it into memory, so very large videos import quickly.
  async function storeFile(source, type) {
    const ext = extension(type);
    if (!(await fs.stat(source)).isFile()) throw Error('Media must be a regular file.');
    const hash = crypto.createHash('sha256');
    for await (const chunk of createReadStream(source)) hash.update(chunk);
    const name = hash.digest('hex') + '.' + ext;
    await commit(name, temporary => fs.copyFile(source, temporary));
    return urlFor(name);
  }
  async function respond(request) {
    const url = new URL(request.url), name = url.pathname.slice(1);
    if (url.hostname !== 'media' || !NAME.test(name)) return new Response('Not found', {status: 404});
    const file = path.join(directory(), name);
    let size;
    try {size = (await fs.stat(file)).size;} catch {return new Response('Not found', {status: 404});}
    const headers = {'Content-Type': MEDIA_TYPES[name.split('.')[1]], 'Accept-Ranges': 'bytes', 'Access-Control-Allow-Origin': '*', 'Cache-Control': 'private, max-age=31536000, immutable'};
    const range = /^bytes=(\d*)-(\d*)$/.exec(request.headers.get('range') || '');
    if (!range || (!range[1] && !range[2])) return new Response(Readable.toWeb(createReadStream(file)), {headers: {...headers, 'Content-Length': String(size)}});
    const start = range[1] ? Number(range[1]) : Math.max(0, size - Number(range[2])),
      end = range[1] && range[2] ? Math.min(Number(range[2]), size - 1) : size - 1;
    if (start >= size || start > end) return new Response(null, {status: 416, headers: {...headers, 'Content-Range': `bytes */${size}`}});
    return new Response(Readable.toWeb(createReadStream(file, {start, end})), {status: 206,
      headers: {...headers, 'Content-Range': `bytes ${start}-${end}/${size}`, 'Content-Length': String(end - start + 1)}});
  }
  return {storeBytes, storeFile, respond};
}
module.exports = {createMediaStore, MEDIA_TYPES};
