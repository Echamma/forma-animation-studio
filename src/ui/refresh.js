import {renderDisappear} from '../animate/disappear.js';
import {renderMotionList} from '../animate/motion.js';
import {renderRotationList} from '../animate/rotation.js';
import {$} from '../dom.js';
import {pasteboard, render} from '../render/canvas.js';
import {editor} from '../state.js';
import {lists} from '../timeline/tracks.js';
import {fields} from './inspector.js';
import {sizePasteboard} from './workspace.js';

export function refresh() {
  const v = pasteboard();
  sizePasteboard();
  $('stage').style.setProperty('--stage-ratio', v.w / v.h);
  render();
  fields();
  lists();
  renderMotionList();
  renderRotationList();
  renderDisappear();
  $('duration').value = editor.scene.duration;
  $('bg').value = editor.scene.bg;
  $('background-picker').value = editor.scene.bg;
  document
    .querySelectorAll('[data-bg]')
    .forEach(b => b.classList.toggle('active', b.dataset.bg.toLowerCase() === editor.scene.bg.toLowerCase()));
  $('format').value = `${editor.scene.width}x${editor.scene.height}`;
  $('canvas-size').innerHTML =
    `${editor.scene.width} × ${editor.scene.height} <span> • </span> ${editor.playing ? 'PREVIEW' : 'EDIT VIEW'}`;
  $('scrub').max = editor.scene.duration;
  document
    .querySelectorAll('.ruler span')
    .forEach((e, i) => (e.textContent = `${+((editor.scene.duration * i) / 4).toFixed(2)}s`));
}
