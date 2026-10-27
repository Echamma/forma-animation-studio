import {test, beforeEach} from 'node:test';
import assert from 'node:assert/strict';
import {editor} from '../../src/state.js';
import {props, state} from '../../src/model/animation.js';
import {sampleLayer} from '../../src/model/sampling.js';
import {continueAfter} from '../../src/model/clips.js';

const key = (t, x, extra = {}) => ({t, x, y: 50, w: 20, h: 20, r: 0, o: 100, ...extra});
const layer = extra => ({id: 'a', name: 'A', type: 'rect', color: '#000000', ease: 'linear', keys: [key(0, 0), key(2, 200)], ...extra});

// What an SVG player shows: linear interpolation between samples, taking the later value when two share a time.
function played(samples, t) {
  let i = samples.findLastIndex(sample => sample.t <= t);
  const a = samples[i], b = samples[i + 1];
  if (!b || b.t === a.t) return a.pose;
  const f = (t - a.t) / (b.t - a.t);
  return Object.fromEntries(props.map(p => [p, a.pose[p] + (b.pose[p] - a.pose[p]) * f]));
}
function matches(s, samples, tolerance = 1e-6) {
  for (let t = 0.013; t < editor.scene.duration; t += 0.137)
    for (const p of props) assert.ok(Math.abs(played(samples, t)[p] - state(s, t)[p]) < tolerance, `${p} at ${t}`);
}

beforeEach(() => {
  editor.scene = {version: 1, width: 960, height: 600, duration: 4, bg: '#ffffff', shapes: []};
});

test('straight moves and still stretches need only their ends', () => {
  const s = layer();
  editor.scene.shapes.push(s);
  const samples = sampleLayer(s);
  assert.deepEqual(samples.map(x => x.t), [0, 2, 4]);
  matches(s, samples);
});

test('curved easing is sampled densely between its keyframes only', () => {
  const s = layer({ease: 'smooth', keys: [key(0, 0), key(1, 100), key(4, 100)]});
  editor.scene.shapes.push(s);
  const samples = sampleLayer(s);
  assert.ok(samples.filter(x => x.t > 0 && x.t < 1).length >= 58, 'about 60 samples in the eased second');
  assert.ok(samples.filter(x => x.t > 1).length <= 2, 'the still part needs no samples');
});

test('hold keys and clip cuts become instant steps', () => {
  const s = layer({ease: 'hold', outTime: 3});
  editor.scene.shapes.push(s);
  const samples = sampleLayer(s);
  const steps = samples.filter((x, i) => i && x.t === samples[i - 1].t).map(x => x.t);
  assert.deepEqual(steps, [2, 3]);
  matches(s, samples);
});

test('a stop-motion sequence plays back exactly', () => {
  const first = layer({keys: [key(0, 10)], outTime: 0.5});
  editor.scene.shapes.push(first);
  let clip = first;
  for (const x of [60, 110, 160]) {
    clip = continueAfter(clip);
    clip.keys[0].x = x;
  }
  for (const s of editor.scene.shapes) {
    const samples = sampleLayer(s);
    assert.ok(samples.length <= 6, `${s.name} needs ${samples.length} samples`);
    matches(s, samples);
  }
});

test('rotation clips with easing are sampled; linear ones are not', () => {
  const s = layer({keys: [key(0, 0)], rotationClips: [{id: 'r', startTime: 1, duration: 1, degrees: 90, ease: 'linear'}]});
  editor.scene.shapes.push(s);
  assert.deepEqual(sampleLayer(s).map(x => x.t), [0, 1, 2, 4]);
  s.rotationClips[0].ease = 'smooth';
  assert.ok(sampleLayer(s).length > 50);
  // Curves are sampled at 60 fps, so playback between samples is close rather than exact.
  matches(s, sampleLayer(s), 0.05);
});
