// Where imported media lives. The desktop app keeps files in its media library and layers point at forma-asset:// URLs;
// the browser preview has no library and embeds data URLs instead. Exports always embed media so they work anywhere.
import {editor} from './state.js';

const TYPES = {png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg', webp: 'image/webp', mp4: 'video/mp4', webm: 'video/webm', ogv: 'video/ogg'};
export const LIBRARY_PATTERN = /forma-asset:\/\/media\/[0-9a-f]{64}\.[a-z0-9]+/g;
const desktopLibrary = () => window.formaDesktop?.storeMedia;

export const mediaTypeOf = file => (TYPES[file.type?.split('/')[1]] && file.type) || TYPES[file.name?.split('.').pop().toLowerCase()];

export function blobToDataUrl(blob) {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result);
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(blob);
  });
}

export function storeMedia(file, type = mediaTypeOf(file)) {
  return desktopLibrary() ? window.formaDesktop.storeMedia(file, type) : blobToDataUrl(file);
}

export const mediaBlob = url => fetch(url).then(response => response.blob());

// Replace every library URL in exported text (SVG markup or project JSON) with an embedded data URL.
export async function inlineMedia(text) {
  for (const url of new Set(text.match(LIBRARY_PATTERN) || [])) text = text.split(url).join(await blobToDataUrl(await mediaBlob(url)));
  return text;
}

// Move embedded data URLs from older projects into the media library, so they no longer sit in memory, undo
// snapshots, and autosaves. Clips that share a picture share one stored file.
export async function migrateMedia(scene = editor.scene) {
  if (!desktopLibrary()) return 0;
  const converted = new Map();
  let changed = 0;
  for (const s of scene.shapes)
    for (const field of ['assetData', 'posterData']) {
      const value = s[field];
      if (typeof value !== 'string' || !value.startsWith('data:')) continue;
      if (!converted.has(value))
        converted.set(
          value,
          mediaBlob(value).then(blob => storeMedia(new File([blob], 'media', {type: blob.type}), blob.type))
        );
      s[field] = await converted.get(value);
      changed++;
    }
  return changed;
}
