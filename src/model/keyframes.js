import {props} from './animation.js';
import {editor} from '../state.js';

function keyAt(s, at, p) {
  const t = Math.round(at * 100) / 100;
  let k = s.keys.find(k => Math.abs(k.t - t) < 0.011);
  if (k) Object.assign(k, p);
  else s.keys.push({t, ...p});
  s.keys.sort((a, b) => a.t - b.t);
}
export function key(s, p) {
  keyAt(s, editor.time, p);
}
export function extendSceneTo(end) {
  if (!Number.isFinite(end) || end < 0) throw Error('Enter a finite duration');
  if (end > editor.scene.duration) editor.scene.duration = end;
}
function scaledMotionPoints(m) {
  const scale = m.distance / 100,
    start = m.startPose;
  return m.sourcePoints.map(p => ({x: start.x + (p.x - start.x) * scale, y: start.y + (p.y - start.y) * scale}));
}
export function motionGuideFor(m) {
  const points = scaledMotionPoints(m);
  return {type: m.type, start: points[0], end: points.at(-1), points};
}
export function restoreMotionKeys(s, m) {
  s.keys = s.keys.filter(k => k.motionId !== m.id);
  if (!(m.baseKeys || []).some(k => Math.abs(k.t - m.startTime) < 0.011))
    s.keys = s.keys.filter(
      k => !(Math.abs(k.t - m.startTime) < 0.011 && props.every(p => Math.abs(k[p] - m.startPose[p]) < 0.011))
    );
  for (const k of m.baseKeys || []) {
    s.keys = s.keys.filter(x => Math.abs(x.t - k.t) > 0.001);
    s.keys.push({...k});
  }
  s.keys.sort((a, b) => a.t - b.t);
}
export function applyMotion(s, m) {
  s.keys = s.keys.filter(k => k.motionId !== m.id);
  const points = scaledMotionPoints(m),
    end = Math.min(editor.scene.duration, m.startTime + m.duration),
    span = end - m.startTime;
  if (span <= 0.01) return;
  const startKey = {t: m.startTime, ...m.startPose, motionId: m.id};
  s.keys = s.keys.filter(k => k.t < m.startTime - 0.001 || k.t > end + 0.001);
  s.keys.push(startKey);
  const samples =
    m.type === 'arrow'
      ? points
      : points.filter((p, i, a) => i === 0 || i === a.length - 1 || Math.hypot(p.x - a[i - 1].x, p.y - a[i - 1].y) > 3);
  samples.forEach((p, i) => {
    if (i === 0) return;
    const f = i / (samples.length - 1),
      next = samples[Math.min(i + 1, samples.length - 1)],
      prev = samples[Math.max(0, i - 1)],
      pose = {...m.startPose, x: p.x, y: p.y, r: m.startPose.r + m.spin * f};
    if (m.face) pose.r = (Math.atan2(next.y - prev.y, next.x - prev.x) * 180) / Math.PI + m.spin * f;
    s.keys.push({t: Math.round((m.startTime + span * f) * 100) / 100, ...pose, motionId: m.id});
  });
  s.keys.sort((a, b) => a.t - b.t);
  s.keys = s.keys.filter((k, i, a) => i === a.length - 1 || Math.abs(k.t - a[i + 1].t) > 0.001);
}
