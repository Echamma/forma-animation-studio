import {$, toast} from '../dom.js';
import {mediaBlob, storeMedia} from '../media-library.js';
import {MAX_LAYERS} from '../model/validate.js';
import {arrangeAsPoses} from '../model/clips.js';
import {recordSceneChange} from '../persistence.js';
import {stop} from '../playback.js';
import {editor} from '../state.js';
import {sanitizeSvg} from '../svg/sanitize.js';
import {refresh} from './refresh.js';
import {closeDrawer} from './tabs.js';

function imageInfo(src) {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.crossOrigin = 'anonymous';
    img.onload = () => resolve({width: img.naturalWidth, height: img.naturalHeight});
    img.onerror = reject;
    img.src = src;
  });
}
function videoInfo(src) {
  return new Promise((resolve, reject) => {
    const v = document.createElement('video');
    v.crossOrigin = 'anonymous';
    v.muted = true;
    v.preload = 'auto';
    v.src = src;
    v.onloadedmetadata = async () => {
      // Recordings made by browsers often omit their length; seeking far past the end makes the decoder find it.
      if (!Number.isFinite(v.duration)) {
        await new Promise(resolve => {
          const done = () => Number.isFinite(v.duration) && resolve();
          v.ondurationchange = done;
          setTimeout(resolve, 3000);
          v.currentTime = 1e9;
        });
        v.ondurationchange = null;
        if (!Number.isFinite(v.duration)) return reject(Error('The video length could not be read'));
      }
      const capture = () => {
        try {
          const max = 720,
            scale = Math.min(1, max / Math.max(v.videoWidth, v.videoHeight)),
            c = document.createElement('canvas');
          c.width = Math.max(1, Math.round(v.videoWidth * scale));
          c.height = Math.max(1, Math.round(v.videoHeight * scale));
          c.getContext('2d').drawImage(v, 0, 0, c.width, c.height);
          resolve({
            width: v.videoWidth,
            height: v.videoHeight,
            duration: v.duration,
            poster: c.toDataURL('image/jpeg', 0.82)
          });
        } catch (error) {
          reject(error);
        }
      };
      v.onseeked = capture;
      v.currentTime = Math.min(0.05, Math.max(0, v.duration / 10));
      if (v.currentTime === 0) setTimeout(capture, 50);
    };
    v.onerror = reject;
    v.load();
  });
}
function svgInfo(source) {
  const doc = new DOMParser().parseFromString(source, 'image/svg+xml'),
    root = doc.documentElement,
    box = (root.getAttribute('viewBox') || '').trim().split(/[ ,]+/).map(Number),
    width = parseFloat(root.getAttribute('width')),
    height = parseFloat(root.getAttribute('height'));
  return {
    width: Number.isFinite(width) && width > 0 ? width : Number.isFinite(box[2]) && box[2] > 0 ? box[2] : 300,
    height: Number.isFinite(height) && height > 0 ? height : Number.isFinite(box[3]) && box[3] > 0 ? box[3] : 300
  };
}
function fittedAssetSize(width, height) {
  const max = Math.min(editor.scene.width, editor.scene.height) * 0.38,
    ratio = Math.max(0.05, Math.min(20, width / height));
  return ratio >= 1 ? {w: max, h: max / ratio} : {w: max * ratio, h: max};
}
// A picked, dropped or pasted file. Canopy hands over sources whose media is already in the library (see canopy.js).
const fileSource = file => ({name: file.name, type: file.type, text: () => file.text(), store: () => storeMedia(file)});
export const importMediaFiles = files => importMediaSources([...files].map(fileSource));
export async function importMediaSources(sources) {
  const errors = [],
    layers = [];
  let added = 0;
  stop();
  for (const file of sources) {
    try {
      if (editor.scene.shapes.length >= MAX_LAYERS) throw Error(`Scene is full (${MAX_LAYERS} layers)`);
      const id = crypto.randomUUID(),
        name = file.name.replace(/\.[^.]+$/, '').slice(0, 70) || 'Media';
      let shape;
      if (file.type === 'image/svg+xml' || /\.svg$/i.test(file.name)) {
        const source = sanitizeSvg(await file.text()),
          info = svgInfo(source),
          size = fittedAssetSize(info.width, info.height);
        shape = {
          id,
          name,
          type: 'svg',
          svgSource: source,
          color: '#888888',
          ease: 'smooth',
          keys: [{t: 0, x: editor.scene.width / 2, y: editor.scene.height / 2, w: size.w, h: size.h, r: 0, o: 100}]
        };
      } else if (['video/mp4', 'video/webm', 'video/ogg'].includes(file.type) || /\.(mp4|webm|ogv)$/i.test(file.name)) {
        const data = await file.store(),
          info = await videoInfo(data),
          size = fittedAssetSize(info.width, info.height),
          poster = await mediaBlob(info.poster);
        shape = {
          id,
          name,
          type: 'video',
          assetData: data,
          posterData: await storeMedia(new File([poster], 'poster.jpg', {type: 'image/jpeg'})),
          mediaDuration: info.duration,
          color: '#888888',
          ease: 'smooth',
          keys: [{t: 0, x: editor.scene.width / 2, y: editor.scene.height / 2, w: size.w, h: size.h, r: 0, o: 100}]
        };
      } else if (
        ['image/png', 'image/jpeg', 'image/webp'].includes(file.type) ||
        /\.(png|jpe?g|webp)$/i.test(file.name)
      ) {
        const data = await file.store(),
          info = await imageInfo(data),
          size = fittedAssetSize(info.width, info.height);
        shape = {
          id,
          name,
          type: 'image',
          assetData: data,
          color: '#888888',
          ease: 'smooth',
          keys: [{t: 0, x: editor.scene.width / 2, y: editor.scene.height / 2, w: size.w, h: size.h, r: 0, o: 100}]
        };
      } else throw Error('Unsupported media type');
      const offset = ((added % 7) - 3) * Math.min(36, editor.scene.width * 0.035);
      shape.keys[0].x += offset;
      shape.keys[0].y += offset;
      editor.scene.shapes.push(shape);
      editor.selected = id;
      layers.push(shape);
      added++;
      refresh();
      recordSceneChange('import');
    } catch (error) {
      errors.push(`${file.name}: ${error.message || 'Could not import'}`);
    }
  }
  if (added) closeDrawer();
  if (added || errors.length)
    toast(
      `${added} media layer${added === 1 ? '' : 's'} added${errors.length ? `; ${errors.length} skipped (${errors[0]})` : ''}`
    );
  return {added, errors, layers};
}

// The stop-motion start: pictures picked in order become poses on one track.
let asPoses = false;
export function pickPoses() {
  asPoses = true;
  $('asset-file').click();
}
export async function importPoses(files) {
  const {layers} = await importMediaFiles(files),
    pictures = layers.filter(s => s.type !== 'video');
  if (!pictures.length) return pictures;
  editor.scene.duration = Math.max(1, pictures.length * 0.5);
  arrangeAsPoses(pictures);
  editor.selected = pictures[0].id;
  editor.time = 0;
  refresh();
  recordSceneChange('arrange poses');
  toast(`${pictures.length} pose${pictures.length === 1 ? '' : 's'} added. Press play, or drag a pose to move it.`);
  return pictures;
}
let mediaDragDepth = 0;

export function setupImport() {
  $('import-asset').onclick = () => $('asset-file').click();
  $('asset-file').multiple = true;
  $('asset-file').onchange = e => {
    const files = [...e.target.files],
      poses = asPoses;
    asPoses = false;
    e.target.value = '';
    if (poses) importPoses(files);
    else importMediaFiles(files);
  };
  $('asset-file').addEventListener('cancel', () => (asPoses = false));
  document.addEventListener('dragenter', e => {
    if (!e.dataTransfer?.types?.includes('Files')) return;
    e.preventDefault();
    mediaDragDepth++;
    document.body.classList.add('media-dragging');
  });
  document.addEventListener('dragover', e => {
    if (!e.dataTransfer?.types?.includes('Files')) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = 'copy';
  });
  document.addEventListener('dragleave', e => {
    if (!e.dataTransfer?.types?.includes('Files')) return;
    e.preventDefault();
    if (--mediaDragDepth <= 0) {
      mediaDragDepth = 0;
      document.body.classList.remove('media-dragging');
    }
  });
  document.addEventListener('drop', e => {
    if (!e.dataTransfer?.files?.length) return;
    e.preventDefault();
    mediaDragDepth = 0;
    document.body.classList.remove('media-dragging');
    importMediaFiles(e.dataTransfer.files);
  });
}
