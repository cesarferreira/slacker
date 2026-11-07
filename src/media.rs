use std::{
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};

pub const IMAGE_ID: u32 = 31;

pub fn transmit(
    out: &mut impl Write,
    rgb: &[u8],
    width: u32,
    height: u32,
    columns: u16,
    rows: u16,
) -> io::Result<()> {
    let data = STANDARD.encode(rgb);
    let chunks = data.as_bytes().chunks(4096);
    let total = chunks.len();
    for (i, chunk) in chunks.enumerate() {
        if i == 0 {
            write!(
                out,
                "\x1b_Ga=T,f=24,s={width},v={height},i={IMAGE_ID},c={columns},r={rows},C=1,q=2,"
            )?;
        } else {
            write!(out, "\x1b_G")?;
        }
        write!(out, "m={};", u8::from(i + 1 != total))?;
        out.write_all(chunk)?;
        out.write_all(b"\x1b\\")?;
    }
    out.flush()
}

pub fn clear(out: &mut impl Write) -> io::Result<()> {
    write!(out, "\x1b_Ga=d,d=I,i={IMAGE_ID},q=2;\x1b\\")?;
    out.flush()
}

pub fn supported() -> bool {
    let identifiers =
        ["TERM", "TERM_PROGRAM"].map(|key| std::env::var(key).unwrap_or_default().to_lowercase());
    std::env::var_os("TMUX").is_none()
        && (identifiers.iter().any(|identifier| {
            ["kitty", "ghostty", "wezterm"]
                .iter()
                .any(|name| identifier.contains(name))
        }) || std::env::var_os("KITTY_WINDOW_ID").is_some())
}

pub fn video_path(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
        matches!(
            s.to_lowercase().as_str(),
            "mp4" | "mov" | "webm" | "mkv" | "avi" | "m4v" | "gif"
        )
    })
}

pub struct Pixels {
    pub rgb: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl Pixels {
    pub fn open(path: &Path) -> Result<Self> {
        let reader = image::ImageReader::open(path)?.with_guessed_format()?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(16384);
        limits.max_image_height = Some(16384);
        limits.max_alloc = Some(256 * 1024 * 1024);
        let mut reader = reader;
        reader.limits(limits);
        let image = reader
            .decode()
            .context("Could not decode image (PNG, JPEG, GIF and WebP supported)")?
            .thumbnail(1280, 960)
            .to_rgb8();
        Ok(Self {
            width: image.width(),
            height: image.height(),
            rgb: image.into_raw(),
        })
    }

    pub fn fit(&self, columns: u16, rows: u16) -> (u16, u16) {
        // Typical terminal cells are twice as tall as they are wide.
        let scale = (f64::from(columns) / f64::from(self.width))
            .min(f64::from(rows) * 2.0 / f64::from(self.height));
        (
            ((f64::from(self.width) * scale).floor() as u16).max(1),
            ((f64::from(self.height) * scale / 2.0).floor() as u16).max(1),
        )
    }
}

pub struct Prepared {
    pub title: String,
    pub path: PathBuf,
    pub video: bool,
    pub image: Option<Pixels>,
    pub temporary: Option<tempfile::NamedTempFile>,
}

impl Prepared {
    pub fn local(path: PathBuf, video: bool) -> Result<Self> {
        if !path.is_file() {
            bail!("Media file does not exist: {}", path.display());
        }
        let image = if video {
            None
        } else {
            Some(Pixels::open(&path)?)
        };
        Ok(Self {
            title: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            path,
            video,
            image,
            temporary: None,
        })
    }
}

pub struct Video {
    child: Child,
    frames: Receiver<Pixels>,
    pub paused: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

impl Video {
    pub fn start(path: &Path) -> Result<Self> {
        let mut child = Command::new("ffmpeg").args(["-nostdin", "-loglevel", "error", "-i"]).arg(path)
            .args(["-an", "-vf", "fps=12,scale=640:360:force_original_aspect_ratio=decrease,pad=640:360:(ow-iw)/2:(oh-ih)/2", "-f", "rawvideo", "-pix_fmt", "rgb24", "pipe:1"])
            .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().context("Video playback requires ffmpeg on PATH")?;
        let mut stdout = child
            .stdout
            .take()
            .context("Could not read FFmpeg frames")?;
        let (tx, frames) = mpsc::sync_channel(1);
        let paused = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let (pause_reader, stop_reader) = (paused.clone(), stop.clone());
        thread::spawn(move || {
            while !stop_reader.load(Ordering::Relaxed) {
                let start = Instant::now();
                if pause_reader.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(20));
                    continue;
                }
                let mut rgb = vec![0; 640 * 360 * 3];
                if stdout.read_exact(&mut rgb).is_err() {
                    break;
                }
                if tx
                    .try_send(Pixels {
                        rgb,
                        width: 640,
                        height: 360,
                    })
                    .is_err()
                    && stop_reader.load(Ordering::Relaxed)
                {
                    break;
                }
                thread::sleep(Duration::from_millis(83).saturating_sub(start.elapsed()));
            }
        });
        Ok(Self {
            child,
            frames,
            paused,
            stop,
        })
    }
    pub fn poll(&self) -> Option<Pixels> {
        self.frames.try_recv().ok()
    }
    pub fn toggle(&self) {
        self.paused.fetch_xor(true, Ordering::Relaxed);
    }
    pub fn ended(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_some()
    }
}

impl Drop for Video {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
