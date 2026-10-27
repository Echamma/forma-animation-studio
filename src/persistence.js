import {$, toast} from './dom.js';
import {assetCache} from './media.js';
import {valid} from './model/validate.js';
import {stop} from './playback.js';
import {ensureProjectIdentity, syncProjectName} from './project.js';
import {editor} from './state.js';
import {sanitizeSvg} from './svg/sanitize.js';
import {closeKeyframeMenu} from './ui/keyframe-menu.js';
import {refresh} from './ui/refresh.js';

let historyReady = false;
let lastProjectJson = '';
export const currentProjectJson = () => lastProjectJson;
let undoHistory = [];
let gestureStart = null;
let autosaveTimer = null;
let pendingAutosaveJson = null;
let saveRevision = 0;
let lastInputSource = '';
let lastInputAt = 0;
let lastGestureScan = 0;
// Media strings are stored once per distinct content, so clips that share a picture share one saved asset.
const tokenByValue = new Map();
const assetRegistry = new Map();
const savedAssetTokens = new Set();
const assetFields = ['assetData', 'posterData', 'svgSource'];
function packScene() {
  const shapes = editor.scene.shapes.map(shape => {
    const copy = {...shape};
    for (const field of assetFields) {
      const value = shape[field];
      if (typeof value !== 'string') continue;
      let token = tokenByValue.get(value);
      if (!token) {
        token = crypto.randomUUID();
        tokenByValue.set(value, token);
        assetRegistry.set(token, value);
      }
      copy[field + 'Ref'] = token;
      delete copy[field];
    }
    return copy;
  });
  return JSON.stringify({...editor.scene, shapes});
}
function unpackScene(snapshot) {
  const project = JSON.parse(snapshot);
  for (const shape of project.shapes)
    for (const field of assetFields) {
      const token = shape[field + 'Ref'];
      if (!token) continue;
      const value = assetRegistry.get(token);
      if (value === undefined) throw Error('Undo media is unavailable');
      shape[field] = value;
      delete shape[field + 'Ref'];
    }
  return project;
}
function autosavePayload(projectJson = packScene()) {
  const assets = [];
  for (const shape of JSON.parse(projectJson).shapes)
    for (const field of assetFields) {
      const token = shape[field + 'Ref'];
      if (token && !savedAssetTokens.has(token)) assets.push({token, data: assetRegistry.get(token)});
    }
  return {revision: ++saveRevision, projectJson, assets};
}
function sendAutosave(payload) {
  return window.formaDesktop.autosave(payload).then(
    saved => {
      if (saved) for (const item of payload.assets) savedAssetTokens.add(item.token);
      if (payload.revision === saveRevision) $('autosave-status').textContent = saved ? 'Saved locally' : 'Saving…';
      return saved;
    },
    error => {
      $('autosave-status').textContent = 'Autosave failed';
      toast(error.message || 'Autosave failed');
      throw error;
    }
  );
}
function pushUndo(snapshot) {
  undoHistory.push(snapshot);
  let bytes = undoHistory.reduce((total, item) => total + item.length, 0);
  while (undoHistory.length > 30 || (bytes > 180 * 1024 * 1024 && undoHistory.length > 1))
    bytes -= undoHistory.shift().length;
}
function scheduleAutosave(projectJson, delay = 150) {
  if (!window.formaDesktop?.autosave) return;
  pendingAutosaveJson = projectJson;
  $('autosave-status').textContent = 'Saving…';
  clearTimeout(autosaveTimer);
  autosaveTimer = setTimeout(() => {
    autosaveTimer = null;
    const payload = autosavePayload(pendingAutosaveJson);
    pendingAutosaveJson = null;
    sendAutosave(payload).catch(() => {});
  }, delay);
}
export function recordSceneChange(source = 'edit', recordHistory = true) {
  if (!historyReady) return false;
  const current = packScene();
  if (current === lastProjectJson) return false;
  if (recordHistory && gestureStart === null) {
    const now = performance.now(),
      grouped = source.startsWith('input:') && source === lastInputSource && now - lastInputAt < 1000;
    if (!grouped) pushUndo(lastProjectJson);
    lastInputSource = source.startsWith('input:') ? source : '';
    lastInputAt = now;
  }
  lastProjectJson = current;
  scheduleAutosave(current, source.startsWith('input:') || source === 'gesture' ? 150 : 0);
  return true;
}
function undoScene() {
  recordSceneChange('undo preparation');
  const previous = undoHistory.pop();
  if (!previous) return toast('Nothing to undo');
  stop();
  closeKeyframeMenu();
  assetCache.clear();
  editor.scene = unpackScene(previous);
  syncProjectName();
  editor.selected = editor.scene.shapes.some(s => s.id === editor.selected)
    ? editor.selected
    : editor.scene.shapes[0]?.id;
  editor.selectedMotionId = editor.selectedRotationId = null;
  editor.motionGuide = editor.motionDraft = editor.motionMode = null;
  $('stage').classList.remove('motion-mode');
  editor.time = Math.min(editor.time, editor.scene.duration);
  lastProjectJson = previous;
  lastInputSource = '';
  gestureStart = null;
  refresh();
  scheduleAutosave(previous, 0);
  toast('Undid last change');
}
function endSceneGesture() {
  if (gestureStart === null) return;
  recordSceneChange('gesture', false);
  if (lastProjectJson !== gestureStart) pushUndo(gestureStart);
  gestureStart = null;
  lastInputSource = '';
}
export function restoreProject(saved) {
  if (Array.isArray(saved?.shapes))
    for (const shape of saved.shapes) {
      const refs = assetFields.map(field => ({field, token: shape[field + 'Ref'], value: shape[field]}));
      for (const {field} of refs) delete shape[field + 'Ref'];
      if (shape.type === 'svg') shape.svgSource = sanitizeSvg(shape.svgSource);
      for (const {field, token, value} of refs)
        if (token && shape[field] === value) {
          const known = assetRegistry.get(token);
          if (known === undefined) {
            assetRegistry.set(token, value);
            if (!tokenByValue.has(value)) tokenByValue.set(value, token);
          } else shape[field] = known;
          savedAssetTokens.add(token);
        }
    }
  if (!valid(saved)) return false;
  stop();
  assetCache.clear();
  editor.scene = saved;
  syncProjectName();
  editor.selected = editor.scene.shapes[0]?.id;
  editor.selectedMotionId = editor.selectedRotationId = null;
  editor.motionGuide = null;
  editor.time = 0;
  refresh();
  return true;
}

export function setupPersistence() {
  $('undo-button').onclick = () => undoScene();
  document.addEventListener('click', () => queueMicrotask(() => recordSceneChange('click')));
  document.addEventListener('input', e => {
    const source = 'input:' + (e.target.id || e.target.name || e.target.tagName);
    queueMicrotask(() => recordSceneChange(source));
  });
  document.addEventListener('change', () => queueMicrotask(() => recordSceneChange('change')));
  document.addEventListener(
    'pointerdown',
    e => {
      if (e.target.closest?.('#canvas,.motion-bar')) gestureStart = lastProjectJson;
    },
    true
  );
  document.addEventListener('pointermove', () => {
    if (gestureStart !== null && performance.now() - lastGestureScan > 150) {
      lastGestureScan = performance.now();
      queueMicrotask(() => recordSceneChange('gesture', false));
    }
  });
  document.addEventListener('pointerup', endSceneGesture);
  document.addEventListener('pointercancel', endSceneGesture);
  document.addEventListener(
    'keydown',
    e => {
      if (
        (e.ctrlKey || e.metaKey) &&
        !e.shiftKey &&
        e.key.toLowerCase() === 'z' &&
        !e.target.closest?.('textarea,[contenteditable="true"]') &&
        !document.body.classList.contains('mcp-working')
      ) {
        e.preventDefault();
        e.stopImmediatePropagation();
        undoScene();
      }
    },
    true
  );
  document.addEventListener('visibilitychange', () => {
    if (document.hidden) window.FormaSession?.flushAutosave?.().catch(() => {});
  });
  window.addEventListener('blur', () => {
    if (pendingAutosaveJson) window.FormaSession?.flushAutosave?.().catch(() => {});
  });
  window.FormaSession = {
    snapshot: () => JSON.parse(JSON.stringify(editor.scene)),
    autosavePayload,
    flushAutosave: async () => {
      recordSceneChange('flush');
      clearTimeout(autosaveTimer);
      autosaveTimer = null;
      pendingAutosaveJson = null;
      if (window.formaDesktop?.autosave) await sendAutosave(autosavePayload());
    }
  };
  window.FormaSession.ready = (
    window.formaDesktop
      ? window.formaDesktop
          .loadRecovery()
          .then(saved => {
            if (saved && restoreProject(saved)) toast('Previous animation restored');
          })
          .catch(() => toast('Could not restore the previous session'))
      : Promise.resolve()
  ).finally(() => {
    ensureProjectIdentity();
    syncProjectName();
    lastProjectJson = packScene();
    historyReady = true;
    if (!window.formaDesktop) $('autosave-status').textContent = 'Browser preview only';
  });
}
