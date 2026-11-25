use crate::effects::{Kind, Parameter::*, Sample};
use rayon::prelude::*;
use std::collections::HashMap;
use tiny_skia::{IntRect, Pixmap, Rect};

const CACHE_LIMIT: usize = 64 * 1024 * 1024;
const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];
const BATCH: usize = 8192;

type Pixel = [u8; 4];

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Region {
    pub area: IntRect,
    pub bounds: Rect,
    pub scale: f32,
}

impl Region {
    fn point(self, index: usize) -> (f32, f32) {
        let width = self.area.width() as usize;
        ((index % width) as f32, (index / width) as f32)
    }

    fn relative(self, x: f32, y: f32) -> (f32, f32) {
        (
            x + self.area.x() as f32 + 0.5 - (self.bounds.left() + self.bounds.width() * 0.5),
            y + self.area.y() as f32 + 0.5 - (self.bounds.top() + self.bounds.height() * 0.5),
        )
    }
}

struct Cached {
    region: Region,
    stack: Vec<Sample>,
    input: Pixmap,
    output: Pixmap,
    used: u64,
}

#[derive(Default)]
pub(crate) struct Cache {
    layers: HashMap<String, Cached>,
    bytes: usize,
    clock: u64,
}

impl Cache {
    pub fn apply(&mut self, id: &str, stack: Vec<Sample>, region: Region, input: Pixmap) -> Pixmap {
        self.clock = self.clock.saturating_add(1);
        if let Some(layer) = self.layers.get_mut(id)
            && layer.region == region
            && layer.stack == stack
            && layer.input.data() == input.data()
        {
            layer.used = self.clock;
            return layer.output.clone();
        }
        let mut output = input.clone();
        for effect in &stack {
            apply(&mut output, effect, region);
        }
        let bytes = input.data().len() * 2;
        if bytes <= CACHE_LIMIT {
            self.remove(id);
            while self.bytes + bytes > CACHE_LIMIT {
                let Some(oldest) = self
                    .layers
                    .iter()
                    .min_by_key(|(_, layer)| layer.used)
                    .map(|(id, _)| id.clone())
                else {
                    break;
                };
                self.remove(&oldest);
            }
            self.layers.insert(
                id.into(),
                Cached {
                    region,
                    stack,
                    input,
                    output: output.clone(),
                    used: self.clock,
                },
            );
            self.bytes += bytes;
        }
        output
    }

    fn remove(&mut self, id: &str) {
        if let Some(layer) = self.layers.remove(id) {
            self.bytes -= layer.input.data().len() * 2;
        }
    }
}

pub(crate) fn margin(stack: &[Sample], scale: f32, bounds: Rect) -> f32 {
    stack
        .iter()
        .map(|effect| {
            let n = |p| effect.get(p) * scale;
            match effect.kind {
                Kind::GaussianBlur | Kind::OuterGlow | Kind::Glow => n(Radius) * 3.0,
                Kind::Outline => n(Radius),
                Kind::DropShadow => n(Distance) + n(Softness) * 3.0,
                Kind::DirectionalBlur | Kind::ChromaticAberration => n(Distance),
                Kind::ZoomBlur => bounds.width().max(bounds.height()) * effect.get(Zoom) * 0.004,
                Kind::WaveWarp => n(WaveHeight),
                Kind::TurbulentDisplace => n(Displacement) * 0.5,
                Kind::Pixelate => n(PixelSize),
                _ => 0.0,
            }
        })
        .sum()
}

fn apply(pixmap: &mut Pixmap, effect: &Sample, region: Region) {
    let n = |p| effect.get(p) * region.scale;
    match effect.kind {
        Kind::GaussianBlur => gaussian(pixmap, n(Radius)),
        Kind::DirectionalBlur
        | Kind::ZoomBlur
        | Kind::ChromaticAberration
        | Kind::Pixelate
        | Kind::WaveWarp
        | Kind::TurbulentDisplace
        | Kind::Bulge
        | Kind::Twirl => {
            resample(pixmap, effect, region);
        }
        Kind::DropShadow | Kind::OuterGlow | Kind::InnerGlow | Kind::Glow | Kind::Outline => {
            light(pixmap, effect, region);
        }
        _ => grade(pixmap, effect, region),
    }
}

fn for_pixels(bytes: &mut [u8], f: impl Fn(usize, &mut [u8]) + Sync) {
    let run = |base: usize, chunk: &mut [u8]| {
        for (i, pixel) in chunk.chunks_exact_mut(4).enumerate() {
            f(base + i, pixel);
        }
    };
    if bytes.len() < BATCH * 4 * 3 {
        run(0, bytes);
    } else {
        bytes
            .par_chunks_mut(BATCH * 4)
            .enumerate()
            .for_each(|(chunk, data)| run(chunk * BATCH, data));
    }
}

fn unit(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}
fn byte(v: f32) -> u8 {
    (unit(v) * 255.0).round() as u8
}
fn luma([r, g, b]: [f32; 3]) -> f32 {
    r * LUMA[0] + g * LUMA[1] + b * LUMA[2]
}
fn mix(a: f32, b: f32, f: f32) -> f32 {
    a + (b - a) * f
}

fn mix_rgb(a: [f32; 3], b: [f32; 3], f: f32) -> [f32; 3] {
    std::array::from_fn(|i| mix(a[i], b[i], f))
}

fn straight(p: &[u8]) -> [f32; 3] {
    let alpha = f32::from(p[3].max(1));
    [p[0], p[1], p[2]].map(|v| f32::from(v) / alpha)
}

fn write_rgb(pixel: &mut [u8], color: [f32; 3]) {
    let alpha = f32::from(pixel[3]) / 255.0;
    for (dst, src) in pixel.iter_mut().zip(color) {
        *dst = byte(unit(src) * alpha);
    }
}

pub(crate) type Matrix = [[f32; 4]; 3];

pub(crate) fn color_matrix(effect: &Sample) -> Option<Matrix> {
    let n = |p| effect.get(p) / 100.0;
    let mut m: Matrix = std::array::from_fn(|i| std::array::from_fn(|j| (i == j) as u8 as f32));
    match effect.kind {
        Kind::BrightnessContrast => {
            let slope = 1.0 + n(Contrast);
            for (i, row) in m.iter_mut().enumerate() {
                row[i] = slope;
                row[3] = 0.5 * (1.0 - slope) + n(Brightness);
            }
        }
        Kind::Invert | Kind::Fill | Kind::Tint => {
            let amount = n(Amount);
            let black = effect.rgb(BlackColor);
            let white = effect.rgb(WhiteColor);
            for (i, row) in m.iter_mut().enumerate() {
                row[i] = 1.0 - amount;
                row[3] = match effect.kind {
                    Kind::Invert => {
                        row[i] -= amount;
                        amount
                    }
                    Kind::Fill => effect.rgb(Color)[i] * amount,
                    _ => {
                        for (coef, weight) in row[..3].iter_mut().zip(LUMA) {
                            *coef += weight * (white[i] - black[i]) * amount;
                        }
                        black[i] * amount
                    }
                };
            }
        }
        Kind::HueSaturationLightness => {
            // SVG's hueRotate matrix around perceptual luminance, followed by saturation and lightness.
            let (sin, cos) = effect.get(Hue).to_radians().sin_cos();
            let sin_rows = [
                [-0.213, -0.715, 0.928],
                [0.143, 0.140, -0.283],
                [-0.787, 0.715, 0.072],
            ];
            let sat = 1.0 + n(Saturation);
            let light = n(Lightness);
            for (i, row) in m.iter_mut().enumerate() {
                for (j, (value, lum)) in row[..3].iter_mut().zip([0.213, 0.715, 0.072]).enumerate()
                {
                    let identity = (i == j) as u8 as f32;
                    *value = (lum + sat * ((identity - lum) * cos + sin_rows[i][j] * sin))
                        * (1.0 - light.abs());
                }
                row[3] = light.max(0.0);
            }
        }
        _ => return None,
    }
    Some(m)
}

fn grade(pixmap: &mut Pixmap, effect: &Sample, region: Region) {
    if let Some(matrix) = color_matrix(effect) {
        for_pixels(pixmap.data_mut(), |_, pixel| {
            if pixel[3] == 0 {
                return;
            }
            let [r, g, b] = straight(pixel);
            write_rgb(
                pixel,
                matrix.map(|row| row[0] * r + row[1] * g + row[2] * b + row[3]),
            );
        });
        return;
    }
    let amount = effect.get(Amount) / 100.0;
    let levels = if effect.kind == Kind::Levels {
        let low = effect.get(InputBlack);
        let span = (effect.get(InputWhite) - low).max(1.0);
        let gamma = 1.0 / effect.get(Gamma);
        let (a, b) = (
            effect.get(OutputBlack) / 255.0,
            effect.get(OutputWhite) / 255.0,
        );
        Some(std::array::from_fn::<_, 256, _>(|i| {
            mix(a, b, unit((i as f32 - low) / span).powf(gamma))
        }))
    } else {
        None
    };
    let (sin, cos) = effect.get(Angle).to_radians().sin_cos();
    let half_w = (region.bounds.width() * 0.5).max(0.5);
    let half_h = (region.bounds.height() * 0.5).max(0.5);
    let span = (cos.abs() * half_w + sin.abs() * half_h).max(0.5);
    let grain = (effect.get(GrainSize) * region.scale).max(1.0);
    let seed = effect.get(Seed);
    let spread = effect.get(Spread) * 0.0095;
    for_pixels(pixmap.data_mut(), |i, pixel| {
        if pixel[3] == 0 {
            return;
        }
        let input = straight(pixel);
        let (x, y) = region.point(i);
        let (dx, dy) = region.relative(x, y);
        let distance = || (dx / half_w).hypot(dy / half_h);
        let output = match effect.kind {
            Kind::Levels => levels.map_or(input, |map| input.map(|v| map[byte(v) as usize])),
            Kind::GradientRamp => {
                let t = if effect.radial {
                    distance()
                } else {
                    0.5 + (dx * cos + dy * sin) / (2.0 * span)
                };
                let ramp = mix_rgb(effect.rgb(StartColor), effect.rgb(EndColor), unit(t));
                mix_rgb(input, ramp, amount)
            }
            Kind::Vignette => {
                let t = unit((distance() - spread) / (1.0 - spread));
                mix_rgb(input, effect.rgb(Color), amount * t * t * (3.0 - 2.0 * t))
            }
            Kind::Noise => {
                let gx = (dx / grain).floor() as i32 as u32;
                let gy = (dy / grain).floor() as i32 as u32;
                let a = hash_noise(gx, gy, seed.floor() as u32);
                let b = hash_noise(gx, gy, seed.floor() as u32 + 1);
                let delta = (mix(a, b, seed.fract()) - 0.5) * amount;
                input.map(|v| v + delta)
            }
            _ => input,
        };
        write_rgb(pixel, output);
    });
}

fn hash_noise(x: u32, y: u32, seed: u32) -> f32 {
    let mut n =
        x.wrapping_mul(0x9e3779b1) ^ y.wrapping_mul(0x85ebca77) ^ seed.wrapping_mul(0xc2b2ae3d);
    n = (n ^ (n >> 16)).wrapping_mul(0x7feb352d);
    n = (n ^ (n >> 15)).wrapping_mul(0x846ca68b);
    ((n ^ (n >> 16)) >> 8) as f32 / 16_777_215.0
}

struct Source<'a> {
    width: i32,
    height: i32,
    bytes: &'a [u8],
}

impl<'a> Source<'a> {
    fn new(pixmap: &'a Pixmap) -> Self {
        Self {
            width: pixmap.width() as i32,
            height: pixmap.height() as i32,
            bytes: pixmap.data(),
        }
    }

    fn pixel(&self, x: i32, y: i32) -> Pixel {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return [0; 4];
        }
        let offset = (y as usize * self.width as usize + x as usize) * 4;
        self.bytes[offset..offset + 4]
            .try_into()
            .unwrap_or_default()
    }

    fn sample(&self, x: f32, y: f32) -> [f32; 4] {
        let (left, top) = (x.floor(), y.floor());
        let (fx, fy) = (x - left, y - top);
        let (left, top) = (left as i32, top as i32);
        let [a, b, c, d] = [
            self.pixel(left, top),
            self.pixel(left + 1, top),
            self.pixel(left, top + 1),
            self.pixel(left + 1, top + 1),
        ];
        std::array::from_fn(|i| {
            mix(
                mix(f32::from(a[i]), f32::from(b[i]), fx),
                mix(f32::from(c[i]), f32::from(d[i]), fx),
                fy,
            )
        })
    }
}

fn resample(pixmap: &mut Pixmap, effect: &Sample, region: Region) {
    let input = pixmap.clone();
    let source = Source::new(&input);
    let (sin, cos) = effect.get(Angle).to_radians().sin_cos();
    let distance = effect.get(Distance) * region.scale;
    let (ox, oy) = (cos * distance, sin * distance);
    let zoom = effect.get(Zoom) * 0.004;
    let block = (effect.get(PixelSize) * region.scale).round().max(1.0);
    let radius = (effect.get(Radius) * region.scale).max(0.01);
    let wave = Wave::new(effect, region.scale);
    let evolution = turbulence_offset(effect) * region.scale;
    let freq = 1.0 / (effect.get(TurbulenceSize) * region.scale).max(0.01);
    let displace = effect.get(Displacement) * region.scale;
    let field = (effect.kind == Kind::TurbulentDisplace).then(Turbulence::new);
    for_pixels(pixmap.data_mut(), |i, pixel| {
        let (x, y) = region.point(i);
        let (dx, dy) = region.relative(x, y);
        let sample = |sx, sy| source.sample(sx, sy);
        let output = match effect.kind {
            Kind::DirectionalBlur => average_blur(&source, x, y, ox * 0.5, oy * 0.5),
            Kind::ZoomBlur => average_blur(&source, x, y, dx * zoom, dy * zoom),
            Kind::ChromaticAberration => {
                let r = sample(x + ox, y + oy);
                let [_, g, _, a] = sample(x, y);
                let b = sample(x - ox, y - oy);
                [r[0], g, b[2], r[3].max(a).max(b[3])]
            }
            Kind::Pixelate => {
                let center = |v: f32| (v / block).floor() * block + block * 0.5;
                sample(x + center(dx) - dx, y + center(dy) - dy)
            }
            Kind::WaveWarp => {
                let delta = wave.offset(dx, dy);
                sample(x - wave.sin * delta, y + wave.cos * delta)
            }
            Kind::TurbulentDisplace => {
                let px = (x + region.area.x() as f32 - evolution) * freq;
                let py = (y + region.area.y() as f32 - evolution) * freq;
                let offset = |channel| {
                    field
                        .as_ref()
                        .map_or(0.0, |field| field.at(px, py, channel))
                        * displace
                };
                sample(x + offset(0), y + offset(1))
            }
            Kind::Bulge | Kind::Twirl => {
                let falloff = unit(1.0 - dx.hypot(dy) / radius);
                if effect.kind == Kind::Bulge {
                    let amount = effect.get(Bulge) * 0.009 * falloff * falloff;
                    sample(x - dx * amount, y - dy * amount)
                } else {
                    let (sin, cos) =
                        (-effect.get(Twist).to_radians() * falloff * falloff).sin_cos();
                    sample(x - dx + dx * cos - dy * sin, y - dy + dx * sin + dy * cos)
                }
            }
            _ => sample(x, y),
        };
        let alpha = output[3].round() as u8;
        for (dst, value) in pixel.iter_mut().zip(output) {
            *dst = (value.round() as u8).min(alpha);
        }
    });
}

fn average_blur(source: &Source<'_>, x: f32, y: f32, dx: f32, dy: f32) -> [f32; 4] {
    let taps = (dx.abs().max(dy.abs()).ceil() as u32 + 1).clamp(2, 12);
    let mut sum = [0.0; 4];
    for i in 0..taps {
        let t = 2.0 * (i as f32 + 0.5) / taps as f32 - 1.0;
        let value = source.sample(x + t * dx, y + t * dy);
        for (sum, v) in sum.iter_mut().zip(value) {
            *sum += v;
        }
    }
    sum.map(|v| v / taps as f32)
}

struct Wave {
    sin: f32,
    cos: f32,
    height: f32,
    frequency: f32,
    phase: f32,
}

impl Wave {
    fn new(effect: &Sample, scale: f32) -> Self {
        let (sin, cos) = effect.get(Direction).to_radians().sin_cos();
        Self {
            sin,
            cos,
            height: effect.get(WaveHeight) * scale,
            frequency: std::f32::consts::TAU / (effect.get(WaveWidth) * scale).max(0.01),
            phase: effect.phase * std::f32::consts::TAU,
        }
    }

    fn offset(&self, x: f32, y: f32) -> f32 {
        ((x * self.cos + y * self.sin) * self.frequency + self.phase).sin() * self.height
    }
}

pub(crate) fn turbulence_offset(effect: &Sample) -> f32 {
    effect.get(Evolution) / 360.0 * effect.get(TurbulenceSize)
}

// SVG Filter Effects' Park–Miller permutation and gradient generator, with seed 1 and
// two non-stitched octaves. Keep the unused channels in initialization to match SVG's shuffle.
struct Turbulence {
    lattice: [usize; 256],
    gradients: [[[f32; 2]; 256]; 2],
}

impl Turbulence {
    fn new() -> Self {
        let mut seed = 1i64;
        let mut next = || {
            seed = seed * 16807 % 2_147_483_647;
            seed as usize
        };
        let mut gradients = [[[0.0; 2]; 256]; 2];
        for channel in 0..4 {
            for i in 0..256 {
                let x = (next() % 512) as f32 - 256.0;
                let y = (next() % 512) as f32 - 256.0;
                if let Some(table) = gradients.get_mut(channel) {
                    let len = x.hypot(y).max(f32::EPSILON);
                    table[i] = [x / len, y / len];
                }
            }
        }
        let mut lattice = std::array::from_fn(|i| i);
        for i in (1..256).rev() {
            lattice.swap(i, next() % 256);
        }
        Self { lattice, gradients }
    }

    fn at(&self, x: f32, y: f32, channel: usize) -> f32 {
        (self.noise(x, y, channel) + self.noise(x * 2.0, y * 2.0, channel) * 0.5) * 0.5
    }

    fn noise(&self, x: f32, y: f32, channel: usize) -> f32 {
        let (lx, ly) = (x.floor(), y.floor());
        let (u, v) = (x - lx, y - ly);
        let (ix, iy) = (lx as i32, ly as i32);
        let dot = |dx: i32, dy: i32| {
            let p = self.lattice[(ix.wrapping_add(dx) & 255) as usize];
            let index = (p + (iy.wrapping_add(dy) & 255) as usize) & 255;
            let [gx, gy] = self.gradients[channel][index];
            gx * (u - dx as f32) + gy * (v - dy as f32)
        };
        let smooth = |t| t * t * (3.0 - 2.0 * t);
        mix(
            mix(dot(0, 0), dot(1, 0), smooth(u)),
            mix(dot(0, 1), dot(1, 1), smooth(u)),
            smooth(v),
        )
    }
}

fn light(pixmap: &mut Pixmap, effect: &Sample, region: Region) {
    let mut alpha = pixmap.clone();
    let threshold = effect.get(Threshold) / 100.0;
    for_pixels(alpha.data_mut(), |_, p| {
        let gain = if effect.kind == Kind::Glow {
            // Retain highlights while softly rolling in above the threshold.
            unit((luma(straight(p)) - threshold) / (1.0 - threshold).max(0.001))
        } else {
            1.0
        };
        let a = (f32::from(p[3]) * gain).round() as u8;
        p.copy_from_slice(&[0, 0, 0, a]);
    });
    let sigma = if effect.kind == Kind::DropShadow {
        effect.get(Softness)
    } else {
        effect.get(Radius)
    };
    if effect.kind == Kind::Outline {
        dilate_alpha(&mut alpha, (sigma * region.scale).round() as usize);
    } else {
        gaussian(&mut alpha, sigma * region.scale);
    }
    let alpha = Source::new(&alpha);
    let strength = if effect.kind == Kind::Glow {
        effect.get(Intensity)
    } else {
        effect.get(Opacity)
    } / 100.0;
    let color = effect.rgb(Color);
    let (sin, cos) = effect.get(Angle).to_radians().sin_cos();
    let distance = effect.get(Distance) * region.scale;
    for_pixels(pixmap.data_mut(), |i, pixel| {
        let (x, y) = region.point(i);
        let a = f32::from(pixel[3]) / 255.0;
        let filtered = if effect.kind == Kind::DropShadow {
            alpha.sample(x - cos * distance, y - sin * distance)[3] / 255.0
        } else {
            f32::from(alpha.bytes[i * 4 + 3]) / 255.0
        };
        if effect.kind == Kind::InnerGlow {
            write_rgb(
                pixel,
                mix_rgb(straight(pixel), color, unit((1.0 - filtered) * strength)),
            );
        } else {
            let light = unit(filtered * strength);
            let behind = light * (1.0 - a);
            for (channel, ink) in pixel[..3].iter_mut().zip(color) {
                let original = f32::from(*channel) / 255.0;
                let gain = if effect.kind == Kind::Glow {
                    behind + light * (a - original)
                } else {
                    behind
                };
                *channel = byte(original + ink * gain);
            }
            pixel[3] = byte(a + behind);
        }
    });
}

fn dilate_alpha(pixmap: &mut Pixmap, radius: usize) {
    if radius == 0 {
        return;
    }
    let w = pixmap.width() as usize;
    let h = pixmap.height() as usize;
    let mut a: Vec<_> = pixmap.data().chunks_exact(4).map(|p| p[3]).collect();
    let mut b = vec![0; a.len()];
    let pass = |from: &[u8], to: &mut [u8], width, height| {
        to.par_chunks_mut(width)
            .zip(from.par_chunks(width))
            .for_each(|(out, row)| {
                let mut queue = std::collections::VecDeque::new();
                for (x, dst) in out.iter_mut().enumerate() {
                    let start = if x == 0 { 0 } else { x + radius };
                    for j in start..=(x + radius).min(width - 1) {
                        while queue.back().is_some_and(|&i| row[i] <= row[j]) {
                            queue.pop_back();
                        }
                        queue.push_back(j);
                    }
                    while queue.front().is_some_and(|&i| i + radius < x) {
                        queue.pop_front();
                    }
                    *dst = queue.front().map_or(0, |&i| row[i]);
                }
            });
        transpose::<1>(to, width, height, from.len())
    };
    a = pass(&a, &mut b, w, h);
    b = pass(&a, &mut b, h, w);
    for_pixels(pixmap.data_mut(), |i, p| p[3] = b[i]);
}

fn gaussian(pixmap: &mut Pixmap, sigma: f32) {
    let w = pixmap.width() as usize;
    let h = pixmap.height() as usize;
    let sizes = crate::render::box_sizes(sigma.clamp(0.0, 512.0));
    if sizes == [1; 3] {
        return;
    }
    if w * h < 32_768 {
        crate::render::gaussian_blur(pixmap, sigma.clamp(0.0, 512.0));
        return;
    }
    let mut scratch = vec![0; w * h * 4];
    for size in sizes.into_iter().filter(|&size| size > 1) {
        let radius = (size - 1) / 2;
        blur_rows(pixmap.data(), &mut scratch, w, radius);
        let mut vertical = transpose::<4>(&scratch, w, h, w * h * 4);
        blur_rows(&vertical, &mut scratch, h, radius);
        transpose_into::<4>(&scratch, h, w, &mut vertical);
        pixmap.data_mut().copy_from_slice(&vertical);
    }
}

fn blur_rows(source: &[u8], output: &mut [u8], width: usize, radius: usize) {
    output
        .par_chunks_mut(width * 4)
        .zip(source.par_chunks(width * 4))
        .for_each(|(out, input)| {
            crate::render::box_blur::<4>(input, out, 1, width, 4, width * 4, radius)
        });
}

fn transpose<const C: usize>(input: &[u8], w: usize, h: usize, len: usize) -> Vec<u8> {
    let mut output = vec![0; len];
    transpose_into::<C>(input, w, h, &mut output);
    output
}

fn transpose_into<const C: usize>(input: &[u8], w: usize, h: usize, output: &mut [u8]) {
    output
        .par_chunks_mut(16 * h * C)
        .enumerate()
        .for_each(|(block, dst)| {
            let x0 = block * 16;
            let count = dst.len() / (h * C);
            for (y, row) in input.chunks_exact(w * C).enumerate() {
                for x in 0..count {
                    let to = (x * h + y) * C;
                    let from = (x0 + x) * C;
                    dst[to..to + C].copy_from_slice(&row[from..from + C]);
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::{Effect, Parameter};
    use tiny_skia::{Color, Paint, Transform};

    fn area() -> Region {
        Region {
            area: IntRect::from_xywh(0, 0, 64, 64).unwrap(),
            bounds: Rect::from_xywh(0.0, 0.0, 64.0, 64.0).unwrap(),
            scale: 1.0,
        }
    }

    fn square(color: [u8; 3]) -> Pixmap {
        let mut pixmap = Pixmap::new(64, 64).unwrap();
        let mut paint = Paint::default();
        paint.set_color(Color::from_rgba8(color[0], color[1], color[2], 255));
        pixmap.fill_rect(
            Rect::from_xywh(24.0, 24.0, 16.0, 16.0).unwrap(),
            &paint,
            Transform::identity(),
            None,
        );
        pixmap
    }

    fn pixel(pixmap: &Pixmap, x: i32, y: i32) -> Pixel {
        Source::new(pixmap).pixel(x, y)
    }

    fn configured(kind: Kind, settings: &[(Parameter, f32)]) -> Effect {
        let mut effect = Effect::new(1, kind);
        for &(parameter, value) in settings {
            effect.edit(parameter, 0.0, [value, 0.0, 0.0]);
        }
        effect
    }

    fn run(image: &Pixmap, effect: &Effect) -> Pixmap {
        let mut image = image.clone();
        let effect = effect.sample(0.0, "linear");
        if effect.active() {
            apply(&mut image, &effect, area());
        }
        image
    }

    #[test]
    fn each_effect_has_an_exact_identity_setting() {
        let input = square([64, 128, 192]);
        let neutral = |kind| match kind {
            Kind::GaussianBlur => vec![(Radius, 0.0)],
            Kind::DirectionalBlur | Kind::ChromaticAberration => vec![(Distance, 0.0)],
            Kind::ZoomBlur => vec![(Zoom, 0.0)],
            Kind::DropShadow | Kind::InnerGlow | Kind::OuterGlow | Kind::Outline => {
                vec![(Opacity, 0.0)]
            }
            Kind::Glow => vec![(Intensity, 0.0)],
            Kind::BrightnessContrast => vec![(Brightness, 0.0), (Contrast, 0.0)],
            Kind::HueSaturationLightness => vec![(Hue, 0.0), (Saturation, 0.0), (Lightness, 0.0)],
            Kind::Levels => vec![],
            Kind::Pixelate => vec![(PixelSize, 1.0)],
            Kind::WaveWarp => vec![(WaveHeight, 0.0)],
            Kind::TurbulentDisplace => vec![(Displacement, 0.0)],
            Kind::Bulge => vec![(Bulge, 0.0)],
            Kind::Twirl => vec![(Twist, 0.0)],
            _ => vec![(Amount, 0.0)],
        };
        for &kind in Kind::ALL {
            assert_eq!(
                run(&input, &configured(kind, &neutral(kind))).data(),
                input.data(),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn color_effects_produce_known_rgb_and_preserve_alpha() {
        let input = square([64, 128, 192]);
        let mut tint = configured(Kind::Tint, &[]);
        tint.edit(BlackColor, 0.0, [0.0; 3]);
        tint.edit(WhiteColor, 0.0, [255.0; 3]);
        let mut gradient = configured(Kind::GradientRamp, &[(Angle, 0.0)]);
        gradient.edit(StartColor, 0.0, [0.0; 3]);
        gradient.edit(EndColor, 0.0, [255.0; 3]);
        let mut radial = gradient.clone();
        radial.radial = true;
        let cases = [
            (
                configured(Kind::BrightnessContrast, &[(Brightness, 20.0)]),
                [115, 179, 243],
            ),
            (
                configured(Kind::HueSaturationLightness, &[(Saturation, -100.0)]),
                [119; 3],
            ),
            (tint, [119; 3]),
            (configured(Kind::Fill, &[]), [237, 121, 94]),
            (
                configured(Kind::Levels, &[(OutputWhite, 128.0)]),
                [32, 64, 96],
            ),
            (configured(Kind::Invert, &[]), [191, 127, 63]),
            (gradient, [129, 129, 129]),
            (radial, [6; 3]),
        ];
        for (effect, expected) in cases {
            let result = run(&input, &effect);
            assert_eq!(
                pixel(&result, 32, 32),
                [expected[0], expected[1], expected[2], 255],
                "{:?}",
                effect.kind
            );
            assert_eq!(pixel(&result, 10, 10), [0; 4]);
        }
        let ramp = configured(Kind::Vignette, &[(Amount, 100.0), (Spread, 0.0)]);
        assert_eq!(pixel(&run(&input, &ramp), 39, 39), [48, 95, 143, 255]);
        let mut noise = configured(
            Kind::Noise,
            &[(Amount, 60.0), (GrainSize, 8.0), (Seed, 0.0)],
        );
        let seeded = run(&input, &noise);
        assert_eq!(pixel(&seeded, 32, 32), [0, 52, 116, 255]);
        assert_eq!(pixel(&seeded, 32, 32), pixel(&seeded, 39, 39));
        noise.edit(Seed, 0.0, [1.0, 0.0, 0.0]);
        assert_ne!(pixel(&seeded, 32, 32), pixel(&run(&input, &noise), 32, 32));
    }

    #[test]
    fn blurs_and_light_read_the_expected_alpha_neighbors() {
        let input = square([255, 0, 0]);
        let gaussian = run(&input, &configured(Kind::GaussianBlur, &[(Radius, 4.0)]));
        assert_eq!(pixel(&gaussian, 21, 32), pixel(&gaussian, 32, 21));
        assert!((50..90).contains(&pixel(&gaussian, 21, 32)[3]));
        let directional = run(&input, &configured(Kind::DirectionalBlur, &[]));
        assert!(pixel(&directional, 19, 32)[3] > 40);
        assert_eq!(pixel(&directional, 32, 21), [0; 4]);
        let zoom = run(&input, &configured(Kind::ZoomBlur, &[(Zoom, 100.0)]));
        assert!(pixel(&zoom, 22, 32)[3] > 40);
        assert_eq!(pixel(&zoom, 32, 32), [255, 0, 0, 255]);

        let shadow = run(
            &input,
            &configured(Kind::DropShadow, &[(Angle, 0.0), (Softness, 0.0)]),
        );
        assert_eq!(pixel(&shadow, 48, 32), [0, 0, 0, 153]);
        assert_eq!(pixel(&shadow, 20, 32), [0; 4]);
        let outer = run(&input, &configured(Kind::OuterGlow, &[(Radius, 4.0)]));
        assert!(pixel(&outer, 21, 32)[1] > 30);
        assert_eq!(pixel(&outer, 32, 32), [255, 0, 0, 255]);
        let inner = run(&input, &configured(Kind::InnerGlow, &[(Radius, 3.0)]));
        assert!(pixel(&inner, 24, 32)[1] > 60);
        assert_eq!(pixel(&inner, 21, 32), [0; 4]);
        assert_eq!(pixel(&inner, 24, 32)[3], 255);
        let glow = configured(Kind::Glow, &[(Radius, 4.0)]);
        assert_eq!(run(&input, &glow).data(), input.data());
        let bright = run(&square([255; 3]), &glow);
        assert!(pixel(&bright, 21, 32)[1] > 40);

        let outline = run(&input, &configured(Kind::Outline, &[]));
        assert_eq!(pixel(&outline, 21, 32), [0, 0, 0, 255]);
        assert_eq!(pixel(&outline, 20, 32), [0; 4]);
    }

    #[test]
    fn stylize_and_distort_sample_known_positions() {
        let input = square([255; 3]);
        let fringe = run(&input, &configured(Kind::ChromaticAberration, &[]));
        assert_eq!(pixel(&fringe, 40, 32), [0, 0, 255, 255]);
        assert_eq!(pixel(&fringe, 23, 32), [255, 0, 0, 255]);
        let blocks = run(&input, &configured(Kind::Pixelate, &[]));
        assert_eq!(pixel(&blocks, 22, 32), [255; 4]);
        assert_eq!(pixel(&blocks, 19, 32), [0; 4]);
        let wave = run(
            &input,
            &configured(
                Kind::WaveWarp,
                &[(WaveWidth, 32.0), (WaveHeight, 10.0), (Speed, 0.0)],
            ),
        );
        assert_eq!(pixel(&wave, 24, 44), [255; 4]);
        assert_eq!(pixel(&wave, 24, 28), [0; 4]);

        let turbulence = configured(
            Kind::TurbulentDisplace,
            &[(TurbulenceSize, 12.0), (Displacement, 30.0)],
        );
        let displaced = run(&input, &turbulence);
        assert_eq!(pixel(&displaced, 25, 32), [255; 4]);
        assert_ne!(displaced.data(), input.data());
        let bulge = run(
            &input,
            &configured(Kind::Bulge, &[(Bulge, 100.0), (Radius, 20.0)]),
        );
        assert_eq!(pixel(&bulge, 22, 32), [255; 4]);
        let twirl = run(&input, &configured(Kind::Twirl, &[(Twist, 45.0)]));
        assert_eq!(pixel(&twirl, 23, 32), [255; 4]);
        assert_eq!(pixel(&twirl, 12, 32), [0; 4]);
    }

    #[test]
    fn cache_reuses_static_inputs_and_invalidates_on_parameters_pixels_and_order() {
        let source = square([64, 128, 192]);
        let mut cache = Cache::default();
        let invert = configured(Kind::Invert, &[]).sample(0.0, "linear");
        let fill = configured(Kind::Fill, &[]).sample(0.0, "linear");
        let paint = |cache: &mut Cache, stack, image: &Pixmap| {
            cache.apply("shape", stack, area(), image.clone())
        };
        let initial = paint(&mut cache, vec![invert.clone(), fill.clone()], &source);
        let saved = cache.layers["shape"].input.data().as_ptr();
        let same = paint(&mut cache, vec![invert.clone(), fill.clone()], &source);
        assert_eq!(same.data(), initial.data());
        assert_eq!(cache.layers["shape"].input.data().as_ptr(), saved);
        let reversed = paint(&mut cache, vec![fill, invert.clone()], &source);
        assert_ne!(reversed.data(), initial.data());

        let brighter = square([96, 128, 192]);
        let a = paint(&mut cache, vec![invert.clone()], &source);
        let b = paint(&mut cache, vec![invert], &brighter);
        assert_eq!(pixel(&a, 32, 32), [191, 127, 63, 255]);
        assert_eq!(pixel(&b, 32, 32), [159, 127, 63, 255]);
        let half = configured(Kind::Invert, &[(Amount, 50.0)]).sample(0.0, "linear");
        assert_eq!(
            pixel(&paint(&mut cache, vec![half], &brighter), 32, 32),
            [128, 128, 128, 255]
        );
    }
}
