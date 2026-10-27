import {test, beforeEach} from 'node:test';
import assert from 'node:assert/strict';
import {editor} from '../../src/state.js';
import {animationClips, editableState, rawState, reappearAt, rotationAt, state, visibilityAt} from '../../src/model/animation.js';

const pose = (t, x, extra = {}) => ({t, x, y: 0, w: 100, h: 100, r: 0, o: 100, ...extra});
const layer = (id, ease, keys, extra = {}) => ({id, name: id, type: 'rect', color: '#000000', ease, keys, ...extra});

beforeEach(() => {
  editor.scene = {version: 1, width: 960, height: 600, duration: 10, bg: '#ffffff', shapes: []};
  editor.time = 0;
});

test('linear interpolation is proportional between keys', () => {
  const s = layer('a', 'linear', [pose(0, 0), pose(2, 200)]);
  assert.equal(rawState(s, 0.5).x, 50);
  assert.equal(rawState(s, 1).x, 100);
});

test('smooth interpolation eases in and out but meets the midpoint', () => {
  const s = layer('a', 'smooth', [pose(0, 0), pose(2, 200)]);
  assert.equal(rawState(s, 1).x, 100);
  assert.ok(rawState(s, 0.5).x < 50, 'starts slower than linear');
  assert.ok(rawState(s, 1.5).x > 150, 'ends slower than linear');
});

test('hold keeps the previous pose until the next key', () => {
  const s = layer('a', 'hold', [pose(0, 0), pose(2, 200)]);
  assert.equal(rawState(s, 1.99).x, 0);
  assert.equal(rawState(s, 2).x, 200);
});

test('poses clamp before the first and after the last key', () => {
  const s = layer('a', 'linear', [pose(1, 10), pose(2, 20)]);
  assert.equal(rawState(s, 0).x, 10);
  assert.equal(rawState(s, 9).x, 20);
});

test('rawState defaults to the playhead', () => {
  const s = layer('a', 'linear', [pose(0, 0), pose(2, 200)]);
  editor.time = 1;
  assert.equal(rawState(s).x, 100);
});

test('rotation clips stack on top of each other', () => {
  const s = layer('a', 'linear', [pose(0, 0)], {
    rotationClips: [
      {id: 'r1', startTime: 0, duration: 2, degrees: 360, ease: 'linear'},
      {id: 'r2', startTime: 1, duration: 1, degrees: -90, ease: 'smooth'}
    ]
  });
  assert.equal(rotationAt(s, 0), 0);
  assert.equal(rotationAt(s, 1), 180);
  assert.equal(rotationAt(s, 1.5), 270 - 45);
  assert.equal(rotationAt(s, 5), 360 - 90);
});

test('state adds base rotation and rotation clips to keyframed rotation', () => {
  const s = layer('a', 'linear', [pose(0, 0, {r: 10})], {
    baseRotation: 30,
    rotationClips: [{id: 'r', startTime: 0, duration: 1, degrees: 90, ease: 'linear'}]
  });
  editor.scene.shapes.push(s);
  assert.equal(state(s, 1).r, 130);
  assert.equal(editableState(s, 1).r, 10, 'editable pose excludes non-keyframe rotation');
});

test('disappear fades out and stays hidden', () => {
  const s = layer('a', 'linear', [pose(0, 0)], {disappear: {startTime: 1, duration: 1, reappearMode: 'never'}});
  editor.scene.shapes.push(s);
  assert.equal(visibilityAt(s, 0.5), 1);
  assert.equal(visibilityAt(s, 1.5), 0.5);
  assert.equal(visibilityAt(s, 9), 0);
  assert.equal(state(s, 1.5).o, 50);
  assert.equal(editableState(s, 1.5).o, 100, 'editing a hidden layer keeps its keyed opacity');
});

test('disappear can reappear at a set time', () => {
  const s = layer('a', 'linear', [pose(0, 0)], {disappear: {startTime: 1, duration: 0, reappearMode: 'time', reappearTime: 3}});
  editor.scene.shapes.push(s);
  assert.equal(visibilityAt(s, 2), 0);
  assert.equal(visibilityAt(s, 3), 1);
});

test('reappearance linked to another clip follows that clip when it is retimed', () => {
  const rotation = {id: 'spin', startTime: 2, duration: 2, degrees: 90, ease: 'linear'};
  const other = layer('b', 'linear', [pose(0, 0)], {rotationClips: [rotation]});
  const s = layer('a', 'linear', [pose(0, 0)], {
    disappear: {startTime: 1, duration: 0.5, reappearMode: 'clip', triggerShapeId: 'b', triggerClipId: 'spin'}
  });
  editor.scene.shapes.push(s, other);
  assert.equal(reappearAt(s), 4);
  rotation.duration = 3;
  assert.equal(reappearAt(s), 5);
  assert.equal(visibilityAt(s, 4.99), 0);
  assert.equal(visibilityAt(s, 5), 1);
});

test('a missing linked clip keeps the layer hidden', () => {
  const s = layer('a', 'linear', [pose(0, 0)], {
    disappear: {startTime: 1, duration: 0, reappearMode: 'clip', triggerShapeId: 'gone', triggerClipId: 'x'}
  });
  editor.scene.shapes.push(s);
  assert.equal(reappearAt(s), Infinity);
});

test('animationClips lists keyframe ranges, moves and rotations of other layers', () => {
  const a = layer('a', 'linear', [pose(0, 0), pose(2, 10)]);
  const b = layer('b', 'linear', [pose(0, 0)], {
    motions: [{id: 'm', type: 'arrow', startTime: 1, duration: 1}],
    rotationClips: [{id: 'r', startTime: 0, duration: 3, degrees: 90, ease: 'linear'}]
  });
  editor.scene.shapes.push(a, b);
  const clips = animationClips('a');
  assert.deepEqual(clips.map(c => `${c.shape.id}:${c.clip.id}`), ['b:m', 'b:r']);
  assert.deepEqual(animationClips('b').map(c => c.clip), [{id: 'keys', startTime: 0, duration: 2}]);
});

test('linked text follows its target position and rotation', () => {
  const target = layer('t', 'linear', [pose(0, 100), pose(2, 300)]);
  const caption = layer('c', 'linear', [pose(0, 0)], {
    type: 'text', text: 'Hi', linkId: 't', linkBase: {w: 100, h: 100}, offsetX: 0, offsetY: -80,
    textWidth: 120, fontSize: 30, rotationOffset: 0, opacityFactor: 1
  });
  editor.scene.shapes.push(target, caption);
  const p = state(caption, 1);
  assert.equal(p.x, 200);
  assert.equal(p.y, -80);
  assert.equal(p.w, 120);
});
