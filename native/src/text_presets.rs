use crate::model::hundredths;
use crate::text_animator::{Animator, RangeShape, TextKey, TextLayout, TextProperty, Units};

#[derive(Clone, Copy, Debug)]
pub(crate) enum Preset {
    Typewriter,
    FadeUp,
    Pop,
    Wave,
    Slide,
    Tracking,
    Blur,
    Bounce,
}

impl Preset {
    pub const ALL: [Self; 8] = [
        Self::Typewriter,
        Self::FadeUp,
        Self::Pop,
        Self::Wave,
        Self::Slide,
        Self::Tracking,
        Self::Blur,
        Self::Bounce,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Typewriter => "Typewriter",
            Self::FadeUp => "Fade up by word",
            Self::Pop => "Pop letters",
            Self::Wave => "Wave",
            Self::Slide => "Slide in by line",
            Self::Tracking => "Tracking in",
            Self::Blur => "Blur in",
            Self::Bounce => "Scramble-free bounce",
        }
    }

    pub fn length(self) -> f64 {
        match self {
            Self::Wave | Self::Bounce => 1.6,
            Self::Typewriter | Self::FadeUp | Self::Pop | Self::Slide => 1.4,
            Self::Tracking | Self::Blur => 1.0,
        }
    }

    pub fn apply(self, text: &mut TextLayout, start: f64, end: f64, em: f32) -> Option<u64> {
        use TextProperty::*;
        let mut primary = Animator {
            name: self.label().into(),
            ..Animator::default()
        };
        let mut companions = Vec::new();
        let set = |a: &mut Animator, property, x, y| a.enable(property, [x, y, 0.0]);
        let times = |a, b| {
            (
                hundredths(start + (end - start) * a),
                hundredths(start + (end - start) * b),
            )
        };
        match self {
            Self::Typewriter => {
                set(&mut primary, Opacity, 0.0, 0.0);
                keys(&mut primary, Offset, times(0.0, 1.0), 0.0, 100.0, "linear");
            }
            Self::FadeUp => {
                primary.units = Units::Words;
                reveal(&mut primary, times(0.0, 1.0), 38.0);
                set(&mut primary, Position, 0.0, em * 0.8);
                set(&mut primary, Opacity, 0.0, 0.0);
            }
            Self::Pop => {
                primary.units = Units::VisibleCharacters;
                reveal(&mut primary, times(0.0, 0.8), 24.0);
                set(&mut primary, Scale, 5.0, 5.0);
                set(&mut primary, Opacity, 0.0, 0.0);
                let mut overshoot = Animator {
                    name: "Pop overshoot".into(),
                    ..Animator::default()
                };
                sweep(&mut overshoot, times(0.14, 1.0), 36.0);
                set(&mut overshoot, Scale, 150.0, 150.0);
                companions.push(overshoot);
            }
            Self::Wave => {
                sweep(&mut primary, times(0.0, 1.0), 60.0);
                set(&mut primary, Position, 0.0, -em * 0.8);
                set(&mut primary, Rotation, -15.0, 0.0);
            }
            Self::Slide => {
                primary.units = Units::Lines;
                reveal(&mut primary, times(0.0, 1.0), 43.0);
                set(&mut primary, Position, -em * 2.8, 0.0);
                set(&mut primary, Opacity, 20.0, 0.0);
            }
            Self::Tracking => {
                primary.units = Units::VisibleCharacters;
                keys(
                    &mut primary,
                    Tracking,
                    times(0.0, 1.0),
                    em * 0.45,
                    0.0,
                    "ease-out",
                );
            }
            Self::Blur => {
                keys(
                    &mut primary,
                    Blur,
                    times(0.0, 1.0),
                    (em * 0.18).clamp(2.0, 24.0),
                    0.0,
                    "ease-out",
                );
            }
            Self::Bounce => {
                sweep(&mut primary, times(0.0, 0.78), 33.0);
                set(&mut primary, Position, 0.0, -em);
                let mut rebound = Animator {
                    name: "Landing rebound".into(),
                    ..Animator::default()
                };
                sweep(&mut rebound, times(0.22, 1.0), 28.0);
                set(&mut rebound, Position, 0.0, em * 0.34);
                companions.push(rebound);
            }
        }
        if text.animators.len() + companions.len() >= 64 {
            return None;
        }
        let id = text.add(primary)?;
        for animator in companions {
            text.add(animator);
        }
        Some(id)
    }
}

fn reveal(animator: &mut Animator, times: (f64, f64), width: f32) {
    animator.range_shape = RangeShape::RampUp;
    animator.enable(TextProperty::End, [width, 0.0, 0.0]);
    keys(
        animator,
        TextProperty::Offset,
        times,
        -width,
        100.0,
        "linear",
    );
}

fn sweep(animator: &mut Animator, times: (f64, f64), width: f32) {
    animator.units = Units::VisibleCharacters;
    animator.range_shape = RangeShape::Smooth;
    animator.enable(TextProperty::End, [width, 0.0, 0.0]);
    keys(
        animator,
        TextProperty::Offset,
        times,
        -width,
        100.0,
        "linear",
    );
}

fn keys(
    animator: &mut Animator,
    property: TextProperty,
    (start, end): (f64, f64),
    from: f32,
    to: f32,
    ease: &str,
) {
    animator.enable(property, [from, 0.0, 0.0]);
    if let Some(track) = animator.tracks.get_mut(&property) {
        let mut first = TextKey::new(start, [from, 0.0, 0.0]);
        first.ease = Some(ease.into());
        track.put(start, first, property);
        track.put(end, TextKey::new(end, [to, 0.0, 0.0]), property);
    }
}
