// Frame-by-frame video export. Each frame is painted at its exact time and encoded with WebCodecs, so no frames are
// dropped, rendering does not wait for real time, and the window can be hidden. Sound from video layers is mixed
// offline and placed where each clip plays.
import {
  ALL_FORMATS,
  AudioBufferSource,
  BlobSource,
  BufferTarget,
  CanvasSource,
  EncodedPacketSink,
  getFirstEncodableAudioCodec,
  getFirstEncodableVideoCodec,
  Input,
  Mp4OutputFormat,
  Output,
  QUALITY_HIGH,
  WebMOutputFormat
} from 'mediabunny';
import {mediaElement, prepareAssets} from '../media.js';
import {clipEnd, clipStart} from '../model/animation.js';
import {ensureFonts, paint} from '../render/paint.js';
import {editor} from '../state.js';

// Codecs in order of preference: H.264 and AAC make MP4 files that play everywhere.
const FORMATS = {
  mp4: {create: () => new Mp4OutputFormat({fastStart: 'in-memory'}), video: ['avc', 'hevc', 'vp9', 'av1'], audio: ['aac', 'opus'], mimeType: 'video/mp4'},
  webm: {create: () => new WebMOutputFormat(), video: ['vp9', 'vp8', 'av1'], audio: ['opus'], mimeType: 'video/webm'}
};
const SAMPLE_RATE = 48000;

// One stereo soundtrack: every video layer plays (and loops) its audio for as long as its clip is on screen.
async function mixSoundtrack(scene) {
  const layers = scene.shapes.filter(s => s.type === 'video' && clipEnd(s) > clipStart(s));
  if (!layers.length) return null;
  const mix = new OfflineAudioContext(2, Math.max(1, Math.ceil(scene.duration * SAMPLE_RATE)), SAMPLE_RATE),
    decoded = new Map();
  let playing = 0;
  for (const s of layers) {
    if (!decoded.has(s.assetData))
      decoded.set(
        s.assetData,
        fetch(s.assetData)
          .then(response => response.arrayBuffer())
          .then(bytes => mix.decodeAudioData(bytes))
          .catch(() => null)
      );
    const buffer = await decoded.get(s.assetData);
    if (!buffer) continue;
    const start = clipStart(s),
      end = Math.min(clipEnd(s), scene.duration),
      source = mix.createBufferSource();
    source.buffer = buffer;
    source.loop = true;
    source.connect(mix.destination);
    source.start(start, (s.mediaStart ?? 0) % buffer.duration, end - start);
    playing++;
  }
  return playing ? mix.startRendering() : null;
}

export async function renderVideo({format = 'mp4', fps = 30, scale = 1, onProgress = () => {}, signal} = {}) {
  const spec = FORMATS[format];
  if (!spec) throw Error(`Unknown video format: ${format}`);
  if (!window.VideoEncoder) throw Error('Video encoding is unavailable in this runtime');
  const scene = editor.scene,
    width = Math.max(2, Math.round((scene.width * scale) / 2) * 2),
    height = Math.max(2, Math.round((scene.height * scale) / 2) * 2),
    frames = Math.max(1, Math.round(scene.duration * fps));
  const videoCodec = await getFirstEncodableVideoCodec(spec.video, {width, height, frameRate: fps, quality: QUALITY_HIGH});
  if (!videoCodec) throw Error(`This computer cannot encode ${format.toUpperCase()} video`);
  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext('2d', {alpha: false}),
    output = new Output({format: spec.create(), target: new BufferTarget()}),
    video = new CanvasSource(canvas, {codec: videoCodec, quality: QUALITY_HIGH, keyFrameInterval: 2});
  output.addVideoTrack(video, {frameRate: fps});
  const soundtrack = await mixSoundtrack(scene),
    audioCodec =
      soundtrack &&
      (await getFirstEncodableAudioCodec(spec.audio, {numberOfChannels: 2, sampleRate: SAMPLE_RATE, quality: QUALITY_HIGH})),
    audio = audioCodec ? new AudioBufferSource({codec: audioCodec, quality: QUALITY_HIGH}) : null;
  if (audio) output.addAudioTrack(audio);
  const wasRecording = editor.recording;
  editor.recording = true;
  try {
    await ensureFonts();
    await output.start();
    for (let i = 0; i < frames; i++) {
      if (signal?.aborted) throw new DOMException('Export cancelled', 'AbortError');
      const t = i / fps;
      await prepareAssets(t, false, 0.001);
      context.save();
      context.scale(width / scene.width, height / scene.height);
      paint(context, t);
      context.restore();
      await video.add(t, 1 / fps);
      onProgress((i + 1) / frames);
    }
    if (audio) await audio.add(soundtrack);
    await output.finalize();
    return {blob: new Blob([output.target.buffer], {type: spec.mimeType}), videoCodec, audioCodec: audio ? audioCodec : null};
  } catch (error) {
    await output.cancel().catch(() => {});
    throw error;
  } finally {
    editor.recording = wasRecording;
    for (const s of scene.shapes) if (s.type === 'video') mediaElement(s).pause();
  }
}

// Reads back an exported video: how many frames it holds, how long it lasts, and which codecs it uses.
export async function probeVideo(blob) {
  const input = new Input({formats: ALL_FORMATS, source: new BlobSource(blob)});
  try {
    const track = await input.getPrimaryVideoTrack(),
      audio = await input.getPrimaryAudioTrack();
    let frames = 0;
    for await (const packet of new EncodedPacketSink(track).packets()) if (packet) frames++;
    return {frames, duration: await input.computeDuration(), videoCodec: track.codec, audioCodec: audio?.codec ?? null};
  } finally {
    input.dispose();
  }
}
