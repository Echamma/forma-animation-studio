// Clip editing controls: timing fields, Continue after / Split / Duplicate, copy and paste, and the clip menu.
import {$, toast} from '../dom.js';
import {clipEnd, clipStart, inClip} from '../model/animation.js';
import {cloneLayer, continueAfter, duplicateLayer, isLinkedText, MIN_CLIP, splitAt, trimClip} from '../model/clips.js';
import {recordSceneChange} from '../persistence.js';
import {stop} from '../playback.js';
import {editor} from '../state.js';
import {refresh} from './refresh.js';

let clipboard = null,
  menuTarget = null;
const menu = $('clip-menu');
const selectedLayer = () => editor.scene.shapes.find(s => s.id === editor.selected);

function focusOn(layer, message) {
  editor.selected = layer.id;
  editor.selectedMotionId = editor.selectedRotationId = null;
  editor.motionGuide = null;
  editor.time = clipStart(layer);
  refresh();
  recordSceneChange(message);
  toast(message);
}

// Each action reports problems as a toast instead of throwing into an event handler.
function act(action) {
  try {
    stop();
    action();
  } catch (error) {
    toast(error.message || 'That clip action is not possible here');
  }
}

export function continueSelected(template) {
  act(() => {
    const s = selectedLayer();
    if (!s) return toast('Select a clip first');
    const copy = continueAfter(s, template || s);
    focusOn(copy, `${copy.name} added at ${copy.inTime.toFixed(2)} s — drag it to its next position`);
  });
}
export function splitSelected() {
  act(() => {
    const s = selectedLayer();
    if (!s) return toast('Select a clip first');
    const second = splitAt(s, editor.time);
    focusOn(second, `Clip split at ${second.inTime.toFixed(2)} s`);
  });
}
export function duplicateSelected(source = selectedLayer()) {
  act(() => {
    if (!source) return toast('Select a layer first');
    const copy = duplicateLayer(source);
    editor.selected = copy.id;
    if (!inClip(copy, editor.time)) editor.time = clipStart(copy);
    refresh();
    recordSceneChange('duplicate');
    toast(`${copy.name} added`);
  });
}
function copySelected() {
  const s = selectedLayer();
  if (!s) return;
  clipboard = cloneLayer(s);
  toast(`${s.name} copied`);
}

export function clipFields(s) {
  const linked = !!s && isLinkedText(s);
  for (const id of ['clip-in', 'clip-out', 'duplicate-layer']) $(id).disabled = !s;
  for (const id of ['continue-clip', 'split-clip']) $(id).disabled = !s || linked;
  if (!s) return;
  $('clip-in').value = clipStart(s);
  $('clip-out').value = s.outTime ?? '';
  $('clip-out').placeholder = `End (${editor.scene.duration.toFixed(2)})`;
  $('split-clip').disabled ||= !(editor.time > clipStart(s) + MIN_CLIP && editor.time < clipEnd(s) - MIN_CLIP);
}

function closeClipMenu() {
  menu.hidden = true;
  menuTarget = null;
}
export function openClipMenu(e, s) {
  e.preventDefault();
  e.stopPropagation();
  stop();
  editor.selected = s.id;
  refresh();
  menuTarget = s;
  const linked = isLinkedText(s);
  $('clip-menu-title').textContent = s.name;
  menu.querySelector('[data-clip-action="continue"]').disabled = linked;
  menu.querySelector('[data-clip-action="paste-after"]').disabled = linked || !clipboard || isLinkedText(clipboard);
  menu.querySelector('[data-clip-action="split"]').disabled =
    linked || !(editor.time > clipStart(s) + MIN_CLIP && editor.time < clipEnd(s) - MIN_CLIP);
  menu.hidden = false;
  menu.style.left = `${Math.max(8, Math.min(e.clientX, innerWidth - menu.offsetWidth - 8))}px`;
  menu.style.top = `${Math.max(8, Math.min(e.clientY, innerHeight - menu.offsetHeight - 8))}px`;
  menu.querySelector('button:not(:disabled)')?.focus();
}

const typing = target => target.closest?.('input, textarea, select, [contenteditable="true"]');

export function setupClips() {
  $('clip-in').onchange = () =>
    act(() => {
      const s = selectedLayer(),
        value = Number($('clip-in').value);
      if (!s) return;
      if (!$('clip-in').value.trim() || !Number.isFinite(value) || value < 0 || value > clipEnd(s) - MIN_CLIP) {
        refresh();
        return toast('Start the clip at least 0.05 s before it ends');
      }
      trimClip(s, value, s.outTime);
      if (!inClip(s, editor.time)) editor.time = clipStart(s);
      refresh();
    });
  $('clip-out').onchange = () =>
    act(() => {
      const s = selectedLayer(),
        raw = $('clip-out').value.trim(),
        value = Number(raw);
      if (!s) return;
      if (raw && (!Number.isFinite(value) || value < clipStart(s) + MIN_CLIP)) {
        refresh();
        return toast('End the clip at least 0.05 s after it starts');
      }
      trimClip(s, clipStart(s), raw ? value : undefined);
      refresh();
    });
  $('continue-clip').onclick = () => continueSelected();
  $('split-clip').onclick = () => splitSelected();
  $('duplicate-layer').onclick = () => duplicateSelected();
  $('onion-skin').onchange = () => {
    editor.onionSkin = $('onion-skin').checked;
    refresh();
  };
  menu.onclick = e => {
    const action = e.target.closest('[data-clip-action]')?.dataset.clipAction,
      s = menuTarget;
    if (!action || !s) return;
    closeClipMenu();
    editor.selected = s.id;
    if (action === 'continue') continueSelected();
    if (action === 'paste-after') continueSelected(clipboard);
    if (action === 'split') splitSelected();
    if (action === 'duplicate') duplicateSelected(s);
    if (action === 'delete') $('delete').click();
  };
  document.addEventListener('pointerdown', e => {
    if (!menu.hidden && !menu.contains(e.target)) closeClipMenu();
  });
  document.addEventListener('keydown', e => {
    if (e.key === 'Escape' && !menu.hidden) return closeClipMenu();
    if (typing(e.target) || document.querySelector('dialog[open]') || document.body.classList.contains('mcp-working'))
      return;
    const command = e.ctrlKey || e.metaKey,
      key = e.key.toLowerCase();
    if (command && !e.shiftKey && key === 'c' && !getSelection().toString()) copySelected();
    else if (command && e.shiftKey && key === 'v') continueSelected(clipboard || undefined);
    else if (command && !e.shiftKey && key === 'v') {
      if (clipboard) duplicateSelected(clipboard);
    } else if (command && !e.shiftKey && key === 'd') duplicateSelected();
    else if (!command && !e.altKey && !e.shiftKey && key === 's') splitSelected();
    else return;
    e.preventDefault();
  });
}
