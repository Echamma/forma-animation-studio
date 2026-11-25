use crate::mask_render::LayerPixels;
use crate::spatial::{Camera, Plane};
use rayon::prelude::*;
use tiny_skia::{IntRect, Pixmap, Transform};

const NEAR: f32 = 0.5;
type Point = [f32; 3];

struct Mapping {
    inverse: [Point; 3],
}

fn cross(a: Point, b: Point) -> Point {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: Point, b: Point) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

impl Mapping {
    fn new([a, b, d]: [Point; 3]) -> Option<Self> {
        let u = std::array::from_fn(|i| b[i] - a[i]);
        let v = std::array::from_fn(|i| d[i] - a[i]);
        let inverse = [cross(v, a), cross(a, u), cross(u, v)];
        let det = dot(u, inverse[0]);
        if !det.is_finite() || det.abs() < 1e-7 {
            return None;
        }
        let inverse = inverse.map(|row| row.map(|v| v / det));
        Some(Self { inverse })
    }

    fn at(&self, x: f32, y: f32) -> Point {
        self.inverse.map(|[dx, dy, base]| dx * x + dy * y + base)
    }

    #[cfg(test)]
    fn sample(&self, x: f32, y: f32) -> Option<[f32; 2]> {
        uv(self.at(x, y))
    }
}

fn uv([x, y, w]: Point) -> Option<[f32; 2]> {
    if !(w > 0.0 && w <= NEAR.recip()) {
        return None;
    }
    let depth = w.recip();
    let (u, v) = (x * depth, y * depth);
    (u >= 0.0 && u <= 1.0 && v >= 0.0 && v <= 1.0).then_some([u, v])
}

fn bounds(corners: [Point; 4], size: (u32, u32), margin: f32) -> Option<IntRect> {
    let mut min = [f32::INFINITY; 2];
    let mut max = [f32::NEG_INFINITY; 2];
    let mut include = |p: Point| {
        for i in 0..2 {
            let v = p[i] / p[2];
            min[i] = min[i].min(v);
            max[i] = max[i].max(v);
        }
    };
    for i in 0..4 {
        let (a, b) = (corners[i], corners[(i + 1) % 4]);
        let (a_visible, b_visible) = (a[2] >= NEAR, b[2] >= NEAR);
        if a_visible {
            include(a);
        }
        if a_visible != b_visible {
            let t = (NEAR - a[2]) / (b[2] - a[2]);
            let [x, y, _] = std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t);
            include([x, y, NEAR]);
        }
    }
    let limit = [size.0 as f32, size.1 as f32];
    for i in 0..2 {
        min[i] = min[i].floor().max(-margin) - margin;
        max[i] = max[i].ceil().min(limit[i] + margin) + margin;
    }
    IntRect::from_ltrb(min[0] as i32, min[1] as i32, max[0] as i32, max[1] as i32)
}

pub(crate) fn warp(
    source: &LayerPixels,
    plane: Plane<'_>,
    camera: Camera,
    to_scene: Transform,
    view: Transform,
    size: (u32, u32),
    blur: f32,
) -> Option<LayerPixels> {
    let area = source.area;
    let project = |(x, y): (i32, i32)| {
        let (x, y) = crate::transform::point(to_scene, x as f32, y as f32);
        let [x, y, z] = camera.homogeneous(plane.world([x, y, 0.0]));
        [
            view.sx * x + view.kx * y + view.tx * z,
            view.ky * x + view.sy * y + view.ty * z,
            z,
        ]
    };
    let [a, b, c, d] = [
        (area.left(), area.top()),
        (area.right(), area.top()),
        (area.right(), area.bottom()),
        (area.left(), area.bottom()),
    ]
    .map(project);
    let mapping = Mapping::new([a, b, d])?;
    let area = bounds([a, b, c, d], size, (blur * 3.0).ceil() + 1.0)?;
    if u64::from(area.width()) * u64::from(area.height()) > 24 * 1024 * 1024 {
        return None;
    }
    let mut pixmap = Pixmap::new(area.width(), area.height())?;
    let width = area.width() as usize;
    let sample = Sampler::new(&source.pixmap);
    let [dx, dy, dw] = mapping.inverse.map(|row| row[0]);
    let draw = |(block, bytes): (usize, &mut [u8])| {
        for (row, bytes) in bytes.chunks_exact_mut(width * 4).enumerate() {
            let y = area.top() as f32 + (block * 16 + row) as f32 + 0.5;
            let [x, y, w] = mapping.at(area.left() as f32 + 0.5, y);
            for (column, pixel) in bytes.chunks_exact_mut(4).enumerate() {
                let i = column as f32;
                if let Some([u, v]) = uv([x + dx * i, y + dy * i, w + dw * i]) {
                    sample.bilinear(u, v, pixel);
                }
            }
        }
    };
    let target = pixmap.data_mut();
    let chunk = width * 4 * 16;
    if target.len() >= 1024 * 1024 {
        target.par_chunks_mut(chunk).enumerate().for_each(draw);
    } else {
        target.chunks_mut(chunk).enumerate().for_each(draw);
    }
    if blur > 0.0 {
        crate::render::gaussian_blur(&mut pixmap, blur);
    }
    Some(LayerPixels { area, pixmap })
}

struct Sampler<'a> {
    data: &'a [u8],
    stride: usize,
    width: usize,
    height: usize,
}

impl<'a> Sampler<'a> {
    fn new(pixmap: &'a Pixmap) -> Self {
        let width = pixmap.width() as usize;
        Self {
            data: pixmap.data(),
            stride: width * 4,
            width,
            height: pixmap.height() as usize,
        }
    }

    fn bilinear(&self, u: f32, v: f32, target: &mut [u8]) {
        let x = (u * self.width as f32 - 0.5)
            .max(0.0)
            .min((self.width - 1) as f32);
        let y = (v * self.height as f32 - 0.5)
            .max(0.0)
            .min((self.height - 1) as f32);
        let (x0, y0) = (x as usize, y as usize);
        let (x1, y1) = ((x0 + 1).min(self.width - 1), (y0 + 1).min(self.height - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let (w0, w1, w2, w3) = (
            (1.0 - fx) * (1.0 - fy),
            fx * (1.0 - fy),
            (1.0 - fx) * fy,
            fx * fy,
        );
        let (a, b, c, d) = (
            y0 * self.stride + x0 * 4,
            y0 * self.stride + x1 * 4,
            y1 * self.stride + x0 * 4,
            y1 * self.stride + x1 * 4,
        );
        for channel in 0..4 {
            target[channel] = (f32::from(self.data[a + channel]) * w0
                + f32::from(self.data[b + channel]) * w1
                + f32::from(self.data[c + channel]) * w2
                + f32::from(self.data[d + channel]) * w3)
                .round() as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn homogeneous_warp_maps_quad_corners_and_uses_projective_midpoint() {
        let corners = [
            [10.0, 10.0, 1.0],
            [80.0, 30.0, 2.0],
            [80.0, 90.0, 2.0],
            [10.0, 70.0, 1.0],
        ];
        let mapping = Mapping::new([corners[0], corners[1], corners[3]]).unwrap();
        for (u, v) in [
            (0.001, 0.001),
            (0.999, 0.001),
            (0.999, 0.999),
            (0.001, 0.999),
            (0.5, 0.5),
        ] {
            let x = (10.0 + 70.0 * u) / (1.0 + u);
            let y = (10.0 + 20.0 * u + 60.0 * v) / (1.0 + u);
            let [sx, sy] = mapping.sample(x, y).unwrap();
            assert!((sx - u).abs() < 1e-4 && (sy - v).abs() < 1e-4);
        }
        assert!(mapping.sample(20.0, 4.0).is_none());
    }

    #[test]
    fn near_plane_clipping_keeps_the_visible_part() {
        let corners = [
            [0.0, 0.0, 0.0],
            [60.0, 0.0, 2.0],
            [60.0, 60.0, 2.0],
            [0.0, 60.0, 0.0],
        ];
        let area = bounds(corners, (100, 100), 1.0).unwrap();
        assert_eq!(area.left(), 29);
        assert_eq!(area.right(), 31);
        assert!(bounds(corners.map(|[x, y, z]| [x, y, z - 3.0]), (100, 100), 1.0).is_none());
    }
}
