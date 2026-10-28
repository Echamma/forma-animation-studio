import {$, toast} from '../dom.js';
import {rawState, state} from '../model/animation.js';
import {stop} from '../playback.js';
import {render} from '../render/canvas.js';
import {editor} from '../state.js';
import {lists} from '../timeline/tracks.js';
import {refresh} from './refresh.js';
import {closeDrawer, showInspectorTab} from './tabs.js';

// With a layer selected, new text follows it; otherwise it is free text in the middle of the canvas.
export function addText() {
  const target = editor.scene.shapes.find(s => s.id === editor.selected);
  if (target && target.type !== 'text') return $('add-linked-text').click();
  stop();
  const fontSize = Math.round(Math.max(24, Math.min(editor.scene.width, editor.scene.height) * 0.08)),
    textWidth = fontSize * 4.2,
    id = crypto.randomUUID();
  editor.scene.shapes.push({
    id,
    name: 'Your text',
    type: 'text',
    text: 'Your text',
    linkId: '',
    offsetX: 0,
    offsetY: 0,
    textWidth,
    fontSize,
    color: '#1F3D2B',
    ease: 'smooth',
    keys: [{t: 0, x: editor.scene.width / 2, y: editor.scene.height / 2, w: textWidth, h: fontSize, r: 0, o: 100}]
  });
  editor.selected = id;
  editor.selectedMotionId = editor.selectedRotationId = null;
  editor.motionGuide = null;
  closeDrawer();
  showInspectorTab('edit');
  refresh();
  $('text-content').focus();
  $('text-content').select();
  toast('Text added. Type your words in the panel on the right.');
}

export function setupLayers() {
  document.querySelectorAll('[data-add]').forEach(
    b =>
      (b.onclick = () => {
        stop();
        const type = b.dataset.add,
          id = crypto.randomUUID(),
          size = Math.round(Math.min(editor.scene.width, editor.scene.height) * 0.18),
          w = type === 'arrow' ? size * 1.55 : size,
          h = type === 'arrow' ? size * 0.8 : size;
        editor.scene.shapes.push({
          id,
          name: `${type[0].toUpperCase() + type.slice(1)} ${editor.scene.shapes.length + 1}`,
          type,
          color: ['#ed795e', '#aaa0ce', '#c3d77f', '#83bfc0'][editor.scene.shapes.length % 4],
          ease: 'smooth',
          keys: [{t: 0, x: editor.scene.width / 2, y: editor.scene.height / 2, w, h, r: 0, o: 100}]
        });
        editor.selected = id;
        editor.selectedMotionId = editor.selectedRotationId = null;
        editor.motionGuide = null;
        closeDrawer();
        refresh();
      })
  );
  $('add-linked-text').onclick = () => {
    const target = editor.scene.shapes.find(s => s.id === editor.selected);
    if (!target || target.type === 'text') return toast('Select a shape, image, SVG, or video to link the text to');
    stop();
    const p = rawState(target),
      id = crypto.randomUUID(),
      fontSize = Math.max(24, Math.min(editor.scene.width, editor.scene.height) * 0.07),
      textWidth = fontSize * 4.2,
      offsetY = -(p.h / 2 + fontSize);
    editor.scene.shapes.push({
      id,
      name: `Text for ${target.name}`,
      type: 'text',
      text: 'Your text',
      linkId: target.id,
      linkBase: {w: p.w, h: p.h},
      offsetX: 0,
      offsetY,
      textWidth,
      fontSize,
      rotationOffset: 0,
      opacityFactor: 1,
      color: '#ffffff',
      ease: 'smooth',
      keys: [{t: 0, x: p.x, y: p.y + offsetY, w: textWidth, h: fontSize, r: p.r, o: p.o}]
    });
    editor.selected = id;
    editor.selectedMotionId = editor.selectedRotationId = null;
    editor.motionGuide = null;
    closeDrawer();
    showInspectorTab('edit');
    refresh();
    $('text-content').focus();
    $('text-content').select();
    toast(`Text linked to ${target.name}`);
  };
  $('text-content').oninput = () => {
    const s = editor.scene.shapes.find(s => s.id === editor.selected);
    if (s?.type === 'text') {
      s.text = $('text-content').value || 'Text';
      s.name = s.text.slice(0, 32) || 'Text';
      $('selected-title').textContent = s.name;
      render();
      lists();
    }
  };
  $('text-link').onchange = () => {
    const s = editor.scene.shapes.find(s => s.id === editor.selected),
      target = editor.scene.shapes.find(x => x.id === $('text-link').value);
    if (s?.type !== 'text' || !target) return;
    const visible = state(s),
      p = rawState(target),
      angle = (-p.r * Math.PI) / 180,
      dx = visible.x - p.x,
      dy = visible.y - p.y;
    s.linkId = target.id;
    s.linkBase = {w: p.w, h: p.h};
    s.offsetX = dx * Math.cos(angle) - dy * Math.sin(angle);
    s.offsetY = dx * Math.sin(angle) + dy * Math.cos(angle);
    s.textWidth = visible.w;
    s.fontSize = visible.h;
    s.rotationOffset = visible.r - p.r;
    s.opacityFactor = p.o ? visible.o / p.o : 1;
    refresh();
    toast(`Text now follows ${target.name}`);
  };
}
