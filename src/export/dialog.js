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

export function setupDialog() {
  $('export').onclick = () => {
    $('export-status').textContent = '';
    $('export-dialog').showModal();
  };
  $('svg-export').onclick = async () => {
    download(new Blob([await inlineMedia(svgText(true))], {type: 'image/svg+xml'}), 'forma-animation.svg');
    $('export-status').textContent = editor.scene.shapes.some(s => s.type === 'video')
      ? 'Animated SVG exported with poster frames for video layers.'
      : 'Animated SVG exported.';
  };
  $('project-export').onclick = async () => {
    try {
      $('export-status').textContent = 'Packing the project and its media…';
      const name = (editor.scene.projectName || 'Forma project').replace(/[\\/:*?"<>|]+/g, ' ').trim();
      download(await packProjectFile(), `${name || 'Forma project'}.forma`);
      $('export-status').textContent = 'Project file exported. Open it in Forma on any computer.';
    } catch (error) {
      $('export-status').textContent = 'Could not pack the project: ' + error.message;
    }
  };
  $('png-export').onclick = async () => {
    try {
      await ensureFonts();
      await prepareAssets(editor.time);
      const c = document.createElement('canvas');
      c.width = editor.scene.width;
      c.height = editor.scene.height;
      paint(c.getContext('2d'), editor.time);
      c.toBlob(b => {
        download(b, 'forma-frame.png');
        $('export-status').textContent = 'Current frame exported.';
      });
    } catch {
      $('export-status').textContent = 'A media layer could not be prepared for export.';
    }
  };
  $('video-export').onclick = async () => {
    if (videoExport || editor.recording) return;
    stop();
    const format = $('video-format').value,
      progress = $('video-progress');
    videoExport = new AbortController();
    progress.value = 0;
    progress.hidden = $('video-cancel').hidden = false;
    $('video-export').disabled = true;
    try {
      const {blob, audioCodec} = await renderVideo({
        format,
        fps: Number($('video-fps').value),
        scale: Number($('video-scale').value),
        signal: videoExport.signal,
        onProgress: done => {
          progress.value = done;
          $('export-status').textContent = `Rendering frames… ${Math.round(done * 100)}%`;
        }
      });
      download(blob, `forma-animation.${format}`);
      $('export-status').textContent = audioCodec ? 'Video exported with sound from your video layers.' : 'Video exported.';
    } catch (error) {
      $('export-status').textContent =
        error.name === 'AbortError' ? 'Video export cancelled.' : 'Could not export the video: ' + error.message;
    } finally {
      videoExport = null;
      progress.hidden = $('video-cancel').hidden = true;
      $('video-export').disabled = false;
      render();
    }
  };
  $('video-cancel').onclick = () => videoExport?.abort();
}
