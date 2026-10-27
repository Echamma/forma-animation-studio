// When an animated SVG needs a value for a layer. Values are exact at every keyframe, clip edge, rotation and fade
// boundary; stretches that move in a straight line or stand still need only their ends, curved stretches are sampled at
// `fps`, and instant jumps (hold keys, clip cuts) get two values at the same moment.
import {clipEnd, colorAt, props, reappearAt, state} from './animation.js';
import {editor} from '../state.js';

// The value just before an event is read this much earlier, which is longer than the clip-edge tolerance in
// animation.js; differences smaller than JUMP over that instant are continuous change, not a step.
const EPSILON = 1e-5,
  JUMP = 0.01;

function eventTimes(s, seen = new Set([s.id])) {
  const times = [s.inTime, s.outTime, ...s.keys.map(k => k.t), ...(s.colorKeys || []).map(k => k.t)];
  for (const c of s.rotationClips || []) times.push(c.startTime, c.startTime + c.duration);
  if (s.disappear) times.push(s.disappear.startTime, s.disappear.startTime + s.disappear.duration, reappearAt(s));
  const target = s.type === 'text' && s.linkId && editor.scene.shapes.find(x => x.id === s.linkId);
  if (target && !seen.has(target.id)) times.push(...eventTimes(target, seen.add(target.id)), clipEnd(target));
  return times;
}

// Whether anything about the layer changes along a curve between two neighbouring event times.
function curved(s, a, b) {
  if (s.type === 'text' && s.linkId) return true;
  const mid = (a + b) / 2;
  let ka = s.keys[0],
    kb = ka;
  for (const k of s.keys) {
    if (k.t <= mid) ka = k;
    if (k.t >= mid) {
      kb = k;
      break;
    }
    kb = k;
  }
  const moving = ka !== kb && props.some(p => ka[p] !== kb[p]),
    kind = ka.ease ?? s.ease;
  if (moving && kind !== 'linear' && kind !== 'hold') return true;
  if ((s.rotationClips || []).some(c => c.degrees && c.ease !== 'linear' && c.startTime < b && c.startTime + c.duration > a))
    return true;
  const d = s.disappear;
  if (d && d.duration > 0 && d.startTime < b && d.startTime + d.duration > a && moving && ka.o !== kb.o) return true;
  return colorAt(s, a + EPSILON) !== colorAt(s, b - EPSILON) && s.ease !== 'linear' && s.ease !== 'hold';
}

const same = (p, q) => props.every(key => Math.abs(p.pose[key] - q.pose[key]) < JUMP) && p.color === q.color;

export function sampleLayer(s, duration = editor.scene.duration, fps = Math.min(60, 12000 / duration)) {
  const at = t => ({t, pose: state(s, t), color: colorAt(s, t)}),
    events = [
      ...new Set([0, duration, ...eventTimes(s).filter(t => Number.isFinite(t) && t > 0 && t < duration)])
    ].sort((a, b) => a - b),
    samples = [];
  events.forEach((a, i) => {
    const sample = at(a);
    if (i > 0) {
      const before = {...at(a - EPSILON), t: a};
      if (!same(before, sample)) samples.push(before);
    }
    samples.push(sample);
    const b = events[i + 1];
    if (b !== undefined && curved(s, a, b))
      for (let frame = Math.floor(a * fps + EPSILON) + 1; frame / fps < b - EPSILON; frame++) samples.push(at(frame / fps));
  });
  return samples;
}
