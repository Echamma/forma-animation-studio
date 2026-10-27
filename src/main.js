// Renderer entry point. Each feature module wires its own controls; order matches the original single-file app.
import {editor} from './state.js';
import {setupTabs} from './ui/tabs.js';
import {setupKeyframeMenu} from './ui/keyframe-menu.js';
import {setupDisappear} from './animate/disappear.js';
import {setupWorkspace} from './ui/workspace.js';
import {setupPlayback, tick} from './playback.js';
import {setupInspector} from './ui/inspector.js';
import {setupSceneSettings} from './ui/scene-settings.js';
import {setupLayers} from './ui/layers.js';
import {setupImport} from './ui/import.js';
import {setupMotion} from './animate/motion.js';
import {setupRotation} from './animate/rotation.js';
import {setupSvgEditor} from './ui/svg-editor.js';
import {setupCanvasInput} from './ui/canvas-input.js';
import {example, setupProject} from './project.js';
import {setupDialog} from './export/dialog.js';
import {currentProjectJson, setupPersistence} from './persistence.js';
import {setupAutomation} from './automation.js';
import * as animation from './model/animation.js';
import * as geometry from './model/geometry.js';
import * as keyframes from './model/keyframes.js';
import * as validate from './model/validate.js';
import * as canvas from './render/canvas.js';
import * as paint from './render/paint.js';
import * as media from './media.js';
import * as svg from './export/svg.js';
import * as motion from './animate/motion.js';
import * as rotation from './animate/rotation.js';
import * as inspector from './ui/inspector.js';
import * as importer from './ui/import.js';
import * as project from './project.js';
import * as persistence from './persistence.js';
import * as playback from './playback.js';
import * as refresh from './ui/refresh.js';

setupTabs();
setupKeyframeMenu();
setupDisappear();
setupWorkspace();
setupPlayback();
setupInspector();
setupSceneSettings();
setupLayers();
setupImport();
setupMotion();
setupRotation();
setupSvgEditor();
setupCanvasInput();
setupProject();
setupDialog();
example();
requestAnimationFrame(tick);
setupPersistence();
setupAutomation();

// Test builds get one handle onto the editor's functions and state; tests evaluate their checks inside `with (__forma)`.
if (window.formaDesktop?.testMode) {
  const handle = Object.assign(
    {},
    animation,
    geometry,
    keyframes,
    validate,
    canvas,
    paint,
    media,
    svg,
    motion,
    rotation,
    inspector,
    importer,
    project,
    persistence,
    playback,
    refresh
  );
  for (const key of Object.keys(editor))
    Object.defineProperty(handle, key, {get: () => editor[key], set: value => (editor[key] = value), enumerable: true});
  Object.defineProperty(handle, 'lastProjectJson', {get: currentProjectJson});
  window.__forma = handle;
}
