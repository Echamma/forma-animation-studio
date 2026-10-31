// Inside Canopy, Forma is one pane of a shared space. The hub sends files from the space (already copied into the media
// library, see app-host.cjs) and asks Forma to open the space's projects from its sidebar.
import {toast} from './dom.js';
import {closeStartScreen, openSavedProject} from './project.js';
import {editor} from './state.js';
import {importMediaSources} from './ui/import.js';

const storedSource = item => ({name: item.name, type: item.type, text: async () => item.text, store: async () => item.url});

export function setupCanopy() {
  const desktop = window.formaDesktop;
  if (!desktop?.canopy) return;
  desktop.onCanopyImport(async items => {
    await window.FormaSession.ready;
    closeStartScreen();
    await importMediaSources(items.map(storedSource));
  });
  desktop.onCanopyOpenProject(async id => {
    await window.FormaSession.ready;
    try {
      if (id !== editor.scene.projectId) await openSavedProject(id);
      closeStartScreen();
      toast('Project opened');
    } catch (error) {
      toast(error.message || 'That project could not be opened');
    }
  });
}
