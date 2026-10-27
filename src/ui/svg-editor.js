import {$, toast} from '../dom.js';
import {assetCache, assetHref} from '../media.js';
import {render} from '../render/canvas.js';
import {editor} from '../state.js';
import {recolorSvg, sanitizeSvg} from '../svg/sanitize.js';
import {lists} from '../timeline/tracks.js';

let selectedSvgPart = null;
function svgPreview() {
  const preview = $('svg-preview');
  preview.replaceChildren();
  selectedSvgPart = null;
  $('svg-part').textContent = 'Select a part above';
  for (const id of ['svg-fill', 'svg-stroke', 'svg-delete-part']) $(id).disabled = true;
  try {
    const source = sanitizeSvg($('svg-source').value),
      doc = new DOMParser().parseFromString(source, 'image/svg+xml'),
      root = document.importNode(doc.documentElement, true);
    preview.append(root);
    $('svg-status').textContent = '';
    return root;
  } catch {
    $('svg-status').textContent = 'Fix the SVG markup to preview it.';
  }
}
function svgPartColor(part, attribute) {
  const value = part.getAttribute(attribute) || part.style[attribute];
  return /^#[0-9a-f]{6}$/i.test(value) ? value : '#d2f583';
}

export function setupSvgEditor() {
  $('svg-preview').onclick = e => {
    const root = $('svg-preview').firstElementChild,
      part = e.target;
    if (!root || part === root || !(part instanceof SVGElement)) return;
    selectedSvgPart = part;
    $('svg-part').textContent = `Selected: ${part.tagName.toLowerCase()}`;
    for (const attr of ['fill', 'stroke']) {
      $('svg-' + attr).disabled = false;
      $('svg-' + attr).value = svgPartColor(part, attr);
    }
    $('svg-delete-part').disabled = false;
  };
  for (const attr of ['fill', 'stroke'])
    $('svg-' + attr).oninput = () => {
      if (!selectedSvgPart) return;
      selectedSvgPart.style.removeProperty(attr);
      selectedSvgPart.setAttribute(attr, $('svg-' + attr).value);
      $('svg-source').value = new XMLSerializer().serializeToString($('svg-preview').firstElementChild);
    };
  $('svg-delete-part').onclick = () => {
    if (!selectedSvgPart) return;
    selectedSvgPart.remove();
    selectedSvgPart = null;
    $('svg-source').value = new XMLSerializer().serializeToString($('svg-preview').firstElementChild);
    svgPreview();
  };
  $('svg-source').oninput = () => svgPreview();
  $('edit-svg').onclick = () => {
    const s = editor.scene.shapes.find(s => s.id === editor.selected);
    if (s?.type !== 'svg') return;
    $('svg-source').value = s.svgSource;
    $('svg-status').textContent = '';
    svgPreview();
    $('svg-dialog').showModal();
  };
  $('recolor-svg').onclick = () => {
    try {
      $('svg-source').value = recolorSvg($('svg-source').value, $('svg-color').value);
      svgPreview();
      $('svg-status').textContent = 'Artwork recolored. Apply the changes to update the layer.';
    } catch {
      $('svg-status').textContent = 'The SVG markup is not valid yet.';
    }
  };
  $('apply-svg').onclick = () => {
    const s = editor.scene.shapes.find(s => s.id === editor.selected);
    if (s?.type !== 'svg') return;
    try {
      const old = assetHref(s);
      s.svgSource = sanitizeSvg($('svg-source').value);
      assetCache.delete(old);
      $('svg-dialog').close();
      render();
      lists();
      toast('SVG artwork updated');
    } catch {
      $('svg-status').textContent = 'Fix the SVG markup before applying it.';
    }
  };
}
