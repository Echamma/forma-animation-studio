// Fixed, typed editor commands for the MCP bridge. No arbitrary code or browser automation endpoint.
import {$, toast} from './dom.js';
import {svgText} from './export/svg.js';
import {assetCache, assetHref, mediaElement, prepareAssets} from './media.js';
import {state} from './model/animation.js';
import {continueAfter, duplicateLayer} from './model/clips.js';
import {usableProject} from './model/validate.js';
import {recordSceneChange} from './persistence.js';
import {stop} from './playback.js';
import {ensureFonts, paint} from './render/paint.js';
import {editor} from './state.js';
import {sanitizeSvg} from './svg/sanitize.js';
import {refresh} from './ui/refresh.js';

export function setupAutomation() {
  const clone = value => JSON.parse(JSON.stringify(value));
  const problem = (code, message) => Object.assign(new Error(message), {code});
  let busy = false;
  function snapshot(includeAssets = false) {
    const project = includeAssets
      ? clone(editor.scene)
      : clone({
          ...editor.scene,
          shapes: editor.scene.shapes.map(layer => {
            const copy = {...layer};
            for (const field of ['assetData', 'posterData', 'svgSource']) {
              if (typeof copy[field] === 'string') {
                copy[field + 'Bytes'] = copy[field].length;
                delete copy[field];
              }
            }
            return copy;
          })
        });
    return {
      project,
      selected: editor.selected || null,
      time: editor.time,
      playing: editor.playing,
      transforms: editor.scene.shapes.map(layer => ({id: layer.id, ...state(layer, editor.time)}))
    };
  }
  function checkedTime(value) {
    const at = value ?? editor.time;
    if (!Number.isFinite(at) || at < 0 || at > editor.scene.duration)
      throw problem('INVALID_TIME', 'Time must be inside 0–' + editor.scene.duration + ' seconds.');
    return at;
  }
  function find(project, id) {
    const layer = project.shapes.find(layer => layer.id === id);
    if (!layer)
      throw problem('LAYER_NOT_FOUND', 'No layer has ID ' + id + '. Inspect forma_get_scene for current IDs.');
    return layer;
  }
  function install(project, selection = editor.selected, resetTime = false) {
    if (!usableProject(project))
      throw problem(
        'INVALID_PROJECT',
        'The edit would create an invalid Forma scene. Check sizes, timestamps, colors, motion metadata, and layer limits.'
      );
    stop();
    assetCache.clear();
    editor.scene = project;
    editor.selected = editor.scene.shapes.some(layer => layer.id === selection)
      ? selection
      : editor.scene.shapes[0]?.id;
    editor.time = resetTime ? 0 : Math.min(editor.time, editor.scene.duration);
    editor.motionMode =
      editor.motionDraft =
      editor.motionGuide =
      editor.selectedMotionId =
      editor.selectedRotationId =
        null;
    $('stage').classList.remove('motion-mode');
    refresh();
    recordSceneChange('automation');
  }
  // inTime/outTime trim a clip without moving its keyframes, and the scene grows to fit. A null outTime lasts to the end.
  function applyClipTiming(project, layer, args) {
    if (args.inTime !== undefined) layer.inTime = args.inTime;
    if (args.outTime === null) delete layer.outTime;
    else if (args.outTime !== undefined) layer.outTime = args.outTime;
    if (args.trackId === '') delete layer.trackId;
    else if (args.trackId !== undefined) layer.trackId = args.trackId;
    if (layer.outTime !== undefined && layer.outTime <= (layer.inTime ?? 0))
      throw problem('INVALID_TIMING', 'outTime must be later than inTime.');
    project.duration = Math.max(project.duration, layer.inTime ?? 0, layer.outTime ?? 0);
  }
  function newLayer(args, type = args.type) {
    const width = args.w ?? (type === 'text' ? 240 : 160);
    const height = args.h ?? (type === 'text' ? 36 : 160);
    const layer = {
      id: crypto.randomUUID(),
      name: args.name || (type === 'text' ? 'Text' : type[0].toUpperCase() + type.slice(1)),
      type,
      color: args.color || '#ed795e',
      ease: args.ease || 'smooth',
      keys: [
        {
          t: 0,
          x: args.x ?? editor.scene.width / 2,
          y: args.y ?? editor.scene.height / 2,
          w: width,
          h: height,
          r: args.r ?? 0,
          o: args.o ?? 100
        }
      ]
    };
    if (type === 'text')
      Object.assign(layer, {
        text: args.text ?? 'Your text',
        linkId: '',
        offsetX: 0,
        offsetY: 0,
        textWidth: width,
        fontSize: height
      });
    return layer;
  }
  function summarized(layer) {
    const result = clone(layer);
    for (const field of ['assetData', 'posterData', 'svgSource']) delete result[field];
    return {layer: result};
  }
  function base64(blob) {
    return new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onerror = () => reject(problem('EXPORT_FAILED', 'Could not encode export.'));
      reader.onload = () => resolve(reader.result.split(',')[1]);
      reader.readAsDataURL(blob);
    });
  }
  async function frame(at, maxWidth) {
    await ensureFonts();
    await prepareAssets(at);
    const ratio = maxWidth ? Math.min(1, maxWidth / editor.scene.width, 1600 / editor.scene.height) : 1;
    const canvas = document.createElement('canvas');
    canvas.width = Math.max(1, Math.round(editor.scene.width * ratio));
    canvas.height = Math.max(1, Math.round(editor.scene.height * ratio));
    const context = canvas.getContext('2d');
    context.scale(ratio, ratio);
    paint(context, at);
    const blob = await new Promise(resolve => canvas.toBlob(resolve, 'image/png'));
    if (!blob) throw problem('EXPORT_FAILED', 'PNG rendering failed.');
    return {data: await base64(blob), mimeType: 'image/png', width: canvas.width, height: canvas.height, time: at};
  }
  async function video() {
    const mimeType = ['video/webm;codecs=vp9', 'video/webm;codecs=vp8', 'video/webm'].find(type =>
      MediaRecorder.isTypeSupported(type)
    );
    if (!mimeType) throw problem('UNSUPPORTED_EXPORT', 'WebM recording is unavailable in this runtime.');
    const originalTime = editor.time;
    const wasPlaying = editor.playing;
    stop();
    editor.recording = true;
    let stream, recorder, deadline, frameTimer;
    try {
      await ensureFonts();
      await prepareAssets(0, true);
      const canvas = document.createElement('canvas');
      canvas.width = editor.scene.width;
      canvas.height = editor.scene.height;
      const context = canvas.getContext('2d');
      paint(context, 0);
      stream = canvas.captureStream(30);
      recorder = new MediaRecorder(stream, {mimeType, videoBitsPerSecond: 8000000});
      const blob = await new Promise((resolve, reject) => {
        const chunks = [];
        recorder.ondataavailable = event => {
          if (event.data.size) chunks.push(event.data);
        };
        recorder.onstop = () => resolve(new Blob(chunks, {type: 'video/webm'}));
        recorder.onerror = () => reject(problem('EXPORT_FAILED', 'Video recording failed.'));
        deadline = setTimeout(
          () => reject(problem('EXPORT_FAILED', 'Video recording timed out.')),
          (editor.scene.duration + 15) * 1000
        );
        recorder.start();
        const start = performance.now();
        const draw = () => {
          const at = Math.min((performance.now() - start) / 1000, editor.scene.duration);
          paint(context, at);
          if (at < editor.scene.duration) frameTimer = setTimeout(draw, 1000 / 30);
          else recorder.stop();
        };
        draw();
      });
      if (!blob.size) throw problem('EXPORT_FAILED', 'The recorded video was empty.');
      return {data: await base64(blob), mimeType: 'video/webm'};
    } finally {
      clearTimeout(deadline);
      clearTimeout(frameTimer);
      if (recorder?.state === 'recording') recorder.stop();
      stream?.getTracks().forEach(track => track.stop());
      for (const layer of editor.scene.shapes) if (layer.type === 'video') mediaElement(layer).pause();
      editor.recording = false;
      editor.time = originalTime;
      refresh();
      if (wasPlaying) $('play').click();
    }
  }
  async function dispatch(name, args) {
    await window.FormaSession.ready;
    if (busy || editor.recording) throw problem('EDITOR_BUSY', 'Wait for the current export or tool call to finish.');
    busy = true;
    document.body.classList.add('mcp-working');
    try {
      switch (name) {
        case 'forma_get_scene':
          return snapshot(args.includeAssets);
        case 'forma_create_scene':
          if (args.replace !== true) throw problem('CONFIRMATION_REQUIRED', 'replace=true is required.');
          install(
            {
              version: 1,
              width: args.width ?? 960,
              height: args.height ?? 600,
              duration: args.duration ?? 4,
              bg: args.bg ?? '#f4f0e8',
              shapes: []
            },
            null,
            true
          );
          return snapshot();
        case 'forma_update_scene': {
          const next = {...clone(editor.scene), ...args};
          if (next.shapes.some(
              layer =>
                layer.keys.some(key => key.t > next.duration) ||
                (layer.inTime ?? 0) > next.duration ||
                (layer.outTime ?? 0) > next.duration
            ))
            throw problem('KEYFRAMES_OUT_OF_RANGE', 'Remove or retime later keyframes and clips before shortening the scene.');
          install(next);
          return snapshot();
        }
        case 'forma_add_shape': {
          const next = clone(editor.scene),
            layer = newLayer(args);
          applyClipTiming(next, layer, args);
          next.shapes.push(layer);
          install(next, layer.id);
          return summarized(layer);
        }
        case 'forma_update_shape': {
          const next = clone(editor.scene),
            layer = find(next, args.id);
          if ((args.text !== undefined || args.linkId !== undefined) && layer.type !== 'text')
            throw problem('INVALID_LAYER_TYPE', 'Only text layers have text or linkId.');
          if (args.linkId !== undefined) {
            const pose = state(find(editor.scene, layer.id), editor.time);
            if (args.linkId) {
              const target = find(editor.scene, args.linkId);
              if (target.type === 'text') throw problem('INVALID_LINK', 'Text must follow a non-text layer.');
              const targetPose = state(target, editor.time),
                angle = (-targetPose.r * Math.PI) / 180;
              const dx = pose.x - targetPose.x,
                dy = pose.y - targetPose.y;
              Object.assign(layer, {
                linkId: target.id,
                linkBase: {w: targetPose.w, h: targetPose.h},
                offsetX: dx * Math.cos(angle) - dy * Math.sin(angle),
                offsetY: dx * Math.sin(angle) + dy * Math.cos(angle),
                textWidth: pose.w,
                fontSize: pose.h,
                rotationOffset: pose.r - targetPose.r,
                opacityFactor: targetPose.o ? pose.o / targetPose.o : 1
              });
            } else {
              layer.linkId = '';
              layer.keys = [{t: 0, ...pose}];
            }
          }
          for (const field of ['name', 'color', 'ease', 'text'])
            if (args[field] !== undefined) layer[field] = args[field];
          applyClipTiming(next, layer, args);
          install(next, layer.id);
          return summarized(layer);
        }
        case 'forma_duplicate_shape': {
          const next = clone(editor.scene),
            source = find(next, args.id),
            live = editor.scene;
          let copy;
          editor.scene = next;
          try {
            copy = (args.placement ?? 'after') === 'after' ? continueAfter(source) : duplicateLayer(source);
          } catch (error) {
            throw problem('INVALID_CLIP', error.message);
          } finally {
            editor.scene = live;
          }
          const first = copy.keys[0],
            dx = (args.x ?? first.x) - first.x,
            dy = (args.y ?? first.y) - first.y;
          const move = p => {
            p.x += dx;
            p.y += dy;
          };
          copy.keys.forEach(move);
          for (const m of copy.motions || []) {
            move(m.startPose);
            m.sourcePoints.forEach(move);
            (m.baseKeys || []).forEach(move);
          }
          install(next, copy.id);
          return summarized(copy);
        }
        case 'forma_set_keyframes': {
          const next = clone(editor.scene),
            layer = find(next, args.id);
          if (new Set(args.keyframes.map(key => key.t)).size !== args.keyframes.length)
            throw problem('DUPLICATE_TIME', 'Each keyframe needs a unique time.');
          const frames = new Map((args.replace ? [] : layer.keys).map(key => [key.t, key]));
          for (const key of args.keyframes) frames.set(key.t, clone(key));
          layer.keys = [...frames.values()].sort((a, b) => a.t - b.t);
          delete layer.motions;
          install(next, layer.id);
          return summarized(layer);
        }
        case 'forma_delete_shape': {
          if (args.confirm !== true) throw problem('CONFIRMATION_REQUIRED', 'confirm=true is required.');
          find(editor.scene, args.id);
          const next = clone(editor.scene);
          for (const layer of next.shapes)
            if (layer.type === 'text' && layer.linkId === args.id) {
              layer.keys = [{t: 0, ...state(find(editor.scene, layer.id), editor.time)}];
              layer.linkId = '';
            }
          next.shapes = next.shapes.filter(layer => layer.id !== args.id);
          install(next);
          return {deleted: args.id};
        }
        case 'forma_set_playback':
          if (args.action === 'seek') {
            if (args.time === undefined) throw problem('INVALID_TIME', 'Seek requires time.');
            const at = checkedTime(args.time);
            stop();
            editor.time = at;
            refresh();
          } else if (args.action === 'pause') {
            stop();
            refresh();
          } else if (args.action === 'play') {
            if (!editor.playing) $('play').click();
          } else throw problem('INVALID_ACTION', 'Unknown playback action.');
          return {time: editor.time, playing: editor.playing};
        case 'forma_preview':
          return await frame(checkedTime(args.time), args.maxWidth ?? 960);
        case 'forma_export': {
          const at = checkedTime(args.time);
          if (args.format === 'png') return await frame(at);
          if (args.format === 'webm') return await video();
          if (args.format === 'project')
            return {
              data: await base64(new Blob([JSON.stringify(editor.scene, null, 2)], {type: 'application/json'})),
              mimeType: 'application/json'
            };
          const originalTime = editor.time;
          let markup;
          try {
            editor.time = at;
            markup = svgText(args.format === 'animated_svg');
          } finally {
            editor.time = originalTime;
          }
          return {data: await base64(new Blob([markup], {type: 'image/svg+xml'})), mimeType: 'image/svg+xml'};
        }
        case 'forma_open_project': {
          if (args.replace !== true) throw problem('CONFIRMATION_REQUIRED', 'replace=true is required.');
          const next = clone(args.project);
          if (!usableProject(next)) throw problem('INVALID_PROJECT', 'This file is not a valid Forma project.');
          for (const layer of next.shapes) {
            layer.keys.sort((a, b) => a.t - b.t);
            if (layer.type === 'svg') layer.svgSource = sanitizeSvg(layer.svgSource);
          }
          install(next, null, true);
          return snapshot();
        }
        case 'forma_import_asset': {
          const layer = newLayer(args, args.assetType);
          if (layer.type === 'svg') layer.svgSource = sanitizeSvg(args.svgSource);
          else layer.assetData = args.assetData;
          const image = new Image();
          image.src = assetHref(layer);
          await image.decode();
          if (!image.naturalWidth || !image.naturalHeight)
            throw problem('INVALID_ASSET', 'Image has no usable dimensions.');
          layer.keys[0].w = args.w ?? Math.min(image.naturalWidth, editor.scene.width * 0.6);
          layer.keys[0].h = args.h ?? Math.min(10000, (layer.keys[0].w * image.naturalHeight) / image.naturalWidth);
          const next = clone(editor.scene);
          next.shapes.push(layer);
          install(next, layer.id);
          return summarized(layer);
        }
        default:
          throw problem('UNKNOWN_TOOL', 'Unknown Forma tool.');
      }
    } finally {
      busy = false;
      document.body.classList.remove('mcp-working');
    }
  }
  window.FormaSession.dispatch = dispatch;
  document.addEventListener(
    'keydown',
    event => {
      if (busy) {
        event.preventDefault();
        event.stopImmediatePropagation();
      }
    },
    true
  );
  if (!window.formaDesktop) return;
  const button = document.createElement('button');
  button.id = 'mcp-control';
  button.textContent = 'MCP off';
  button.title = 'Allow Codex or Claude to control this editor through Forma MCP';
  $('open').before(button);
  function showStatus(status) {
    button.textContent = status.enabled ? 'MCP on' : 'MCP off';
    button.setAttribute('aria-pressed', String(status.enabled));
    button.classList.toggle('mcp-enabled', status.enabled);
    button.title = status.enabled
      ? 'Click to turn off assistant control. Exports: ' + status.exportDirectory
      : 'Click to enable Codex or Claude control';
  }
  button.onclick = async () => {
    button.disabled = true;
    try {
      const current = await window.formaDesktop.mcpStatus();
      showStatus(await window.formaDesktop.mcpToggle(!current.enabled));
    } catch (error) {
      toast('MCP could not start: ' + error.message);
    } finally {
      button.disabled = false;
    }
  };
  window.formaDesktop.onMcpStatus(showStatus);
  window.formaDesktop.mcpStatus().then(showStatus);
  window.formaDesktop.onCommand(async ({requestId, name, args}) => {
    try {
      window.formaDesktop.reply({requestId, result: await dispatch(name, args)});
    } catch (error) {
      window.formaDesktop.reply({requestId, error: {code: error.code || 'EDITOR_ERROR', message: error.message}});
    }
  });
}
