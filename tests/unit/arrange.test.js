import {test} from 'node:test';
import assert from 'node:assert/strict';
import {align, distribute} from '../../src/model/arrange.js';

const poses = [
  {x: 100, y: 100, w: 40, h: 20},
  {x: 300, y: 160, w: 100, h: 60},
  {x: 180, y: 40, w: 20, h: 80}
];

test('align left lines up left edges', () => {
  assert.deepEqual(align(poses, 'left').map((p, i) => p.x - poses[i].w / 2), [80, 80, 80]);
});

test('align right and bottom use the outermost edges', () => {
  assert.deepEqual(align(poses, 'right').map(p => p.x), [330, 300, 340]);
  assert.deepEqual(align(poses, 'bottom').map(p => p.y), [180, 160, 150]);
  assert.deepEqual(align(poses, 'bottom').map(p => p.x), [100, 300, 180], 'the other axis is untouched');
});

test('center and middle use the middle of the combined box', () => {
  assert.deepEqual(align(poses, 'center').map(p => p.x), [215, 215, 215]);
  assert.deepEqual(align(poses, 'middle').map(p => p.y), [95, 95, 95]);
});

test('distribute leaves the outer layers and spaces the gaps evenly', () => {
  const across = distribute(poses, 'x');
  assert.equal(across[0].x, 100);
  assert.equal(across[1].x, 300);
  const gaps = [across[2].x - 10 - (across[0].x + 20), across[1].x - 50 - (across[2].x + 10)];
  assert.ok(Math.abs(gaps[0] - gaps[1]) < 1e-9, `equal gaps ${gaps}`);
  assert.deepEqual(across.map(p => p.y), [100, 160, 40]);
});
