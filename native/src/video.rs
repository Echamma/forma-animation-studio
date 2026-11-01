//! Exports shape, picture and text scenes to a silent MP4 through FFmpeg, with frames rendered in Rust.

use crate::{model::Scene, render::Renderer};
use canopy_ui::Place;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
    mpsc::{self, Receiver},
};

pub struct Export {
    pub progress: Arc<AtomicU32>,
    pub cancel: Arc<AtomicBool>,
    pub result: Receiver<Result<PathBuf, String>>,
}

pub fn start(
    scene: &Scene,
    place: &Place,
    output: PathBuf,
    ctx: egui::Context,
) -> Result<Export, String> {
    if scene.shapes.iter().any(|shape| {
        !matches!(
            shape.kind.as_str(),
            "circle" | "rect" | "triangle" | "diamond" | "arrow" | "image" | "text"
        )
    }) {
        return Err("Video export of SVG and video layers is coming later.".into());
    }
    if scene.shapes.iter().any(|shape| {
        shape
            .extra
            .get("blur")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
            > 0.0
    }) {
        return Err("Video export of blurred layers is coming later.".into());
    }
    let renderer = Renderer::new(scene, place)?;
    let scene = scene.clone();
    let progress = Arc::new(AtomicU32::new(0));
    let cancel = Arc::new(AtomicBool::new(false));
    let (send, result) = mpsc::channel();
    let (progress_out, cancel_in) = (progress.clone(), cancel.clone());
    std::thread::spawn(move || {
        let outcome = render_video(&scene, &renderer, &output, &progress_out, &cancel_in);
        let _ = send.send(outcome.map(|_| output));
        ctx.request_repaint();
    });
    Ok(Export {
        progress,
        cancel,
        result,
    })
}

fn render_video(
    scene: &Scene,
    renderer: &Renderer,
    output: &PathBuf,
    progress: &AtomicU32,
    cancel: &AtomicBool,
) -> Result<(), String> {
    const FPS: u32 = 30;
    let (width, height) = (scene.width.round() as u32, scene.height.round() as u32);
    let frames = (scene.duration * FPS as f64).ceil() as u32;
    let mut command = Command::new("ffmpeg.exe");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
        .args([
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
            "30",
            "-i",
            "pipe:0",
            "-an",
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
        ])
        .arg(output)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| format!("FFmpeg couldn't start ({e}). Install FFmpeg to export MP4."))?;
    let mut stderr = child.stderr.take().unwrap();
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
            match renderer.frame(scene, index as f64 / FPS as f64) {
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
    if cancel.load(Ordering::Relaxed) || failure.is_some() || !status.success() {
        let _ = std::fs::remove_file(output);
        return Err(failure.unwrap_or_else(|| {
            if cancel.load(Ordering::Relaxed) {
                "Export stopped".into()
            } else {
                format!(
                    "FFmpeg failed: {}",
                    errors.lines().last().unwrap_or("no details")
                )
            }
        }));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_a_real_short_mp4() {
        let root = std::env::temp_dir().join(format!("forma-native-video-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let place = Place {
            space: None,
            projects_dir: root.clone(),
            data_dir: root.clone(),
            exports_dir: root.clone(),
        };
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
        let run = start(&scene, &place, output.clone(), egui::Context::default()).unwrap();
        assert_eq!(run.result.recv().unwrap(), Ok(output.clone()));
        assert!(std::fs::metadata(&output).unwrap().len() > 1000);
        assert!(
            Command::new("ffmpeg.exe")
                .args(["-v", "error", "-i"])
                .arg(&output)
                .args(["-frames:v", "1", "-f", "null", "-"])
                .status()
                .unwrap()
                .success()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
