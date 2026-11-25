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
    Mov,
    PngSequence,
    Gif,
}

/// What Electron Forma's export dialog lets you choose for a video.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub format: Format,
    pub fps: u32,
    /// 1 is the scene's size, 2 twice it.
    pub scale: u32,
    pub transparent: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            format: Format::Mp4,
            fps: 30,
            scale: 1,
            transparent: false,
        }
    }
}

impl Settings {
    /// The scene's size times `scale`, in even numbers (H.264 and VP9 need them), as Electron rounds it.
    pub fn size(&self, scene: &Scene) -> (u32, u32) {
        let even = |side: f32| (((side * self.scale as f32) / 2.0).round() as u32 * 2).max(2);
        let whole = |side: f32| (side * self.scale as f32).round().max(1.0) as u32;
        if matches!(self.format, Format::PngSequence | Format::Gif) {
            return (whole(scene.width), whole(scene.height));
        }
        (even(scene.width), even(scene.height))
    }

    pub fn frames(&self, scene: &Scene) -> u32 {
        ((scene.duration * self.fps as f64).round() as u32).max(1)
    }

    pub fn extension(&self) -> &'static str {
        match self.format {
            Format::Mp4 => "mp4",
            Format::WebM => "webm",
            Format::Mov => "mov",
            Format::PngSequence => "zip",
            Format::Gif => "gif",
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
    if !matches!(settings.fps, 1..=120) || !matches!(settings.scale, 1..=2) {
        return Err("Choose an export frame rate from 1 to 120 and a size of 1× or 2×.".into());
    }
    if settings.transparent && settings.format == Format::Mp4 {
        return Err("Choose WebM, MOV, PNG sequence or GIF for a transparent background.".into());
    }
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
    speed: f64,
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
                &format!("{:.3}", (sound.end - sound.start) * sound.speed),
                "-i",
            ]
            .map(String::from),
        );
        args.push(sound.file.to_string_lossy().into_owned());
        let delay = (sound.start * 1000.0).round() as u64;
        let tempo = crate::composition_audio::tempo_filter(sound.speed);
        filters.push(format!("[{}:a]aformat=sample_fmts=fltp:sample_rates=48000:channel_layouts=stereo,{tempo}adelay={delay}:all=1[s{index}]", index + 1));
    }
    if settings.format == Format::Gif {
        args.extend(["-filter_complex_threads", "1", "-filter_complex",
            "[0:v]split[a][b];[a]palettegen=reserve_transparent=1:stats_mode=single[p];[b][p]paletteuse=new=1:alpha_threshold=128[v]",
            "-map", "[v]"].map(String::from));
    } else {
        args.extend(["-map", "0:v"].map(String::from));
    }
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
        let audio = if settings.format == Format::WebM {
            ["-c:a", "libopus", "-b:a", "160k"]
        } else {
            ["-c:a", "aac", "-b:a", "192k"]
        };
        args.extend(audio.map(String::from));
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
            if settings.transparent || scene.transparent_background {
                "yuva420p"
            } else {
                "yuv420p"
            },
        ],
        Format::Mov => &[
            "-c:v",
            "prores_ks",
            "-profile:v",
            "4",
            "-pix_fmt",
            "yuva444p10le",
            "-alpha_bits",
            "16",
        ],
        Format::Gif => &["-loop", "0"],
        Format::PngSequence => &[],
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
    let mut videos: HashMap<String, (&Path, Info, (u32, u32))> = HashMap::new();
    for shape in scene.shapes.iter().filter(|s| s.kind == "video") {
        let file = renderer
            .video(&shape.id)
            .ok_or_else(|| format!("{}: the video file is missing", shape.name))?;
        let info = media::probe(file)?;
        let times = (0..frames)
            .take_while(|_| !cancel.load(Ordering::Relaxed))
            .flat_map(|index| {
                let time = index as f64 / f64::from(settings.fps);
                let shutter = if shape.motion_blur {
                    crate::motion_blur::exposure(scene, time, settings.fps)
                } else {
                    Vec::new()
                };
                std::iter::once(time).chain(shutter)
            });
        let decode_size =
            render::limit_decode_size(render::video_size(scene, shape, size, times), info);
        if cancel.load(Ordering::Relaxed) {
            return Err("Export stopped".into());
        }
        videos.insert(shape.id.clone(), (file, info, decode_size));
    }
    let mut sounds = Vec::new();
    if !matches!(settings.format, Format::PngSequence | Format::Gif) {
        let mut known: HashMap<&Path, Info> = videos
            .values()
            .map(|&(file, info, _)| (file, info))
            .collect();
        for clip in crate::composition_audio::video_spans(scene, renderer)? {
            let info = match known.get(clip.file) {
                Some(info) => *info,
                None => {
                    let info = media::probe(clip.file)?;
                    known.insert(clip.file, info);
                    info
                }
            };
            if info.sound {
                sounds.push(Sound {
                    file: clip.file,
                    start: clip.start,
                    end: clip.end,
                    offset: clip.offset.rem_euclid(info.duration),
                    speed: clip.speed,
                });
            }
        }
    }
    // Each video layer on screen reads its frames from one FFmpeg run that started where its clip comes in.
    let mut readers: HashMap<String, media::Frames> = HashMap::new();
    let mut shutter_readers: HashMap<String, Option<media::ShutterReader>> = HashMap::new();
    let mut frame_at = |t: f64| -> Result<tiny_skia::Pixmap, String> {
        let shutter = crate::motion_blur::exposure(scene, t, settings.fps);
        let sampled = !shutter.is_empty();
        if let Some(&first) = shutter.first() {
            shutter_readers.retain(|id, _| {
                scene.shapes.iter().any(|shape| {
                    &shape.id == id && first <= shape.out_time.unwrap_or(scene.duration)
                })
            });
        }
        let showing: Vec<_> = scene
            .shapes
            .iter()
            .filter(|s| {
                s.kind == "video" && s.visible(t, scene.duration) && !(sampled && s.motion_blur)
            })
            .collect();
        readers.retain(|id, _| showing.iter().any(|s| &s.id == id));
        let mut current = render::Frames::new();
        for shape in showing {
            let (file, info, (width, height)) = videos[&shape.id];
            if !readers.contains_key(&shape.id) {
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
        renderer.export_frame(
            scene,
            t,
            size,
            settings.fps,
            &current,
            |sample| {
                if cancel.load(Ordering::Relaxed) {
                    return Err("Export stopped".into());
                }
                let mut pixels = render::SampleFrames::new();
                for shape in scene.shapes.iter().filter(|shape| {
                    shape.kind == "video"
                        && shape.motion_blur
                        && shape.visible(sample, scene.duration)
                }) {
                    let (file, info, size) = videos[&shape.id];
                    let reader = shutter_readers.entry(shape.id.clone()).or_default();
                    let frame = media::ShutterReader::at(
                        reader,
                        (file, info.duration, size),
                        shape,
                        sample,
                        settings.fps,
                    )?;
                    pixels.insert(shape.id.clone(), frame);
                }
                Ok(Some(pixels))
            },
            settings.transparent,
        )
    };
    if settings.format == Format::PngSequence {
        let file = std::fs::File::create(output).map_err(|e| e.to_string())?;
        let mut zip = zip::ZipWriter::new(file);
        for index in 0..frames {
            if cancel.load(Ordering::Relaxed) {
                return Err("Export stopped".into());
            }
            let frame = frame_at(index as f64 / f64::from(settings.fps))?;
            zip.start_file(
                format!("frame-{:010}.png", index + 1),
                zip::write::SimpleFileOptions::default(),
            )
            .map_err(|e| e.to_string())?;
            zip.write_all(&frame.encode_png().map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            progress.store(
                ((u64::from(index) + 1) * 1000 / u64::from(frames)) as u32,
                Ordering::Relaxed,
            );
        }
        zip.finish().map_err(|e| e.to_string())?;
        return Ok(());
    }
    let mut child = media::command(media::ffmpeg())
        .args(arguments(scene, &sounds, settings, output))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("FFmpeg couldn't start ({e}). Install FFmpeg to export video."))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or("FFmpeg error output is unavailable")?;
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });
    let mut failure = None;
    if let Some(mut stdin) = child.stdin.take() {
        for index in 0..frames {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            match frame_at(index as f64 / settings.fps as f64) {
                Ok(frame) => {
                    let result = if settings.transparent || scene.transparent_background {
                        let mut rgba = Vec::with_capacity(frame.data().len());
                        for pixel in frame.pixels() {
                            let pixel = pixel.demultiply();
                            rgba.extend([pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()]);
                        }
                        stdin.write_all(&rgba)
                    } else {
                        stdin.write_all(frame.data())
                    };
                    if let Err(error) = result {
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
    fn alpha_survives_webm_mov_png_sequence_and_gif() {
        let root = std::env::temp_dir().join(format!("forma-alpha-export-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let mut scene = Scene::empty("test".into());
        (scene.width, scene.height, scene.duration) = (64.0, 48.0, 1.0);
        scene.add_shape("box".into(), "rect");
        scene.shapes[0].color = "#ec5824".into();
        scene.shapes[0].keys[0].w = 20.0;
        scene.shapes[0].keys[0].h = 20.0;
        for format in [Format::WebM, Format::Mov, Format::PngSequence, Format::Gif] {
            let settings = Settings {
                format,
                fps: 4,
                transparent: true,
                ..Settings::default()
            };
            let path = root.join(format!("alpha.{}", settings.extension()));
            let export = start(
                &scene,
                &place(&root),
                path.clone(),
                settings,
                egui::Context::default(),
            )
            .unwrap();
            assert_eq!(
                export.result.recv().unwrap(),
                Ok(path.clone()),
                "{format:?}"
            );
            let input = if format == Format::PngSequence {
                let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
                assert_eq!(zip.len(), 4);
                let mut entry = zip.by_name("frame-0000000001.png").unwrap();
                let png = root.join("first.png");
                std::io::copy(&mut entry, &mut std::fs::File::create(&png).unwrap()).unwrap();
                png
            } else {
                path
            };
            let probe = media::command(media::ffprobe())
                .args([
                    "-v",
                    "error",
                    "-select_streams",
                    "v:0",
                    "-show_entries",
                    "stream=pix_fmt:stream_tags=alpha_mode",
                    "-of",
                    "json",
                ])
                .arg(&input)
                .output()
                .unwrap();
            assert!(probe.status.success(), "{:?}", probe.stderr);
            let info: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
            let stream = &info["streams"][0];
            match format {
                // FFprobe uses its default VP9 decoder, which only reports the color plane.
                Format::WebM => {
                    assert!(matches!(
                        stream["pix_fmt"].as_str(),
                        Some("yuv420p" | "yuva420p")
                    ));
                    let tags = stream["tags"].as_object().unwrap();
                    assert!(
                        tags.iter()
                            .any(|(key, value)| key.eq_ignore_ascii_case("alpha_mode")
                                && value == "1")
                    );
                }
                Format::Mov => assert!(stream["pix_fmt"].as_str().unwrap().starts_with("yuva444p")),
                Format::PngSequence => assert_eq!(stream["pix_fmt"], "rgba"),
                Format::Gif => assert!(matches!(stream["pix_fmt"].as_str(), Some("bgra" | "rgba"))),
                Format::Mp4 => unreachable!(),
            }
            let mut ffmpeg = media::command(media::ffmpeg());
            ffmpeg.args(["-v", "error"]);
            if format == Format::WebM {
                ffmpeg.args(["-c:v", "libvpx-vp9"]);
            }
            let frame = ffmpeg
                .arg("-i")
                .arg(&input)
                .args([
                    "-frames:v",
                    "1",
                    "-pix_fmt",
                    "rgba",
                    "-f",
                    "rawvideo",
                    "pipe:1",
                ])
                .output()
                .unwrap();
            assert!(
                frame.status.success(),
                "{}",
                String::from_utf8_lossy(&frame.stderr)
            );
            assert_eq!(frame.stdout[3], 0, "{format:?} lost the transparent corner");
            assert_eq!(
                frame.stdout[(24 * 64 + 32) * 4 + 3],
                255,
                "{format:?} lost the visible box"
            );
        }
        std::fs::remove_dir_all(root).unwrap();
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
    fn motion_blur_export_samples_video_content_across_an_edit() {
        let root =
            std::env::temp_dir().join(format!("forma-native-shutter-video-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let place = place(&root);
        let source = media::tests::video(&root, "source.mp4", 2, false);
        let (url, poster, info) = crate::assets::import_video(&source, &place).unwrap();
        let mut scene = Scene::empty("test".into());
        (scene.width, scene.height, scene.duration) = (64.0, 48.0, 1.25);
        scene.motion_blur = Some(crate::motion_blur::Settings {
            shutter_angle: 720.0,
            samples: 16,
        });
        scene.add_shape("video".into(), "video");
        let layer = &mut scene.shapes[0];
        layer.motion_blur = true;
        layer.extra.extend([
            ("assetData".into(), url.into()),
            ("posterData".into(), poster.into()),
            ("mediaDuration".into(), info.duration.into()),
        ]);
        let (p, q) = (32.0, 24.0);
        layer.edit_pose(
            0.0,
            crate::model::Pose {
                x: p,
                y: q,
                w: 64.0,
                h: 48.0,
                ..layer.pose(0.0)
            },
        );
        let output = root.join("blurred.mp4");
        let settings = Settings {
            fps: 24,
            ..Settings::default()
        };
        let run = start(
            &scene,
            &place,
            output.clone(),
            settings,
            egui::Context::default(),
        )
        .unwrap();
        assert_eq!(run.result.recv().unwrap(), Ok(output.clone()));
        let frame = |time| {
            media::Frames::start(&output, time, 24, 64, 48)
                .unwrap()
                .next()
                .unwrap()
                .pixel(32, 24)
                .unwrap()
        };
        let (before, mixed, after) = (frame(0.8), frame(1.0), frame(1.12));
        assert!(before.red() > 200 && before.green() < 30, "{before:?}");
        assert!(
            mixed.red() > 35 && mixed.green() > 35 && mixed.blue() < 30,
            "{mixed:?}"
        );
        assert!(after.green() > 200 && after.red() < 30, "{after:?}");
        let still = render::frame(&scene, &place, 1.0)
            .unwrap()
            .pixel(32, 24)
            .unwrap();
        assert!(still.red() > 35 && still.green() > 35, "{still:?}");
        let preview = render::preview_sized(&scene, &place, 1.0, (64, 48))
            .unwrap()
            .pixel(32, 24)
            .unwrap();
        assert!(preview.green() > 200 && preview.red() < 30, "{preview:?}");
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
                speed: 1.0,
            },
            Sound {
                file: Path::new("b.mp4"),
                start: 0.0,
                end: 1.0,
                offset: 0.0,
                speed: 1.0,
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
    fn nested_video_and_sound_follow_the_composition_clock() {
        let root = std::env::temp_dir().join(format!("forma-nested-video-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let place = place(&root);
        let source = media::tests::video(&root, "source.mp4", 2, true);
        let (asset, poster, info) = crate::assets::import_video(&source, &place).unwrap();
        let mut scene = Scene::empty("main".into());
        (scene.width, scene.height, scene.duration) = (160.0, 120.0, 2.4);
        let mut child = Scene::empty("child".into());
        (child.width, child.height, child.duration) = (160.0, 120.0, 2.0);
        child.add_shape("source".into(), "video");
        let layer = &mut child.shapes[0];
        (layer.keys[0].w, layer.keys[0].h) = (160.0, 120.0);
        layer.extra.extend([
            ("assetData".into(), asset.into()),
            ("posterData".into(), poster.into()),
            ("mediaDuration".into(), info.duration.into()),
        ]);
        scene.compositions.push(child);
        scene
            .add_composition_layer("instance".into(), "child")
            .unwrap();
        scene.shapes[0].in_time = Some(0.5);
        let playback = scene.shapes[0].composition.as_mut().unwrap();
        (playback.offset, playback.speed, playback.looping) = (0.25, 2.0, true);

        let path = root.join("nested.mp4");
        let job = start(
            &scene,
            &place,
            path.clone(),
            Settings::default(),
            egui::Context::default(),
        )
        .unwrap();
        assert_eq!(job.result.recv().unwrap(), Ok(path.clone()));
        assert!(media::probe(&path).unwrap().sound);
        let color = |at| {
            let frame = media::Frames::start(&path, at, 30, 16, 12)
                .unwrap()
                .next()
                .unwrap();
            let pixel = frame.pixel(8, 6).unwrap();
            [pixel.red(), pixel.green()]
        };
        assert!(color(0.65)[0] > 200, "{:?}", color(0.65));
        assert!(color(1.05)[1] > 200, "{:?}", color(1.05));
        assert!(color(1.55)[0] > 200, "{:?}", color(1.55));
        let amplitude = |time: &str| {
            let audio = media::command(media::ffmpeg())
                .args(["-v", "error", "-ss", time, "-i"])
                .arg(&path)
                .args(["-t", "0.15", "-vn", "-ac", "1", "-f", "s16le", "pipe:1"])
                .output()
                .unwrap();
            assert!(audio.status.success());
            audio
                .stdout
                .chunks_exact(2)
                .map(|v| i16::from_le_bytes([v[0], v[1]]).unsigned_abs())
                .max()
                .unwrap_or(0)
        };
        assert!(amplitude("0.1") < 20);
        assert!(amplitude("0.7") > 100);
        assert!(amplitude("1.65") > 100);
        std::fs::remove_dir_all(root).unwrap();
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
            ..Settings::default()
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
