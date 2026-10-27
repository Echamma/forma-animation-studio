import {NS} from '../dom.js';

export function sanitizeSvg(source) {
  const doc = new DOMParser().parseFromString(source, 'image/svg+xml'),
    root = doc.documentElement;
  if (!root || root.localName !== 'svg' || doc.querySelector('parsererror')) throw Error('Invalid SVG markup');
  root.querySelectorAll('script,style,foreignObject,iframe,object,embed,audio,video').forEach(e => e.remove());
  [root, ...root.querySelectorAll('*')].forEach(e => {
    for (const a of [...e.attributes]) {
      const n = a.name.toLowerCase(),
        v = a.value.trim();
      if (
        n.startsWith('on') ||
        n === 'srcdoc' ||
        ((n === 'href' || n.endsWith(':href')) && !v.startsWith('#') && !v.startsWith('data:image/')) ||
        (/url\s*\(/i.test(v) && /url\s*\(/i.test(v.replace(/url\s*\(\s*#[a-z0-9_-]+\s*\)/gi, ''))) ||
        /javascript:|@import|expression\s*\(/i.test(v)
      )
        e.removeAttribute(a.name);
    }
  });
  root.setAttribute('xmlns', NS);
  return new XMLSerializer().serializeToString(root);
}
export function recolorSvg(source, color) {
  const doc = new DOMParser().parseFromString(source, 'image/svg+xml'),
    root = doc.documentElement;
  if (!root || root.localName !== 'svg' || doc.querySelector('parsererror')) throw Error('Invalid SVG markup');
  root.setAttribute('fill', color);
  [root, ...root.querySelectorAll('*')].forEach(e => {
    for (const attr of ['fill', 'stroke']) {
      const value = e.getAttribute(attr);
      if (value && value !== 'none' && !value.startsWith('url(')) e.setAttribute(attr, color);
    }
    const style = e.getAttribute('style');
    if (style) e.setAttribute('style', style.replace(/(fill|stroke)\s*:\s*(?!none|url\()[^;]+/gi, `$1:${color}`));
  });
  return sanitizeSvg(new XMLSerializer().serializeToString(root));
}
