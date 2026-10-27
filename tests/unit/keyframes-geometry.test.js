import {test, beforeEach} from 'node:test';
import assert from 'node:assert/strict';
import {editor} from '../../src/state.js';
import {applyMotion, extendSceneTo, key, motionGuideFor, restoreMotionKeys} from '../../src/model/keyframes.js';
import {rawState} from '../../src/model/animation.js';
import {resizedPose} from '../../src/model/geometry.js';

const base = {x: 100, y: 100, w: 100, h: 100, r: 0, o: 100};

beforeEach(() => {
  editor.scene = {version: 1, width: 960, height: 600, duration: 4, bg: '#ffffff', shapes: []};
  editor.time = 0;
});

test('key() adds a keyframe at the playhead and keeps keys sorted', () => {
  const s = {keys: [{t: 0, ...base}, {t: 2, ...base}]};
  editor.time = 1;
  key(s, {...base, x: 300});
  assert.deepEqual(s.keys.map(k => k.t), [0, 1, 2]);
});

test('key() updates a keyframe within 0.01 s instead of adding a duplicate', () => {
  const s = {keys: [{t: 0, ...base}, {t: 1, ...base}]};
  editor.time = 1.004;
  key(s, {...base, x: 5});
  assert.equal(s.keys.length, 2);
  assert.equal(s.keys[1].x, 5);
});

test('extendSceneTo only ever lengthens the scene', () => {
  extendSceneTo(2);
  assert.equal(editor.scene.duration, 4);
  extendSceneTo(6.5);
  assert.equal(editor.scene.duration, 6.5);
  assert.throws(() => extendSceneTo(Infinity));
});

test('an arrow move generates keys from its start pose to the scaled destination', () => {
  const s = {ease: 'linear', keys: [{t: 0, ...base}]};
  const m = {id: 'm1', type: 'arrow', startTime: 1, duration: 2, distance: 50, spin: 90, face: false,
    startPose: {...base}, sourcePoints: [{x: 100, y: 100}, {x: 300, y: 100}], baseKeys: []};
  applyMotion(s, m);
  const end = s.keys.at(-1);
  assert.equal(end.t, 3);
  assert.equal(end.x, 200, 'distance 50% halves the travel');
  assert.equal(end.r, 90, 'spin is applied over the move');
  assert.equal(rawState(s, 2).x, 150);
  assert.deepEqual(motionGuideFor(m).end, {x: 200, y: 100});
});

test('restoring a move brings back the keys it replaced', () => {
  const original = {t: 2, ...base, x: 999};
  const s = {ease: 'linear', keys: [{t: 0, ...base}, original]};
  const m = {id: 'm1', type: 'arrow', startTime: 1, duration: 2, distance: 100, spin: 0, face: false,
    startPose: {...base}, sourcePoints: [{x: 100, y: 100}, {x: 300, y: 100}], baseKeys: [{...original}]};
  applyMotion(s, m);
  assert.ok(!s.keys.some(k => k.x === 999));
  restoreMotionKeys(s, m);
  assert.deepEqual(s.keys.map(k => k.t), [0, 2]);
  assert.equal(s.keys[1].x, 999);
});

test('dragging a corner scales proportionally and keeps the opposite corner fixed', () => {
  const next = resizedPose(base, 0, 'se', {x: 50, y: 50});
  assert.equal(next.w, 150);
  assert.equal(next.h, 150);
  assert.equal(next.x - next.w / 2, base.x - base.w / 2);
  assert.equal(next.y - next.h / 2, base.y - base.h / 2);
});

test('a side handle changes one dimension only', () => {
  const next = resizedPose(base, 0, 'e', {x: 40, y: 25});
  assert.equal(next.w, 140);
  assert.equal(next.h, 100);
});

test('freeform corner resize stretches each dimension independently', () => {
  const next = resizedPose(base, 0, 'se', {x: 30, y: 10}, true);
  assert.equal(next.w, 130);
  assert.equal(next.h, 110);
});

test('resizing a rotated layer works along its own axes', () => {
  const next = resizedPose(base, 90, 'e', {x: 0, y: 40});
  assert.equal(next.w, 140);
  assert.equal(next.h, 100);
});

test('resizing never shrinks below 5 px', () => {
  const next = resizedPose(base, 0, 'e', {x: -500, y: 0});
  assert.equal(next.w, 5);
});
