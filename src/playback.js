import {setMotionMode} from './animate/motion.js';
import {$} from './dom.js';
import {mediaElement} from './media.js';
import {render} from './render/canvas.js';
import {editor} from './state.js';
import {fields} from './ui/inspector.js';
import {refresh} from './ui/refresh.js';

export function stop() {
  editor.playing = false;
  editor.previewEnd = null;
  for (const s of editor.scene?.shapes || []) if (s.type === 'video') mediaElement(s).pause();
  $('play').textContent = '▶';
  $('play').setAttribute('aria-label', 'Play animation');
  if (editor.scene)
    $('canvas-size').innerHTML = `${editor.scene.width} × ${editor.scene.height} <span> • </span> EDIT VIEW`;
}
export function tick(now) {
  if (editor.playing) {
    editor.time += (now - editor.last) / 1000;
    const end = editor.previewEnd ?? editor.scene.duration;
    if (editor.time >= end) {
      if (editor.previewEnd !== null) {
        editor.time = end;
        stop();
        fields();
      } else if ($('loop').checked) editor.time %= editor.scene.duration;
      else {
        editor.time = editor.scene.duration;
        stop();
        fields();
      }
    }
    render();
  }
  editor.last = now;
  requestAnimationFrame(tick);
}

export function setupPlayback() {
  $('play').onclick = () => {
    if (editor.playing) {
      stop();
      fields();
      render();
    } else {
      if (editor.motionMode) setMotionMode(null);
      if (editor.time >= editor.scene.duration) editor.time = 0;
      editor.playing = true;
      editor.last = performance.now();
      $('play').textContent = 'Ⅱ';
      $('play').setAttribute('aria-label', 'Pause animation');
      $('canvas-size').innerHTML = `${editor.scene.width} × ${editor.scene.height} <span> • </span> CLIPPED PREVIEW`;
      render();
    }
  };
  $('start').onclick = () => {
    stop();
    editor.time = 0;
    refresh();
  };
  $('scrub').oninput = e => {
    stop();
    editor.time = +e.target.value;
    render();
    fields();
  };
}
