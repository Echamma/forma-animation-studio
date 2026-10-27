import {finishMotion} from '../animate/motion.js';
import {$} from '../dom.js';
import {editableState, state} from '../model/animation.js';
import {resizedPose} from '../model/geometry.js';
import {key} from '../model/keyframes.js';
import {recordSceneChange} from '../persistence.js';
import {stop} from '../playback.js';
import {render} from '../render/canvas.js';
import {editor} from '../state.js';
import {lists} from '../timeline/tracks.js';
import {fields, setLinkedTextPose} from './inspector.js';
import {refresh} from './refresh.js';
import {focusInSelection, selectedLayers, toggleInSelection} from './selection.js';

let drag = null;
let rotationDrag = null;
let resizeDrag = null;
function point(e) {
  const svg = $('canvas'),
    p = new DOMPoint(e.clientX, e.clientY).matrixTransform(svg.getScreenCTM().inverse());
  return {x: p.x, y: p.y};
}

export function setupCanvasInput() {
  $('canvas').onpointerdown = e => {
    if (e.button != null && e.button !== 0) return;
    if (editor.motionMode) {
      const s = editor.scene.shapes.find(s => s.id === editor.selected);
      if (!s) return;
      stop();
      const p = point(e),
        startState = editableState(s);
      editor.motionDraft = {
        type: editor.motionMode,
        shapeId: s.id,
        start: {x: startState.x, y: startState.y},
        end: p,
        startState,
        points: editor.motionMode === 'path' ? [{x: startState.x, y: startState.y}, p] : undefined
      };
      $('canvas').setPointerCapture(e.pointerId);
      render();
      return;
    }
    const resizeHandle = e.target.closest('[data-resize-id]');
    if (resizeHandle) {
      const s = editor.scene.shapes.find(s => s.id === resizeHandle.getAttribute('data-resize-id'));
      if (!s) return;
      e.preventDefault?.();
      stop();
      editor.selected = s.id;
      editor.selectedMotionId = editor.selectedRotationId = null;
      editor.motionGuide = null;
      resizeDrag = {
        id: s.id,
        handle: resizeHandle.getAttribute('data-resize-handle'),
        start: point(e),
        value: editableState(s),
        angle: state(s).r
      };
      $('canvas').setPointerCapture(e.pointerId);
      refresh();
      return;
    }
    const handle = e.target.closest('[data-rotate-id]');
    if (handle) {
      const s = editor.scene.shapes.find(s => s.id === handle.getAttribute('data-rotate-id'));
      if (!s) return;
      stop();
      editor.selected = s.id;
      editor.selectedMotionId = editor.selectedRotationId = null;
      editor.motionGuide = null;
      const center = state(s),
        cursor = point(e);
      rotationDrag = {
        id: s.id,
        center: {x: center.x, y: center.y},
        lastAngle: (Math.atan2(cursor.y - center.y, cursor.x - center.x) * 180) / Math.PI
      };
      $('canvas').setPointerCapture(e.pointerId);
      refresh();
      return;
    }
    const g = e.target.closest('[data-id]');
    if (!g) return;
    stop();
    if (e.shiftKey) {
      toggleInSelection(g.dataset.id);
      refresh();
      return;
    }
    focusInSelection(g.dataset.id);
    // Every selected layer moves together; linked text whose shape is also selected follows it on its own.
    const group = selectedLayers(),
      moving = group.filter(s => !(s.type === 'text' && s.linkId && group.some(x => x.id === s.linkId)));
    drag = {start: point(e), layers: moving.map(s => ({id: s.id, value: editableState(s)}))};
    $('canvas').setPointerCapture(e.pointerId);
    refresh();
  };
  $('canvas').onpointermove = e => {
    if (rotationDrag) {
      const s = editor.scene.shapes.find(s => s.id === rotationDrag.id);
      if (!s) return;
      const cursor = point(e),
        angle = (Math.atan2(cursor.y - rotationDrag.center.y, cursor.x - rotationDrag.center.x) * 180) / Math.PI,
        delta = ((angle - rotationDrag.lastAngle + 540) % 360) - 180;
      s.baseRotation = Math.max(-10000, Math.min(10000, (s.baseRotation || 0) + delta));
      rotationDrag.lastAngle = angle;
      render();
      fields();
      return;
    }
    if (resizeDrag) {
      const s = editor.scene.shapes.find(s => s.id === resizeDrag.id);
      if (!s) return;
      const cursor = point(e),
        delta = {x: cursor.x - resizeDrag.start.x, y: cursor.y - resizeDrag.start.y},
        pose = resizedPose(resizeDrag.value, resizeDrag.angle, resizeDrag.handle, delta, e.shiftKey);
      if (s.type === 'text' && s.linkId) setLinkedTextPose(s, pose);
      else key(s, pose);
      render();
      fields();
      return;
    }
    if (editor.motionDraft) {
      const p = point(e);
      editor.motionDraft.end = p;
      if (editor.motionDraft.type === 'path') {
        const lastPoint = editor.motionDraft.points.at(-1);
        if (Math.hypot(p.x - lastPoint.x, p.y - lastPoint.y) > 3) editor.motionDraft.points.push(p);
      }
      render();
      return;
    }
    if (!drag) return;
    const p = point(e);
    for (const {id, value} of drag.layers) {
      const s = editor.scene.shapes.find(s => s.id === id);
      if (!s) continue;
      const pose = {...value, x: value.x + p.x - drag.start.x, y: value.y + p.y - drag.start.y};
      if (s.type === 'text' && s.linkId) setLinkedTextPose(s, pose);
      else key(s, pose);
    }
    render();
    fields();
  };
  $('canvas').onpointerup = () => {
    if (rotationDrag) {
      rotationDrag = null;
      fields();
      recordSceneChange('canvas gesture');
      return;
    }
    if (resizeDrag) {
      resizeDrag = null;
      lists();
      recordSceneChange('canvas gesture');
      return;
    }
    if (editor.motionDraft) {
      finishMotion();
      return;
    }
    drag = null;
    lists();
    recordSceneChange('canvas gesture');
  };
  $('canvas').onpointercancel = () => {
    editor.motionDraft = null;
    rotationDrag = null;
    resizeDrag = null;
    drag = null;
    render();
    recordSceneChange('canvas gesture');
  };
}
