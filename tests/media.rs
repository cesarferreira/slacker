use slacker::media::{Pixels, Video};
use std::{
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[test]
fn image_decoding_and_cell_fit_preserve_aspect_ratio() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("image.png");
    image::RgbImage::from_pixel(200, 100, image::Rgb([255, 0, 0]))
        .save(&path)
        .unwrap();
    let pixels = Pixels::open(&path).unwrap();
    assert_eq!(pixels.fit(80, 20), (80, 20));
    assert_eq!(pixels.rgb[..3], [255, 0, 0]);
}

#[test]
fn ffmpeg_decodes_video_into_actual_rgb_frames() {
    if !Command::new("ffmpeg")
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
    {
        eprintln!("Skipping FFmpeg integration: ffmpeg is not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("clip.mkv");
    let status = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=red:s=64x36:d=0.5",
            "-c:v",
            "ffv1",
        ])
        .arg(&path)
        .status()
        .unwrap();
    assert!(status.success());
    let video = Video::start(&path).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let pixels = loop {
        if let Some(frame) = video.poll() {
            break frame;
        }
        assert!(Instant::now() < deadline, "FFmpeg produced no frame");
        thread::sleep(Duration::from_millis(10));
    };
    assert_eq!((pixels.width, pixels.height), (640, 360));
    assert_eq!(pixels.rgb.len(), 640 * 360 * 3);
    let center = (180 * 640 + 320) * 3;
    assert!(pixels.rgb[center] > 200 && pixels.rgb[center + 1] < 20 && pixels.rgb[center + 2] < 20);
    video.toggle();
    assert!(video.paused.load(std::sync::atomic::Ordering::Relaxed));
    drop(video);
}
