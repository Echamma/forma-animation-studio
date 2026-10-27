import {props} from './animation.js';
import {EASES, validCurve} from './easing.js';

export const MAX_LAYERS = 500;

// Media is either embedded as a data URL (browser preview, older projects) or stored in the desktop media library.
export const LIBRARY_URL = /^forma-asset:\/\/media\/[0-9a-f]{64}\.(png|jpg|webp|mp4|webm|ogv)$/;
const isImage = value =>
  typeof value === 'string' &&
  (/^data:image\/(png|jpeg|webp);/i.test(value) || (LIBRARY_URL.test(value) && /\.(png|jpg|webp)$/.test(value)));
const isVideo = value =>
  typeof value === 'string' &&
  (/^data:video\/(mp4|webm|ogg);/i.test(value) || (LIBRARY_URL.test(value) && /\.(mp4|webm|ogv)$/.test(value)));
const isPoster = value => typeof value === 'string' && (value.startsWith('data:image/') || isImage(value));

// Clip timing and track fields are optional so projects saved before clips existed stay valid.
function clipTimesOk(x, duration) {
  const time = value => Number.isFinite(value) && value >= 0 && value <= duration + 0.001;
  return (
    (x.inTime === undefined || time(x.inTime)) &&
    (x.outTime === undefined || (time(x.outTime) && x.outTime > (x.inTime ?? 0))) &&
    (x.trackId === undefined || (typeof x.trackId === 'string' && x.trackId.length > 0 && x.trackId.length <= 64)) &&
    (x.mediaStart === undefined || (Number.isFinite(x.mediaStart) && x.mediaStart >= 0))
  );
}
// Optional per-keyframe easing and the separate color keyframe track.
const keyEaseOk = k => (k.ease === undefined || EASES.includes(k.ease)) && (k.curve === undefined || validCurve(k.curve));
function colorKeysOk(x, duration) {
  if (x.colorKeys === undefined) return true;
  return (
    Array.isArray(x.colorKeys) &&
    x.colorKeys.length <= 1000 &&
    new Set(x.colorKeys.map(k => k.t)).size === x.colorKeys.length &&
    x.colorKeys.every(k => Number.isFinite(k.t) && k.t >= 0 && k.t <= duration + 0.001 && /^#[0-9a-f]{6}$/i.test(k.color))
  );
}

export function valid(s) {
  return (
    s &&
    s.version === 1 &&
    Number.isFinite(s.width) &&
    Number.isFinite(s.height) &&
    s.width >= 100 &&
    s.width <= 4000 &&
    s.height >= 100 &&
    s.height <= 4000 &&
    Number.isFinite(s.duration) &&
    s.duration >= 1 &&
    /^#[0-9a-f]{6}$/i.test(s.bg) &&
    Array.isArray(s.shapes) &&
    s.shapes.length <= MAX_LAYERS &&
    new Set(s.shapes.map(x => x.id)).size === s.shapes.length &&
    s.shapes.every(x => {
      const assetOk =
          x.type === 'image'
            ? isImage(x.assetData)
            : x.type === 'svg'
              ? typeof x.svgSource === 'string'
              : x.type === 'video'
                ? isVideo(x.assetData) && isPoster(x.posterData) && Number.isFinite(x.mediaDuration)
                : true,
        textOk =
          x.type !== 'text' ||
          (typeof x.text === 'string' &&
            x.text.length <= 120 &&
            typeof x.linkId === 'string' &&
            Number.isFinite(x.offsetX) &&
            Number.isFinite(x.offsetY) &&
            Number.isFinite(x.textWidth) &&
            Number.isFinite(x.fontSize));
      return (
        typeof x.id === 'string' &&
        typeof x.name === 'string' &&
        x.name.length < 100 &&
        ['circle', 'rect', 'triangle', 'diamond', 'arrow', 'text', 'image', 'svg', 'video'].includes(x.type) &&
        assetOk &&
        textOk &&
        (x.blur === undefined || (Number.isFinite(x.blur) && x.blur >= 0 && x.blur <= 100)) &&
        (x.baseRotation === undefined || (Number.isFinite(x.baseRotation) && Math.abs(x.baseRotation) <= 10000)) &&
        clipTimesOk(x, s.duration) &&
        colorKeysOk(x, s.duration) &&
        x.keys?.every?.(keyEaseOk) &&
        (x.disappear === undefined ||
          (x.disappear &&
            Number.isFinite(x.disappear.startTime) &&
            x.disappear.startTime >= 0 &&
            Number.isFinite(x.disappear.duration) &&
            x.disappear.duration >= 0 &&
            x.disappear.startTime + x.disappear.duration <= s.duration + 0.001 &&
            ['never', 'time', 'clip'].includes(x.disappear.reappearMode) &&
            (x.disappear.reappearMode !== 'time' ||
              (Number.isFinite(x.disappear.reappearTime) &&
                x.disappear.reappearTime >= x.disappear.startTime + x.disappear.duration &&
                x.disappear.reappearTime <= s.duration + 0.001)) &&
            (x.disappear.reappearMode !== 'clip' ||
              (typeof x.disappear.triggerShapeId === 'string' && typeof x.disappear.triggerClipId === 'string')))) &&
        (x.rotationClips === undefined ||
          (Array.isArray(x.rotationClips) &&
            x.rotationClips.length <= 1000 &&
            new Set(x.rotationClips.map(c => c.id)).size === x.rotationClips.length &&
            x.rotationClips.every(
              c =>
                typeof c.id === 'string' &&
                Number.isFinite(c.startTime) &&
                c.startTime >= 0 &&
                Number.isFinite(c.duration) &&
                c.duration > 0 &&
                c.startTime + c.duration <= s.duration + 0.001 &&
                Number.isFinite(c.degrees) &&
                Math.abs(c.degrees) <= 10000 &&
                ['linear', 'smooth'].includes(c.ease)
            ))) &&
        ['smooth', 'linear', 'hold'].includes(x.ease) &&
        /^#[0-9a-f]{6}$/i.test(x.color) &&
        Array.isArray(x.keys) &&
        x.keys.length > 0 &&
        x.keys.length <= 1000 &&
        x.keys.every(
          k =>
            Number.isFinite(k.t) &&
            k.t >= 0 &&
            k.t <= s.duration &&
            props.every(p => Number.isFinite(k[p]) && Math.abs(k[p]) <= 10000) &&
            k.w > 0 &&
            k.h > 0 &&
            k.o >= 0 &&
            k.o <= 100
        ) &&
        new Set(x.keys.map(k => k.t)).size === x.keys.length
      );
    })
  );
}

// Stricter check for projects arriving from MCP tools: also validates linked text and drawn motion metadata.
export function usableProject(project) {
  try {
    if (!valid(project)) return false;
    const number = value => Number.isFinite(value) && Math.abs(value) <= 10000;
    const timeValue = value => Number.isFinite(value) && value >= 0;
    const pose = value =>
      value && props.every(key => number(value[key])) && value.w > 0 && value.h > 0 && value.o >= 0 && value.o <= 100;
    return project.shapes.every(layer => {
      if (layer.type === 'text') {
        if (layer.textWidth <= 0 || layer.fontSize <= 0) return false;
        if (
          layer.linkBase &&
          (!number(layer.linkBase.w) || !number(layer.linkBase.h) || layer.linkBase.w <= 0 || layer.linkBase.h <= 0)
        )
          return false;
        if (layer.rotationOffset !== undefined && !number(layer.rotationOffset)) return false;
        if (layer.opacityFactor !== undefined && !Number.isFinite(layer.opacityFactor)) return false;
      }
      if (layer.motions === undefined) return true;
      return (
        Array.isArray(layer.motions) &&
        layer.motions.length <= 1000 &&
        layer.motions.every(
          motion =>
            motion &&
            typeof motion.id === 'string' &&
            ['arrow', 'path'].includes(motion.type) &&
            timeValue(motion.startTime) &&
            motion.startTime <= project.duration &&
            timeValue(motion.duration) &&
            motion.duration > 0 &&
            motion.startTime + motion.duration <= project.duration + 0.001 &&
            number(motion.distance) &&
            motion.distance >= 0 &&
            motion.distance <= 500 &&
            number(motion.spin) &&
            typeof motion.face === 'boolean' &&
            pose(motion.startPose) &&
            Array.isArray(motion.sourcePoints) &&
            motion.sourcePoints.length >= 1 &&
            motion.sourcePoints.length <= 1000 &&
            motion.sourcePoints.every(point => point && number(point.x) && number(point.y)) &&
            (motion.baseKeys === undefined ||
              (Array.isArray(motion.baseKeys) &&
                motion.baseKeys.length <= 1000 &&
                motion.baseKeys.every(key => pose(key) && timeValue(key.t) && key.t <= project.duration)))
        )
      );
    });
  } catch {
    return false;
  }
}
