import {$, toast} from './dom.js';
import {assetCache} from './media.js';
import {valid} from './model/validate.js';
import {unpackProjectFile} from './export/project-file.js';
import {recordSceneChange, restoreProject, scheduleMediaMigration} from './persistence.js';
import {stop} from './playback.js';
import {editor} from './state.js';
import {sanitizeSvg} from './svg/sanitize.js';
import {refresh} from './ui/refresh.js';
import {pickPoses} from './ui/import.js';

export function example() {
  editor.scene = {
    version: 1,
    width: 960,
    height: 600,
    duration: 4,
    bg: '#f4f0e8',
    shapes: [
      {
        id: 'a',
        name: 'Coral circle',
        type: 'circle',
        color: '#ed795e',
        ease: 'smooth',
        keys: [
          {t: 0, x: -120, y: 265, w: 190, h: 190, r: 0, o: 100},
          {t: 2, x: 565, y: 275, w: 190, h: 190, r: 0, o: 100},
          {t: 4, x: 360, y: 265, w: 190, h: 190, r: 0, o: 100}
        ]
      },
      {
        id: 'b',
        name: 'Lavender block',
        type: 'rect',
        color: '#aaa0ce',
        ease: 'smooth',
        keys: [
          {t: 0, x: 555, y: 325, w: 180, h: 180, r: 15, o: 100},
          {t: 2, x: 370, y: 320, w: 180, h: 180, r: 105, o: 100},
          {t: 4, x: 1080, y: 325, w: 180, h: 180, r: 195, o: 100}
        ]
      },
      {
        id: 'c',
        name: 'Lime triangle',
        type: 'triangle',
        color: '#c3d77f',
        ease: 'smooth',
        keys: [
          {t: 0, x: 470, y: -90, w: 125, h: 120, r: 0, o: 100},
          {t: 2, x: 470, y: 390, w: 125, h: 120, r: 180, o: 100},
          {t: 4, x: 470, y: 200, w: 125, h: 120, r: 360, o: 100}
        ]
      }
    ]
  };
  editor.selected = 'b';
  editor.time = 0;
  editor.selectedMotionId = editor.selectedRotationId = null;
  editor.motionMode = editor.motionDraft = editor.motionGuide = null;
  $('stage')?.classList.remove('motion-mode');
  stop();
  refresh();
}
export function ensureProjectIdentity() {
  editor.scene.projectId = /^[0-9a-f-]{36}$/i.test(editor.scene.projectId || '')
    ? editor.scene.projectId
    : crypto.randomUUID();
  editor.scene.projectName = (
    typeof editor.scene.projectName === 'string' && editor.scene.projectName.trim()
      ? editor.scene.projectName.trim()
      : 'Untitled exploration'
  ).slice(0, 80);
}
export function syncProjectName() {
  ensureProjectIdentity();
  $('project-name').value = editor.scene.projectName;
  document.title = `${editor.scene.projectName} — Forma`;
}
const projectDialog = $('projects-dialog');
const initials = name => name.trim().split(/\s+/).slice(0, 2).map(word => word[0]).join('').toUpperCase() || 'F';

// The start screen: what to make next, the canvas shape, and the projects saved on this device.
export async function showStartScreen() {
  try {
    if (window.formaDesktop) await window.FormaSession.flushAutosave();
    await listProjects();
  } catch (error) {
    $('projects-status').textContent = error.message || 'Could not list your projects';
  }
  if (!projectDialog.open) projectDialog.showModal();
}

async function listProjects() {
  const list = $('projects-list');
  list.replaceChildren();
  $('projects-status').textContent = '';
  if (!window.formaDesktop) {
    list.textContent = 'The desktop app keeps your projects here. In the browser, use Open a file.';
    return;
  }
  const projects = await window.formaDesktop.listProjects();
  for (const item of projects) {
    const button = document.createElement('button'),
      thumb = document.createElement('span'),
      text = document.createElement('span'),
      title = document.createElement('b'),
      meta = document.createElement('small');
    button.type = 'button';
    button.className = 'project-entry';
    thumb.className = 'project-thumb';
    thumb.setAttribute('aria-hidden', 'true');
    thumb.textContent = initials(item.name);
    title.textContent = item.name + (item.id === editor.scene.projectId ? ' · open now' : '');
    meta.textContent = `${item.layerCount} layer${item.layerCount === 1 ? '' : 's'} · ${new Date(item.savedAt).toLocaleString()}`;
    text.append(title, meta);
    button.append(thumb, text);
    button.onclick = async () => {
      if (item.id === editor.scene.projectId) return projectDialog.close();
      try {
        await openSavedProject(item.id);
        projectDialog.close();
        toast('Project opened');
      } catch (error) {
        $('projects-status').textContent = error.message;
      }
    };
    list.append(button);
  }
  if (!projects.length) list.textContent = 'Nothing saved yet. Projects appear here as you work.';
}

// Switches to a project saved on this device (or, inside Canopy, in the open space).
export async function openSavedProject(id) {
  await window.FormaSession.flushAutosave();
  const saved = await window.formaDesktop.openLocalProject(id);
  if (!saved || !restoreProject(saved)) throw Error('That project could not be opened');
  recordSceneChange('switch project');
  await window.FormaSession.flushAutosave();
}
export const closeStartScreen = () => projectDialog.open && projectDialog.close();

// A new project: the example shapes, or an empty canvas, in the shape chosen on the start screen.
export async function newProject(kind) {
  try {
    await window.FormaSession.flushAutosave();
    example();
    if (kind !== 'shapes') {
      editor.scene.shapes = [];
      editor.selected = undefined;
    }
    const format = document.querySelector('input[name="start-format"]:checked')?.value;
    if (format && format !== `${editor.scene.width}x${editor.scene.height}`) {
      $('format').value = format;
      $('format').dispatchEvent(new Event('change'));
    }
    editor.scene.projectId = crypto.randomUUID();
    editor.scene.projectName = 'Untitled exploration';
    syncProjectName();
    refresh();
    recordSceneChange('new project');
    await window.FormaSession.flushAutosave();
    if (projectDialog.open) projectDialog.close();
    toast(kind === 'shapes' ? 'New project created' : 'Empty project created. Add something with the tools on the left.');
    return true;
  } catch (error) {
    toast(error.message || 'Could not create a project');
    return false;
  }
}

export function setupProject() {
  $('open').onclick = () => $('file').click();
  $('project-name').addEventListener('input', () => {
    editor.scene.projectName = $('project-name').value.slice(0, 80);
    document.title = `${editor.scene.projectName || 'Untitled exploration'} — Forma`;
  });
  $('project-name').addEventListener('blur', () => {
    editor.scene.projectName = editor.scene.projectName.trim() || 'Untitled exploration';
    syncProjectName();
    recordSceneChange('rename project');
  });
  $('projects').onclick = () => showStartScreen();
  $('new-project').onclick = () => newProject('shapes');
  $('new-empty').onclick = () => newProject('empty');
  $('new-stop-motion').onclick = async () => {
    if (await newProject('empty')) pickPoses();
  };
  $('file').onchange = async e => {
    const f = e.target.files[0];
    e.target.value = '';
    if (f) await openProjectFile(f.name, () => f.arrayBuffer());
  };
  window.formaDesktop?.onOpenFile(async ({name, bytes}) => {
    await window.FormaSession.ready;
    await openProjectFile(name, async () => bytes);
  });
  $('reset').onclick = async () => {
    try {
      const saved = await window.formaDesktop?.loadDefault();
      if (saved && restoreProject(saved)) {
        editor.scene.projectId = crypto.randomUUID();
        syncProjectName();
        recordSceneChange('load default');
        toast('Default project loaded');
        return;
      }
    } catch {}
    for (const media of assetCache.values()) if (media.tagName === 'VIDEO') media.pause();
    assetCache.clear();
    example();
    editor.scene.projectId = crypto.randomUUID();
    syncProjectName();
    recordSceneChange('load example');
  };
}

// Opens a .forma archive or a JSON project picked in the app or opened from the system as a new local project.
export async function openProjectFile(name, read) {
  try {
    const bytes = new Uint8Array(await read());
    const s = /\.forma$/i.test(name) ? await unpackProjectFile(bytes) : JSON.parse(new TextDecoder().decode(bytes));
    s.width ??= 960;
    s.height ??= 600;
    if (Array.isArray(s.shapes))
      for (const shape of s.shapes) if (shape.type === 'svg') shape.svgSource = sanitizeSvg(shape.svgSource);
    if (!valid(s)) throw Error();
    s.shapes.forEach(x => x.keys.sort((a, b) => a.t - b.t));
    await window.FormaSession.flushAutosave();
    stop();
    assetCache.clear();
    editor.scene = s;
    editor.scene.projectId = crypto.randomUUID();
    editor.scene.projectName =
      typeof s.projectName === 'string' && s.projectName.trim() ? s.projectName.trim() : name.replace(/\.[^.]+$/, '');
    syncProjectName();
    editor.selected = s.shapes[0]?.id;
    editor.selectedMotionId = editor.selectedRotationId = null;
    editor.motionGuide = null;
    editor.time = 0;
    refresh();
    recordSceneChange('open project');
    scheduleMediaMigration();
    toast('Project opened');
    return true;
  } catch {
    toast('This file is not a valid Forma project');
    return false;
  }
}
