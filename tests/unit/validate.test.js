import {test} from 'node:test';
import assert from 'node:assert/strict';
import {usableProject, valid} from '../../src/model/validate.js';

const project = (changes = {}) => ({
  version: 1, width: 960, height: 600, duration: 4, bg: '#f4f0e8',
  shapes: [{id: 'a', name: 'Circle', type: 'circle', color: '#ed795e', ease: 'smooth',
    keys: [{t: 0, x: 10, y: 10, w: 50, h: 50, r: 0, o: 100}, {t: 2, x: 90, y: 10, w: 50, h: 50, r: 0, o: 100}]}],
  ...changes
});
const withLayer = changes => {
  const p = project();
  Object.assign(p.shapes[0], changes);
  return p;
};

test('a minimal project is valid', () => {
  assert.ok(valid(project()));
  assert.ok(usableProject(project()));
});

test('scene size, duration and background are checked', () => {
  assert.ok(!valid(project({width: 50})));
  assert.ok(!valid(project({height: 5000})));
  assert.ok(!valid(project({duration: 0.5})));
  assert.ok(!valid(project({bg: 'red'})));
  assert.ok(!valid(project({version: 2})));
});

test('layer ids must be unique', () => {
  const p = project();
  p.shapes.push({...p.shapes[0]});
  assert.ok(!valid(p));
});

test('keyframes must sit inside the scene and have unique times', () => {
  assert.ok(!valid(withLayer({keys: [{t: 5, x: 0, y: 0, w: 1, h: 1, r: 0, o: 100}]})));
  assert.ok(!valid(withLayer({keys: [{t: 1, x: 0, y: 0, w: 1, h: 1, r: 0, o: 100}, {t: 1, x: 5, y: 0, w: 1, h: 1, r: 0, o: 100}]})));
  assert.ok(!valid(withLayer({keys: []})));
  assert.ok(!valid(withLayer({keys: [{t: 0, x: 0, y: 0, w: 0, h: 1, r: 0, o: 100}]})));
  assert.ok(!valid(withLayer({keys: [{t: 0, x: 0, y: 0, w: 1, h: 1, r: 0, o: 101}]})));
});

test('media layers need matching data URLs', () => {
  assert.ok(!valid(withLayer({type: 'image', assetData: 'data:text/html;base64,AAAA'})));
  assert.ok(valid(withLayer({type: 'image', assetData: 'data:image/png;base64,AAAA'})));
  assert.ok(!valid(withLayer({type: 'video', assetData: 'data:video/mp4;base64,AAAA'})), 'video needs a poster and duration');
});

test('blur, layer angle and rotation clips are range checked', () => {
  assert.ok(valid(withLayer({blur: 100})));
  assert.ok(!valid(withLayer({blur: 101})));
  assert.ok(!valid(withLayer({baseRotation: 20000})));
  assert.ok(valid(withLayer({rotationClips: [{id: 'r', startTime: 1, duration: 2, degrees: 90, ease: 'linear'}]})));
  assert.ok(!valid(withLayer({rotationClips: [{id: 'r', startTime: 3, duration: 2, degrees: 90, ease: 'linear'}]})), 'clip past scene end');
  assert.ok(!valid(withLayer({rotationClips: [{id: 'r', startTime: 0, duration: 1, degrees: 90, ease: 'bounce'}]})));
});

test('disappearance timing is validated', () => {
  assert.ok(valid(withLayer({disappear: {startTime: 1, duration: 1, reappearMode: 'never'}})));
  assert.ok(valid(withLayer({disappear: {startTime: 1, duration: 1, reappearMode: 'time', reappearTime: 3}})));
  assert.ok(!valid(withLayer({disappear: {startTime: 1, duration: 1, reappearMode: 'time', reappearTime: 1.5}})));
  assert.ok(!valid(withLayer({disappear: {startTime: 1, duration: 1, reappearMode: 'clip'}})), 'clip mode needs a trigger');
});

test('usableProject also checks drawn motion metadata', () => {
  const motion = {id: 'm', type: 'arrow', startTime: 0, duration: 1, distance: 100, spin: 0, face: false,
    startPose: {x: 0, y: 0, w: 10, h: 10, r: 0, o: 100}, sourcePoints: [{x: 0, y: 0}, {x: 10, y: 0}]};
  assert.ok(usableProject(withLayer({motions: [motion]})));
  assert.ok(!usableProject(withLayer({motions: [{...motion, distance: 900}]})));
  assert.ok(!usableProject(withLayer({motions: [{...motion, type: 'teleport'}]})));
  assert.ok(!usableProject(withLayer({motions: [{...motion, sourcePoints: []}]})));
});
