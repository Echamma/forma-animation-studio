// The Easing section of the inspector: how the keyframe at the playhead eases into the next one.
import {$} from '../dom.js';
import {editor} from '../state.js';
import {openKeyframeMenu} from './keyframe-menu.js';
import {refresh} from './refresh.js';

const atPlayhead = k => Math.abs(k.t - editor.time) < 0.011;
function keyAtPlayhead(s) {
  const k = s?.keys.find(atPlayhead);
  return k && k !== s.keys.at(-1) ? k : null;
}

export function easingFields(s) {
  const k = keyAtPlayhead(s),
    current = k ? (k.ease ?? s.ease) : null;
  for (const button of document.querySelectorAll('[data-key-ease]')) {
    button.disabled = !k;
    button.setAttribute('aria-checked', String(button.dataset.keyEase === current));
  }
  $('key-ease-at').textContent = k ? `at ${k.t.toFixed(2)} s` : '';
  $('key-ease-hint').textContent = !s
    ? 'Select a layer to change its easing.'
    : k
      ? 'How this keyframe moves into the next one.'
      : s.keys.length === 1
        ? 'This layer holds one pose. Add another keyframe to choose how it moves between them.'
        : atPlayhead(s.keys.at(-1))
          ? 'This is the last keyframe, so there is nothing after it to ease into.'
          : 'Move the playhead onto a keyframe to choose how it eases into the next one.';
}

export function setupEasingPanel() {
  for (const button of document.querySelectorAll('[data-key-ease]'))
    button.onclick = () => {
      const s = editor.scene.shapes.find(x => x.id === editor.selected),
        k = keyAtPlayhead(s),
        value = button.dataset.keyEase;
      if (!k) return;
      if (value === 'custom') {
        k.ease = 'custom';
        k.curve ??= [0.42, 0, 0.58, 1];
        refresh();
        const box = button.getBoundingClientRect();
        openKeyframeMenu({preventDefault() {}, stopPropagation() {}, clientX: box.left - 270, clientY: box.top - 140}, s, k);
        return;
      }
      // Choosing the layer's own easing removes the override, so later changes to the layer apply here too.
      if (value === s.ease) delete k.ease;
      else k.ease = value;
      delete k.curve;
      refresh();
    };
}
