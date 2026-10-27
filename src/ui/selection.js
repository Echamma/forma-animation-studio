// Multi-selection. Tools act on editor.selected, the primary layer; Shift-click adds more layers to a group that only
// lasts while the primary stays the same, so any code that selects a single layer ends the group automatically.
import {editor} from '../state.js';

export function selectedLayers() {
  const primary = editor.scene.shapes.find(s => s.id === editor.selected);
  if (!primary) return [];
  if (editor.group?.primary !== primary.id) return [primary];
  return [primary, ...editor.scene.shapes.filter(s => s !== primary && editor.group.ids.has(s.id))];
}
export function setSelection(ids, primary = ids.at(-1)) {
  editor.selected = primary;
  editor.selectedMotionId = editor.selectedRotationId = null;
  editor.motionGuide = null;
  editor.group = ids.length > 1 ? {primary, ids: new Set(ids.filter(id => id !== primary))} : null;
}
// The layer clicked last becomes the primary; clicking a selected layer again removes it from the group.
export function toggleInSelection(id) {
  const ids = selectedLayers().map(s => s.id);
  if (!ids.includes(id)) return setSelection([...ids, id], id);
  const rest = ids.filter(x => x !== id);
  setSelection(rest, rest.includes(editor.selected) ? editor.selected : rest.at(-1));
}
// Keep the group but make another member the primary, for example the layer a drag starts on.
export function focusInSelection(id) {
  const ids = selectedLayers().map(s => s.id);
  setSelection(ids.includes(id) ? ids : [id], id);
}
