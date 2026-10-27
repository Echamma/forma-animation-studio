import {test, beforeEach} from 'node:test';
import assert from 'node:assert/strict';
import {editor} from '../../src/state.js';
import {colorAt, rawState} from '../../src/model/animation.js';
import {cubicBezier, ease, mixColors} from '../../src/model/easing.js';
import {continueAfter, shiftLayer} from '../../src/model/clips.js';
import {valid} from '../../src/model/validate.js';

const close = (a, b, message) => assert.ok(Math.abs(a - b) < 1e-4, `${message}: ${a} vs ${b}`);
const layer = extra => ({id: 'a', name: 'A', type: 'rect', color: '#000000', ease: 'linear',
  keys: [{t: 0, x: 0, y: 0, w: 10, h: 10, r: 0, o: 100}, {t: 1, x: 100, y: 0, w: 10, h: 10, r: 0, o: 100}], ...extra});

beforeEach(() => {
  editor.scene = {version: 1, width: 960, height: 600, duration: 4, bg: '#ffffff', shapes: []};
});

test('cubic-bezier matches known CSS curve values', () => {
  const easeIn = cubicBezier(0.42, 0, 1, 1);
  close(easeIn(0.5), 0.3153, 'ease-in at 50%');
  const linear = cubicBezier(0, 0, 1, 1);
  close(linear(0.37), 0.37, 'straight line');
  assert.equal(easeIn(0), 0);
  assert.equal(easeIn(1), 1);
});

test('overshooting curves can go beyond the end value', () => {
  const back = cubicBezier(0.3, 0, 0.5, 1.6);
  assert.ok(Math.max(...[0.6, 0.7, 0.8, 0.9].map(back)) > 1);
});

test('presets and the original smooth curve', () => {
  close(ease('smooth', 0.25), 0.15625, 'smoothstep');
  assert.equal(ease('linear', 0.3), 0.3);
  assert.equal(ease('hold', 0.9), 0);
  assert.ok(ease('ease-out', 0.5) > 0.5 && ease('ease-in', 0.5) < 0.5);
  close(ease('custom', 0.5, [0, 0, 1, 1]), 0.5, 'custom straight line');
});

test('a keyframe can override the layer easing for the segment after it', () => {
  const s = layer();
  assert.equal(rawState(s, 0.5).x, 50);
  s.keys[0].ease = 'hold';
  assert.equal(rawState(s, 0.5).x, 0);
  s.keys[0].ease = 'custom';
  s.keys[0].curve = [0.42, 0, 1, 1];
  close(rawState(s, 0.5).x / 100, 0.3153, 'custom curve');
});

test('color keyframes blend between colors without touching movement keys', () => {
  const s = layer({colorKeys: [{t: 1, color: '#000000'}, {t: 3, color: '#ff8000'}]});
  assert.equal(colorAt(s, 0), '#000000');
  assert.equal(colorAt(s, 2), '#804000');
  assert.equal(colorAt(s, 9), '#ff8000');
  assert.equal(colorAt(layer(), 2), '#000000', 'without color keys the layer color is used');
  assert.equal(mixColors('#102030', '#102030', 0.5), '#102030');
  assert.equal(s.keys.length, 2);
});

test('moving a clip moves its color keys; continuing bakes the current color', () => {
  const s = layer({colorKeys: [{t: 0, color: '#000000'}, {t: 1, color: '#ffffff'}], outTime: 1});
  editor.scene.shapes.push(s);
  const next = continueAfter(s);
  assert.equal(next.color, '#ffffff');
  assert.equal(next.colorKeys, undefined);
  shiftLayer(s, 0.5);
  assert.deepEqual(s.colorKeys.map(k => k.t), [0.5, 1.5]);
});

test('easing and color keys are validated', () => {
  const project = shape => ({...editor.scene, shapes: [shape]});
  assert.ok(valid(project(layer({colorKeys: [{t: 0, color: '#abcdef'}]}))));
  assert.ok(!valid(project(layer({colorKeys: [{t: 0, color: 'blue'}]}))));
  assert.ok(!valid(project(layer({colorKeys: [{t: 0, color: '#000000'}, {t: 0, color: '#ffffff'}]}))));
  const eased = layer();
  eased.keys[0].ease = 'ease-out';
  assert.ok(valid(project(eased)));
  eased.keys[0].ease = 'bounce';
  assert.ok(!valid(project(eased)));
  eased.keys[0].ease = 'custom';
  eased.keys[0].curve = [1.5, 0, 1, 1];
  assert.ok(!valid(project(eased)), 'curve x values stay within 0..1');
});
