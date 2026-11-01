//! FFmpeg for video layers: what a video is (size, length, sound), its poster, and its frames. FFmpeg runs as a child
//! process (nothing links against it), as in StreamCuts (`streamcuts/native/src/ffmpeg.rs`), and like StreamCuts this
//! prefers a shared FFmpeg build when PATH has one: a static build takes about half a second just to start.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::OnceLock;
use tiny_skia::{IntSize, Pixmap};

fn find(program: &str) -> PathBuf {
    let exe = format!("{program}.exe");
    let found: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).map(|dir| dir.join(&exe)).filter(|p| p.is_file()).collect())
        .unwrap_or_default();
    let shared = found.iter().find(|p| {
        std::fs::read_dir(p.parent().unwrap())
            .map(|entries| entries.flatten().any(|e| e.file_name().to_string_lossy().starts_with("avcodec-")))
            .unwrap_or(false)
    });
    shared.or(found.first()).cloned().unwrap_or_else(|| PathBuf::from(exe))
}

pub fn ffmpeg() -> &'static Path {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| find("ffmpeg"))
}

fn ffprobe() -> &'static Path {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| find("ffprobe"))
}

/// A command that doesn't flash a console window.
pub fn command(program: &Path) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Info {
    pub width: u32,
    pub height: u32,
    pub duration: f64,
    pub sound: bool,
}

pub fn probe(path: &Path) -> Result<Info, String> {
    let output = command(ffprobe())
        .args(["-v", "error", "-show_entries", "stream=codec_type,width,height:format=duration", "-of", "json"])
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("FFprobe couldn't start ({e}). Install FFmpeg to use videos."))?;
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|_| "This video can't be read".to_owned())?;
    let streams = json["streams"].as_array().cloned().unwrap_or_default();
    let video = streams.iter().find(|s| s["codec_type"] == "video").ok_or("This file has no video")?;
    let duration = json["format"]["duration"].as_str().and_then(|d| d.parse::<f64>().ok()).filter(|d| *d > 0.0).ok_or("This video has no length")?;
    Ok(Info {
        width: video["width"].as_u64().unwrap_or(0) as u32,
        height: video["height"].as_u64().unwrap_or(0) as u32,
        duration,
        sound: streams.iter().any(|s| s["codec_type"] == "audio"),
    })
}

/// Raw RGBA frames `width` × `height` (stretched, like `drawImage`), 30 a second, starting `from` seconds in and
/// looping at the end, as a looping `<video>` plays.
pub struct Frames {
    child: Child,
    output: ChildStdout,
    size: IntSize,
}

impl Frames {
    pub fn start(path: &Path, from: f64, width: u32, height: u32) -> Result<Self, String> {
        let size = IntSize::from_wh(width.max(1), height.max(1)).ok_or("Invalid video size")?;
        let mut child = command(ffmpeg())
            .args(["-v", "error", "-stream_loop", "-1", "-ss", &format!("{:.3}", from.max(0.0)), "-i"])
            .arg(path)
            .args(["-an", "-vf", &format!("fps=30,scale={}:{}:flags=bicubic", size.width(), size.height()), "-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("FFmpeg couldn't start ({e}). Install FFmpeg to use videos."))?;
        let output = child.stdout.take().unwrap();
        Ok(Self { child, output, size })
    }

    pub fn next(&mut self) -> Result<Pixmap, String> {
        let mut rgba = vec![0; self.size.width() as usize * self.size.height() as usize * 4];
        self.output.read_exact(&mut rgba).map_err(|_| "The video stopped early".to_owned())?;
        Pixmap::from_vec(rgba, self.size).ok_or_else(|| "Invalid video frame".into())
    }
}

impl Drop for Frames {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// One frame at `at` seconds, at the video's own size, as a JPEG (Electron Forma's posters are JPEGs).
pub fn poster(path: &Path, at: f64) -> Result<Vec<u8>, String> {
    let output = command(ffmpeg())
        .args(["-v", "error", "-ss", &format!("{at:.3}"), "-i"])
        .arg(path)
        .args(["-frames:v", "1", "-q:v", "3", "-f", "image2pipe", "-c:v", "mjpeg", "pipe:1"])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("FFmpeg couldn't start ({e}). Install FFmpeg to use videos."))?;
    if output.stdout.is_empty() {
        return Err("Couldn't read a picture from this video".into());
    }
    Ok(output.stdout)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A short test video: a color that changes every second, and a tone if `sound`.
    pub fn video(dir: &Path, name: &str, seconds: u32, sound: bool) -> PathBuf {
        let path = dir.join(name);
        let colors: Vec<String> = ["red", "lime", "blue", "yellow"].iter().cycle().take(seconds as usize)
            .map(|c| format!("color=c={c}:s=64x48:r=30:d=1")).collect();
        let mut args: Vec<String> = colors.iter().flat_map(|c| ["-f".into(), "lavfi".into(), "-i".into(), c.clone()]).collect();
        let joined: String = (0..seconds).map(|i| format!("[{i}:v]")).collect();
        let mut filter = format!("{joined}concat=n={seconds}:v=1:a=0[v]");
        if sound {
            args.extend(["-f", "lavfi", "-i", &format!("sine=frequency=440:duration={seconds}")].map(String::from));
            filter.push_str(&format!(";[{seconds}:a]anull[a]"));
        }
        let mut run = command(ffmpeg());
        run.args(["-v", "error", "-y"]).args(&args).args(["-filter_complex", &filter, "-map", "[v]"]);
        if sound {
            run.args(["-map", "[a]", "-c:a", "aac"]);
        }
        assert!(run.args(["-c:v", "libx264", "-pix_fmt", "yuv420p"]).arg(&path).status().unwrap().success());
        path
    }

    #[test]
    fn frames_loop_like_a_looping_video() {
        let dir = std::env::temp_dir().join(format!("forma-media-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = video(&dir, "colors.mp4", 2, true);
        let info = probe(&path).unwrap();
        assert_eq!((info.width, info.height, info.sound), (64, 48, true));
        assert!((info.duration - 2.0).abs() < 0.1, "{info:?}");
        // From 1.5 s: half a second of lime, then red again from the start.
        let mut frames = Frames::start(&path, 1.5, 8, 6).unwrap();
        let colors: Vec<[u8; 3]> = (0..30).map(|_| {
            let frame = frames.next().unwrap();
            let p = frame.pixel(4, 3).unwrap();
            [p.red(), p.green(), p.blue()]
        }).collect();
        assert!(colors[5][1] > 200 && colors[5][0] < 50, "lime first: {:?}", colors[5]);
        assert!(colors[25][0] > 200 && colors[25][1] < 50, "then red: {:?}", colors[25]);
        assert!(image::load_from_memory(&poster(&path, 0.05).unwrap()).is_ok());
        drop(frames);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
