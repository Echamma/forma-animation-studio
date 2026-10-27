import {previewMotion} from './motion.js';
import {$, toast} from '../dom.js';
import {extendSceneTo} from '../model/keyframes.js';
import {recordSceneChange} from '../persistence.js';
import {stop} from '../playback.js';
import {editor} from '../state.js';
import {refresh} from '../ui/refresh.js';
import {showInspectorTab} from '../ui/tabs.js';

export function renderRotationList() {
  const s = editor.scene.shapes.find(s => s.id === editor.selected),
    list = $('rotation-list');
  list.replaceChildren();
  $('add-rotation').disabled = !s;
  $('add-rotation').textContent = editor.selectedMotionId
    ? '＋ Add rotation over this animation'
    : '＋ Add rotation at playhead';
  for (const [i, clip] of (s?.rotationClips || []).entries()) {
    const b = document.createElement('button');
    b.className = 'motion-clip' + (clip.id === editor.selectedRotationId ? ' active' : '');
    b.textContent = `↻ Rotation ${i + 1} · ${clip.degrees}° · ${clip.startTime.toFixed(2)}–${(clip.startTime + clip.duration).toFixed(2)} s`;
    b.onclick = () => selectRotation(s, clip);
    list.append(b);
  }
  $('rotation-edit').hidden = !(s?.rotationClips || []).some(clip => clip.id === editor.selectedRotationId);
}
function selectedRotation() {
  const s = editor.scene.shapes.find(s => s.id === editor.selected);
  return {s, clip: s?.rotationClips?.find(clip => clip.id === editor.selectedRotationId)};
}
export function selectRotation(s, clip) {
  stop();
  showInspectorTab('animate');
  editor.selected = s.id;
  editor.selectedMotionId = null;
  editor.selectedRotationId = clip.id;
  editor.motionGuide = null;
  editor.time = clip.startTime;
  $('rotation-start').value = clip.startTime.toFixed(2);
  $('rotation-length').value = clip.duration.toFixed(2);
  $('rotation-degrees').value = clip.degrees;
  $('rotation-ease').value = clip.ease;
  refresh();
  $('rotation-edit').scrollIntoView({block: 'nearest'});
}
export function editRotation({preview = false, quiet = false} = {}) {
  const {s, clip} = selectedRotation();
  if (!clip) return false;
  const requestedStart = +$('rotation-start').value,
    requestedLength = +$('rotation-length').value;
  if (
    !Number.isFinite(requestedStart) ||
    requestedStart < 0 ||
    !Number.isFinite(requestedLength) ||
    requestedLength < 0.1 ||
    !Number.isFinite(requestedStart + requestedLength)
  ) {
    toast('Enter a valid rotation start and length');
    selectRotation(s, clip);
    return false;
  }
  const start = Math.round(requestedStart * 100) / 100,
    length = Math.round(requestedLength * 100) / 100,
    degrees = Math.max(-10000, Math.min(10000, +$('rotation-degrees').value || 0));
  stop();
  extendSceneTo(start + length);
  Object.assign(clip, {startTime: start, duration: length, degrees, ease: $('rotation-ease').value});
  selectRotation(s, clip);
  recordSceneChange('edit rotation');
  if (!quiet) toast(`Rotation updated · ${degrees}° over ${length.toFixed(2)} s`);
  if (preview) previewMotion(clip);
  return true;
}
let rotationPreviewTimer;

export function setupRotation() {
  $('add-rotation-to-motion').onclick = () => $('add-rotation').click();
  $('add-rotation').onclick = () => {
    const s = editor.scene.shapes.find(s => s.id === editor.selected);
    if (!s) return;
    stop();
    const m = s.motions?.find(m => m.id === editor.selectedMotionId),
      start = m?.startTime ?? editor.time,
      requested = +$('motion-duration').value,
      duration = m?.duration ?? (Number.isFinite(requested) && requested >= 0.1 ? requested : 1),
      clip = {id: crypto.randomUUID(), startTime: start, duration, degrees: 360, ease: 'linear'};
    if (!Number.isFinite(start + duration)) {
      toast('Enter a valid rotation length');
      return;
    }
    extendSceneTo(start + duration);
    s.rotationClips ??= [];
    s.rotationClips.push(clip);
    selectRotation(s, clip);
    toast(m ? 'Rotation added over selected animation' : 'Rotation added at playhead');
  };
  $('update-rotation').onclick = () => editRotation({preview: $('live-rotation-preview').checked});
  $('preview-rotation').onclick = () => {
    const {clip} = selectedRotation();
    previewMotion(clip);
  };
  for (const id of ['rotation-start', 'rotation-length', 'rotation-degrees', 'rotation-ease'])
    $(id).addEventListener(id === 'rotation-ease' ? 'change' : 'input', () => {
      if (!$('live-rotation-preview').checked) return;
      clearTimeout(rotationPreviewTimer);
      rotationPreviewTimer = setTimeout(() => editRotation({preview: true, quiet: true}), 300);
    });
  $('delete-rotation').onclick = () => {
    const {s, clip} = selectedRotation();
    if (!clip) return;
    stop();
    s.rotationClips = s.rotationClips.filter(x => x.id !== clip.id);
    editor.selectedRotationId = null;
    editor.time = clip.startTime;
    refresh();
    toast('Rotation animation deleted');
  };
}
