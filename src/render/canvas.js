import {$, element} from '../dom.js';
import {assetHref, isAsset, mediaElement, syncVideo} from '../media.js';
import {clipEnd, clipStart, colorAt, inClip, state} from '../model/animation.js';
import {resizeCursor, resizeDirections} from '../model/geometry.js';
import {editor} from '../state.js';
import {selectedLayers} from '../ui/selection.js';
import {showQuickAnimate} from '../ui/rail.js';

export function geometry(s, p, exporting = false) {
  if (s.type === 'video' && !exporting) {
    const fo = element('foreignObject', {x: -p.w / 2, y: -p.h / 2, width: p.w, height: p.h});
    fo.append(mediaElement(s));
    return fo;
  }
  if (isAsset(s))
    return element('image', {
      x: -p.w / 2,
      y: -p.h / 2,
      width: p.w,
      height: p.h,
      href: assetHref(s),
      preserveAspectRatio: 'none'
    });
  if (s.type === 'text') {
    const text = element('text', {
      x: 0,
      y: 0,
      'text-anchor': 'middle',
      'dominant-baseline': 'middle',
      'font-family': 'DM Sans, sans-serif',
      'font-size': p.h,
      'font-weight': '600',
      textLength: p.w,
      lengthAdjust: 'spacingAndGlyphs'
    });
    text.textContent = s.text || 'Text';
    return text;
  }
  if (s.type === 'circle') return element('ellipse', {cx: 0, cy: 0, rx: p.w / 2, ry: p.h / 2});
  if (s.type === 'triangle')
    return element('polygon', {points: `0,${-p.h / 2} ${p.w / 2},${p.h / 2} ${-p.w / 2},${p.h / 2}`});
  if (s.type === 'diamond')
    return element('polygon', {points: `0,${-p.h / 2} ${p.w / 2},0 0,${p.h / 2} ${-p.w / 2},0`});
  if (s.type === 'arrow')
    return element('polygon', {
      points: `${-p.w / 2},${-p.h * 0.18} ${p.w * 0.12},${-p.h * 0.18} ${p.w * 0.12},${-p.h / 2} ${p.w / 2},0 ${p.w * 0.12},${p.h / 2} ${p.w * 0.12},${p.h * 0.18} ${-p.w / 2},${p.h * 0.18}`
    });
  return element('rect', {x: -p.w / 2, y: -p.h / 2, width: p.w, height: p.h, rx: 3});
}
export function layerBlur(s) {
  return Number.isFinite(s.blur) ? Math.max(0, Math.min(100, s.blur)) : 0;
}
export function addBlurFilter(defs, s, p) {
  const amount = layerBlur(s);
  if (!amount) return null;
  const width = Math.max(p.w, ...s.keys.map(k => k.w)),
    height = Math.max(p.h, ...s.keys.map(k => k.h)),
    margin = amount * 3 + 4,
    id = `blur-${s.id}`,
    filter = element('filter', {
      id,
      filterUnits: 'userSpaceOnUse',
      x: -width / 2 - margin,
      y: -height / 2 - margin,
      width: width + margin * 2,
      height: height + margin * 2
    });
  filter.append(element('feGaussianBlur', {stdDeviation: amount}));
  defs.append(filter);
  return `url(#${id})`;
}
export function pasteboard() {
  const m = Math.round(Math.max(editor.scene.width, editor.scene.height) * 0.55);
  return {m, x: -m, y: -m, w: editor.scene.width + m * 2, h: editor.scene.height + m * 2};
}
// A faint copy of the previous clip on the selected layer's track, to line up the next pose in a stop-motion sequence.
function onionSkin() {
  const s = editor.scene.shapes.find(shape => shape.id === editor.selected);
  if (!s?.trackId || !inClip(s, editor.time)) return null;
  const previous = editor.scene.shapes
    .filter(x => x !== s && x.trackId === s.trackId && clipEnd(x) <= clipStart(s) + 1e-6)
    .sort((a, b) => clipEnd(b) - clipEnd(a))[0];
  if (!previous) return null;
  const p = state({...previous, inTime: undefined, outTime: undefined}, Math.max(clipStart(previous), clipEnd(previous) - 0.001));
  const g = element('g', {transform: `translate(${p.x} ${p.y}) rotate(${p.r})`, fill: colorAt(previous, clipEnd(previous) - 0.001), opacity: 0.3, class: 'onion-skin', 'pointer-events': 'none'});
  g.append(geometry(previous, p, true));
  return g;
}
export function render() {
  const svg = $('canvas'),
    v = pasteboard();
  svg.replaceChildren();
  svg.setAttribute('viewBox', `${v.x} ${v.y} ${v.w} ${v.h}`);
  svg.style.background = 'transparent';
  const defs = element('defs', {}),
    clip = element('clipPath', {id: 'frame-clip'}),
    marker = element('marker', {
      id: 'motion-arrow-head',
      viewBox: '0 0 10 10',
      refX: 8,
      refY: 5,
      markerWidth: 7,
      markerHeight: 7,
      orient: 'auto-start-reverse'
    });
  clip.append(element('rect', {x: 0, y: 0, width: editor.scene.width, height: editor.scene.height}));
  marker.append(element('path', {d: 'M 0 0 L 10 5 L 0 10 z', fill: '#2F6B45'}));
  defs.append(clip, marker);
  svg.append(defs);
  svg.append(
    element('rect', {
      x: 0,
      y: 0,
      width: editor.scene.width,
      height: editor.scene.height,
      fill: editor.scene.bg,
      class: 'artboard'
    })
  );
  const ghost = !editor.playing && !editor.recording && editor.onionSkin && onionSkin();
  if (ghost) svg.append(ghost);
  const objects = element('g', editor.playing || editor.recording ? {'clip-path': 'url(#frame-clip)'} : {});
  for (const s of editor.scene.shapes) {
    if (!inClip(s, editor.time)) {
      if (s.type === 'video') mediaElement(s).pause();
      continue;
    }
    const p = state(s);
    if (editor.motionDraft?.shapeId === s.id && editor.motionDraft.type === 'path') {
      const lastPoint = editor.motionDraft.points.at(-1);
      p.x = lastPoint.x;
      p.y = lastPoint.y;
    }
    if (s.type === 'video') syncVideo(s);
    const g = element('g', {
      transform: `translate(${p.x} ${p.y}) rotate(${p.r})`,
      fill: colorAt(s),
      opacity: p.o / 100,
      class: 'object',
      'data-id': s.id
    });
    const art = element('g', {}),
      filter = addBlurFilter(defs, s, p);
    if (filter) art.setAttribute('filter', filter);
    art.append(geometry(s, p));
    g.append(art);
    objects.append(g);
  }
  svg.append(objects);
  if (editor.selected && !editor.playing && !editor.recording) {
    const s = editor.scene.shapes.find(shape => shape.id === editor.selected);
    if (s && inClip(s, editor.time)) {
      const p = state(s),
        scale = Math.max(0.1, Math.min(svg.clientWidth / v.w || 1, svg.clientHeight / v.h || 1)),
        smallestSide = Math.min(p.w, p.h) * scale,
        handleSize = Math.min(12, Math.max(8, smallestSide * 0.28)) / scale,
        hitSize = Math.min(28, Math.max(12, smallestSide * 0.5)) / scale;
      const control = element('g', {transform: `translate(${p.x} ${p.y}) rotate(${p.r})`, class: 'selection-control'});
      control.append(element('rect', {x: -p.w / 2, y: -p.h / 2, width: p.w, height: p.h, class: 'selection-box'}));
      const labels = {
        nw: 'top left',
        n: 'top',
        ne: 'top right',
        e: 'right',
        se: 'bottom right',
        s: 'bottom',
        sw: 'bottom left',
        w: 'left'
      };
      for (const [name, [sx, sy]] of Object.entries(resizeDirections)) {
        if ((sx === 0 && p.w * scale < 48) || (sy === 0 && p.h * scale < 48)) continue;
        const x = (sx * p.w) / 2,
          y = (sy * p.h) / 2,
          hit = element('rect', {
            x: x - hitSize / 2,
            y: y - hitSize / 2,
            width: hitSize,
            height: hitSize,
            fill: 'transparent',
            class: 'resize-hit',
            'data-resize-id': s.id,
            'data-resize-handle': name,
            'aria-label': `Resize ${s.name} from ${labels[name]}`
          }),
          visual = element('rect', {
            x: x - handleSize / 2,
            y: y - handleSize / 2,
            width: handleSize,
            height: handleSize,
            class: 'resize-knob',
            'pointer-events': 'none'
          }),
          hint = element('title', {});
        hit.style.cursor = resizeCursor(sx, sy, p.r);
        hint.textContent =
          sx && sy ? `Drag to resize proportionally. Hold Shift to stretch ${s.name}.` : `Drag to resize ${s.name}.`;
        hit.append(hint);
        control.append(hit, visual);
      }
      svg.append(control);
      const top = -p.h / 2,
        handleY = top - 34 / scale,
        rotation = element('g', {transform: `translate(${p.x} ${p.y}) rotate(${p.r})`, class: 'rotation-control'}),
        stem = element('line', {x1: 0, y1: top, x2: 0, y2: handleY}),
        handle = element('circle', {
          cx: 0,
          cy: handleY,
          r: 10 / scale,
          'data-rotate-id': s.id,
          role: 'button',
          'aria-label': 'Drag to rotate layer'
        }),
        icon = element('text', {
          x: 0,
          y: handleY,
          'text-anchor': 'middle',
          'dominant-baseline': 'middle',
          'pointer-events': 'none'
        });
      icon.textContent = '↻';
      icon.style.fontSize = `${14 / scale}px`;
      rotation.append(stem, handle, icon);
      svg.append(rotation);
    }
  }
  if (!editor.playing && !editor.recording)
    for (const s of selectedLayers().slice(1)) {
      if (!inClip(s, editor.time)) continue;
      const p = state(s);
      svg.append(
        element('rect', {
          x: -p.w / 2,
          y: -p.h / 2,
          width: p.w,
          height: p.h,
          class: 'group-box',
          transform: `translate(${p.x} ${p.y}) rotate(${p.r})`
        })
      );
    }
  const guide = editor.motionDraft || editor.motionGuide;
  if (guide && !editor.playing) {
    const group = element('g', {class: 'motion-guide'}),
      points = guide.type === 'path' ? guide.points : [guide.start, guide.end];
    if (points.length > 1) {
      group.append(
        element('polyline', {
          points: points.map(p => `${p.x},${p.y}`).join(' '),
          class: 'motion-guide-line',
          'marker-end': 'url(#motion-arrow-head)'
        })
      );
      group.append(element('circle', {cx: points[0].x, cy: points[0].y, r: 10, class: 'motion-start'}));
      group.append(element('circle', {cx: points.at(-1).x, cy: points.at(-1).y, r: 7, class: 'motion-end'}));
    }
    svg.append(group);
  }
  svg.append(
    element('rect', {
      x: 0,
      y: 0,
      width: editor.scene.width,
      height: editor.scene.height,
      fill: 'none',
      stroke: '#6d746d',
      'stroke-width': Math.max(2, editor.scene.width / 800),
      class: 'frame-edge'
    })
  );
  $('time').innerHTML = `${editor.time.toFixed(2)} s <span>of ${editor.scene.duration.toFixed(2)} s</span>`;
  $('timeline-body').style.setProperty('--progress', Math.min(1, editor.time / editor.scene.duration));
  showQuickAnimate();
  $('scrub').value = editor.time;
}
