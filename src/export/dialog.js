import {$, download} from '../dom.js';
import {svgText} from './svg.js';
import {mediaElement, prepareAssets} from '../media.js';
import {stop} from '../playback.js';
import {render} from '../render/canvas.js';
import {paint} from '../render/paint.js';
import {editor} from '../state.js';

const audioSourceCache = new WeakMap();
let sharedAudioContext;

export function setupDialog() {
  $('export').onclick = () => {
    $('export-status').textContent = '';
    $('export-dialog').showModal();
  };
  $('svg-export').onclick = () => {
    download(new Blob([svgText(true)], {type: 'image/svg+xml'}), 'forma-animation.svg');
    $('export-status').textContent = editor.scene.shapes.some(s => s.type === 'video')
      ? 'Animated SVG exported with poster frames for video layers.'
      : 'Animated SVG exported.';
  };
  $('png-export').onclick = async () => {
    try {
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
    if (editor.recording) return;
    if (!window.MediaRecorder || !HTMLCanvasElement.prototype.captureStream) {
      $('export-status').textContent = 'Video recording is unavailable in this browser.';
      return;
    }
    stop();
    editor.recording = true;
    const c = document.createElement('canvas');
    c.width = editor.scene.width;
    c.height = editor.scene.height;
    const ctx = c.getContext('2d');
    let stream, audioContext, audioDestination, recorder;
    const audioSources = [];
    const finish = () => {
      stream?.getTracks().forEach(track => track.stop());
      for (const s of editor.scene.shapes) if (s.type === 'video') mediaElement(s).pause();
      for (const source of audioSources) {
        source.disconnect(audioDestination);
        source.connect(audioContext.destination);
      }
      editor.recording = false;
      render();
    };
    try {
      await prepareAssets(0);
      paint(ctx, 0);
      stream = c.captureStream(60);
      const videos = editor.scene.shapes.filter(s => s.type === 'video');
      if (videos.length) {
        audioContext = sharedAudioContext ??= new AudioContext();
        audioDestination = audioContext.createMediaStreamDestination();
        for (const s of videos) {
          const video = mediaElement(s);
          let source = audioSourceCache.get(video);
          if (!source) {
            source = audioContext.createMediaElementSource(video);
            audioSourceCache.set(video, source);
          }
          source.disconnect();
          source.connect(audioDestination);
          audioSources.push(source);
        }
        for (const track of audioDestination.stream.getAudioTracks()) stream.addTrack(track);
        await audioContext.resume();
      }
      const mime = ['video/webm;codecs=vp9,opus', 'video/webm;codecs=vp8,opus', 'video/webm', 'video/mp4'].find(t =>
        MediaRecorder.isTypeSupported(t)
      );
      recorder = new MediaRecorder(stream, mime ? {mimeType: mime} : {});
      const chunks = [];
      recorder.ondataavailable = e => {
        if (e.data.size) chunks.push(e.data);
      };
      recorder.onstop = () => {
        finish();
        download(
          new Blob(chunks, {type: recorder.mimeType}),
          'forma-animation.' + (recorder.mimeType.includes('mp4') ? 'mp4' : 'webm')
        );
        $('export-status').textContent = editor.scene.shapes.some(s => s.type === 'video')
          ? 'Video exported with imported audio.'
          : 'Video exported.';
      };
      recorder.onerror = () => {
        $('export-status').textContent = 'Could not record the video.';
        finish();
      };
      recorder.start();
      await Promise.all(videos.map(s => mediaElement(s).play()));
      const start = performance.now();
      function frame(now) {
        if (!editor.recording) return;
        const t = Math.min((now - start) / 1000, editor.scene.duration);
        paint(ctx, t);
        $('export-status').textContent =
          `Recording ${t.toFixed(1)} / ${editor.scene.duration} s — keep this tab visible`;
        if (t < editor.scene.duration) requestAnimationFrame(frame);
        else recorder.stop();
      }
      requestAnimationFrame(frame);
    } catch (error) {
      finish();
      $('export-status').textContent =
        'Could not record video with audio. Check that the imported video plays in Forma.';
    }
  };
}
