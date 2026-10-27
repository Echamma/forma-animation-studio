import {editMotion, selectMotion} from '../animate/motion.js';
import {editRotation, selectRotation} from '../animate/rotation.js';
import {$} from '../dom.js';
import {assetHref, isAsset} from '../media.js';
import {reappearAt} from '../model/animation.js';
import {stop} from '../playback.js';
import {editor} from '../state.js';
import {openKeyframeMenu} from '../ui/keyframe-menu.js';
import {refresh} from '../ui/refresh.js';
import {closeDrawer, showInspectorTab} from '../ui/tabs.js';

export function lists() {
  $('layers').replaceChildren();
  $('tracks').replaceChildren();
  for (const s of editor.scene.shapes) {
    const b = document.createElement('button');
    b.className = 'layer' + (editor.selected === s.id ? ' active' : '');
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
    b.onclick = () => {
      editor.selected = s.id;
      editor.selectedMotionId = editor.selectedRotationId = null;
      editor.motionGuide = null;
      closeDrawer();
      refresh();
    };
    $('layers').append(b);
    const row = document.createElement('div');
    row.className =
      'track' + (editor.selected === s.id && !editor.selectedMotionId && !editor.selectedRotationId ? ' active' : '');
    const label = document.createElement('span');
    label.className = 'track-name';
    label.textContent = s.name;
    const line = document.createElement('div');
    line.className = 'track-line';
    for (const k of s.keys) {
      const d = document.createElement('button');
      d.className = 'key-dot';
      d.dataset.keyTime = k.t;
      d.style.left = `${(k.t / editor.scene.duration) * 100}%`;
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
    row.append(label, line);
    row.onclick = () => {
      editor.selected = s.id;
      editor.selectedMotionId = editor.selectedRotationId = null;
      editor.motionGuide = null;
      refresh();
    };
    $('tracks').append(row);
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
      bar.style.left = `${(d.startTime / editor.scene.duration) * 100}%`;
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
      $('tracks').append(visibilityRow);
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
      clip.style.left = `${(m.startTime / editor.scene.duration) * 100}%`;
      clip.style.width = `${(m.duration / editor.scene.duration) * 100}%`;
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
      $('tracks').append(motionRow);
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
      bar.style.left = `${(clip.startTime / editor.scene.duration) * 100}%`;
      bar.style.width = `${(clip.duration / editor.scene.duration) * 100}%`;
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
      $('tracks').append(rotationRow);
    }
  }
  $('count').textContent = editor.scene.shapes.length;
  $('key-count').textContent =
    `${editor.scene.shapes.length} layers · ${editor.scene.shapes.reduce((n, s) => n + s.keys.length, 0)} keyframes · ${editor.scene.shapes.reduce((n, s) => n + (s.motions?.length || 0) + (s.rotationClips?.length || 0) + (s.disappear ? 1 : 0), 0)} animations`;
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
