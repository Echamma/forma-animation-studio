import {$, toast} from '../dom.js';
import {assetCache, isAsset, mediaElement, mediaKey} from '../media.js';
import {editableState, props, rawState, rotationAt, state} from '../model/animation.js';
import {key} from '../model/keyframes.js';
import {stop} from '../playback.js';
import {layerBlur, render} from '../render/canvas.js';
import {editor} from '../state.js';
import {lists} from '../timeline/tracks.js';
import {removeKeyframe} from './keyframe-menu.js';
import {refresh} from './refresh.js';
import {clipFields} from './clips.js';

export function fields() {
  const s = editor.scene.shapes.find(s => s.id === editor.selected),
    asset = isAsset(s),
    isText = s?.type === 'text';
  $('selected-title').textContent = s ? s.name : 'Select a layer';
  $('inspector-hint').textContent = s
    ? `Editing at ${editor.time.toFixed(2)} s · Open Animate to add movement.`
    : 'Choose a layer on the canvas or in Layers.';
  $('base-rotation').disabled = !s;
  $('base-rotation').value = s ? Math.round((s.baseRotation || 0) * 100) / 100 : '';
  $('layer-blur').disabled = !s;
  $('layer-blur').value = s ? layerBlur(s) : 0;
  $('layer-blur-value').textContent = (s ? layerBlur(s) : 0) + ' px';
  for (const p of props) {
    $(p).disabled = !s;
    $(p).value = s ? Math.round((p === 'o' ? editableState(s)[p] : state(s)[p]) * 100) / 100 : '';
  }
  for (const id of ['ease', 'delete', 'keyframe', 'delete-key', 'draw-arrow', 'record-path']) $(id).disabled = !s;
  $('drop-item').disabled = !s || (isText && !!s.linkId);
  $('color').disabled = !s || asset;
  $('fill-control').classList.toggle('disabled', asset);
  $('edit-svg').hidden = s?.type !== 'svg';
  $('text-panel').hidden = !isText;
  clipFields(s);
  document.querySelectorAll('[data-outside]').forEach(b => (b.disabled = !s));
  if (s) {
    $('color').value = asset ? '#888888' : s.color;
    $('color-value').textContent = asset
      ? s.type === 'svg'
        ? 'VECTOR'
        : s.type === 'video'
          ? 'VIDEO'
          : 'IMAGE'
      : s.color.toUpperCase();
    $('ease').value = s.ease;
  }
  if (isText) {
    $('text-content').value = s.text || '';
    const link = $('text-link');
    link.replaceChildren();
    if (!s.linkId) link.append(new Option('Not linked', '', true, true));
    for (const target of editor.scene.shapes.filter(x => x.id !== s.id && x.type !== 'text')) {
      const option = document.createElement('option');
      option.value = target.id;
      option.textContent = target.name;
      option.selected = target.id === s.linkId;
      link.append(option);
    }
  }
}
export function setLinkedTextPose(s, pose) {
  const target = editor.scene.shapes.find(x => x.id === s.linkId);
  if (!target) return;
  const p = rawState(target);
  p.r += rotationAt(target, editor.time) + (target.baseRotation || 0);
  const base = s.linkBase || {w: p.w, h: p.h},
    sx = p.w / (base.w || p.w),
    sy = p.h / (base.h || p.h),
    angle = (-p.r * Math.PI) / 180,
    dx = pose.x - p.x,
    dy = pose.y - p.y;
  s.offsetX = (dx * Math.cos(angle) - dy * Math.sin(angle)) / (sx || 1);
  s.offsetY = (dx * Math.sin(angle) + dy * Math.cos(angle)) / (sy || 1);
  s.textWidth = pose.w / (Math.abs(sx) || 1);
  s.fontSize = pose.h / (Math.abs(sy) || 1);
  s.rotationOffset = pose.r - p.r - rotationAt(s, editor.time) - (s.baseRotation || 0);
  s.opacityFactor = p.o ? pose.o / p.o : 1;
}

export function setupInspector() {
  for (const p of props)
    $(p).oninput = () => {
      const s = editor.scene.shapes.find(s => s.id === editor.selected);
      if (!s) return;
      stop();
      const v = +$(p).value;
      if (!Number.isFinite(v)) return;
      const n = editableState(s);
      n[p] =
        p === 'r' && !(s.type === 'text' && s.linkId)
          ? v - rotationAt(s, editor.time) - (s.baseRotation || 0)
          : p === 'o'
            ? Math.max(0, Math.min(100, v))
            : p === 'w' || p === 'h'
              ? Math.max(5, Math.min(2000, v))
              : Math.max(-10000, Math.min(10000, v));
      if (s.type === 'text' && s.linkId) setLinkedTextPose(s, n);
      else key(s, n);
      render();
      lists();
    };
  $('base-rotation').oninput = () => {
    const s = editor.scene.shapes.find(s => s.id === editor.selected),
      value = +$('base-rotation').value;
    if (!s || !Number.isFinite(value)) return;
    stop();
    s.baseRotation = Math.max(-10000, Math.min(10000, value));
    render();
    $('r').value = Math.round(state(s).r * 100) / 100;
  };
  $('layer-blur').oninput = () => {
    const s = editor.scene.shapes.find(s => s.id === editor.selected);
    if (!s) return;
    stop();
    s.blur = +$('layer-blur').value;
    $('layer-blur-value').textContent = `${s.blur} px`;
    render();
  };
  $('color').oninput = () => {
    const s = editor.scene.shapes.find(s => s.id === editor.selected);
    if (s) {
      s.color = $('color').value;
      render();
      fields();
      lists();
    }
  };
  $('ease').onchange = () => {
    const s = editor.scene.shapes.find(s => s.id === editor.selected);
    if (s) {
      s.ease = $('ease').value;
      render();
    }
  };
  document.querySelectorAll('[data-outside]').forEach(
    b =>
      (b.onclick = () => {
        const s = editor.scene.shapes.find(s => s.id === editor.selected);
        if (!s) return;
        stop();
        const p = editableState(s),
          gap = Math.max(40, Math.min(editor.scene.width, editor.scene.height) * 0.05);
        if (b.dataset.outside === 'left') p.x = -p.w / 2 - gap;
        if (b.dataset.outside === 'right') p.x = editor.scene.width + p.w / 2 + gap;
        if (b.dataset.outside === 'top') p.y = -p.h / 2 - gap;
        if (b.dataset.outside === 'bottom') p.y = editor.scene.height + p.h / 2 + gap;
        if (s.type === 'text' && s.linkId) setLinkedTextPose(s, p);
        else key(s, p);
        refresh();
        toast(`Placed outside at ${editor.time.toFixed(2)} s`);
      })
  );
  $('delete').onclick = () => {
    const s = editor.scene.shapes.find(s => s.id === editor.selected);
    if (!s) return;
    for (const linked of editor.scene.shapes.filter(x => x.linkId === s.id)) {
      const p = state(linked);
      linked.keys = [{t: 0, ...p}];
      delete linked.linkId;
      delete linked.linkBase;
    }
    if (s.type === 'video') mediaElement(s).pause();
    if (isAsset(s)) assetCache.delete(mediaKey(s));
    editor.scene.shapes = editor.scene.shapes.filter(x => x.id !== editor.selected);
    editor.selected = editor.scene.shapes.at(-1)?.id;
    editor.selectedMotionId = editor.selectedRotationId = null;
    editor.motionGuide = null;
    refresh();
  };
  $('keyframe').onclick = () => {
    const s = editor.scene.shapes.find(s => s.id === editor.selected);
    if (s) {
      stop();
      key(s, editableState(s));
      refresh();
      toast('Keyframe added at ' + editor.time.toFixed(2) + ' s');
    }
  };
  $('delete-key').onclick = () => {
    const s = editor.scene.shapes.find(s => s.id === editor.selected);
    if (!s) return;
    const k = s.keys.find(k => Math.abs(k.t - editor.time) < 0.011);
    if (!k) return toast('Select a keyframe on the timeline first');
    if (!removeKeyframe(s, k)) return toast('Keep at least one keyframe per shape');
    refresh();
  };
}
