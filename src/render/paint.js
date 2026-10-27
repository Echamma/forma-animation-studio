import {isAsset, mediaElement} from '../media.js';
import {colorAt, state} from '../model/animation.js';
import {layerBlur} from './canvas.js';
import {editor} from '../state.js';

// Canvas text does not trigger web font loading, so anything that paints frames for export requests the font first.
export async function ensureFonts() {
  await document.fonts.load('600 16px "DM Sans"');
  await document.fonts.ready;
}

export function paint(ctx, t) {
  ctx.fillStyle = editor.scene.bg;
  ctx.fillRect(0, 0, editor.scene.width, editor.scene.height);
  for (const s of editor.scene.shapes) {
    const p = state(s, t);
    ctx.save();
    ctx.translate(p.x, p.y);
    ctx.rotate((p.r * Math.PI) / 180);
    ctx.globalAlpha = p.o / 100;
    ctx.filter = layerBlur(s) ? 'blur(' + layerBlur(s) + 'px)' : 'none';
    if (isAsset(s)) {
      const media = mediaElement(s);
      if (media.tagName === 'VIDEO' ? media.readyState >= 2 : media.complete)
        ctx.drawImage(media, -p.w / 2, -p.h / 2, p.w, p.h);
    } else if (s.type === 'text') {
      ctx.fillStyle = colorAt(s, t);
      ctx.font = `600 ${p.h}px DM Sans, sans-serif`;
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      const measured = Math.max(1, ctx.measureText(s.text || 'Text').width);
      ctx.scale(p.w / measured, 1);
      ctx.fillText(s.text || 'Text', 0, 0);
    } else {
      ctx.fillStyle = colorAt(s, t);
      ctx.beginPath();
      if (s.type === 'circle') ctx.ellipse(0, 0, p.w / 2, p.h / 2, 0, 0, Math.PI * 2);
      else if (s.type === 'rect') ctx.roundRect(-p.w / 2, -p.h / 2, p.w, p.h, 3);
      else if (s.type === 'arrow') {
        ctx.moveTo(-p.w / 2, -p.h * 0.18);
        ctx.lineTo(p.w * 0.12, -p.h * 0.18);
        ctx.lineTo(p.w * 0.12, -p.h / 2);
        ctx.lineTo(p.w / 2, 0);
        ctx.lineTo(p.w * 0.12, p.h / 2);
        ctx.lineTo(p.w * 0.12, p.h * 0.18);
        ctx.lineTo(-p.w / 2, p.h * 0.18);
        ctx.closePath();
      } else {
        ctx.moveTo(0, -p.h / 2);
        ctx.lineTo(p.w / 2, s.type === 'diamond' ? 0 : p.h / 2);
        if (s.type === 'diamond') ctx.lineTo(0, p.h / 2);
        ctx.lineTo(-p.w / 2, s.type === 'diamond' ? 0 : p.h / 2);
        ctx.closePath();
      }
      ctx.fill();
    }
    ctx.restore();
  }
}
