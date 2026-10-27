import {$, toast} from '../dom.js';
import {MAX_LAYERS} from '../model/validate.js';
import {recordSceneChange} from '../persistence.js';
import {stop} from '../playback.js';
import {editor} from '../state.js';
import {sanitizeSvg} from '../svg/sanitize.js';
import {refresh} from './refresh.js';
import {closeDrawer} from './tabs.js';

function readDataUrl(file) {
  return new Promise((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => resolve(r.result);
    r.onerror = () => reject(r.error);
    r.readAsDataURL(file);
  });
}
function imageInfo(src) {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve({width: img.naturalWidth, height: img.naturalHeight});
    img.onerror = reject;
    img.src = src;
  });
}
function videoInfo(src) {
  return new Promise((resolve, reject) => {
    const v = document.createElement('video');
    v.muted = true;
    v.preload = 'auto';
    v.src = src;
    v.onloadedmetadata = () => {
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
export async function importMediaFiles(files) {
  const items = [...files],
    errors = [];
  let added = 0;
  stop();
  for (const file of items) {
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
        const data = await readDataUrl(file),
          info = await videoInfo(data),
          size = fittedAssetSize(info.width, info.height);
        shape = {
          id,
          name,
          type: 'video',
          assetData: data,
          posterData: info.poster,
          mediaDuration: info.duration,
          color: '#888888',
          ease: 'smooth',
          keys: [{t: 0, x: editor.scene.width / 2, y: editor.scene.height / 2, w: size.w, h: size.h, r: 0, o: 100}]
        };
      } else if (
        ['image/png', 'image/jpeg', 'image/webp'].includes(file.type) ||
        /\.(png|jpe?g|webp)$/i.test(file.name)
      ) {
        const data = await readDataUrl(file),
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
  return {added, errors};
}
let mediaDragDepth = 0;

export function setupImport() {
  $('import-asset').onclick = () => $('asset-file').click();
  $('asset-file').multiple = true;
  $('asset-file').onchange = e => {
    const files = [...e.target.files];
    e.target.value = '';
    importMediaFiles(files);
  };
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
