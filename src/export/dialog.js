import {$, download} from '../dom.js';
import {svgText} from './svg.js';
import {packProjectFile} from './project-file.js';
import {renderVideo} from './video.js';
import {inlineMedia} from '../media-library.js';
import {prepareAssets} from '../media.js';
import {stop} from '../playback.js';
import {render} from '../render/canvas.js';
import {ensureFonts, paint} from '../render/paint.js';
import {editor} from '../state.js';

let videoExport = null;
const LABELS = {video: 'Export video', svg: 'Export SVG', png: 'Save picture', project: 'Save project file'};
const choice = name => document.querySelector(`input[name="${name}"]:checked`)?.value;
const status = text => ($('export-status').textContent = text);

// What the chosen export will contain, shown above the button.
function describeExport() {
  const kind = choice('export-kind'),
    scene = editor.scene,
    fps = Number(choice('video-fps')),
    scale = Number(choice('video-scale')),
    sound = scene.shapes.some(s => s.type === 'video'),
    summary = $('export-summary');
  $('video-options').hidden = kind !== 'video';
  $('export-run-label').textContent = LABELS[kind];
  $('video-size-hint').textContent = `${scene.width * scale} × ${scene.height * scale} pixels`;
  summary.replaceChildren();
  const line = document.createElement('b');
  if (kind === 'video') line.textContent = `${scene.duration.toFixed(1)} seconds · ${Math.round(scene.duration * fps)} frames`;
  else if (kind === 'svg') line.textContent = `${scene.duration.toFixed(1)} seconds, looping`;
  else if (kind === 'png') line.textContent = `The frame at ${editor.time.toFixed(2)} s · ${scene.width} × ${scene.height}`;
  else line.textContent = `${scene.shapes.length} layer${scene.shapes.length === 1 ? '' : 's'} with their pictures and video`;
  summary.append(line);
  if (kind === 'video' && sound) {
    const badge = document.createElement('span');
    badge.className = 'badge';
    badge.textContent = 'Includes sound';
    summary.append(badge);
  }
  if (kind === 'svg' && sound) {
    const note = document.createElement('span');
    note.className = 'muted';
    note.textContent = 'Video layers show their first frame.';
    summary.append(note);
  }
}

async function exportSvg() {
  download(new Blob([await inlineMedia(svgText(true))], {type: 'image/svg+xml'}), 'forma-animation.svg');
  status('Animated SVG saved.');
}
async function exportProject() {
  status('Packing the project and its media…');
  const name = (editor.scene.projectName || 'Forma project').replace(/[\\/:*?"<>|]+/g, ' ').trim();
  download(await packProjectFile(), `${name || 'Forma project'}.forma`);
  status('Project file saved. Open it in Forma on any computer.');
}
async function exportPng() {
  await ensureFonts();
  await prepareAssets(editor.time);
  const c = document.createElement('canvas');
  c.width = editor.scene.width;
  c.height = editor.scene.height;
  paint(c.getContext('2d'), editor.time);
  const blob = await new Promise(resolve => c.toBlob(resolve));
  download(blob, 'forma-frame.png');
  status('Picture saved.');
}
async function exportVideo() {
  const format = choice('video-format'),
    progress = $('video-progress');
  videoExport = new AbortController();
  progress.value = 0;
  progress.hidden = $('video-cancel').hidden = false;
  try {
    const {blob, audioCodec} = await renderVideo({
      format,
      fps: Number(choice('video-fps')),
      scale: Number(choice('video-scale')),
      signal: videoExport.signal,
      onProgress: done => {
        progress.value = done;
        status(`Rendering frames… ${Math.round(done * 100)}%`);
      }
    });
    download(blob, `forma-animation.${format}`);
    status(audioCodec ? 'Video saved with sound from your video layers.' : 'Video saved.');
  } finally {
    videoExport = null;
    progress.hidden = $('video-cancel').hidden = true;
    render();
  }
}

export function setupDialog() {
  $('export').onclick = () => {
    status('');
    describeExport();
    $('export-dialog').showModal();
  };
  $('export-dialog').addEventListener('change', describeExport);
  $('export-run').onclick = async () => {
    if (videoExport || editor.recording) return;
    stop();
    $('export-run').disabled = true;
    try {
      const kind = choice('export-kind');
      if (kind === 'video') await exportVideo();
      else if (kind === 'svg') await exportSvg();
      else if (kind === 'png') await exportPng();
      else await exportProject();
    } catch (error) {
      status(error.name === 'AbortError' ? 'Video export stopped.' : 'Could not export: ' + (error.message || 'a layer could not be prepared'));
    } finally {
      $('export-run').disabled = false;
    }
  };
  $('video-cancel').onclick = () => videoExport?.abort();
}
