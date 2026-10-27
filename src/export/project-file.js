// .forma project files: a zip of project.json plus every image and video it uses, so a project can move to another
// computer intact. Media is stored uncompressed because it is already compressed.
import {strFromU8, strToU8, unzipSync, zipSync} from 'fflate';
import {mediaBlob, mediaTypeOf, storeMedia} from '../media-library.js';
import {editor} from '../state.js';

const MEDIA_FIELDS = ['assetData', 'posterData'];
const EXTENSIONS = {'image/png': 'png', 'image/jpeg': 'jpg', 'image/webp': 'webp', 'video/mp4': 'mp4', 'video/webm': 'webm', 'video/ogg': 'ogv'};

async function digest(bytes) {
  const hash = new Uint8Array(await crypto.subtle.digest('SHA-256', bytes));
  return [...hash].map(byte => byte.toString(16).padStart(2, '0')).join('');
}

export async function packProjectFile(scene = editor.scene) {
  const files = {},
    names = new Map(),
    project = {...scene, shapes: scene.shapes.map(shape => ({...shape}))};
  for (const shape of project.shapes)
    for (const field of MEDIA_FIELDS) {
      const value = shape[field];
      if (typeof value !== 'string') continue;
      if (!names.has(value)) {
        const blob = await mediaBlob(value),
          bytes = new Uint8Array(await blob.arrayBuffer()),
          name = `media/${await digest(bytes)}.${EXTENSIONS[blob.type] || 'bin'}`;
        files[name] = [bytes, {level: 0}];
        names.set(value, name);
      }
      shape[field] = names.get(value);
    }
  files['project.json'] = strToU8(JSON.stringify(project, null, 2));
  return new Blob([zipSync(files)], {type: 'application/zip'});
}

// Media from the archive goes into the desktop library, or becomes data URLs in the browser preview.
export async function unpackProjectFile(bytes) {
  const files = unzipSync(bytes);
  if (!files['project.json']) throw Error('This .forma file has no project.json');
  const project = JSON.parse(strFromU8(files['project.json'])),
    stored = new Map();
  for (const shape of Array.isArray(project.shapes) ? project.shapes : [])
    for (const field of MEDIA_FIELDS) {
      const name = shape[field];
      if (typeof name !== 'string' || !name.startsWith('media/')) continue;
      if (!files[name]) throw Error(`This .forma file is missing ${name}`);
      if (!stored.has(name)) {
        const file = new File([files[name]], name.slice(6), {type: mediaTypeOf({name})});
        stored.set(name, storeMedia(file, file.type));
      }
      shape[field] = await stored.get(name);
    }
  return project;
}
