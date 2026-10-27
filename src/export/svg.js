import {NS, element} from '../dom.js';
import {colorAt, state} from '../model/animation.js';
import {addBlurFilter, geometry} from '../render/canvas.js';
import {editor} from '../state.js';

export function svgText(animated = false) {
  const svg = element('svg', {
    xmlns: NS,
    width: editor.scene.width,
    height: editor.scene.height,
    viewBox: `0 0 ${editor.scene.width} ${editor.scene.height}`,
    overflow: 'hidden'
  });
  const defs = element('defs', {});
  svg.append(defs);
  svg.append(element('rect', {width: editor.scene.width, height: editor.scene.height, fill: editor.scene.bg}));
  for (const s of editor.scene.shapes) {
    const p = state(s, animated ? 0 : editor.time),
      g = element('g', {fill: colorAt(s, animated ? 0 : editor.time)});
    const filter = addBlurFilter(defs, s, p);
    if (filter) g.setAttribute('filter', filter);
    const shape = geometry(s, {...p, w: 1, h: 1}, true);
    if (s.type === 'rect') {
      shape.setAttribute('rx', '0.015');
      shape.setAttribute('ry', '0.015');
    }
    g.setAttribute('transform', `translate(${p.x} ${p.y})`);
    const rot = element('g', {transform: `rotate(${p.r})`}),
      scale = element('g', {transform: `scale(${p.w} ${p.h})`, opacity: p.o / 100});
    scale.append(shape);
    rot.append(scale);
    g.append(rot);
    if (animated) {
      const steps = Math.max(1, Math.min(12000, Math.ceil(editor.scene.duration * 60))),
        values = [];
      for (let i = 0; i <= steps; i++) values.push(state(s, (i / steps) * editor.scene.duration));
      for (const [parent, type, fn] of [
        [g, 'translate', p => `${p.x} ${p.y}`],
        [rot, 'rotate', p => `${p.r}`],
        [scale, 'scale', p => `${p.w} ${p.h}`]
      ])
        parent.append(
          element('animateTransform', {
            attributeName: 'transform',
            type,
            dur: `${editor.scene.duration}s`,
            repeatCount: 'indefinite',
            values: values.map(fn).join(';'),
            calcMode: 'linear'
          })
        );
      scale.append(
        element('animate', {
          attributeName: 'opacity',
          dur: `${editor.scene.duration}s`,
          repeatCount: 'indefinite',
          values: values.map(p => p.o / 100).join(';')
        })
      );
      if (s.colorKeys?.length)
        g.append(
          element('animate', {
            attributeName: 'fill',
            dur: `${editor.scene.duration}s`,
            repeatCount: 'indefinite',
            values: values.map((_, i) => colorAt(s, (i / steps) * editor.scene.duration)).join(';')
          })
        );
    }
    svg.append(g);
  }
  return new XMLSerializer().serializeToString(svg);
}
