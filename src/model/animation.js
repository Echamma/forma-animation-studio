import {editor} from '../state.js';

export const props = ['x', 'y', 'w', 'h', 'r', 'o'];
export function rawState(s, t = editor.time) {
  let a = s.keys[0],
    b = a;
  for (const k of s.keys) {
    if (k.t <= t) a = k;
    if (k.t >= t) {
      b = k;
      break;
    }
    b = k;
  }
  let f = a === b ? 0 : Math.max(0, Math.min(1, (t - a.t) / (b.t - a.t)));
  if (s.ease === 'smooth') f = f * f * (3 - 2 * f);
  if (s.ease === 'hold') f = 0;
  return Object.fromEntries(props.map(p => [p, a[p] + (b[p] - a[p]) * f]));
}
export function rotationAt(s, t = editor.time) {
  return (s.rotationClips || []).reduce((sum, clip) => {
    if (t <= clip.startTime) return sum;
    const f = Math.max(0, Math.min(1, (t - clip.startTime) / clip.duration)),
      e = clip.ease === 'smooth' ? f * f * (3 - 2 * f) : f;
    return sum + clip.degrees * e;
  }, 0);
}
export function animationClips(excludeId) {
  return editor.scene.shapes
    .filter(shape => shape.id !== excludeId)
    .flatMap(shape => [
      ...(shape.keys.length > 1
        ? [
            {
              shape,
              clip: {id: 'keys', startTime: shape.keys[0].t, duration: shape.keys.at(-1).t - shape.keys[0].t},
              label: `${shape.name} · Keyframes`
            }
          ]
        : []),
      ...(shape.motions || []).map((clip, i) => ({
        shape,
        clip,
        label: `${shape.name} · ${clip.type === 'path' ? 'Path' : 'Move'} ${i + 1}`
      })),
      ...(shape.rotationClips || []).map((clip, i) => ({shape, clip, label: `${shape.name} · Rotation ${i + 1}`}))
    ]);
}
export function reappearAt(s) {
  const d = s.disappear;
  if (!d || d.reappearMode === 'never') return Infinity;
  if (d.reappearMode === 'time') return d.reappearTime;
  const target = editor.scene.shapes.find(shape => shape.id === d.triggerShapeId && shape.id !== s.id),
    clip =
      d.triggerClipId === 'keys' && target?.keys.length > 1
        ? {startTime: target.keys[0].t, duration: target.keys.at(-1).t - target.keys[0].t}
        : target?.motions?.find(item => item.id === d.triggerClipId) ||
          target?.rotationClips?.find(item => item.id === d.triggerClipId);
  return clip ? Math.max(d.startTime + d.duration, clip.startTime + clip.duration) : Infinity;
}
export function visibilityAt(s, t) {
  const d = s.disappear;
  if (!d || t < d.startTime) return 1;
  const end = d.startTime + d.duration,
    returnTime = reappearAt(s);
  if (t >= returnTime) return 1;
  if (d.duration === 0 || t >= end) return 0;
  return Math.max(0, 1 - (t - d.startTime) / d.duration);
}
export function state(s, t = editor.time) {
  const own = rawState(s, t);
  own.r += rotationAt(s, t) + (s.baseRotation || 0);
  own.o *= visibilityAt(s, t);
  if (s.type !== 'text' || !s.linkId) return own;
  const target = editor.scene.shapes.find(x => x.id === s.linkId);
  if (!target) return own;
  const p = rawState(target, t);
  p.r += rotationAt(target, t) + (target.baseRotation || 0);
  const base = s.linkBase || {w: p.w, h: p.h},
    sx = p.w / (base.w || p.w),
    sy = p.h / (base.h || p.h),
    angle = (p.r * Math.PI) / 180,
    ox = (s.offsetX || 0) * sx,
    oy = (s.offsetY || 0) * sy;
  return {
    ...own,
    x: p.x + ox * Math.cos(angle) - oy * Math.sin(angle),
    y: p.y + ox * Math.sin(angle) + oy * Math.cos(angle),
    w: (s.textWidth || own.w) * Math.abs(sx),
    h: (s.fontSize || own.h) * Math.abs(sy),
    r: p.r + (s.rotationOffset || 0) + rotationAt(s, t) + (s.baseRotation || 0),
    o: Math.max(0, Math.min(100, p.o * (s.opacityFactor ?? 1) * visibilityAt(target, t) * visibilityAt(s, t)))
  };
}
export function editableState(s, t = editor.time) {
  const pose = state(s, t);
  pose.o = rawState(s, t).o;
  if (s.type !== 'text' || !s.linkId) pose.r -= rotationAt(s, t) + (s.baseRotation || 0);
  return pose;
}
