import {$, element, toast} from '../dom.js';
import {ease} from '../model/easing.js';
import {extendSceneTo} from '../model/keyframes.js';
import {recordSceneChange} from '../persistence.js';
import {stop} from '../playback.js';
import {editor} from '../state.js';
import {refresh} from './refresh.js';
import {showInspectorTab} from './tabs.js';

const keyframeMenu = $('keyframe-menu');
let keyframeMenuTarget = null;
export function closeKeyframeMenu() {
  keyframeMenu.hidden = true;
  keyframeMenuTarget = null;
}
function keyframeMenuError(message) {
  $('keyframe-menu-status').textContent = message;
}
function detachMotionKey(s, k) {
  const motionId = k.motionId || (s.motions || []).find(m => Math.abs(m.startTime - k.t) < 0.011)?.id;
  if (!motionId) return false;
  s.motions = s.motions.filter(m => m.id !== motionId);
  for (const key of s.keys) if (key.motionId === motionId) delete key.motionId;
  if (editor.selectedMotionId === motionId) {
    editor.selectedMotionId = null;
    editor.motionGuide = null;
  }
  return true;
}
export function removeKeyframe(s, k) {
  if (s.keys.length <= 1) return false;
  detachMotionKey(s, k);
  s.keys.splice(s.keys.indexOf(k), 1);
  return true;
}
// Curve editor: progress (0..1) runs left to right, value runs bottom to top with room to overshoot.
const CURVE = {width: 186, height: 150, pad: 14, low: -0.25, high: 1.25};
const toPx = (x, y) => [
  CURVE.pad + x * (CURVE.width - 2 * CURVE.pad),
  CURVE.pad + ((CURVE.high - y) / (CURVE.high - CURVE.low)) * (CURVE.height - 2 * CURVE.pad)
];
const fromPx = (x, y) => [
  (x - CURVE.pad) / (CURVE.width - 2 * CURVE.pad),
  CURVE.high - ((y - CURVE.pad) / (CURVE.height - 2 * CURVE.pad)) * (CURVE.high - CURVE.low)
];
function drawCurve(s, k) {
  const svg = $('curve-editor'),
    kind = k.ease ?? s.ease,
    [x0, y0] = toPx(0, 0),
    [x1, y1] = toPx(1, 1);
  svg.replaceChildren(
    element('rect', {x: x0, y: y1, width: x1 - x0, height: y0 - y1, class: 'curve-frame'}),
    element('polyline', {
      class: 'curve-line',
      points: Array.from({length: 61}, (_, i) => toPx(i / 60, ease(kind, i / 60, k.curve)).join(',')).join(' ')
    })
  );
  if (kind !== 'custom' || !k.curve) return;
  for (const [index, [from, to]] of [[[0, 0], [k.curve[0], k.curve[1]]], [[1, 1], [k.curve[2], k.curve[3]]]].entries()) {
    const [ax, ay] = toPx(...from),
      [hx, hy] = toPx(...to);
    svg.append(element('line', {x1: ax, y1: ay, x2: hx, y2: hy, class: 'curve-arm'}));
    svg.append(element('circle', {cx: hx, cy: hy, r: 6, class: 'curve-handle', 'data-handle': index}));
  }
}
function showEasing(s, k) {
  $('keyframe-ease').value = k.ease ?? '';
  $('keyframe-ease').disabled = k === s.keys.at(-1);
  drawCurve(s, k);
}

export function openKeyframeMenu(e, s, k) {
  e.preventDefault();
  e.stopPropagation();
  stop();
  editor.selected = s.id;
  editor.time = k.t;
  editor.selectedMotionId = k.motionId || null;
  editor.selectedRotationId = null;
  showInspectorTab('edit');
  refresh();
  keyframeMenuTarget = {s, k};
  $('keyframe-move-time').value = k.t;
  $('keyframe-remove').disabled = s.keys.length <= 1;
  showEasing(s, k);
  keyframeMenuError('');
  keyframeMenu.hidden = false;
  keyframeMenu.style.left = `${Math.max(8, Math.min(e.clientX, innerWidth - keyframeMenu.offsetWidth - 8))}px`;
  keyframeMenu.style.top = `${Math.max(8, Math.min(e.clientY, innerHeight - keyframeMenu.offsetHeight - 8))}px`;
  $('keyframe-move-time').focus();
  $('keyframe-move-time').select();
}

export function setupKeyframeMenu() {
  $('keyframe-move').onclick = () => {
    if (!keyframeMenuTarget) return;
    const {s, k} = keyframeMenuTarget,
      entry = $('keyframe-move-time'),
      newTime = Math.round(Number(entry.value) * 100) / 100;
    if (!entry.value.trim() || !Number.isFinite(newTime) || newTime < 0)
      return keyframeMenuError('Enter a time of 0 seconds or more.');
    if (s.keys.some(other => other !== k && Math.abs(other.t - newTime) < 0.001))
      return keyframeMenuError('Another keyframe is already at that time.');
    if (newTime === k.t) {
      closeKeyframeMenu();
      return;
    }
    extendSceneTo(newTime);
    const detached = detachMotionKey(s, k);
    k.t = newTime;
    s.keys.sort((a, b) => a.t - b.t);
    editor.time = newTime;
    closeKeyframeMenu();
    refresh();
    toast(detached ? 'Keyframe moved; animation converted to editable keyframes' : 'Keyframe moved');
  };
  $('keyframe-remove').onclick = () => {
    if (!keyframeMenuTarget) return;
    const {s, k} = keyframeMenuTarget;
    if (!removeKeyframe(s, k)) return keyframeMenuError('Keep at least one keyframe per shape.');
    closeKeyframeMenu();
    refresh();
    toast('Keyframe deleted');
  };
  $('keyframe-ease').onchange = () => {
    if (!keyframeMenuTarget) return;
    const {s, k} = keyframeMenuTarget,
      value = $('keyframe-ease').value;
    if (!value) delete k.ease;
    else k.ease = value;
    if (value === 'custom') k.curve ??= [0.42, 0, 0.58, 1];
    else delete k.curve;
    showEasing(s, k);
    refresh();
  };
  $('curve-editor').onpointerdown = e => {
    const handle = e.target.closest('[data-handle]');
    if (!handle || !keyframeMenuTarget) return;
    const {s, k} = keyframeMenuTarget,
      svg = $('curve-editor'),
      index = Number(handle.dataset.handle) * 2;
    try {
      svg.setPointerCapture(e.pointerId);
    } catch {
      // Synthetic pointers cannot be captured; dragging still works while the pointer stays over the editor.
    }
    svg.onpointermove = move => {
      const point = new DOMPoint(move.clientX, move.clientY).matrixTransform(svg.getScreenCTM().inverse()),
        [x, y] = fromPx(point.x, point.y);
      k.curve[index] = Math.round(Math.max(0, Math.min(1, x)) * 100) / 100;
      k.curve[index + 1] = Math.round(Math.max(CURVE.low, Math.min(CURVE.high, y)) * 100) / 100;
      drawCurve(s, k);
    };
    svg.onpointerup = svg.onpointercancel = () => {
      svg.onpointermove = svg.onpointerup = svg.onpointercancel = null;
      refresh();
      recordSceneChange('keyframe easing');
    };
  };
  $('keyframe-move-time').onkeydown = e => {
    if (e.key === 'Enter') $('keyframe-move').click();
  };
  document.addEventListener('pointerdown', e => {
    if (!keyframeMenu.hidden && !keyframeMenu.contains(e.target)) closeKeyframeMenu();
  });
  document.addEventListener('keydown', e => {
    if (e.key === 'Escape') closeKeyframeMenu();
  });
}
