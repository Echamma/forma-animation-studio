import {$, toast} from '../dom.js';
import {motionGuideFor} from '../model/keyframes.js';
import {stop} from '../playback.js';
import {render} from '../render/canvas.js';
import {editor} from '../state.js';
import {refresh} from './refresh.js';

function setBackground(color) {
  editor.scene.bg = color;
  $('bg').value = color;
  $('background-picker').value = color;
  document
    .querySelectorAll('[data-bg]')
    .forEach(b => b.classList.toggle('active', b.dataset.bg.toLowerCase() === color.toLowerCase()));
  render();
}

export function setupSceneSettings() {
  $('bg').oninput = () => setBackground($('bg').value);
  $('background-picker').oninput = () => setBackground($('background-picker').value);
  document.querySelectorAll('[data-bg]').forEach(b => (b.onclick = () => setBackground(b.dataset.bg)));
  $('duration').onchange = () => {
    const next = +$('duration').value;
    if (!Number.isFinite(next) || next < 1) {
      $('duration').value = editor.scene.duration;
      toast('Enter a scene length of at least 1 second');
      return;
    }
    const latest = Math.max(
      0,
      ...editor.scene.shapes.flatMap(s => [
        ...s.keys.map(k => k.t),
        s.inTime ?? 0,
        s.outTime ?? 0,
        ...(s.motions || []).map(m => m.startTime + m.duration),
        ...(s.rotationClips || []).map(c => c.startTime + c.duration),
        ...(s.disappear
          ? [
              s.disappear.startTime + s.disappear.duration,
              ...(s.disappear.reappearMode === 'time' ? [s.disappear.reappearTime] : [])
            ]
          : [])
      ])
    );
    if (next + 1e-9 < latest) {
      $('duration').value = editor.scene.duration;
      toast(`Move animations or keyframes before shortening below ${latest.toFixed(2)} s`);
      return;
    }
    stop();
    editor.scene.duration = next;
    editor.time = Math.min(editor.time, next);
    refresh();
  };
  $('format').onchange = () => {
    stop();
    const [width, height] = $('format').value.split('x').map(Number),
      sx = width / editor.scene.width,
      sy = height / editor.scene.height,
      scale = Math.min(sx, sy);
    for (const s of editor.scene.shapes) {
      for (const k of s.keys) {
        k.x *= sx;
        k.y *= sy;
        k.w *= scale;
        k.h *= scale;
      }
      if (s.type === 'text') {
        s.offsetX *= scale;
        s.offsetY *= scale;
        s.textWidth *= scale;
        s.fontSize *= scale;
        if (s.linkBase) {
          s.linkBase.w *= scale;
          s.linkBase.h *= scale;
        }
      }
      for (const m of s.motions || []) {
        m.startPose.x *= sx;
        m.startPose.y *= sy;
        m.startPose.w *= scale;
        m.startPose.h *= scale;
        for (const p of m.sourcePoints) {
          p.x *= sx;
          p.y *= sy;
        }
        for (const k of m.baseKeys || []) {
          k.x *= sx;
          k.y *= sy;
          k.w *= scale;
          k.h *= scale;
        }
      }
    }
    editor.scene.width = width;
    editor.scene.height = height;
    if (editor.selectedMotionId) {
      const s = editor.scene.shapes.find(s => s.id === editor.selected),
        m = s?.motions?.find(m => m.id === editor.selectedMotionId);
      if (m) editor.motionGuide = motionGuideFor(m);
    }
    refresh();
    toast(width < height ? 'Canvas changed to 9:16 portrait' : 'Canvas changed to landscape');
  };
}
