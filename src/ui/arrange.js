// Arrange panel: shown when several layers are selected; aligns or spaces them by keying each at the playhead.
import {$, toast} from '../dom.js';
import {editableState, state} from '../model/animation.js';
import {align, distribute} from '../model/arrange.js';
import {key} from '../model/keyframes.js';
import {recordSceneChange} from '../persistence.js';
import {stop} from '../playback.js';
import {editor} from '../state.js';
import {setLinkedTextPose} from './inspector.js';
import {refresh} from './refresh.js';
import {selectedLayers} from './selection.js';

export function arrangeFields() {
  const layers = selectedLayers();
  $('arrange-panel').hidden = layers.length < 2;
  if (layers.length < 2) return;
  $('arrange-count').textContent = `${layers.length} layers`;
  $('selected-title').textContent = `${layers[0].name} + ${layers.length - 1} more`;
  for (const button of document.querySelectorAll('[data-distribute]')) button.disabled = layers.length < 3;
}

function place(layers, centers) {
  stop();
  layers.forEach((s, i) => {
    const pose = {...editableState(s), x: centers[i].x, y: centers[i].y};
    if (s.type === 'text' && s.linkId) setLinkedTextPose(s, pose);
    else key(s, pose);
  });
  refresh();
  recordSceneChange('arrange');
  toast(`Arranged ${layers.length} layers at ${editor.time.toFixed(2)} s`);
}

export function setupArrange() {
  for (const button of document.querySelectorAll('[data-align]'))
    button.onclick = () => {
      const layers = selectedLayers();
      if (layers.length > 1) place(layers, align(layers.map(s => state(s)), button.dataset.align));
    };
  for (const button of document.querySelectorAll('[data-distribute]'))
    button.onclick = () => {
      const layers = selectedLayers();
      if (layers.length > 2) place(layers, distribute(layers.map(s => state(s)), button.dataset.distribute));
    };
}
