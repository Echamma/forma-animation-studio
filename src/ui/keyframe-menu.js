import {$, toast} from '../dom.js';
import {extendSceneTo} from '../model/keyframes.js';
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
