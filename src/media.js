import {inClip} from './model/animation.js';
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
// Images share one element per picture. Every video layer gets its own element because clips of one video can be on
// screen together and need different playback positions.
export const mediaKey = s => (s.type === 'video' ? s.id + '|' + s.assetData : assetHref(s));
export function mediaElement(s) {
  const source = s.type === 'video' ? s.assetData : assetHref(s),
    key = mediaKey(s);
  if (assetCache.has(key)) return assetCache.get(key);
  if (s.type === 'video') {
    const v = document.createElement('video');
    // Library media is served with CORS headers, so painting it keeps export canvases readable.
    v.crossOrigin = 'anonymous';
    v.src = source;
    v.muted = false;
    v.loop = true;
    v.playsInline = true;
    v.preload = 'auto';
    v.style.cssText = 'width:100%;height:100%;object-fit:fill;display:block;pointer-events:none';
    assetCache.set(key, v);
    return v;
  }
  const img = new Image();
  img.crossOrigin = 'anonymous';
  img.src = source;
  assetCache.set(key, img);
  return img;
}
// Position inside the video file at scene time t: clips start their media at inTime, offset by mediaStart, and loop.
export function mediaTime(s, t, duration) {
  const local = Math.max(0, t - (s.inTime ?? 0)) + (s.mediaStart ?? 0);
  return duration > 0 ? local % duration : local;
}
export function syncVideo(s) {
  const v = mediaElement(s);
  if (!Number.isFinite(v.duration) || v.duration <= 0) return;
  const target = mediaTime(s, editor.time, v.duration);
  if (Math.abs(v.currentTime - target) > 0.2) v.currentTime = target;
  if (editor.playing && inClip(s, editor.time)) {
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
// Loads every media layer and seeks videos to time `at`. Video export passes a tight tolerance so each frame is exact.
export async function prepareAssets(at = editor.time, playVideos = false, tolerance = 0.04) {
  for (const s of editor.scene.shapes)
    if (isAsset(s)) {
      const media = mediaElement(s);
      await waitForMedia(media);
      if (s.type === 'video' && Number.isFinite(media.duration)) {
        const target = mediaTime(s, at, media.duration);
        if (Math.abs(media.currentTime - target) > tolerance)
          await new Promise(resolve => {
            media.addEventListener('seeked', resolve, {once: true});
            media.currentTime = target;
          });
        if (playVideos) await media.play().catch(() => {});
      }
    }
}
