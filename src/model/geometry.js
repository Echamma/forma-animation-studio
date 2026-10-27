export const resizeDirections = {
  nw: [-1, -1],
  n: [0, -1],
  ne: [1, -1],
  e: [1, 0],
  se: [1, 1],
  s: [0, 1],
  sw: [-1, 1],
  w: [-1, 0]
};
export function resizeCursor(sx, sy, angle) {
  const degrees = ((((Math.atan2(sy, sx) * 180) / Math.PI + angle) % 180) + 180) % 180;
  return ['ew-resize', 'nwse-resize', 'ns-resize', 'nesw-resize'][Math.round(degrees / 45) % 4];
}
export function resizedPose(start, angle, handle, delta, freeform = false) {
  const [sx, sy] = resizeDirections[handle],
    radians = (angle * Math.PI) / 180,
    cos = Math.cos(radians),
    sin = Math.sin(radians);
  const dx = delta.x * cos + delta.y * sin,
    dy = -delta.x * sin + delta.y * cos;
  const maxW = Math.max(4000, start.w),
    maxH = Math.max(4000, start.h),
    clamp = (value, min, max) => Math.max(min, Math.min(max, value));
  let w = start.w,
    h = start.h;
  if (sx && sy && !freeform) {
    const scale = clamp(
      1 + (sx * dx * start.w + sy * dy * start.h) / (start.w * start.w + start.h * start.h),
      Math.max(5 / start.w, 5 / start.h),
      Math.min(maxW / start.w, maxH / start.h)
    );
    w = start.w * scale;
    h = start.h * scale;
  } else {
    if (sx) w = clamp(start.w + sx * dx, 5, maxW);
    if (sy) h = clamp(start.h + sy * dy, 5, maxH);
  }
  if (Math.abs(w - start.w) < 0.01 && Math.abs(h - start.h) < 0.01) return {...start};
  w = Math.round(w * 10) / 10;
  h = Math.round(h * 10) / 10;
  const shiftX = (sx * (w - start.w)) / 2,
    shiftY = (sy * (h - start.h)) / 2;
  return {...start, w, h, x: start.x + shiftX * cos - shiftY * sin, y: start.y + shiftX * sin + shiftY * cos};
}
