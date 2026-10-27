import {$} from '../dom.js';
import {pasteboard, render} from '../render/canvas.js';
import {editor} from '../state.js';

export function sizePasteboard(center = false) {
  const viewport = $('stage-viewport'),
    v = pasteboard();
  const availableWidth = Math.max(200, viewport.clientWidth - 24),
    availableHeight = Math.max(180, viewport.clientHeight - 24);
  const artboardWidth = Math.min(
    editor.scene.width,
    availableWidth * 0.92,
    (availableHeight * 0.92 * editor.scene.width) / editor.scene.height
  );
  $('stage').style.width = `${((artboardWidth * v.w) / editor.scene.width) * editor.workspaceZoom}px`;
  $('stage').style.height = `${((artboardWidth * v.h) / editor.scene.width) * editor.workspaceZoom}px`;
  if (center) {
    editor.workspacePanX = editor.workspacePanY = 0;
  }
  $('stage').style.transform = `translate(-50%, -50%) translate(${editor.workspacePanX}px, ${editor.workspacePanY}px)`;
  $('zoom-level').textContent = `${Math.round(editor.workspaceZoom * 100)}%`;
}
function setWorkspaceZoom(next, clientX, clientY) {
  const viewport = $('stage-viewport'),
    rect = viewport.getBoundingClientRect(),
    old = editor.workspaceZoom;
  editor.workspaceZoom = Math.max(0.2, Math.min(5, next));
  const x = clientX - rect.left - rect.width / 2,
    y = clientY - rect.top - rect.height / 2,
    ratio = editor.workspaceZoom / old;
  editor.workspacePanX = x - (x - editor.workspacePanX) * ratio;
  editor.workspacePanY = y - (y - editor.workspacePanY) * ratio;
  sizePasteboard();
  render();
}
const workspaceViewport = $('stage-viewport');
function endWorkspacePan(e) {
  if (!editor.workspacePan || e.pointerId !== editor.workspacePan.id) return;
  editor.workspacePan = null;
  workspaceViewport.classList.remove('panning');
}

export function setupWorkspace() {
  workspaceViewport.addEventListener('pointerdown', e => {
    if (e.button !== 1) return;
    const svg = $('canvas'),
      matrix = svg.getScreenCTM();
    if (matrix) {
      const p = new DOMPoint(e.clientX, e.clientY).matrixTransform(matrix.inverse());
      if (p.x >= 0 && p.x <= editor.scene.width && p.y >= 0 && p.y <= editor.scene.height) return;
    }
    e.preventDefault();
    editor.workspacePan = {
      x: e.clientX,
      y: e.clientY,
      panX: editor.workspacePanX,
      panY: editor.workspacePanY,
      id: e.pointerId
    };
    workspaceViewport.setPointerCapture(e.pointerId);
    workspaceViewport.classList.add('panning');
  });
  workspaceViewport.addEventListener('pointermove', e => {
    if (!editor.workspacePan || e.pointerId !== editor.workspacePan.id) return;
    editor.workspacePanX = editor.workspacePan.panX + e.clientX - editor.workspacePan.x;
    editor.workspacePanY = editor.workspacePan.panY + e.clientY - editor.workspacePan.y;
    sizePasteboard();
  });
  workspaceViewport.addEventListener('pointerup', endWorkspacePan);
  workspaceViewport.addEventListener('pointercancel', endWorkspacePan);
  workspaceViewport.addEventListener('auxclick', e => {
    if (e.button === 1) e.preventDefault();
  });
  workspaceViewport.addEventListener(
    'wheel',
    e => {
      e.preventDefault();
      setWorkspaceZoom(editor.workspaceZoom * Math.exp(-e.deltaY * 0.0015), e.clientX, e.clientY);
    },
    {passive: false}
  );
  for (const [id, factor] of [
    ['zoom-in', 1.25],
    ['zoom-out', 0.8]
  ])
    $(id).onclick = () => {
      const r = workspaceViewport.getBoundingClientRect();
      setWorkspaceZoom(editor.workspaceZoom * factor, r.left + r.width / 2, r.top + r.height / 2);
    };
  $('zoom-level').onclick = () => {
    editor.workspaceZoom = 1;
    sizePasteboard(true);
    render();
  };
  new ResizeObserver(() => {
    if (editor.scene) sizePasteboard();
  }).observe($('stage-viewport'));
}
