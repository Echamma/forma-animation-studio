import {$} from '../dom.js';

export function showInspectorTab(name, focus = false) {
  for (const tab of document.querySelectorAll('[data-inspector-tab]')) {
    const active = tab.dataset.inspectorTab === name;
    tab.setAttribute('aria-selected', String(active));
    tab.tabIndex = active ? 0 : -1;
    $('panel-' + tab.dataset.inspectorTab).hidden = !active;
    if (active && focus) tab.focus();
  }
  document.querySelector('.inspector-content').scrollTop = 0;
}
function showDrawerTab(name, focus = false) {
  for (const tab of document.querySelectorAll('[data-drawer-tab]')) {
    const active = tab.dataset.drawerTab === name;
    tab.setAttribute('aria-selected', String(active));
    tab.tabIndex = active ? 0 : -1;
    $('drawer-' + tab.dataset.drawerTab).hidden = !active;
    if (active && focus) tab.focus();
  }
  $('tool-drawer').scrollTop = 0;
}
let drawerReturnFocus = null;
function openDrawer(name) {
  drawerReturnFocus = $(name === 'create' ? 'open-create' : 'open-layers');
  showDrawerTab(name);
  document.querySelector('main').classList.add('drawer-open');
  $('tool-drawer').setAttribute('aria-hidden', 'false');
  $('tool-drawer').removeAttribute('inert');
  $('drawer-scrim').hidden = false;
  for (const id of ['open-create', 'open-layers']) $(id).setAttribute('aria-expanded', 'true');
  $('close-drawer').focus();
}
export function closeDrawer() {
  document.querySelector('main').classList.remove('drawer-open');
  $('tool-drawer').setAttribute('aria-hidden', 'true');
  $('tool-drawer').setAttribute('inert', '');
  $('drawer-scrim').hidden = true;
  for (const id of ['open-create', 'open-layers']) $(id).setAttribute('aria-expanded', 'false');
  drawerReturnFocus?.focus();
}

export function setupTabs() {
  for (const tab of document.querySelectorAll('[data-inspector-tab]')) {
    tab.onclick = () => showInspectorTab(tab.dataset.inspectorTab);
    tab.onkeydown = e => {
      if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(e.key)) return;
      e.preventDefault();
      const names = ['edit', 'animate', 'scene'],
        current = names.indexOf(tab.dataset.inspectorTab),
        next = e.key === 'Home' ? 0 : e.key === 'End' ? 2 : (current + (e.key === 'ArrowRight' ? 1 : 2)) % 3;
      showInspectorTab(names[next], true);
    };
  }
  showInspectorTab('edit');
  $('help-open').onclick = $('guide-open').onclick = () => $('help-dialog').showModal();
  $('open-create').onclick = () => openDrawer('create');
  $('open-layers').onclick = () => openDrawer('layers');
  $('close-drawer').onclick = $('drawer-scrim').onclick = closeDrawer;
  for (const tab of document.querySelectorAll('[data-drawer-tab]')) {
    tab.onclick = () => showDrawerTab(tab.dataset.drawerTab);
    tab.onkeydown = e => {
      if (!['ArrowLeft', 'ArrowRight'].includes(e.key)) return;
      e.preventDefault();
      showDrawerTab(tab.dataset.drawerTab === 'create' ? 'layers' : 'create', true);
    };
  }
  document.addEventListener('keydown', e => {
    if (e.key === 'Escape' && document.querySelector('main').classList.contains('drawer-open')) closeDrawer();
  });
}
