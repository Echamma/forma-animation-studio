import {test, beforeEach} from 'node:test';
import assert from 'node:assert/strict';
import {editor} from '../../src/state.js';
import {clipEnd, clipStart, inClip, state} from '../../src/model/animation.js';
import {continueAfter, duplicateLayer, shiftLayer, splitAt, trackClips, trimClip} from '../../src/model/clips.js';
import {valid} from '../../src/model/validate.js';

const PNG = 'data:image/png;base64,iVBORw0KGgo=';
const image = (extra = {}) => ({
  id: 'img', name: 'Pose', type: 'image', assetData: PNG, color: '#888888', ease: 'linear',
  keys: [{t: 0, x: 100, y: 100, w: 50, h: 80, r: 0, o: 100}], ...extra
});
const visibleAt = t => editor.scene.shapes.filter(s => state(s, t).o > 0).map(s => s.id);

beforeEach(() => {
  editor.scene = {version: 1, width: 960, height: 600, duration: 4, bg: '#ffffff', shapes: []};
  editor.time = 0;
});

test('a layer without timing is visible for the whole scene, including the last frame', () => {
  const s = image();
  editor.scene.shapes.push(s);
  assert.ok(inClip(s, 0) && inClip(s, 4));
  assert.equal(clipStart(s), 0);
  assert.equal(clipEnd(s), 4);
});

test('a clip window hides the layer before inTime and from outTime on', () => {
  const s = image({inTime: 1, outTime: 2});
  editor.scene.shapes.push(s);
  assert.equal(state(s, 0.99).o, 0);
  assert.equal(state(s, 1).o, 100);
  assert.equal(state(s, 1.99).o, 100);
  assert.equal(state(s, 2).o, 0);
});

test('continuing after a clip makes a still copy that starts where the clip ends', () => {
  const s = image({outTime: 0.5});
  editor.scene.shapes.push(s);
  const next = continueAfter(s);
  assert.equal(next.inTime, 0.5);
  assert.equal(next.outTime, 1);
  assert.equal(next.trackId, s.trackId);
  assert.ok(s.trackId);
  assert.equal(next.assetData, s.assetData);
  assert.deepEqual(next.keys, [{t: 0.5, x: 100, y: 100, w: 50, h: 80, r: 0, o: 100}]);
  assert.equal(next.name, 'Pose 2');
  assert.deepEqual(editor.scene.shapes, [s, next]);
  assert.ok(valid(editor.scene));
});

test('the stop-motion workflow shows one pose at a time in four places', () => {
  const first = image({outTime: 0.5});
  editor.scene.shapes.push(first);
  let clip = first;
  for (const x of [200, 300, 400]) {
    clip = continueAfter(clip);
    clip.keys[0].x = x;
  }
  assert.deepEqual(trackClips(first.trackId).map(c => [c.inTime ?? 0, c.outTime]), [[0, 0.5], [0.5, 1], [1, 1.5], [1.5, 2]]);
  for (const [t, x] of [[0.25, 100], [0.75, 200], [1.25, 300], [1.75, 400]]) {
    const shown = editor.scene.shapes.filter(s => state(s, t).o > 0);
    assert.equal(shown.length, 1, `exactly one pose at ${t} s`);
    assert.equal(state(shown[0], t).x, x);
  }
  assert.deepEqual(visibleAt(2), [], 'nothing after the last clip');
});

test('continuing in the middle of a track pushes later clips right', () => {
  const a = image({outTime: 1});
  editor.scene.shapes.push(a);
  const b = continueAfter(a);
  const inserted = continueAfter(a);
  assert.deepEqual([inserted.inTime, inserted.outTime], [1, 2]);
  assert.deepEqual([b.inTime, b.outTime], [2, 3]);
  assert.equal(b.keys[0].t, 2, 'the pushed clip keeps its pose at its new start');
});

test('a clip without an end first ends at the playhead', () => {
  const s = image();
  editor.scene.shapes.push(s);
  editor.time = 0.8;
  const next = continueAfter(s);
  assert.equal(s.outTime, 0.8);
  assert.deepEqual([next.inTime, next.outTime], [0.8, 1.6]);
});

test('continuing past the scene end lengthens the scene', () => {
  const s = image({inTime: 2, outTime: 3.5});
  editor.scene.shapes.push(s);
  continueAfter(s);
  assert.equal(editor.scene.duration, 5);
});

test('continuing with another layer keeps that artwork and size but takes the previous clip position', () => {
  const s = image({outTime: 1, keys: [{t: 0, x: 10, y: 20, w: 50, h: 80, r: 0, o: 100}, {t: 1, x: 300, y: 40, w: 50, h: 80, r: 30, o: 100}]});
  const other = image({id: 'other', name: 'Arm up', assetData: 'data:image/png;base64,QUJD', keys: [{t: 0, x: 0, y: 0, w: 90, h: 60, r: 0, o: 100}]});
  editor.scene.shapes.push(s);
  const next = continueAfter(s, other);
  assert.equal(next.assetData, other.assetData);
  assert.deepEqual(next.keys[0], {t: 1, x: 300, y: 40, w: 90, h: 60, r: 30, o: 100});
});

test('a copy bakes in rotation clips so it matches the last visible angle', () => {
  const s = image({outTime: 1, rotationClips: [{id: 'r', startTime: 0, duration: 1, degrees: 90, ease: 'linear'}]});
  editor.scene.shapes.push(s);
  const next = continueAfter(s);
  assert.equal(next.keys[0].r, 90);
  assert.equal(next.rotationClips, undefined);
});

test('splitting a clip changes nothing on screen', () => {
  const s = image({ease: 'smooth', keys: [{t: 0, x: 0, y: 0, w: 50, h: 50, r: 0, o: 100}, {t: 4, x: 400, y: 0, w: 50, h: 50, r: 0, o: 100}],
    rotationClips: [{id: 'r', startTime: 1, duration: 2, degrees: 180, ease: 'smooth'}]});
  editor.scene.shapes.push(s);
  const before = [0.5, 1.7, 2.3, 3.9].map(t => state(s, t));
  const second = splitAt(s, 2);
  assert.deepEqual([s.outTime, second.inTime, second.outTime], [2, 2, undefined]);
  for (const [i, t] of [0.5, 1.7, 2.3, 3.9].entries()) {
    const shown = editor.scene.shapes.filter(x => state(x, t).o > 0);
    assert.equal(shown.length, 1);
    assert.deepEqual(state(shown[0], t), before[i]);
  }
});

test('splitting a video continues its media where the first half stops', () => {
  const s = image({type: 'video', mediaStart: 1, inTime: 0.5});
  editor.scene.shapes.push(s);
  assert.equal(splitAt(s, 2).mediaStart, 2.5);
});

test('a split must leave both halves at least 0.05 s long', () => {
  const s = image({outTime: 1});
  editor.scene.shapes.push(s);
  assert.throws(() => splitAt(s, 0.02));
  assert.throws(() => splitAt(s, 1.5));
});

test('moving a clip moves its animation with it', () => {
  const s = image({
    keys: [{t: 0.5, x: 0, y: 0, w: 1, h: 1, r: 0, o: 100}, {t: 1, x: 10, y: 0, w: 1, h: 1, r: 0, o: 100}],
    motions: [{id: 'm', startTime: 0.5, duration: 0.5, baseKeys: [{t: 0.5, x: 0, y: 0, w: 1, h: 1, r: 0, o: 100}]}],
    rotationClips: [{id: 'r', startTime: 0.5, duration: 1, degrees: 90, ease: 'linear'}],
    disappear: {startTime: 1, duration: 0.2, reappearMode: 'time', reappearTime: 1.5},
    inTime: 0.5, outTime: 2
  });
  editor.scene.shapes.push(s);
  assert.equal(shiftLayer(s, 3), 3);
  assert.deepEqual([s.inTime, s.outTime], [3.5, 5]);
  assert.deepEqual(s.keys.map(k => k.t), [3.5, 4]);
  assert.equal(s.motions[0].startTime, 3.5);
  assert.equal(s.motions[0].baseKeys[0].t, 3.5);
  assert.equal(s.rotationClips[0].startTime, 3.5);
  assert.deepEqual([s.disappear.startTime, s.disappear.reappearTime], [4, 4.5]);
  assert.equal(editor.scene.duration, 5, 'scene grows to fit');
});

test('a clip cannot be moved before the start of the scene', () => {
  const s = image({inTime: 1, outTime: 2, keys: [{t: 0.5, x: 0, y: 0, w: 1, h: 1, r: 0, o: 100}]});
  editor.scene.shapes.push(s);
  assert.equal(shiftLayer(s, -3), -0.5);
  assert.equal(s.inTime, 0.5);
  assert.equal(s.keys[0].t, 0);
});

test('trimming changes the window but not the animation', () => {
  const s = image();
  editor.scene.shapes.push(s);
  trimClip(s, 0.5, 6);
  assert.deepEqual([s.inTime, s.outTime, s.keys[0].t, editor.scene.duration], [0.5, 6, 0, 6]);
  trimClip(s, 0.5, undefined);
  assert.equal(s.outTime, undefined);
  assert.throws(() => trimClip(s, 1, 1.01));
});

test('duplicates are independent, nudged, and outside the track', () => {
  const s = image({trackId: 't1', outTime: 1});
  editor.scene.shapes.push(s);
  const copy = duplicateLayer(s);
  assert.notEqual(copy.id, s.id);
  assert.equal(copy.trackId, undefined);
  assert.equal(copy.keys[0].x, 120);
  assert.equal(s.keys[0].x, 100);
});

test('the clip fields are validated', () => {
  const p = {...editor.scene, shapes: [image({inTime: 1, outTime: 2, trackId: 't'})]};
  assert.ok(valid(p));
  assert.ok(!valid({...p, shapes: [image({inTime: 2, outTime: 1})]}));
  assert.ok(!valid({...p, shapes: [image({outTime: 9})]}));
  assert.ok(!valid({...p, shapes: [image({trackId: ''})]}));
  assert.ok(!valid({...p, shapes: [image({mediaStart: -1})]}));
});
