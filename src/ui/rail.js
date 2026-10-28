// The tool rail on the left of the editor and the quick-animate bar under the canvas.
import {setMotionMode} from '../animate/motion.js';
import {$, toast} from '../dom.js';
import {editor} from '../state.js';
import {addText} from './layers.js';
import {closeDrawer, showInspectorTab} from './tabs.js';

const selectedLayer = () => editor.scene.shapes.find(s => s.id === editor.selected);
let quickLabel = null;

// Called on every render: the bar shows while a layer is selected and nothing plays or is being drawn.
export function showQuickAnimate() {
  const s = selectedLayer(),
    drawing = !!editor.motionMode || !!editor.motionDraft;
  $('quick-animate').hidden = !s || editor.playing || editor.recording || drawing;
  if (s && quickLabel !== s.name) {
    quickLabel = s.name;
    $('quick-animate-label').textContent = `Animate ${s.name}`;
  }
  $('tool-move').setAttribute('aria-pressed', String(drawing));
  $('tool-select').setAttribute('aria-pressed', String(!drawing));
}

function drawMove() {
  if (!selectedLayer()) return toast('Select a layer, then draw where it should go');
  if (document.querySelector('main').classList.contains('drawer-open')) closeDrawer();
  showInspectorTab('animate');
  if (editor.motionMode !== 'arrow') setMotionMode('arrow');
}

export function setupRail() {
  $('tool-select').onclick = () => {
    if (editor.motionMode) setMotionMode(null);
    if (document.querySelector('main').classList.contains('drawer-open')) closeDrawer();
  };
  $('tool-text').onclick = () => addText();
  $('tool-picture').onclick = () => $('asset-file').click();
  $('tool-move').onclick = () => (editor.motionMode ? setMotionMode(null) : drawMove());
  $('tool-scene').onclick = $('scene-button').onclick = () => showInspectorTab('scene', true);
  $('quick-move').onclick = drawMove;
  $('quick-spin').onclick = () => {
    showInspectorTab('animate');
    $('add-rotation').click();
  };
  $('quick-fade').onclick = () => {
    showInspectorTab('animate');
    $('add-disappear').click();
  };
  $('quick-drop').onclick = () => {
    if ($('drop-item').disabled) return toast('Text that follows a shape drops with its shape');
    $('drop-item').click();
  };
}
