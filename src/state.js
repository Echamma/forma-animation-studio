// Editor state shared across modules. Everything that more than one module reassigns lives here.
export const editor = {
  scene: undefined,
  selected: undefined,
  time: 0,
  playing: false,
  previewEnd: null,
  last: 0,
  recording: false,
  motionMode: null,
  motionDraft: null,
  motionGuide: null,
  selectedMotionId: null,
  selectedRotationId: null,
  workspaceZoom: 1,
  workspacePanX: 0,
  workspacePanY: 0,
  workspacePan: null,
  onionSkin: true
};
