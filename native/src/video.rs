//! Exports scenes to MP4 through FFmpeg, with frames rendered in Rust, as Electron Forma's `src/export/video.js` does:
//! every frame painted at its exact time, video layers' frames read from their files, and their sound mixed in.

use crate::media::{self, Info};
use crate::model::Scene;
use crate::render::{self, Renderer};
use canopy_ui::Place;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
    mpsc::{self, Receiver},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Mp4,
    WebM,
}

/// What Electron Forma's export dialog lets you choose for a video.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub format: Format,
    pub fps: u32,
    /// 1 is the scene's size, 2 twice it.
    pub scale: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            format: Format::Mp4,
            fps: 30,
            scale: 1,
        }
    }
}

impl Settings {
    /// The scene's size times `scale`, in even numbers (H.264 and VP9 need them), as Electron rounds it.
    pub fn size(&self, scene: &Scene) -> (u32, u32) {
        let even = |side: f32| (((side * self.scale as f32) / 2.0).round() as u32 * 2).max(2);
        (even(scene.width), even(scene.height))
    }

    pub fn frames(&self, scene: &Scene) -> u32 {
        ((scene.duration * self.fps as f64).round() as u32).max(1)
    }

    pub fn extension(&self) -> &'static str {
        match self.format {
            Format::Mp4 => "mp4",
            Format::WebM => "webm",
        }
    }
}

pub struct Export {
    pub progress: Arc<AtomicU32>,
    pub cancel: Arc<AtomicBool>,
    pub result: Receiver<Result<PathBuf, String>>,
}

pub fn start(
    scene: &Scene,
    place: &Place,
    output: PathBuf,
    settings: Settings,
    ctx: egui::Context,
) -> Result<Export, String> {
    let renderer = Renderer::new(scene, place);
    renderer.check(scene)?;
    let scene = scene.clone();
    let progress = Arc::new(AtomicU32::new(0));
    let cancel = Arc::new(AtomicBool::new(false));
    let (send, result) = mpsc::channel();
    let (progress_out, cancel_in) = (progress.clone(), cancel.clone());
    std::thread::spawn(move || {
        let outcome = render_video(
            &scene,
            &renderer,
            &output,
            settings,
            &progress_out,
            &cancel_in,
        );
        if outcome.is_err() {
            let _ = std::fs::remove_file(&output);
        }
        let _ = send.send(outcome.map(|_| output));
        ctx.request_repaint();
    });
    Ok(Export {
        progress,
        cancel,
        result,
    })
}

/// A video layer's sound: Electron's `mixSoundtrack` plays it, looping from `mediaStart`, while its clip is on screen.
struct Sound<'a> {
    file: &'a Path,
    start: f64,
    end: f64,
    offset: f64,
}

/// FFmpeg's arguments: frames come in raw on stdin; each sound is its own input, delayed to its clip and mixed.
fn arguments(scene: &Scene, sounds: &[Sound], settings: Settings, output: &Path) -> Vec<String> {
    let (width, height) = settings.size(scene);
    let mut args: Vec<String> = [
        "-v",
        "error",
        "-y",
        "-f",
        "rawvideo",
        "-pix_fmt",
        "rgba",
        "-s",
        &format!("{width}x{height}"),
        "-r",
        &settings.fps.to_string(),
        "-i",
        "pipe:0",
    ]
    .map(String::from)
    .into();
    let length = settings.frames(scene) as f64 / settings.fps as f64;
    let mut filters = Vec::new();
    for (index, sound) in sounds.iter().enumerate() {
        args.extend(
            [
                "-stream_loop",
                "-1",
                "-ss",
                &format!("{:.3}", sound.offset),
                "-t",
                &format!("{:.3}", sound.end - sound.start),
                "-i",
            ]
            .map(String::from),
        );
        args.push(sound.file.to_string_lossy().into_owned());
        let delay = (sound.start * 1000.0).round() as u64;
        filters.push(format!("[{}:a]aformat=sample_fmts=fltp:sample_rates=48000:channel_layouts=stereo,adelay={delay}:all=1[s{index}]", index + 1));
    }
    args.extend(["-map", "0:v"].map(String::from));
    if !sounds.is_empty() {
        // Sounds add up, as in Electron's OfflineAudioContext; the mix lasts exactly as long as the video.
        let inputs: String = (0..sounds.len()).map(|i| format!("[s{i}]")).collect();
        let mix = if sounds.len() > 1 {
            format!(
                "{inputs}amix=inputs={}:normalize=0:duration=longest",
                sounds.len()
            )
        } else {
            format!("{inputs}anull")
        };
        filters.push(format!("{mix},apad,atrim=0:{length:.3}[sound]"));
        args.extend([
            "-filter_complex".into(),
            filters.join(";"),
            "-map".into(),
            "[sound]".into(),
        ]);
        args.extend(
            match settings.format {
                Format::Mp4 => ["-c:a", "aac", "-b:a", "192k"],
                Format::WebM => ["-c:a", "libopus", "-b:a", "160k"],
            }
            .map(String::from),
        );
    }
    let video: &[&str] = match settings.format {
        Format::Mp4 => &[
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-crf",
            "20",
            "-pix_fmt",
            "yuv420p",
            "-movflags",
            "+faststart",
        ],
        Format::WebM => &[
            "-c:v",
            "libvpx-vp9",
            "-b:v",
            "0",
            "-crf",
            "32",
            "-deadline",
            "good",
            "-cpu-used",
            "4",
            "-row-mt",
            "1",
            "-pix_fmt",
            "yuv420p",
        ],
    };
    args.extend(video.iter().map(|s| s.to_string()));
    args.extend(["-t".to_string(), format!("{length:.3}")]);
    args.push(output.to_string_lossy().into_owned());
    args
}

fn render_video(
    scene: &Scene,
    renderer: &Renderer,
    output: &Path,
    settings: Settings,
    progress: &AtomicU32,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let (frames, size) = (settings.frames(scene), settings.size(scene));
    let mut videos: HashMap<String, (&Path, Info)> = HashMap::new();
    for shape in scene.shapes.iter().filter(|s| s.kind == "video") {
        let file = renderer
            .video(&shape.id)
            .ok_or_else(|| format!("{}: the video file is missing", shape.name))?;
        videos.insert(shape.id.clone(), (file, media::probe(file)?));
    }
    let sounds: Vec<Sound> = scene
        .shapes
        .iter()
        .filter_map(|shape| {
            let (file, info) = videos.get(&shape.id).filter(|(_, info)| info.sound)?;
            let (start, end) = (
                shape.in_time.unwrap_or(0.0),
                shape.out_time.unwrap_or(scene.duration).min(scene.duration),
            );
            (end > start).then(|| Sound {
                file,
                start,
                end,
                offset: shape.media_time(start, info.duration),
            })
        })
        .collect();
    let mut child = media::command(media::ffmpeg())
        .args(arguments(scene, &sounds, settings, output))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("FFmpeg couldn't start ({e}). Install FFmpeg to export MP4."))?;
    let mut stderr = child.stderr.take().unwrap();
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });
    // Each video layer on screen reads its frames from one FFmpeg run that started where its clip comes in.
    let mut readers: HashMap<String, media::Frames> = HashMap::new();
    let mut frame_at = |t: f64| -> Result<tiny_skia::Pixmap, String> {
        let showing: Vec<_> = scene
            .shapes
            .iter()
            .filter(|s| s.kind == "video" && s.visible(t, scene.duration))
            .collect();
        readers.retain(|id, _| showing.iter().any(|s| &s.id == id));
        let mut current = render::Frames::new();
        for shape in showing {
            let (file, info) = videos[&shape.id];
            if !readers.contains_key(&shape.id) {
                let (width, height) = render::video_size(shape, settings.scale as f32);
                readers.insert(
                    shape.id.clone(),
                    media::Frames::start(
                        file,
                        shape.media_time(t, info.duration),
                        settings.fps,
                        width,
                        height,
                    )?,
                );
            }
            current.insert(
                shape.id.clone(),
                readers.get_mut(&shape.id).unwrap().next()?,
            );
        }
        renderer.frame(scene, t, size, &current)
    };
    let mut failure = None;
    if let Some(mut stdin) = child.stdin.take() {
        for index in 0..frames {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            match frame_at(index as f64 / settings.fps as f64) {
                Ok(frame) => {
                    if let Err(error) = stdin.write_all(frame.data()) {
                        failure = Some(error.to_string());
                        break;
                    }
                }
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            }
            progress.store(
                ((index + 1) as f64 / frames as f64 * 1000.0) as u32,
                Ordering::Relaxed,
            );
        }
    }
    if failure.is_some() || cancel.load(Ordering::Relaxed) {
        let _ = child.kill();
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    let errors = errors.join().unwrap_or_default();
    if cancel.load(Ordering::Relaxed) {
        return Err("Export stopped".into());
    }
    if let Some(failure) = failure {
        // A broken pipe means FFmpeg stopped first: its own message says why.
        return Err(errors
            .lines()
            .last()
            .map_or(failure, |line| format!("FFmpeg failed: {line}")));
    }
    if !status.success() {
        return Err(format!(
            "FFmpeg failed: {}",
            errors.lines().last().unwrap_or("no details")
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(root: &Path) -> Place {
        Place {
            space: None,
            projects_dir: root.into(),
            data_dir: root.into(),
            exports_dir: root.into(),
        }
    }

    #[test]
    fn renders_a_real_short_mp4() {
        let root = std::env::temp_dir().join(format!("forma-native-video-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let mut scene = Scene::empty("test".into());
        scene.width = 64.0;
        scene.height = 64.0;
        scene.duration = 1.0;
        scene.add_shape("a".into(), "circle");
        let shape = &mut scene.shapes[0];
        let pose = crate::model::Pose {
            x: 32.0,
            y: 32.0,
            w: 30.0,
            h: 30.0,
            ..shape.pose(0.0)
        };
        shape.edit_pose(0.0, pose);
        let output = root.join("out.mp4");
        let run = start(
            &scene,
            &place(&root),
            output.clone(),
            Settings::default(),
            egui::Context::default(),
        )
        .unwrap();
        assert_eq!(run.result.recv().unwrap(), Ok(output.clone()));
        let info = media::probe(&output).unwrap();
        assert!(!info.sound, "nothing in the scene makes sound");
        assert!((info.duration - 1.0).abs() < 0.05, "{info:?}");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn video_layers_play_their_frames_and_sound_from_where_their_clip_starts() {
        let root =
            std::env::temp_dir().join(format!("forma-native-video-layer-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let place = place(&root);
        // Red for a second, then lime, with a tone. The layer comes in at 0.5 s and starts 0.25 s into the video.
        let source = media::tests::video(&root, "source.mp4", 2, true);
        let (url, poster, info) = crate::assets::import_video(&source, &place).unwrap();
        let mut scene = Scene::empty("test".into());
        (scene.width, scene.height, scene.duration) = (64.0, 48.0, 2.5);
        scene.add_shape("v".into(), "video");
        let layer = &mut scene.shapes[0];
        for (field, value) in [
            ("assetData", url.into()),
            ("posterData", poster.into()),
            ("mediaDuration", info.duration.into()),
            ("mediaStart", 0.25.into()),
        ] {
            layer.extra.insert(field.into(), value);
        }
        layer.in_time = Some(0.5);
        let pose = crate::model::Pose {
            x: 32.0,
            y: 24.0,
            w: 64.0,
            h: 48.0,
            ..layer.pose(0.0)
        };
        layer.edit_pose(0.0, pose);
        let output = root.join("out.mp4");
        let run = start(
            &scene,
            &place,
            output.clone(),
            Settings::default(),
            egui::Context::default(),
        )
        .unwrap();
        assert_eq!(run.result.recv().unwrap(), Ok(output.clone()));
        let exported = media::probe(&output).unwrap();
        assert!(
            exported.sound && (exported.duration - 2.5).abs() < 0.1,
            "{exported:?}"
        );
        let color = |t: f64| {
            let frame = media::Frames::start(&output, t, 30, 8, 6)
                .unwrap()
                .next()
                .unwrap();
            let p = frame.pixel(4, 3).unwrap();
            [p.red(), p.green(), p.blue()]
        };
        assert!(
            color(0.2)[2] > 200,
            "the background before the clip: {:?}",
            color(0.2)
        );
        // At 1.0 s the video is at 0.75 s (red); at 1.5 s it's at 1.25 s (lime).
        assert!(
            color(1.0)[0] > 200 && color(1.0)[1] < 60,
            "red: {:?}",
            color(1.0)
        );
        assert!(
            color(1.5)[1] > 200 && color(1.5)[0] < 60,
            "lime: {:?}",
            color(1.5)
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sounds_are_delayed_to_their_clip_and_summed() {
        let scene = Scene {
            duration: 3.0,
            ..Scene::empty("test".into())
        };
        let sounds = [
            Sound {
                file: Path::new("a.mp4"),
                start: 0.5,
                end: 3.0,
                offset: 1.25,
            },
            Sound {
                file: Path::new("b.mp4"),
                start: 0.0,
                end: 1.0,
                offset: 0.0,
            },
        ];
        let args = arguments(&scene, &sounds, Settings::default(), Path::new("out.mp4")).join(" ");
        assert!(
            args.contains("-stream_loop -1 -ss 1.250 -t 2.500 -i a.mp4"),
            "{args}"
        );
        assert!(
            args.contains("adelay=500:all=1[s0]") && args.contains("adelay=0:all=1[s1]"),
            "{args}"
        );
        assert!(
            args.contains(
                "[s0][s1]amix=inputs=2:normalize=0:duration=longest,apad,atrim=0:3.000[sound]"
            ),
            "{args}"
        );
    }

    #[test]
    fn large_webm_at_sixty_frames_a_second() {
        let root = std::env::temp_dir().join(format!("forma-native-webm-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let mut scene = Scene::empty("test".into());
        (scene.width, scene.height, scene.duration) = (101.0, 61.0, 1.0);
        scene.add_shape("a".into(), "circle");
        let settings = Settings {
            format: Format::WebM,
            fps: 60,
            scale: 2,
        };
        assert_eq!(settings.size(&scene), (202, 122));
        let output = root.join("out.webm");
        let run = start(
            &scene,
            &place(&root),
            output.clone(),
            settings,
            egui::Context::default(),
        )
        .unwrap();
        assert_eq!(run.result.recv().unwrap(), Ok(output.clone()));
        let info = media::probe(&output).unwrap();
        assert_eq!((info.width, info.height), (202, 122));
        let frames = media::command(media::ffprobe())
            .args([
                "-v",
                "error",
                "-count_frames",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=nb_read_frames",
                "-of",
                "csv=p=0",
            ])
            .arg(&output)
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&frames.stdout).trim(), "60");
        std::fs::remove_dir_all(root).unwrap();
    }
}
