use crate::compositing::{LayerMask, MaskMode, MaskPose, MaskShape, MatteMode};
use crate::model::Shape;
use crate::transform::{LayerFrame, axis_scales, max_scale};
use std::collections::{HashMap, VecDeque};
use tiny_skia::{FillRule, IntRect, LineJoin, Mask, Path, Pixmap, Point, Rect, Stroke, Transform};

const CACHE_BYTES: usize = 48 * 1024 * 1024;

struct MaskSample<'a> {
    mask: &'a LayerMask,
    pose: MaskPose,
    space: Transform,
}

impl MaskSample<'_> {
    fn matches(&self, cached: &MaskSettings) -> bool {
        self.pose == cached.pose
            && self.space == cached.space
            && self.mask.shape == cached.shape
            && self.mask.mode == cached.mode
            && self.mask.inverted == cached.inverted
    }

    fn settings(&self) -> MaskSettings {
        MaskSettings {
            pose: self.pose,
            space: self.space,
            shape: self.mask.shape.clone(),
            mode: self.mask.mode,
            inverted: self.mask.inverted,
        }
    }
}

struct MaskSettings {
    pose: MaskPose,
    space: Transform,
    shape: MaskShape,
    mode: MaskMode,
    inverted: bool,
}

struct Cached {
    area: IntRect,
    masks: Vec<MaskSettings>,
    alpha: Mask,
}

#[derive(Default)]
pub(crate) struct MaskCache {
    layers: HashMap<String, Cached>,
    recent: VecDeque<String>,
    bytes: usize,
}

impl MaskCache {
    pub fn apply(
        &mut self,
        layer: &mut Pixmap,
        area: IntRect,
        shape: &Shape,
        frame: LayerFrame,
        view: Transform,
        t: f64,
    ) {
        let samples: Vec<_> = shape
            .masks
            .iter()
            .map(|mask| MaskSample {
                mask,
                pose: mask.pose(t, &shape.ease),
                space: mask.space(frame).post_concat(view),
            })
            .collect();
        if let Some(cached) = self.layers.get(&shape.id)
            && cached.area == area
            && cached.masks.len() == samples.len()
            && samples
                .iter()
                .zip(&cached.masks)
                .all(|(sample, prior)| sample.matches(prior))
        {
            apply_alpha(layer, &cached.alpha);
            self.mark_recent(&shape.id);
            return;
        }
        let Some(alpha) = combine(area, &samples) else {
            return;
        };
        apply_alpha(layer, &alpha);
        let bytes = alpha.data().len();
        if bytes > CACHE_BYTES {
            return;
        }
        self.remove(&shape.id);
        while self.bytes + bytes > CACHE_BYTES {
            let Some(oldest) = self.recent.pop_front() else {
                break;
            };
            self.remove(&oldest);
        }
        self.bytes += bytes;
        self.layers.insert(
            shape.id.clone(),
            Cached {
                area,
                masks: samples.iter().map(MaskSample::settings).collect(),
                alpha,
            },
        );
        self.mark_recent(&shape.id);
    }

    fn remove(&mut self, id: &str) {
        if let Some(previous) = self.layers.remove(id) {
            self.bytes -= previous.alpha.data().len();
        }
        self.recent.retain(|key| key != id);
    }

    fn mark_recent(&mut self, id: &str) {
        self.recent.retain(|key| key != id);
        self.recent.push_back(id.to_owned());
    }
}

fn apply_alpha(layer: &mut Pixmap, mask: &Mask) {
    for (pixel, &alpha) in layer.data_mut().chunks_exact_mut(4).zip(mask.data()) {
        match alpha {
            255 => {}
            0 => pixel.fill(0),
            alpha => {
                for value in pixel {
                    *value = mul(*value, alpha);
                }
            }
        }
    }
}

fn combine(area: IntRect, samples: &[MaskSample<'_>]) -> Option<Mask> {
    if let [only] = samples
        && only.pose.opacity == 100.0
        && !only.mask.inverted
        && only.mask.mode != MaskMode::Subtract
    {
        return rasterize(only, area);
    }
    let mut result = Mask::new(area.width(), area.height())?;
    if samples
        .first()
        .is_some_and(|first| matches!(first.mask.mode, MaskMode::Subtract | MaskMode::Intersect))
    {
        result.data_mut().fill(255);
    }
    for sample in samples {
        let alpha = rasterize(sample, area)?;
        let opacity = (sample.pose.opacity * 2.55).round() as u8;
        for (a, coverage) in result.data_mut().iter_mut().zip(alpha.data()) {
            let coverage = if sample.mask.inverted {
                255 - coverage
            } else {
                *coverage
            };
            let b = mul(coverage, opacity);
            *a = combine_pixel(*a, b, sample.mask.mode);
        }
    }
    Some(result)
}

fn combine_pixel(a: u8, b: u8, mode: MaskMode) -> u8 {
    match mode {
        MaskMode::Add => a + mul(255 - a, b),
        MaskMode::Subtract => mul(a, 255 - b),
        MaskMode::Intersect => mul(a, b),
        MaskMode::Difference => mul(a, 255 - b) + mul(255 - a, b),
    }
}

fn mul(a: u8, b: u8) -> u8 {
    ((u16::from(a) * u16::from(b) + 127) / 255) as u8
}

fn rasterize(sample: &MaskSample<'_>, area: IntRect) -> Option<Mask> {
    let mut output = Mask::new(area.width(), area.height())?;
    let pose = sample.pose;
    let Some(path) = sample.mask.shape.outline(pose.size) else {
        return Some(output);
    };
    let transform = pose.placement().post_concat(sample.space);
    let scale = max_scale(sample.space);
    if !transform.is_valid() || !scale.is_finite() {
        return Some(output);
    }
    let sigma = pose.feather * scale;
    if !sigma.is_finite() {
        return Some(output);
    }
    let transform = transform.post_translate(-area.x() as f32, -area.y() as f32);
    if sigma == 0.0 {
        fill(&mut output, &path, pose.expansion, transform);
        return Some(output);
    }
    let (sx, sy) = axis_scales(transform);
    let dot = transform.sx * transform.kx + transform.ky * transform.sy;
    if (sx - sy).abs() > 0.001 * scale || dot.abs() > 0.001 * sx * sy {
        return feather_local(output, &path, pose, transform, scale);
    }
    // Downsample wide blurs. Keep three sigma beyond the sampled area so feathering
    // can reach the layer even when the outline itself lies outside its pixel bounds.
    let step = (sigma / 16.0).ceil().max(1.0);
    let reduced_sigma = sigma / step;
    let pad = (3.0 * reduced_sigma).ceil() as u32 + 2;
    let (w, h) = (area.width() as f32, area.height() as f32);
    let mut tile = Mask::new(
        (w / step).ceil() as u32 + 2 * pad,
        (h / step).ceil() as u32 + 2 * pad,
    )?;
    let transform = transform
        .post_scale(step.recip(), step.recip())
        .post_translate(pad as f32, pad as f32);
    fill(&mut tile, &path, pose.expansion, transform);
    blur_alpha(&mut tile, reduced_sigma);
    let stride = tile.width() as usize;
    let src = tile.data();
    let width = area.width() as usize;
    if step == 1.0 {
        let pad = pad as usize;
        for (y, row) in output.data_mut().chunks_exact_mut(width).enumerate() {
            let start = (y + pad) * stride + pad;
            row.copy_from_slice(&src[start..start + width]);
        }
    } else {
        let sample_at = |i: usize| {
            let offset = (i as f32 + 0.5) / step + pad as f32 - 0.5;
            (offset.floor() as usize, offset.fract())
        };
        let xs: Vec<_> = (0..width).map(sample_at).collect();
        for (y, row) in output.data_mut().chunks_exact_mut(width).enumerate() {
            let (y, fy) = sample_at(y);
            for (pixel, &(x, fx)) in row.iter_mut().zip(&xs) {
                *pixel = bilinear(src, stride, x, y, fx, fy);
            }
        }
    }
    Some(output)
}

fn bilinear(data: &[u8], stride: usize, x: usize, y: usize, fx: f32, fy: f32) -> u8 {
    let i = y * stride + x;
    let mix = |a, b, f| f32::from(a) * (1.0 - f) + f32::from(b) * f;
    let north = mix(data[i], data[i + 1], fx);
    let south = mix(data[i + stride], data[i + stride + 1], fx);
    (north * (1.0 - fy) + south * fy).round() as u8
}

fn feather_local(
    mut output: Mask,
    path: &Path,
    pose: MaskPose,
    transform: Transform,
    scale: f32,
) -> Option<Mask> {
    // A stretched or sheared layer also stretches its feather. Blur in the authored
    // coordinates and sample that alpha through the same affine as the outline.
    let Some(inverse) = transform.invert().filter(Transform::is_valid) else {
        return Some(output);
    };
    let (w, h) = (output.width() as f32, output.height() as f32);
    let mut corners = [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)].map(|(x, y)| Point::from_xy(x, y));
    inverse.map_points(&mut corners);
    let Some(bounds) = Rect::from_points(&corners) else {
        return Some(output);
    };
    let resolution = scale
        .min(16.0 / pose.feather)
        .min(4096.0 / bounds.width().max(bounds.height()));
    if !resolution.is_finite() || resolution <= 0.0 {
        return Some(output);
    }
    let sigma = resolution * pose.feather;
    let pad = (3.0 * sigma).ceil() as u32 + 2;
    let shape_to_tile = Transform::from_scale(resolution, resolution).post_translate(
        pad as f32 - bounds.left() * resolution,
        pad as f32 - bounds.top() * resolution,
    );
    let mut tile = Mask::new(
        (bounds.width() * resolution).ceil() as u32 + pad * 2,
        (bounds.height() * resolution).ceil() as u32 + pad * 2,
    )?;
    fill(&mut tile, path, pose.expansion, shape_to_tile);
    blur_alpha(&mut tile, sigma);
    let to_tile = inverse
        .post_concat(shape_to_tile)
        .post_translate(-0.5, -0.5);
    let stride = tile.width() as usize;
    let data = tile.data();
    for (y, row) in output.data_mut().chunks_exact_mut(w as usize).enumerate() {
        let mut start = Point::from_xy(0.5, y as f32 + 0.5);
        to_tile.map_point(&mut start);
        for (x, pixel) in row.iter_mut().enumerate() {
            let sx = start.x + x as f32 * to_tile.sx;
            let sy = start.y + x as f32 * to_tile.ky;
            if sx >= 0.0
                && sx < tile.width() as f32 - 1.0
                && sy >= 0.0
                && sy < tile.height() as f32 - 1.0
            {
                *pixel = bilinear(
                    data,
                    stride,
                    sx as usize,
                    sy as usize,
                    sx.fract(),
                    sy.fract(),
                );
            }
        }
    }
    Some(output)
}

fn fill(alpha: &mut Mask, path: &Path, expansion: f32, transform: Transform) {
    alpha.fill_path(path, FillRule::EvenOdd, true, transform);
    if expansion == 0.0 {
        return;
    }
    let stroke = Stroke {
        width: 2.0 * expansion.abs(),
        line_join: LineJoin::Round,
        ..Stroke::default()
    };
    let Some(edge) = path.stroke(&stroke, max_scale(transform).clamp(0.01, 32.0)) else {
        return;
    };
    let Some(mut border) = Mask::new(alpha.width(), alpha.height()) else {
        return;
    };
    border.fill_path(&edge, FillRule::Winding, true, transform);
    for (a, &b) in alpha.data_mut().iter_mut().zip(border.data()) {
        *a = if expansion > 0.0 {
            (*a).max(b)
        } else {
            mul(*a, 255 - b)
        };
    }
}

fn blur_alpha(alpha: &mut Mask, sigma: f32) {
    let (w, h) = (alpha.width() as usize, alpha.height() as usize);
    let mut scratch = vec![0; w * h];
    let data = alpha.data_mut();
    for size in crate::render::box_sizes(sigma) {
        let radius = (size - 1) / 2;
        if radius > 0 {
            crate::render::box_blur::<1>(data, &mut scratch, h, w, 1, w, radius);
            crate::render::box_blur::<1>(&scratch, data, w, h, w, 1, radius);
        }
    }
}

pub(crate) struct LayerPixels {
    pub area: IntRect,
    pub pixmap: Pixmap,
}

impl LayerPixels {
    pub fn paint_over(&self, target: &mut Pixmap) {
        let Some(area) = IntRect::from_xywh(0, 0, target.width(), target.height())
            .and_then(|canvas| self.area.intersect(&canvas))
        else {
            return;
        };
        let width = area.width() as usize * 4;
        let src_stride = self.area.width() as usize * 4;
        let dst_stride = target.width() as usize * 4;
        let src_x = (area.x() - self.area.x()) as usize * 4;
        let src_y = (area.y() - self.area.y()) as usize;
        let dst_x = area.x() as usize * 4;
        let dst_y = area.y() as usize;
        let src = self.pixmap.data();
        let dst = target.data_mut();
        for y in 0..area.height() as usize {
            let from = (src_y + y) * src_stride + src_x;
            let to = (dst_y + y) * dst_stride + dst_x;
            for (src, dst) in src[from..from + width]
                .chunks_exact(4)
                .zip(dst[to..to + width].chunks_exact_mut(4))
            {
                match src[3] {
                    0 => {}
                    255 => dst.copy_from_slice(src),
                    alpha => {
                        for (d, s) in dst.iter_mut().zip(src) {
                            *d = s.saturating_add(mul(*d, 255 - alpha));
                        }
                    }
                }
            }
        }
    }

    pub fn apply_matte(&mut self, matte: Option<&Self>, mode: MatteMode) {
        let invert = matches!(mode, MatteMode::AlphaInverted | MatteMode::LumaInverted);
        let luma = matches!(mode, MatteMode::Luma | MatteMode::LumaInverted);
        let Some((matte, overlap)) = matte.and_then(|m| Some((m, m.area.intersect(&self.area)?)))
        else {
            if !invert {
                self.pixmap.data_mut().fill(0);
            }
            return;
        };
        let dst_width = self.area.width() as usize;
        let src_width = matte.area.width() as usize;
        let dst_x = (overlap.x() - self.area.x()) as usize;
        let dst_y = (overlap.y() - self.area.y()) as usize;
        let src_x = (overlap.x() - matte.area.x()) as usize;
        let src_y = (overlap.y() - matte.area.y()) as usize;
        let w = overlap.width() as usize;
        let h = overlap.height() as usize;
        let src = matte.pixmap.data();
        for (y, row) in self
            .pixmap
            .data_mut()
            .chunks_exact_mut(dst_width * 4)
            .enumerate()
        {
            if !(dst_y..dst_y + h).contains(&y) {
                if !invert {
                    row.fill(0)
                }
                continue;
            }
            let (left, row) = row.split_at_mut(dst_x * 4);
            let (row, right) = row.split_at_mut(w * 4);
            if !invert {
                left.fill(0);
                right.fill(0);
            }
            let from = ((src_y + y - dst_y) * src_width + src_x) * 4;
            for (pixel, matte) in row
                .chunks_exact_mut(4)
                .zip(src[from..from + w * 4].chunks_exact(4))
            {
                // Premultiplied RGB already includes the source's opacity.
                let coverage = if luma {
                    ((u32::from(matte[0]) * 2126
                        + u32::from(matte[1]) * 7152
                        + u32::from(matte[2]) * 722
                        + 5000)
                        / 10_000) as u8
                } else {
                    matte[3]
                };
                let coverage = if invert { 255 - coverage } else { coverage };
                for channel in pixel {
                    *channel = mul(*channel, coverage);
                }
            }
        }
    }
}
