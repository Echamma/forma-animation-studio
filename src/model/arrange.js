// Align and distribute layer poses by their unrotated bounding boxes. Returns the new centers as [{x, y}] in input order.
const AXES = {
  left: ['x', 'w', 'min'],
  center: ['x', 'w', 'mid'],
  right: ['x', 'w', 'max'],
  top: ['y', 'h', 'min'],
  middle: ['y', 'h', 'mid'],
  bottom: ['y', 'h', 'max']
};

export function align(poses, edge) {
  const [axis, size, mode] = AXES[edge],
    low = Math.min(...poses.map(p => p[axis] - p[size] / 2)),
    high = Math.max(...poses.map(p => p[axis] + p[size] / 2));
  return poses.map(p => ({
    x: p.x,
    y: p.y,
    [axis]: mode === 'min' ? low + p[size] / 2 : mode === 'max' ? high - p[size] / 2 : (low + high) / 2
  }));
}

// Equal gaps between neighbours; the first and last layers along the axis stay where they are.
export function distribute(poses, axis) {
  const size = axis === 'x' ? 'w' : 'h',
    order = poses.map((p, i) => i).sort((a, b) => poses[a][axis] - poses[b][axis]),
    first = poses[order[0]],
    last = poses[order.at(-1)],
    span = last[axis] + last[size] / 2 - (first[axis] - first[size] / 2),
    gap = (span - poses.reduce((sum, p) => sum + p[size], 0)) / (poses.length - 1);
  const result = poses.map(p => ({x: p.x, y: p.y}));
  let edge = first[axis] - first[size] / 2;
  for (const i of order) {
    result[i][axis] = edge + poses[i][size] / 2;
    edge += poses[i][size] + gap;
  }
  return result;
}
