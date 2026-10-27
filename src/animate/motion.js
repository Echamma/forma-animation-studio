import {$, toast} from '../dom.js';
import {editableState} from '../model/animation.js';
import {applyMotion, extendSceneTo, motionGuideFor, restoreMotionKeys} from '../model/keyframes.js';
import {recordSceneChange} from '../persistence.js';
import {stop} from '../playback.js';
import {render} from '../render/canvas.js';
import {editor} from '../state.js';
import {refresh} from '../ui/refresh.js';
import {showInspectorTab} from '../ui/tabs.js';

export function renderMotionList() {
  const s = editor.scene.shapes.find(s => s.id === editor.selected),
    motions = s?.motions || [],
    list = $('motion-list');
  list.replaceChildren();
  $('motion-count').textContent = motions.length;
  if (!motions.length) {
    const p = document.createElement('p');
    p.className = 'empty-motion';
    p.textContent = 'No drawn animations yet.';
    list.append(p);
  }
  for (const [i, m] of motions.entries()) {
    const b = document.createElement('button');
    b.className = 'motion-clip' + (m.id === editor.selectedMotionId ? ' active' : '');
    b.innerHTML = `<span class="motion-icon">${m.type === 'path' ? '⌁' : '↗'}</span><b>${m.type === 'path' ? 'Mouse path' : 'Arrow move'} ${i + 1}</b><small>${m.startTime.toFixed(1)}–${(m.startTime + m.duration).toFixed(1)} s · ${m.distance}% distance</small>`;
    b.onclick = () => selectMotion(s, m);
    list.append(b);
  }
  const active = motions.find(m => m.id === editor.selectedMotionId);
  $('motion-edit').hidden = !active;
}
function motionWindow() {
  const value = +$('motion-duration').value,
    duration = Number.isFinite(value) && value >= 0.1 ? value : 1;
  return {start: editor.time, end: editor.time + duration};
}
export function selectMotion(s, m) {
  stop();
  showInspectorTab('animate');
  editor.selected = s.id;
  editor.selectedMotionId = m.id;
  editor.selectedRotationId = null;
  editor.time = m.startTime;
  $('motion-duration').value = m.duration;
  $('motion-distance').value = m.distance;
  $('motion-spin').value = m.spin;
  $('face-path').checked = m.face;
  $('motion-start').value = m.startTime.toFixed(2);
  $('motion-length').value = m.duration.toFixed(2);
  $('motion-edit-distance').value = m.distance;
  $('motion-edit-spin').value = m.spin;
  $('motion-edit-face').checked = m.face;
  editor.motionGuide = motionGuideFor(m);
  refresh();
  $('motion-edit').scrollIntoView({block: 'nearest'});
}
export function setMotionMode(mode) {
  const s = editor.scene.shapes.find(s => s.id === editor.selected);
  if (!s) return toast('Select a shape first');
  stop();
  editor.motionMode = editor.motionMode === mode ? null : mode;
  editor.motionDraft = null;
  editor.motionGuide = null;
  $('stage').classList.toggle('motion-mode', !!editor.motionMode);
  $('draw-arrow').classList.toggle('active', editor.motionMode === 'arrow');
  $('record-path').classList.toggle('active', editor.motionMode === 'path');
  $('motion-help').textContent =
    editor.motionMode === 'arrow'
      ? 'Drag anywhere on the canvas to set the arrow destination.'
      : editor.motionMode === 'path'
        ? 'Hold and drag on the canvas. The selected shape will follow your mouse.'
        : 'Choose a tool, then drag anywhere on the canvas. The motion begins at the playhead.';
  render();
}
export function finishMotion() {
  if (!editor.motionDraft) return;
  const s = editor.scene.shapes.find(s => s.id === editor.motionDraft.shapeId),
    window = motionWindow(),
    spin = Math.max(-10000, Math.min(10000, +$('motion-spin').value || 0)),
    face = $('face-path').checked,
    start = editor.motionDraft.startState,
    distance = Math.max(0, Math.min(500, +$('motion-distance').value || 0));
  if (!s || !Number.isFinite(window.end) || window.end - window.start < 0.05) {
    editor.motionDraft = null;
    setMotionMode(null);
    toast('Enter a valid animation length');
    return;
  }
  extendSceneTo(window.end);
  const raw =
      editor.motionDraft.type === 'arrow'
        ? [editor.motionDraft.start, editor.motionDraft.end]
        : editor.motionDraft.points.filter((p, i, a) => i === 0 || Math.hypot(p.x - a[i - 1].x, p.y - a[i - 1].y) > 3),
    step = Math.max(1, Math.ceil(raw.length / 90)),
    sourcePoints = raw.filter((_, i) => i % step === 0 || i === raw.length - 1),
    m = {
      id: crypto.randomUUID(),
      type: editor.motionDraft.type,
      startTime: window.start,
      duration: window.end - window.start,
      distance,
      spin,
      face,
      startPose: {...start},
      sourcePoints,
      baseKeys: []
    };
  s.motions ??= [];
  const overlapping = s.motions.filter(x => x.startTime < window.end && x.startTime + x.duration > window.start);
  for (const old of overlapping) restoreMotionKeys(s, old);
  s.motions = s.motions.filter(x => !overlapping.includes(x));
  m.baseKeys = s.keys
    .filter(k => k.t >= window.start - 0.001 && k.t <= window.end + 0.001 && !k.motionId)
    .map(k => ({...k}));
  s.motions.push(m);
  applyMotion(s, m);
  editor.selectedMotionId = m.id;
  editor.selectedRotationId = null;
  editor.motionGuide = motionGuideFor(m);
  editor.motionDraft = null;
  editor.motionMode = null;
  $('stage').classList.remove('motion-mode');
  $('draw-arrow').classList.remove('active');
  $('record-path').classList.remove('active');
  editor.time = window.end;
  $('motion-help').textContent = 'Animation selected. Adjust its timing and settings, then preview it.';
  selectMotion(s, m);
  recordSceneChange('create motion');
  toast(`Animation created · ${m.duration.toFixed(1)} s · ${m.distance}% distance`);
}
function selectedAnimation() {
  const s = editor.scene.shapes.find(s => s.id === editor.selected);
  return {s, m: s?.motions?.find(m => m.id === editor.selectedMotionId)};
}
export function editMotion({preview = false, quiet = false} = {}) {
  const {s, m} = selectedAnimation();
  if (!m) return false;
  const requestedStart = +$('motion-start').value,
    requestedLength = +$('motion-length').value;
  if (
    !Number.isFinite(requestedStart) ||
    requestedStart < 0 ||
    !Number.isFinite(requestedLength) ||
    requestedLength < 0.1 ||
    !Number.isFinite(requestedStart + requestedLength)
  ) {
    toast('Enter a valid start and animation length');
    selectMotion(s, m);
    return false;
  }
  const start = Math.round(requestedStart * 100) / 100,
    length = Math.round(requestedLength * 100) / 100;
  if (
    s.motions.some(
      other =>
        other !== m && start < other.startTime + other.duration - 0.001 && start + length > other.startTime + 0.001
    )
  ) {
    toast('This animation overlaps another animation on the same shape');
    selectMotion(s, m);
    return false;
  }
  stop();
  extendSceneTo(start + length);
  restoreMotionKeys(s, m);
  m.startTime = start;
  m.duration = length;
  m.distance = Math.max(0, Math.min(500, +$('motion-edit-distance').value || 0));
  m.spin = Math.max(-10000, Math.min(10000, +$('motion-edit-spin').value || 0));
  m.face = $('motion-edit-face').checked;
  m.baseKeys = s.keys
    .filter(k => k.t >= start - 0.001 && k.t <= start + length + 0.001 && !k.motionId)
    .map(k => ({...k}));
  applyMotion(s, m);
  editor.motionGuide = motionGuideFor(m);
  editor.time = start;
  selectMotion(s, m);
  recordSceneChange('edit motion');
  if (!quiet) toast(`Animation updated · ${length.toFixed(2)} s`);
  if (preview) previewMotion(m);
  return true;
}
export function previewMotion(m) {
  if (!m) return;
  stop();
  editor.time = m.startTime;
  editor.previewEnd = Math.min(editor.scene.duration, m.startTime + m.duration);
  editor.playing = true;
  editor.last = performance.now();
  $('play').textContent = 'Ⅱ';
  $('play').setAttribute('aria-label', 'Pause preview');
  render();
}
let editPreviewTimer;

export function setupMotion() {
  $('draw-arrow').onclick = () => setMotionMode('arrow');
  $('record-path').onclick = () => setMotionMode('path');
  $('drop-item').onclick = () => {
    const s = editor.scene.shapes.find(shape => shape.id === editor.selected),
      requested = Number($('drop-duration').value);
    if (!s || (s.type === 'text' && s.linkId)) return;
    if (!Number.isFinite(requested) || requested < 0.4 || requested > 10)
      return toast('Choose a drop length from 0.4 to 10 seconds');
    const duration = Math.round(requested * 100) / 100,
      start = Math.round(editor.time * 100) / 100,
      end = Math.round((start + duration) * 100) / 100;
    if (
      (s.motions || []).some(m => m.startTime < end - 0.005 && m.startTime + m.duration > start + 0.005) ||
      s.keys.some(k => k.t > start + 0.005 && k.t <= end + 0.005)
    )
      return toast('Move the playhead to a clear section of this layer first');
    const landing = editableState(s),
      above = -landing.h / 2 - Math.max(40, editor.scene.height * 0.08);
    if (
      landing.x + landing.w / 2 < 0 ||
      landing.x - landing.w / 2 > editor.scene.width ||
      landing.y + landing.h / 2 < 0 ||
      landing.y - landing.h / 2 > editor.scene.height
    )
      return toast('Place the item on the canvas where it should land first');
    stop();
    if (editor.motionMode) setMotionMode(null);
    extendSceneTo(end);
    const at = (fraction, changes) => ({
      t: Math.round((start + duration * fraction) * 100) / 100,
      ...landing,
      ...changes
    });
    const squashY = landing.y + landing.h * 0.09;
    s.keys = s.keys.filter(k => k.t < start - 0.005 || k.t > end + 0.005);
    s.keys.push(
      at(0, {y: above}),
      at(0.18, {y: above + (landing.y - above) * 0.07}),
      at(0.6, {y: squashY, w: landing.w * 1.13, h: landing.h * 0.82}),
      at(0.71, {y: landing.y - landing.h * 0.17, w: landing.w * 0.94, h: landing.h * 1.06}),
      at(0.8, {y: landing.y - landing.h * 0.28}),
      at(0.91, {y: landing.y + landing.h * 0.04, w: landing.w * 1.06, h: landing.h * 0.93}),
      at(1, {})
    );
    s.keys.sort((a, b) => a.t - b.t);
    editor.selectedMotionId = editor.selectedRotationId = null;
    editor.motionGuide = null;
    editor.time = start;
    refresh();
    recordSceneChange('add drop animation');
    toast('Drop animation added');
    previewMotion({startTime: start, duration});
  };
  $('update-motion').onclick = () => editMotion({preview: $('live-motion-preview').checked});
  $('preview-motion').onclick = () => {
    const {m} = selectedAnimation();
    previewMotion(m);
  };
  for (const id of ['motion-start', 'motion-length', 'motion-edit-distance', 'motion-edit-spin', 'motion-edit-face'])
    $(id).addEventListener(id === 'motion-edit-face' ? 'change' : 'input', () => {
      if (!$('live-motion-preview').checked) return;
      clearTimeout(editPreviewTimer);
      editPreviewTimer = setTimeout(() => editMotion({preview: true, quiet: true}), 300);
    });
  $('delete-motion').onclick = () => {
    const {s, m} = selectedAnimation();
    if (!m) return;
    stop();
    restoreMotionKeys(s, m);
    s.motions = s.motions.filter(x => x.id !== m.id);
    editor.time = m.startTime;
    editor.selectedMotionId = editor.selectedRotationId = null;
    editor.motionGuide = null;
    $('motion-help').textContent = 'Animation deleted. Choose a tool to draw another motion.';
    refresh();
    toast('Animation deleted');
  };
}
