// Video-editor style clip operations: each layer is a clip with a visible window, and clips that share a trackId sit
// one after another on a single timeline row.
import {clipEnd, clipStart, rawState, rotationAt} from './animation.js';
import {extendSceneTo} from './keyframes.js';
import {MAX_LAYERS} from './validate.js';
import {editor} from '../state.js';

export const MIN_CLIP = 0.05;
const ASSET_FIELDS = ['assetData', 'posterData', 'svgSource'];
const round = value => Math.round(value * 100) / 100;

export const isLinkedText = s => s.type === 'text' && !!s.linkId;
export const trackClips = trackId => editor.scene.shapes.filter(s => trackId && s.trackId === trackId);

// Deep copy with a fresh id. Media strings are shared rather than copied so clips of one image cost no extra memory.
export function cloneLayer(s) {
  const copy = JSON.parse(JSON.stringify(s, (key, value) => (ASSET_FIELDS.includes(key) ? undefined : value)));
  for (const field of ASSET_FIELDS) if (s[field] !== undefined) copy[field] = s[field];
  copy.id = crypto.randomUUID();
  return copy;
}

function numberedName(s) {
  const base = s.name.replace(/ \d+$/, '').slice(0, 90),
    pattern = new RegExp(`^${base.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')} (\\d+)$`);
  const highest = editor.scene.shapes.reduce((n, x) => Math.max(n, +(x.name.match(pattern)?.[1] || 1)), 1);
  return `${base} ${highest + 1}`;
}

function insertAfter(s, copy) {
  if (editor.scene.shapes.length >= MAX_LAYERS) throw Error(`Scene is full (${MAX_LAYERS} layers)`);
  const shapes = editor.scene.shapes;
  shapes.splice(shapes.indexOf(s) + 1, 0, copy);
}

// Earliest and latest moments a layer's timing data touches, so moves stay inside the scene.
function earliestTime(s) {
  return Math.min(
    clipStart(s),
    ...s.keys.map(k => k.t),
    ...(s.motions || []).map(m => m.startTime),
    ...(s.rotationClips || []).map(c => c.startTime),
    ...(s.disappear ? [s.disappear.startTime] : [])
  );
}
export function latestTime(s) {
  return Math.max(
    s.outTime ?? 0,
    ...s.keys.map(k => k.t),
    ...(s.motions || []).map(m => m.startTime + m.duration),
    ...(s.rotationClips || []).map(c => c.startTime + c.duration),
    ...(s.disappear ? [s.disappear.startTime + s.disappear.duration] : []),
    ...(s.disappear?.reappearMode === 'time' ? [s.disappear.reappearTime] : [])
  );
}
export const earliestShift = s => -earliestTime(s);

// Move a clip and everything timed inside it: keyframes, drawn moves, rotations, and its fade out.
export function shiftLayer(s, delta) {
  delta = Math.max(earliestShift(s), round(delta));
  if (!delta) return 0;
  s.inTime = round(clipStart(s) + delta);
  if (s.outTime !== undefined) s.outTime = round(s.outTime + delta);
  for (const k of s.keys) k.t = round(k.t + delta);
  for (const m of s.motions || []) {
    m.startTime = round(m.startTime + delta);
    for (const k of m.baseKeys || []) k.t = round(k.t + delta);
  }
  for (const c of s.rotationClips || []) c.startTime = round(c.startTime + delta);
  if (s.disappear) {
    s.disappear.startTime = round(s.disappear.startTime + delta);
    if (s.disappear.reappearMode === 'time') s.disappear.reappearTime = round(s.disappear.reappearTime + delta);
  }
  extendSceneTo(latestTime(s));
  return delta;
}

// Change where a clip starts or ends without moving its animation.
export function trimClip(s, start, end) {
  start = round(Math.max(0, start));
  end = end === undefined ? undefined : round(end);
  if (end !== undefined && end < start + MIN_CLIP) throw Error('A clip must last at least 0.05 seconds');
  if (start > 0 || s.inTime !== undefined) s.inTime = start;
  if (end === undefined) delete s.outTime;
  else {
    extendSceneTo(end);
    s.outTime = end;
  }
}

// The pose a layer shows at time t as keyframe values: rotation clips are baked in, the layer angle stays separate.
function stillPose(s, t) {
  const pose = rawState(s, t);
  pose.r += rotationAt(s, t);
  return pose;
}

// Place a still copy of `template` right after `source` on the same track, so moving the copy makes the picture jump
// to a new spot. Later clips on the track move right to make room. Without an end time, the source first ends at the
// playhead (or after one second).
export function continueAfter(source, template = source) {
  if (isLinkedText(source) || isLinkedText(template))
    throw Error('Linked text follows its shape; continue the shape instead');
  const start = clipStart(source);
  if (source.outTime === undefined) {
    const playhead = editor.time;
    source.outTime = round(playhead > start + MIN_CLIP && playhead < editor.scene.duration ? playhead : start + 1);
  }
  const end = source.outTime,
    length = round(end - start);
  source.trackId ??= crypto.randomUUID();
  for (const other of trackClips(source.trackId))
    if (other !== source && clipStart(other) >= end - 1e-6) shiftLayer(other, length);
  const copy = cloneLayer(template),
    ending = stillPose(source, end);
  const pose = template === source ? ending : {...stillPose(template, clipStart(template)), x: ending.x, y: ending.y, r: ending.r};
  Object.assign(copy, {name: numberedName(template), trackId: source.trackId, inTime: end, outTime: round(end + length)});
  copy.keys = [{t: end, ...pose}];
  if (template !== source) copy.baseRotation = source.baseRotation;
  if (copy.baseRotation === undefined) delete copy.baseRotation;
  delete copy.motions;
  delete copy.rotationClips;
  delete copy.disappear;
  if (copy.type === 'video') copy.mediaStart = template === source ? round((source.mediaStart ?? 0) + length) : template.mediaStart ?? 0;
  if (!copy.mediaStart) delete copy.mediaStart;
  extendSceneTo(copy.outTime);
  insertAfter(source, copy);
  return copy;
}

// Cut a clip in two at time t. Both halves keep the full animation, so the result looks exactly the same.
export function splitAt(s, t) {
  if (isLinkedText(s)) throw Error('Linked text follows its shape; split the shape instead');
  t = round(t);
  const start = clipStart(s);
  if (t < start + MIN_CLIP || t > clipEnd(s) - MIN_CLIP) throw Error('Move the playhead inside the clip to split it');
  const second = cloneLayer(s);
  s.trackId ??= crypto.randomUUID();
  Object.assign(second, {name: numberedName(s), trackId: s.trackId, inTime: t});
  if (s.outTime === undefined) delete second.outTime;
  s.outTime = t;
  if (s.type === 'video') second.mediaStart = round((s.mediaStart ?? 0) + t - start);
  insertAfter(s, second);
  return second;
}

// An independent copy at the same time, nudged so it does not hide the original.
export function duplicateLayer(s, offset = 20) {
  const copy = cloneLayer(s);
  copy.name = numberedName(s);
  delete copy.trackId;
  if (!isLinkedText(copy)) {
    const nudge = p => {
      p.x += offset;
      p.y += offset;
    };
    copy.keys.forEach(nudge);
    for (const m of copy.motions || []) {
      nudge(m.startPose);
      m.sourcePoints.forEach(nudge);
      (m.baseKeys || []).forEach(nudge);
    }
  } else copy.offsetX += offset;
  insertAfter(editor.scene.shapes.includes(s) ? s : editor.scene.shapes.at(-1), copy);
  extendSceneTo(latestTime(copy));
  return copy;
}
