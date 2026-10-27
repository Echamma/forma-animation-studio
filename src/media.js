import {editor} from './state.js';

export const assetCache = new Map();
export function isAsset(s) {
  return s?.type === 'image' || s?.type === 'svg' || s?.type === 'video';
}
function svgData(source) {
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(source)}`;
}
export function assetHref(s) {
  return s.type === 'svg' ? svgData(s.svgSource) : s.type === 'video' ? s.posterData : s.assetData;
}
export function mediaElement(s) {
  const source = s.type === 'video' ? s.assetData : assetHref(s);
  if (assetCache.has(source)) return assetCache.get(source);
  if (s.type === 'video') {
    const v = document.createElement('video');
    v.src = source;
    v.muted = false;
    v.loop = true;
    v.playsInline = true;
    v.preload = 'auto';
    v.style.cssText = 'width:100%;height:100%;object-fit:fill;display:block;pointer-events:none';
    assetCache.set(source, v);
    return v;
  }
  const img = new Image();
  img.src = source;
  assetCache.set(source, img);
  return img;
}
export function syncVideo(s) {
  const v = mediaElement(s);
  if (!Number.isFinite(v.duration) || v.duration <= 0) return;
  const target = editor.time % v.duration;
  if (Math.abs(v.currentTime - target) > 0.2) v.currentTime = target;
  if (editor.playing) {
    if (v.paused) v.play().catch(() => {});
  } else v.pause();
}
function waitForMedia(media) {
  return new Promise((resolve, reject) => {
    if ((media.tagName === 'VIDEO' && media.readyState >= 2) || (media.tagName === 'IMG' && media.complete))
      return resolve();
    const ready = () => {
        cleanup();
        resolve();
      },
      fail = () => {
        cleanup();
        reject(Error('Media could not be decoded'));
      },
      cleanup = () => {
        media.removeEventListener('loadeddata', ready);
        media.removeEventListener('load', ready);
        media.removeEventListener('error', fail);
      };
    media.addEventListener(media.tagName === 'VIDEO' ? 'loadeddata' : 'load', ready, {once: true});
    media.addEventListener('error', fail, {once: true});
  });
}
export async function prepareAssets(at = editor.time, playVideos = false) {
  for (const s of editor.scene.shapes)
    if (isAsset(s)) {
      const media = mediaElement(s);
      await waitForMedia(media);
      if (s.type === 'video' && Number.isFinite(media.duration)) {
        const target = at % media.duration;
        if (Math.abs(media.currentTime - target) > 0.04)
          await new Promise(resolve => {
            media.addEventListener('seeked', resolve, {once: true});
            media.currentTime = target;
          });
        if (playVideos) await media.play().catch(() => {});
      }
    }
}
