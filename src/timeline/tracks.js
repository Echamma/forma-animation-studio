import {editMotion, selectMotion} from '../animate/motion.js';
import {editRotation, selectRotation} from '../animate/rotation.js';
import {$, toast} from '../dom.js';
import {assetHref, isAsset} from '../media.js';
import {clipEnd, clipStart, inClip, reappearAt} from '../model/animation.js';
import {earliestShift, MIN_CLIP, shiftLayer, trimClip} from '../model/clips.js';
import {recordSceneChange} from '../persistence.js';
import {stop} from '../playback.js';
import {editor} from '../state.js';
import {openClipMenu} from '../ui/clips.js';
import {openKeyframeMenu} from '../ui/keyframe-menu.js';
import {refresh} from '../ui/refresh.js';
import {selectedLayers, toggleInSelection} from '../ui/selection.js';
import {closeDrawer, showInspectorTab} from '../ui/tabs.js';

const percent = t => `${(t / editor.scene.duration) * 100}%`;

export function selectClip(s) {
  stop();
  editor.selected = s.id;
  editor.selectedMotionId = editor.selectedRotationId = null;
  editor.motionGuide = null;
  if (!inClip(s, editor.time)) editor.time = clipStart(s);
  refresh();
}

function toggleClip(s) {
  stop();
  toggleInSelection(s.id);
  refresh();
}

export function lists() {
  $('layers').replaceChildren();
  $('tracks').replaceChildren();
  // One timeline row per layer, except that clips sharing a trackId sit side by side on one row.
  const rows = new Map(),
    chosen = new Set(selectedLayers().map(s => s.id));
  for (const s of editor.scene.shapes) {
    const b = document.createElement('button');
    b.className = 'layer' + (chosen.has(s.id) ? ' active' : '');
    const sw = document.createElement('span');
    sw.className = 'swatch' + (isAsset(s) ? ' asset' : '');
    if (isAsset(s)) sw.style.backgroundImage = `url("${assetHref(s).replace(/"/g, '%22')}")`;
    else sw.style.background = s.color;
    const name = document.createElement('span');
    name.textContent = s.name;
    const end = document.createElement('span');
    end.className = 'layer-end';
    end.textContent = s.type === 'svg' ? '✎' : s.type === 'video' ? '▶' : '◇';
    b.append(sw, name, end);
    b.onclick = e => {
      if (e.shiftKey) return toggleClip(s);
      closeDrawer();
      selectClip(s);
    };
    $('layers').append(b);
    const rowKey = s.trackId || s.id;
    let entry = rows.get(rowKey);
    if (!entry) {
      const row = document.createElement('div');
      row.className = 'track' + (s.trackId ? ' clip-track' : '');
      row.dataset.trackKey = rowKey;
      const label = document.createElement('span');
      label.className = 'track-name';
      label.textContent = s.trackId ? s.name.replace(/ \d+$/, '') : s.name;
      label.title = label.textContent;
      const line = document.createElement('div');
      line.className = 'track-line';
      row.append(label, line);
      $('tracks').append(row);
      entry = {row, line, clips: [], last: row};
      row.onclick = () =>
        selectClip(
          entry.clips.find(c => c.id === editor.selected) || entry.clips.find(c => inClip(c, editor.time)) || entry.clips[0]
        );
      rows.set(rowKey, entry);
    }
    entry.clips.push(s);
    const {row, line} = entry;
    if (editor.selected === s.id && !editor.selectedMotionId && !editor.selectedRotationId) row.classList.add('active');
    line.append(clipBar(s, line, chosen));
    for (const k of s.keys) {
      if (k.t < clipStart(s) - 1e-6 || k.t > clipEnd(s) + 1e-6) continue;
      const d = document.createElement('button');
      d.className = 'key-dot';
      d.dataset.keyTime = k.t;
      d.dataset.layerId = s.id;
      d.style.left = percent(k.t);
      d.title = `${s.name} at ${k.t.toFixed(2)} seconds — right-click to move or delete`;
      d.onclick = e => {
        e.stopPropagation();
        stop();
        editor.time = k.t;
        editor.selected = s.id;
        editor.selectedMotionId = k.motionId || null;
        editor.selectedRotationId = null;
        refresh();
      };
      d.oncontextmenu = e => openKeyframeMenu(e, s, k);
      line.append(d);
    }
    for (const k of s.colorKeys || []) {
      if (k.t < clipStart(s) - 1e-6 || k.t > clipEnd(s) + 1e-6) continue;
      const dot = document.createElement('button');
      dot.className = 'color-dot';
      dot.style.left = percent(k.t);
      dot.style.background = k.color;
      dot.title = `${s.name} color ${k.color.toUpperCase()} at ${k.t.toFixed(2)} s — right-click to delete`;
      dot.onclick = e => {
        e.stopPropagation();
        stop();
        editor.time = k.t;
        editor.selected = s.id;
        refresh();
      };
      dot.oncontextmenu = e => {
        e.preventDefault();
        e.stopPropagation();
        s.colorKeys = s.colorKeys.filter(x => x !== k);
        if (!s.colorKeys.length) {
          s.color = k.color;
          delete s.colorKeys;
        }
        refresh();
        recordSceneChange('delete color keyframe');
        toast('Color keyframe deleted');
      };
      line.append(dot);
    }
    // Animation rows sit under their layer; on a clip track only the selected clip shows them.
    if (s.trackId && editor.selected !== s.id) continue;
    const place = subRow => {
      entry.last.after(subRow);
      entry.last = subRow;
    };
    if (s.disappear) {
      const d = s.disappear,
        visibilityRow = document.createElement('div'),
        visibilityLabel = document.createElement('span'),
        visibilityLine = document.createElement('div'),
        bar = document.createElement('button'),
        end = reappearAt(s);
      visibilityRow.className = 'track motion-track disappear-track' + (editor.selected === s.id ? ' active' : '');
      visibilityLabel.className = 'track-name';
      visibilityLabel.textContent = '↳ Disappear';
      visibilityLine.className = 'track-line';
      bar.className = 'motion-bar disappear-bar';
      bar.style.left = percent(d.startTime);
      bar.style.width = `${Math.max(1.5, ((Math.min(end, editor.scene.duration) - d.startTime) / editor.scene.duration) * 100)}%`;
      bar.title = `Fade out ${d.startTime.toFixed(2)}–${(d.startTime + d.duration).toFixed(2)} s${Number.isFinite(end) ? `; reappear ${end.toFixed(2)} s` : '; stay hidden'}`;
      bar.textContent = '◌ Hidden';
      bar.onclick = e => {
        e.stopPropagation();
        stop();
        editor.selected = s.id;
        editor.time = d.startTime;
        showInspectorTab('animate');
        refresh();
        $('disappear-edit').scrollIntoView({block: 'nearest'});
      };
      visibilityLine.append(bar);
      visibilityRow.append(visibilityLabel, visibilityLine);
      visibilityRow.onclick = bar.onclick;
      place(visibilityRow);
    }
    for (const [i, m] of (s.motions || []).entries()) {
      const motionRow = document.createElement('div');
      motionRow.className = 'track motion-track' + (m.id === editor.selectedMotionId ? ' active' : '');
      motionRow.dataset.motionId = m.id;
      const motionLabel = document.createElement('span');
      motionLabel.className = 'track-name';
      motionLabel.textContent = `↳ ${m.type === 'path' ? 'Path' : 'Move'} ${i + 1}`;
      const motionLine = document.createElement('div');
      motionLine.className = 'track-line';
      const clip = document.createElement('button');
      clip.className = 'motion-bar';
      clip.style.left = percent(m.startTime);
      clip.style.width = percent(m.duration);
      clip.title = `${s.name}: ${m.startTime.toFixed(2)}–${(m.startTime + m.duration).toFixed(2)} s`;
      const clipText = document.createElement('span');
      clipText.textContent = m.type === 'path' ? 'Mouse path' : 'Arrow move';
      const handle = document.createElement('span');
      handle.className = 'motion-handle';
      handle.title = 'Drag to change animation length';
      clip.append(clipText, handle);
      clip.onclick = e => {
        e.stopPropagation();
        if (clip.dataset.ignoreClick === 'yes') return;
        selectMotion(s, m);
      };
      clip.onpointerdown = e => beginMotionClipDrag(e, s, m, clip, motionLine);
      motionLine.append(clip);
      motionRow.append(motionLabel, motionLine);
      motionRow.onclick = () => selectMotion(s, m);
      place(motionRow);
    }
    for (const [i, clip] of (s.rotationClips || []).entries()) {
      const rotationRow = document.createElement('div');
      rotationRow.className =
        'track motion-track rotation-track' + (clip.id === editor.selectedRotationId ? ' active' : '');
      rotationRow.dataset.rotationId = clip.id;
      const rotationLabel = document.createElement('span');
      rotationLabel.className = 'track-name';
      rotationLabel.textContent = `↳ Rotation ${i + 1}`;
      const rotationLine = document.createElement('div');
      rotationLine.className = 'track-line';
      const bar = document.createElement('button');
      bar.className = 'motion-bar rotation-bar';
      bar.style.left = percent(clip.startTime);
      bar.style.width = percent(clip.duration);
      bar.title = `Rotation ${clip.degrees}° · ${clip.startTime.toFixed(2)}–${(clip.startTime + clip.duration).toFixed(2)} s`;
      const text = document.createElement('span');
      text.textContent = `↻ ${clip.degrees}°`;
      const handle = document.createElement('span');
      handle.className = 'motion-handle';
      handle.title = 'Drag to change rotation length';
      bar.append(text, handle);
      bar.onclick = e => {
        e.stopPropagation();
        if (bar.dataset.ignoreClick !== 'yes') selectRotation(s, clip);
      };
      bar.onpointerdown = e => beginRotationClipDrag(e, s, clip, bar, rotationLine);
      rotationLine.append(bar);
      rotationRow.append(rotationLabel, rotationLine);
      rotationRow.onclick = () => selectRotation(s, clip);
      place(rotationRow);
    }
  }
  $('count').textContent = editor.scene.shapes.length;
  $('key-count').textContent =
    `${editor.scene.shapes.length} layers · ${editor.scene.shapes.reduce((n, s) => n + s.keys.length, 0)} keyframes · ${editor.scene.shapes.reduce((n, s) => n + (s.motions?.length || 0) + (s.rotationClips?.length || 0) + (s.disappear ? 1 : 0), 0)} animations`;
}

// The bar that shows when a layer is visible. Drag it to move the clip with its animation, or drag an edge to trim.
function clipBar(s, line, chosen) {
  const start = clipStart(s),
    end = Math.min(clipEnd(s), editor.scene.duration),
    bar = document.createElement('button');
  bar.className = 'clip-bar' + (chosen.has(s.id) ? ' selected' : '');
  bar.dataset.layerId = s.id;
  bar.style.left = percent(start);
  bar.style.width = percent(end - start);
  bar.title = `${s.name}: visible ${start.toFixed(2)}–${end.toFixed(2)} s. Drag to move, drag an edge to trim, right-click for more.`;
  const label = document.createElement('span');
  label.className = 'clip-label';
  label.textContent = s.trackId ? s.name : '';
  for (const edge of ['start', 'end']) {
    const handle = document.createElement('span');
    handle.className = 'clip-edge';
    handle.dataset.edge = edge;
    handle.title = edge === 'start' ? 'Drag to change when this layer appears' : 'Drag to change when this layer ends';
    bar.append(handle);
  }
  bar.insertBefore(label, bar.lastChild);
  bar.onpointerdown = e => beginClipDrag(e, s, bar, line);
  bar.onclick = e => {
    e.stopPropagation();
    if (bar.dataset.ignoreClick === 'yes') delete bar.dataset.ignoreClick;
    else if (e.shiftKey) toggleClip(s);
    else selectClip(s);
  };
  bar.oncontextmenu = e => openClipMenu(e, s);
  return bar;
}

// Snap to the playhead, the scene edges, and other clips' edges within 8 px; otherwise to 0.1 s steps.
function snapper(s, line) {
  const targets = [0, editor.time, editor.scene.duration];
  for (const other of editor.scene.shapes)
    if (other !== s) targets.push(clipStart(other), ...(other.outTime === undefined ? [] : [other.outTime]));
  const tolerance = (8 / line.getBoundingClientRect().width) * editor.scene.duration;
  return (t, free) => {
    if (free) return {value: Math.round(t * 100) / 100, target: false};
    const nearest = targets.reduce((best, x) => (Math.abs(x - t) < Math.abs(best - t) ? x : best), Infinity);
    return Math.abs(nearest - t) <= tolerance
      ? {value: nearest, target: true}
      : {value: Math.round(t * 10) / 10, target: false};
  };
}

export function beginClipDrag(e, s, bar, line) {
  if (e.button !== 0) return;
  e.stopPropagation();
  const mode = e.target.closest('.clip-edge')?.dataset.edge || 'move',
    start0 = clipStart(s),
    end0 = clipEnd(s),
    x0 = e.clientX,
    snap = snapper(s, line),
    label = bar.querySelector('.clip-label'),
    minShift = earliestShift(s);
  let moved = false,
    start = start0,
    end = end0;
  try {
    bar.setPointerCapture(e.pointerId);
  } catch {
    // Capture is only a convenience; synthetic or already released pointers cannot be captured.
  }
  bar.onpointermove = move => {
    if (!moved && Math.abs(move.clientX - x0) < 3) return;
    moved = true;
    const delta = ((move.clientX - x0) / line.getBoundingClientRect().width) * editor.scene.duration,
      free = move.altKey;
    if (mode === 'start') start = Math.max(0, Math.min(end0 - MIN_CLIP, snap(start0 + delta, free).value));
    else if (mode === 'end') end = Math.max(start0 + MIN_CLIP, snap(end0 + delta, free).value);
    else {
      const a = snap(start0 + delta, free),
        b = snap(end0 + delta, free),
        useEnd = b.target && (!a.target || Math.abs(b.value - end0 - delta) < Math.abs(a.value - start0 - delta)),
        shift = Math.max(minShift, useEnd ? b.value - end0 : a.value - start0);
      start = start0 + shift;
      end = end0 + shift;
    }
    bar.style.left = percent(Math.min(start, editor.scene.duration));
    bar.style.width = percent(Math.max(0, Math.min(end, editor.scene.duration) - start));
    label.textContent = `${start.toFixed(2)}–${end.toFixed(2)} s`;
  };
  const finish = commit => {
    bar.onpointermove = bar.onpointerup = bar.onpointercancel = null;
    if (!moved) return;
    bar.dataset.ignoreClick = 'yes';
    if (commit) {
      stop();
      if (mode === 'move') shiftLayer(s, start - start0);
      else trimClip(s, start, mode === 'end' || s.outTime !== undefined ? end : undefined);
      editor.selected = s.id;
      if (!inClip(s, editor.time)) editor.time = clipStart(s);
      recordSceneChange('clip timing');
    }
    refresh();
  };
  bar.onpointerup = () => finish(true);
  bar.onpointercancel = () => finish(false);
}

function beginMotionClipDrag(e, s, m, clip, line) {
  if (e.button !== 0) return;
  e.stopPropagation();
  const resizing = !!e.target.closest('.motion-handle'),
    initialX = e.clientX,
    initialStart = m.startTime,
    initialLength = m.duration;
  let moved = false,
    start = initialStart,
    length = initialLength;
  clip.setPointerCapture(e.pointerId);
  clip.onpointermove = move => {
    const delta = ((move.clientX - initialX) / line.getBoundingClientRect().width) * editor.scene.duration;
    if (Math.abs(delta) < 0.015) return;
    moved = true;
    if (resizing)
      length = Math.max(
        0.1,
        Math.min(editor.scene.duration - initialStart, Math.round((initialLength + delta) * 100) / 100)
      );
    else
      start = Math.max(
        0,
        Math.min(editor.scene.duration - initialLength, Math.round((initialStart + delta) * 100) / 100)
      );
    clip.style.left = `${(start / editor.scene.duration) * 100}%`;
    clip.style.width = `${(length / editor.scene.duration) * 100}%`;
    clip.title = `${start.toFixed(2)}–${(start + length).toFixed(2)} s`;
  };
  clip.onpointerup = () => {
    clip.onpointermove = null;
    clip.onpointerup = null;
    if (!moved) return;
    clip.dataset.ignoreClick = 'yes';
    selectMotion(s, m);
    $('motion-start').value = start.toFixed(2);
    $('motion-length').value = length.toFixed(2);
    editMotion({preview: $('live-motion-preview').checked});
  };
  clip.onpointercancel = () => {
    clip.onpointermove = null;
    clip.onpointerup = null;
    lists();
  };
}
function beginRotationClipDrag(e, s, clip, bar, line) {
  if (e.button !== 0) return;
  e.stopPropagation();
  const resizing = !!e.target.closest('.motion-handle'),
    initialX = e.clientX,
    initialStart = clip.startTime,
    initialLength = clip.duration;
  let moved = false,
    start = initialStart,
    length = initialLength;
  bar.setPointerCapture(e.pointerId);
  bar.onpointermove = move => {
    const delta = ((move.clientX - initialX) / line.getBoundingClientRect().width) * editor.scene.duration;
    if (Math.abs(delta) < 0.015) return;
    moved = true;
    if (resizing)
      length = Math.max(
        0.1,
        Math.min(editor.scene.duration - initialStart, Math.round((initialLength + delta) * 100) / 100)
      );
    else
      start = Math.max(
        0,
        Math.min(editor.scene.duration - initialLength, Math.round((initialStart + delta) * 100) / 100)
      );
    bar.style.left = `${(start / editor.scene.duration) * 100}%`;
    bar.style.width = `${(length / editor.scene.duration) * 100}%`;
  };
  bar.onpointerup = () => {
    bar.onpointermove = null;
    bar.onpointerup = null;
    if (!moved) return;
    bar.dataset.ignoreClick = 'yes';
    selectRotation(s, clip);
    $('rotation-start').value = start.toFixed(2);
    $('rotation-length').value = length.toFixed(2);
    editRotation({preview: $('live-rotation-preview').checked});
  };
  bar.onpointercancel = () => {
    bar.onpointermove = null;
    bar.onpointerup = null;
    lists();
  };
}
