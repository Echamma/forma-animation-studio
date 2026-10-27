import {$, toast} from './dom.js';
import {assetCache} from './media.js';
import {valid} from './model/validate.js';
import {recordSceneChange, restoreProject} from './persistence.js';
import {stop} from './playback.js';
import {editor} from './state.js';
import {sanitizeSvg} from './svg/sanitize.js';
import {refresh} from './ui/refresh.js';

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
  $('projects').onclick = async () => {
    try {
      await window.FormaSession.flushAutosave();
      const projects = await window.formaDesktop.listProjects(),
        list = $('projects-list');
      list.replaceChildren();
      for (const item of projects) {
        const button = document.createElement('button');
        button.className = 'project-entry';
        const title = document.createElement('b'),
          meta = document.createElement('small');
        title.textContent = item.name + (item.id === editor.scene.projectId ? ' · Current' : '');
        meta.textContent = `${item.layerCount} layers · ${new Date(item.savedAt).toLocaleString()}`;
        button.append(title, meta);
        button.onclick = async () => {
          if (item.id === editor.scene.projectId) {
            projectDialog.close();
            return;
          }
          try {
            await window.FormaSession.flushAutosave();
            const saved = await window.formaDesktop.openLocalProject(item.id);
            if (!saved || !restoreProject(saved)) throw Error('Project could not be opened');
            recordSceneChange('switch project');
            await window.FormaSession.flushAutosave();
            projectDialog.close();
            toast('Project opened');
          } catch (error) {
            $('projects-status').textContent = error.message;
          }
        };
        list.append(button);
      }
      if (!projects.length) list.textContent = 'No saved projects yet.';
      $('projects-status').textContent = '';
      projectDialog.showModal();
    } catch (error) {
      toast(error.message || 'Could not list projects');
    }
  };
  $('new-project').onclick = async () => {
    try {
      await window.FormaSession.flushAutosave();
      example();
      editor.scene.projectId = crypto.randomUUID();
      editor.scene.projectName = 'Untitled exploration';
      syncProjectName();
      recordSceneChange('new project');
      await window.FormaSession.flushAutosave();
      toast('New project created');
    } catch (error) {
      toast(error.message || 'Could not create project');
    }
  };
  $('file').onchange = async e => {
    try {
      const f = e.target.files[0];
      if (!f) return;
      const s = JSON.parse(await f.text());
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
        typeof s.projectName === 'string' && s.projectName.trim()
          ? s.projectName.trim()
          : f.name.replace(/\.[^.]+$/, '');
      syncProjectName();
      editor.selected = s.shapes[0]?.id;
      editor.selectedMotionId = editor.selectedRotationId = null;
      editor.motionGuide = null;
      editor.time = 0;
      refresh();
      recordSceneChange('open project');
      toast('Project opened');
    } catch {
      toast('This file is not a valid Forma project');
    }
    e.target.value = '';
  };
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
