import {$, toast} from '../dom.js';
import {animationClips, reappearAt} from '../model/animation.js';
import {extendSceneTo} from '../model/keyframes.js';
import {recordSceneChange} from '../persistence.js';
import {showPlaying, stop} from '../playback.js';
import {render} from '../render/canvas.js';
import {editor} from '../state.js';
import {refresh} from '../ui/refresh.js';

function showReappearFields() {
  const mode = $('reappear-mode').value;
  $('reappear-time-wrap').hidden = mode !== 'time';
  $('reappear-clip-wrap').hidden = mode !== 'clip';
}
export function renderDisappear() {
  const s = editor.scene.shapes.find(shape => shape.id === editor.selected),
    d = s?.disappear;
  $('add-disappear').disabled = !s;
  $('add-disappear').hidden = !!d;
  $('disappear-edit').hidden = !d;
  if (!d) return;
  $('disappear-start').value = d.startTime;
  $('disappear-length').value = d.duration;
  $('reappear-mode').value = d.reappearMode;
  $('reappear-time').value =
    d.reappearMode === 'time' ? d.reappearTime : Math.min(editor.scene.duration, d.startTime + d.duration + 1);
  const select = $('reappear-clip');
  select.replaceChildren();
  for (const {shape, clip, label} of animationClips(s.id)) {
    const option = document.createElement('option');
    option.value = `${shape.id}|${clip.id}`;
    option.textContent = `${label} · ends ${(clip.startTime + clip.duration).toFixed(2)} s`;
    select.append(option);
  }
  const wanted = `${d.triggerShapeId || ''}|${d.triggerClipId || ''}`;
  if ([...select.options].some(option => option.value === wanted)) select.value = wanted;
  else if (d.reappearMode === 'clip') {
    const option = document.createElement('option');
    option.value = '';
    option.textContent = 'Linked animation is missing';
    select.prepend(option);
    select.value = '';
  }
  if (!select.options.length) {
    const option = document.createElement('option');
    option.value = '';
    option.textContent = 'Animate another layer first';
    select.append(option);
  }
  $('disappear-help').textContent =
    d.reappearMode === 'clip' && !select.value
      ? 'Linked animation is missing. Choose another animation to restore reappearance.'
      : 'Fade out starts at the chosen time. Reappearance is instant.';
  showReappearFields();
}

export function setupDisappear() {
  $('reappear-mode').onchange = showReappearFields;
  $('add-disappear').onclick = () => {
    const s = editor.scene.shapes.find(shape => shape.id === editor.selected);
    if (!s) return;
    stop();
    s.disappear = {
      startTime: editor.time,
      duration: Math.min(0.5, Math.max(0, editor.scene.duration - editor.time)),
      reappearMode: 'never'
    };
    refresh();
    $('disappear-edit').scrollIntoView({block: 'nearest'});
    toast('Disappearance added');
  };
  $('update-disappear').onclick = () => {
    const s = editor.scene.shapes.find(shape => shape.id === editor.selected);
    if (!s?.disappear) return;
    const start = Number($('disappear-start').value),
      duration = Number($('disappear-length').value),
      mode = $('reappear-mode').value,
      reappearTime = Number($('reappear-time').value);
    if (
      !$('disappear-start').value.trim() ||
      !$('disappear-length').value.trim() ||
      !Number.isFinite(start) ||
      start < 0 ||
      !Number.isFinite(duration) ||
      duration < 0
    ) {
      toast('Enter a valid fade time and length');
      return;
    }
    const d = {
      startTime: Math.round(start * 100) / 100,
      duration: Math.round(duration * 100) / 100,
      reappearMode: mode
    };
    const fadeEnd = d.startTime + d.duration;
    if (mode === 'time') {
      if (!$('reappear-time').value.trim() || !Number.isFinite(reappearTime) || reappearTime < fadeEnd) {
        toast('Reappearance must follow the fade out');
        return;
      }
      d.reappearTime = Math.round(reappearTime * 100) / 100;
    } else if (mode === 'clip') {
      const [shapeId, clipId] = $('reappear-clip').value.split('|'),
        match = animationClips(s.id).find(({shape, clip}) => shape.id === shapeId && clip.id === clipId);
      if (!match) {
        toast('Choose an animation on another layer');
        return;
      }
      if (match.clip.startTime + match.clip.duration < fadeEnd - 0.001) {
        toast('That animation ends before the fade out finishes');
        return;
      }
      d.triggerShapeId = shapeId;
      d.triggerClipId = clipId;
    }
    stop();
    extendSceneTo(Math.max(fadeEnd, mode === 'time' ? d.reappearTime : 0));
    s.disappear = d;
    refresh();
    recordSceneChange('edit disappearance');
    toast('Disappearance updated');
  };
  $('delete-disappear').onclick = () => {
    const s = editor.scene.shapes.find(shape => shape.id === editor.selected);
    if (!s?.disappear) return;
    stop();
    delete s.disappear;
    refresh();
    toast('Disappearance removed');
  };
  $('preview-disappear').onclick = () => {
    const s = editor.scene.shapes.find(shape => shape.id === editor.selected),
      d = s?.disappear;
    if (!d) return;
    stop();
    editor.time = Math.max(0, d.startTime - 0.25);
    editor.previewEnd = Math.min(editor.scene.duration, Math.max(d.startTime + d.duration, reappearAt(s)) + 0.25);
    if (!Number.isFinite(editor.previewEnd)) editor.previewEnd = editor.scene.duration;
    editor.playing = true;
    editor.last = performance.now();
    showPlaying('Pause preview');
    render();
  };
}
