// Easing for the stretch of animation that starts at a keyframe. 'smooth' is Forma's original curve; the other
// presets match their CSS namesakes, and 'custom' uses the keyframe's own cubic-bezier curve.
export const EASES = ['smooth', 'linear', 'ease-in', 'ease-out', 'hold', 'custom'];
const PRESETS = {'ease-in': [0.42, 0, 1, 1], 'ease-out': [0, 0, 0.58, 1]};
const curves = new Map();

export function cubicBezier(x1, y1, x2, y2) {
  const cx = 3 * x1,
    bx = 3 * (x2 - x1) - cx,
    ax = 1 - cx - bx,
    cy = 3 * y1,
    by = 3 * (y2 - y1) - cy,
    ay = 1 - cy - by;
  const x = t => ((ax * t + bx) * t + cx) * t,
    y = t => ((ay * t + by) * t + cy) * t,
    slope = t => (3 * ax * t + 2 * bx) * t + cx;
  return progress => {
    if (progress <= 0) return 0;
    if (progress >= 1) return 1;
    // Newton's method converges quickly for most curves; bisection covers flat spots.
    let t = progress;
    for (let i = 0; i < 8; i++) {
      const error = x(t) - progress,
        d = slope(t);
      if (Math.abs(error) < 1e-7) return y(t);
      if (Math.abs(d) < 1e-6) break;
      t -= error / d;
    }
    let lo = 0,
      hi = 1;
    t = progress;
    for (let i = 0; i < 50 && Math.abs(x(t) - progress) >= 1e-7; i++) {
      if (x(t) < progress) lo = t;
      else hi = t;
      t = (lo + hi) / 2;
    }
    return y(t);
  };
}

function curveFor(points) {
  const key = points.join();
  if (!curves.has(key)) curves.set(key, cubicBezier(...points));
  return curves.get(key);
}

export function ease(kind, f, curve) {
  if (kind === 'linear') return f;
  if (kind === 'hold') return 0;
  if (kind === 'custom' && curve) return curveFor(curve)(f);
  if (PRESETS[kind]) return curveFor(PRESETS[kind])(f);
  return f * f * (3 - 2 * f);
}

export const validCurve = curve =>
  Array.isArray(curve) &&
  curve.length === 4 &&
  curve.every(Number.isFinite) &&
  curve[0] >= 0 &&
  curve[0] <= 1 &&
  curve[2] >= 0 &&
  curve[2] <= 1 &&
  Math.abs(curve[1]) <= 5 &&
  Math.abs(curve[3]) <= 5;

const channels = hex => [1, 3, 5].map(i => parseInt(hex.slice(i, i + 2), 16));
export function mixColors(from, to, f) {
  const a = channels(from),
    b = channels(to);
  return '#' + a.map((value, i) => Math.round(value + (b[i] - value) * f).toString(16).padStart(2, '0')).join('');
}
