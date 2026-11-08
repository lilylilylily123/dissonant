//! Fetching a sample from a link: yt-dlp downloads the audio, the store keeps our own copy,
//! analysis finds the hits, the tempo and the key, and the result lands as a new sampler track.
//!
//! The pipeline ([`fetch`]) is plain blocking code reporting through a callback, so it runs the
//! same inside the app and headless in a test; the Tauri commands at the bottom only add the
//! event, the document edit and the engine sync.
//!
//! yt-dlp is an external program. The user's own copy wins when there is one, because it is the
//! one they keep up to date and YouTube breaks old versions often; otherwise the official
//! standalone build is installed into `<app data>/bin`, checked against the release's own
//! `SHA2-256SUMS`. ffmpeg is never installed for the user: we ask for m4a, which we decode
//! ourselves, so it is only needed for the rare video that offers nothing else.

use crate::audio_files::{self, DecodedAudio};
use crate::{AppState, Snapshot};
use dissonant_core::{
    chop_region, suggested_region, AudioSource, Chopper, Command, Onset, SampleAnalysis, Sampler, ScaleType,
    DEFAULT_CHOP_COUNT, MAX_SAMPLE_SECONDS,
};
use dissonant_engine::audio::AudioBuffer;
use dissonant_engine::{analyze, Analysis};
use parking_lot::Mutex;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

/// The event every progress report goes out on.
pub const EVENT: &str = "sample-fetch";
/// The error a cancelled fetch ends with. The UI tells it apart from a failure by this string.
pub const CANCELLED: &str = "cancelled";
const LABEL: &str = "sample from link";
const RELEASE_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download";
const TOO_LONG: &str = "that video is longer than 10 minutes; samples are limited to 10 minutes";
/// Where Homebrew and hand installs put tools. A Finder-launched app's PATH is only
/// `/usr/bin:/bin:/usr/sbin:/sbin`, so without these the user's own yt-dlp would go unseen.
#[cfg(unix)]
const EXTRA_TOOL_DIRS: [&str; 2] = ["/opt/homebrew/bin", "/usr/local/bin"];
#[cfg(not(unix))]
const EXTRA_TOOL_DIRS: [&str; 0] = [];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SampleFetchPhase {
    Locating,
    Installing,
    Downloading,
    Converting,
    Decoding,
    Analyzing,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SampleFetchProgress {
    pub phase: SampleFetchPhase,
    /// 0…1, or -1 when the phase cannot say how far along it is.
    pub fraction: f64,
    /// The video's title, once yt-dlp has told us.
    pub title: Option<String>,
    /// Only on `failed`: what went wrong, in words for the user.
    pub message: Option<String>,
}

/// The one fetch that may run at a time, and the handle that lets another thread cancel it.
#[derive(Default)]
pub struct FetchControl {
    busy: AtomicBool,
    cancelled: AtomicBool,
    /// The yt-dlp or ffmpeg process running right now, so a cancel can kill it mid-download.
    child: Mutex<Option<Child>>,
    title: Mutex<Option<String>>,
}

impl FetchControl {
    /// Claim the fetch slot; `false` when another fetch holds it.
    fn try_begin(&self) -> bool {
        if self.busy.swap(true, Ordering::SeqCst) {
            return false;
        }
        self.cancelled.store(false, Ordering::SeqCst);
        *self.title.lock() = None;
        true
    }

    fn finish(&self) {
        self.busy.store(false, Ordering::SeqCst);
    }

    /// Stop the running fetch. The fetch itself notices, cleans up, and ends with [`CANCELLED`].
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.kill();
    }

    fn check(&self) -> Result<(), String> {
        if self.cancelled.load(Ordering::SeqCst) {
            Err(CANCELLED.into())
        } else {
            Ok(())
        }
    }

    fn title(&self) -> Option<String> {
        self.title.lock().clone()
    }

    fn kill(&self) {
        if let Some(child) = self.child.lock().as_mut() {
            kill_tree(child);
        }
    }

    /// Run `cmd` to completion, handing each stdout line to `on_line` as it arrives.
    ///
    /// An `Err` from `on_line` kills the process and becomes the result; a cancel at any point
    /// wins over whatever the process said. stderr comes back whole, for error messages.
    ///
    /// The pipes are read on their own threads and the end is decided by the *process*, not by
    /// end-of-file: on macOS a pipe is made close-on-exec only after it is created, so a process
    /// spawned at that moment by another thread can inherit our write end and hold the pipe open
    /// long after yt-dlp is dead. Waiting for EOF would then hang the fetch (and a cancel) until
    /// that unrelated process exits.
    fn run(&self, mut cmd: std::process::Command, on_line: &mut dyn FnMut(&str) -> Result<(), String>) -> Result<Finished, String> {
        self.check()?;
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        // Its own process group, so a kill reaches what it spawned too: the standalone yt-dlp
        // is a launcher that unpacks and starts the real program as a child, and yt-dlp may run
        // ffmpeg. Killing only the launcher would leave the download running, holding our pipes.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let program = Path::new(cmd.get_program())
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut child = cmd.spawn().map_err(|e| format!("could not start {program}: {e}"))?;
        let stdout = child.stdout.take().ok_or("no stdout pipe")?;
        let mut stderr = child.stderr.take().ok_or("no stderr pipe")?;
        let (line_tx, lines) = mpsc::channel::<std::io::Result<Vec<u8>>>();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = Vec::new();
                match reader.read_until(b'\n', &mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        if line_tx.send(Ok(line)).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = line_tx.send(Err(e));
                        break;
                    }
                }
            }
        });
        // Drained on its own thread too: a child that fills the stderr pipe while we only read
        // stdout would block forever.
        let (err_tx, err_text) = mpsc::channel::<String>();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stderr.read_to_end(&mut bytes);
            let _ = err_tx.send(String::from_utf8_lossy(&bytes).into_owned());
        });
        *self.child.lock() = Some(child);
        // A cancel that landed between the spawn and the line above found no child to kill.
        if self.cancelled.load(Ordering::SeqCst) {
            self.kill();
        }

        let mut failure = None;
        let mut handle = |line: Vec<u8>, failure: &mut Option<String>| -> bool {
            let text = String::from_utf8_lossy(&line);
            match on_line(text.trim_end_matches(['\r', '\n'])) {
                Ok(()) => true,
                Err(e) => {
                    *failure = Some(e);
                    self.kill();
                    false
                }
            }
        };
        loop {
            match lines.recv_timeout(POLL) {
                Ok(Ok(line)) => {
                    if !handle(line, &mut failure) {
                        break;
                    }
                }
                Ok(Err(e)) => {
                    failure = Some(format!("could not read {program}'s output: {e}"));
                    self.kill();
                    break;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if self.cancelled.load(Ordering::SeqCst) {
                        self.kill();
                        break;
                    }
                    let exited = self.child.lock().as_mut().is_some_and(|c| matches!(c.try_wait(), Ok(Some(_))));
                    if exited {
                        // What it wrote before exiting is still in the pipe: take it, then stop
                        // at the first quiet spell rather than at an EOF that may never come.
                        while let Ok(Ok(line)) = lines.recv_timeout(DRAIN) {
                            if !handle(line, &mut failure) {
                                break;
                            }
                        }
                        break;
                    }
                }
            }
        }

        let child = self.child.lock().take();
        let status = child
            .ok_or("the process went missing")?
            .wait()
            .map_err(|e| format!("could not wait for {program}: {e}"))?;
        let stderr = err_text.recv_timeout(DRAIN).unwrap_or_default();
        self.check()?;
        match failure {
            Some(e) => Err(e),
            None => Ok(Finished { status, stderr }),
        }
    }
}

/// How often a running tool is checked for having exited or been cancelled.
const POLL: std::time::Duration = std::time::Duration::from_millis(100);
/// How long the output of an exited tool may go quiet before it is taken as complete.
const DRAIN: std::time::Duration = std::time::Duration::from_millis(250);

struct Finished {
    status: ExitStatus,
    stderr: String,
}

fn kill_tree(child: &mut Child) {
    #[cfg(unix)]
    {
        // The child leads its own group (`process_group(0)`), so its pid is the group id. It
        // has not been waited on yet, so neither id can have been reused.
        // SAFETY: `kill` takes plain integers and touches no memory of ours.
        unsafe {
            libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
        }
    }
    #[cfg(windows)]
    {
        // `/T` takes the launcher's children with it, as the process group does on unix.
        let _ = tool_command(Path::new("taskkill"))
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
}

/// Reports progress through the caller's sink, tagged with the title once known, at most once
/// per percent within a phase — enough for a smooth bar, few enough not to flood the webview.
struct Reporter<'a> {
    control: &'a FetchControl,
    sink: &'a mut dyn FnMut(SampleFetchProgress),
    last: Option<(SampleFetchPhase, f64)>,
}

impl Reporter<'_> {
    fn report(&mut self, phase: SampleFetchPhase, fraction: f64) {
        if let Some((p, f)) = self.last {
            if p == phase && (fraction - f).abs() < 0.01 && !(fraction >= 1.0 && f < 1.0) {
                return;
            }
        }
        self.last = Some((phase, fraction));
        (self.sink)(SampleFetchProgress {
            phase,
            fraction,
            title: self.control.title(),
            message: None,
        });
    }
}

/// A fetched source, already written to the store, with its analysis.
pub struct Fetched {
    pub source: AudioSource,
    pub buffer: AudioBuffer,
    pub analysis: SampleAnalysis,
}

/// Download, decode, store and analyse the audio behind `url`.
///
/// `data_dir` is the app data folder: the audio store and the installed yt-dlp live under it.
/// Blocking, and long: run it on a worker. On any error nothing is left behind in the store.
pub fn fetch(
    url: &str,
    data_dir: &Path,
    control: &FetchControl,
    on_progress: &mut dyn FnMut(SampleFetchProgress),
) -> Result<Fetched, String> {
    let url = validate_url(url)?;
    let mut reporter = Reporter { control, sink: on_progress, last: None };

    reporter.report(SampleFetchPhase::Locating, -1.0);
    let ytdlp = match locate("yt-dlp", data_dir) {
        Some(path) => path,
        None => install_ytdlp(data_dir, control, &mut reporter)?,
    };

    let scratch = Scratch::new()?;
    let downloaded = download(&ytdlp, &url, scratch.path(), control, &mut reporter)?;
    let path = if audio_files::is_audio_path(&downloaded) {
        downloaded
    } else {
        reporter.report(SampleFetchPhase::Converting, -1.0);
        transcode(&downloaded, scratch.path(), data_dir, control)?
    };

    reporter.report(SampleFetchPhase::Decoding, 0.0);
    let mut audio = audio_files::decode(&path, &mut |fraction| reporter.report(SampleFetchPhase::Decoding, fraction as f64))?;
    drop(scratch);
    control.check()?;
    keep_two_channels(&mut audio);
    let frames = audio.frames();
    if frames == 0 {
        return Err("that video has no audio".into());
    }
    // A second's slack: yt-dlp's duration is rounded, and the filter already let it through.
    // This catches what it could not see, a video that reported no duration at all.
    if frames as f64 / audio.sample_rate as f64 > MAX_SAMPLE_SECONDS + 1.0 {
        return Err(TOO_LONG.into());
    }

    let store = audio_files::store_dir(data_dir);
    let id = Uuid::new_v4();
    audio_files::write_source(&store, id, &audio)?;
    let (sample_rate, channels) = (audio.sample_rate, audio.channels);
    reporter.report(SampleFetchPhase::Analyzing, -1.0);
    let buffer = AudioBuffer::new(sample_rate, channels, audio.data);
    let analysis = to_sample_analysis(analyze(&buffer));
    if let Err(e) = control.check() {
        let _ = fs::remove_file(audio_files::source_path(&store, id));
        return Err(e);
    }

    let title = control.title().map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    Ok(Fetched {
        source: AudioSource {
            id,
            name: title.unwrap_or_else(|| "sample".into()),
            frames: frames as u64,
            sample_rate,
            channels,
            origin_url: Some(url),
        },
        buffer,
        analysis,
    })
}

/// The edit that puts a fetched source on a new sampler track: a region suggested from its
/// first strong hit and tempo (bars of `beats_per_bar`), auto-chopped at the hits inside it.
/// The region and the chop tool are stored, so moving the region re-cuts it the same way.
pub fn add_sampler_track(source: AudioSource, analysis: SampleAnalysis, beats_per_bar: u32) -> Command {
    let seconds = source.frames as f64 / source.sample_rate.max(1) as f64;
    let region = suggested_region(&analysis, seconds, beats_per_bar);
    let chopper = Chopper::Auto { count: DEFAULT_CHOP_COUNT };
    let chops = chop_region(&chopper, &region, Some(&analysis), beats_per_bar);
    let mut sampler = Sampler::new(source.id, chops);
    sampler.analysis = Some(analysis);
    sampler.region = Some(region);
    sampler.chopper = Some(chopper);
    Command::AddSamplerTrack { name: source.name.clone(), source, sampler }
}

/// The trimmed link, if it is an `http(s)` URL with a host. Anything else never reaches yt-dlp,
/// which would happily read a local path or a `file:` URL.
pub fn validate_url(raw: &str) -> Result<String, String> {
    let url = raw.trim();
    let lower = url.to_ascii_lowercase();
    let rest = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://"));
    match rest {
        Some(rest)
            if !rest.is_empty()
                && !rest.starts_with(['/', '?', '#'])
                && !url.chars().any(|c| c.is_whitespace() || c.is_control()) =>
        {
            Ok(url.to_string())
        }
        _ => Err("paste an http(s) link, e.g. https://www.youtube.com/watch?v=…".into()),
    }
}

fn to_sample_analysis(a: Analysis) -> SampleAnalysis {
    SampleAnalysis {
        bpm: a.bpm,
        bpm_confidence: a.bpm_confidence,
        key_root: a.key_root,
        key_scale: if a.key_minor { ScaleType::Minor } else { ScaleType::Major },
        key_confidence: a.key_confidence,
        onsets: a.onsets.into_iter().map(|(seconds, strength)| Onset { seconds, strength }).collect(),
    }
}

/// Drop every channel past the first two, as the engine would when playing it anyway; a 5.1
/// soundtrack would otherwise cost three times the memory for nothing.
fn keep_two_channels(audio: &mut DecodedAudio) {
    let channels = audio.channels as usize;
    if channels <= 2 {
        return;
    }
    let frames = audio.frames();
    // In place, front to back: frame `f`'s destination `2f` never passes its source `cf`.
    for f in 0..frames {
        audio.data[2 * f] = audio.data[channels * f];
        audio.data[2 * f + 1] = audio.data[channels * f + 1];
    }
    audio.data.truncate(2 * frames);
    audio.channels = 2;
}

// ─── yt-dlp ─────────────────────────────────────────────────────────────────────────────────

/// One stdout line of the invocation in [`download`].
#[derive(Debug, Clone, PartialEq)]
enum YtLine {
    Meta { title: String, duration: Option<f64> },
    /// 0…1, `None` when yt-dlp knows neither the size nor an estimate.
    Progress(Option<f64>),
    File(PathBuf),
}

fn parse_line(line: &str) -> Option<YtLine> {
    if let Some(rest) = line.strip_prefix("META ") {
        // From the right: a title may contain `|`, the duration and extension cannot.
        let mut parts = rest.rsplitn(3, '|');
        let _ext = parts.next()?;
        let duration = parts.next()?.trim().parse::<f64>().ok().filter(|d| d.is_finite());
        let title = parts.next()?.to_string();
        return Some(YtLine::Meta { title, duration });
    }
    if let Some(rest) = line.strip_prefix("PROG ") {
        let mut numbers = rest.split_whitespace().map(|n| n.parse::<f64>().ok().filter(|v| v.is_finite()));
        let done = numbers.next()?;
        let total = numbers.next().flatten().filter(|t| *t > 0.0);
        let estimate = numbers.next().flatten().filter(|t| *t > 0.0);
        let fraction = done.zip(total.or(estimate)).map(|(d, t)| (d / t).clamp(0.0, 1.0));
        return Some(YtLine::Progress(fraction));
    }
    line.strip_prefix("FILE ").map(|path| YtLine::File(PathBuf::from(path)))
}

/// What to tell the user when yt-dlp fails: its last `ERROR:` line, without the extractor and
/// video id it prefixes them with, else its last line of any kind that is not a warning.
fn ytdlp_error(stderr: &str) -> Option<String> {
    let lines = || stderr.lines().map(str::trim).filter(|l| !l.is_empty());
    if let Some(error) = lines().filter_map(|l| l.strip_prefix("ERROR:")).next_back() {
        let mut error = error.trim();
        if let Some(rest) = error.strip_prefix('[').and_then(|r| r.split_once("] ")).map(|(_, r)| r) {
            error = match rest.split_once(": ") {
                Some((id, message)) if !id.contains(char::is_whitespace) => message,
                _ => rest,
            };
        }
        return Some(error.to_string());
    }
    lines().filter(|l| !l.starts_with("WARNING:")).next_back().map(str::to_string)
}

/// Run yt-dlp on `url` in `dir`; the downloaded file's path.
fn download(ytdlp: &Path, url: &str, dir: &Path, control: &FetchControl, reporter: &mut Reporter<'_>) -> Result<PathBuf, String> {
    reporter.report(SampleFetchPhase::Downloading, -1.0);
    let mut cmd = tool_command(ytdlp);
    cmd.current_dir(dir).args([
        "--no-playlist",
        // A link to a playlist itself still names many videos; a sample is one.
        "--playlist-items",
        "1",
        "-f",
        "bestaudio[ext=m4a]/bestaudio",
        // `?` lets a video with no reported duration through; the decoded length catches it.
        "--match-filter",
        "!is_live & duration <=? 600",
        "--newline",
        "--progress",
        "--no-simulate",
        "--print",
        "before_dl:META %(title)s|%(duration)s|%(ext)s",
        "--print",
        "after_move:FILE %(filepath)s",
        "--progress-template",
        "download:PROG %(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s",
        "-o",
        "%(id)s.%(ext)s",
        "--",
        url,
    ]);

    let mut saw_meta = false;
    let mut file = None;
    let finished = control.run(cmd, &mut |line| {
        match parse_line(line) {
            Some(YtLine::Meta { title, duration }) => {
                saw_meta = true;
                *control.title.lock() = Some(title);
                if duration.is_some_and(|d| d > MAX_SAMPLE_SECONDS) {
                    return Err(TOO_LONG.into());
                }
                reporter.report(SampleFetchPhase::Downloading, 0.0);
            }
            Some(YtLine::Progress(fraction)) => reporter.report(SampleFetchPhase::Downloading, fraction.unwrap_or(-1.0)),
            Some(YtLine::File(path)) => file = Some(path),
            None => {}
        }
        Ok(())
    })?;

    if !finished.status.success() {
        return Err(ytdlp_error(&finished.stderr).unwrap_or_else(|| format!("yt-dlp failed ({})", finished.status)));
    }
    match file {
        Some(path) if path.is_absolute() => Ok(path),
        Some(path) => Ok(dir.join(path)),
        // The filter skips a video silently, before its metadata line.
        None if !saw_meta => Err(format!("yt-dlp skipped that link: {TOO_LONG}, and live streams cannot be sampled")),
        None => Err("yt-dlp finished without saving any audio".into()),
    }
}

/// Convert something we cannot decode into a WAV we can.
fn transcode(input: &Path, dir: &Path, data_dir: &Path, control: &FetchControl) -> Result<PathBuf, String> {
    let ext = input.extension().and_then(|e| e.to_str()).unwrap_or("unknown");
    let ffmpeg = locate("ffmpeg", data_dir).ok_or_else(|| {
        format!(
            "that video only offers .{ext} audio, which needs ffmpeg to convert, and ffmpeg is not installed. \
             Install it (on a Mac: brew install ffmpeg) and try again"
        )
    })?;
    let output = dir.join("converted.wav");
    let mut cmd = tool_command(&ffmpeg);
    cmd.args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(input)
        .args(["-vn", "-map", "0:a:0", "-c:a", "pcm_f32le"])
        .arg(&output);
    let finished = control.run(cmd, &mut |_| Ok(()))?;
    if !finished.status.success() {
        let reason = finished.stderr.lines().map(str::trim).filter(|l| !l.is_empty()).next_back().map(str::to_string);
        return Err(format!(
            "ffmpeg could not convert the audio: {}",
            reason.unwrap_or_else(|| finished.status.to_string())
        ));
    }
    Ok(output)
}

/// A command for an external tool, with the extra tool folders on its PATH: yt-dlp looks for
/// ffmpeg and a JavaScript runtime there itself.
fn tool_command(program: &Path) -> std::process::Command {
    let mut cmd = std::process::Command::new(program);
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    for extra in EXTRA_TOOL_DIRS {
        if !dirs.iter().any(|d| d == Path::new(extra)) {
            dirs.push(PathBuf::from(extra));
        }
    }
    if let Ok(path) = std::env::join_paths(dirs) {
        cmd.env("PATH", path);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: a GUI app's child would otherwise flash a console.
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

fn exe_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// PATH, then the usual tool folders, then our own `bin`, so the user's copy always wins.
fn search_dirs(data_dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default();
    dirs.extend(EXTRA_TOOL_DIRS.iter().map(PathBuf::from));
    dirs.push(data_dir.join("bin"));
    dirs
}

fn locate(name: &str, data_dir: &Path) -> Option<PathBuf> {
    let exe = exe_name(name);
    search_dirs(data_dir).into_iter().map(|d| d.join(&exe)).find(|p| is_executable(p))
}

fn is_executable(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else { return false };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        meta.is_file()
    }
}

/// The standalone yt-dlp build published for `os`/`arch` (as in `std::env::consts`).
fn release_asset(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        // Universal: one binary for Apple silicon and Intel.
        ("macos", _) => Some("yt-dlp_macos"),
        ("linux", "x86_64") => Some("yt-dlp_linux"),
        ("linux", "aarch64") => Some("yt-dlp_linux_aarch64"),
        ("windows", "x86_64") => Some("yt-dlp.exe"),
        ("windows", "aarch64") => Some("yt-dlp_arm64.exe"),
        ("windows", "x86") => Some("yt-dlp_x86.exe"),
        _ => None,
    }
}

/// `asset`'s lowercase hex digest in a `sha256sum`-style listing.
fn checksum_for(sums: &str, asset: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, name) = line.trim().split_once(char::is_whitespace)?;
        // `sha256sum -b` marks binary mode with a leading `*`.
        let name = name.trim_start();
        let name = name.strip_prefix('*').unwrap_or(name);
        (name == asset && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())).then(|| hash.to_ascii_lowercase())
    })
}

/// Install the standalone yt-dlp into `<data_dir>/bin`, verified against the release's
/// checksums. Nothing is left behind unless it verified.
///
/// Both downloads go through `latest`, so a release published between them fails the check;
/// the user retries and gets a matching pair.
fn install_ytdlp(data_dir: &Path, control: &FetchControl, reporter: &mut Reporter<'_>) -> Result<PathBuf, String> {
    let asset = release_asset(std::env::consts::OS, std::env::consts::ARCH)
        .ok_or("yt-dlp is not installed, and it publishes no standalone build for this system; install it and try again")?;
    reporter.report(SampleFetchPhase::Installing, -1.0);

    let sums_url = format!("{RELEASE_URL}/SHA2-256SUMS");
    let sums = ureq::get(&sums_url)
        .call()
        .map_err(|e| format!("could not download {sums_url}: {e}"))?
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("could not read {sums_url}: {e}"))?;
    let expected = checksum_for(&sums, asset).ok_or_else(|| format!("the yt-dlp release lists no checksum for {asset}"))?;

    let bin = data_dir.join("bin");
    fs::create_dir_all(&bin).map_err(|e| format!("could not create {}: {e}", bin.display()))?;
    let exe = exe_name("yt-dlp");
    let partial = bin.join(format!(".{exe}.download"));
    let installed = (|| {
        let actual = download_hashing(&format!("{RELEASE_URL}/{asset}"), &partial, control, reporter)?;
        if actual != expected {
            return Err(format!(
                "the downloaded yt-dlp failed its checksum (expected {expected}, got {actual}); nothing was installed"
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&partial, fs::Permissions::from_mode(0o755))
                .map_err(|e| format!("could not make yt-dlp executable: {e}"))?;
        }
        let target = bin.join(&exe);
        fs::rename(&partial, &target).map_err(|e| format!("could not install yt-dlp: {e}"))?;
        Ok(target)
    })();
    if installed.is_err() {
        let _ = fs::remove_file(&partial);
    }
    installed
}

/// Stream `url` into `path`; the lowercase hex SHA-256 of what arrived.
fn download_hashing(url: &str, path: &Path, control: &FetchControl, reporter: &mut Reporter<'_>) -> Result<String, String> {
    let response = ureq::get(url).call().map_err(|e| format!("could not download {url}: {e}"))?;
    let total = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|t| *t > 0);
    let mut reader = response.into_body().into_reader();
    let mut file = File::create(path).map_err(|e| format!("could not create {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut done = 0u64;
    loop {
        control.check()?;
        let n = reader.read(&mut buf).map_err(|e| format!("could not read {url}: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n]).map_err(|e| format!("could not write {}: {e}", path.display()))?;
        done += n as u64;
        reporter.report(
            SampleFetchPhase::Installing,
            total.map_or(-1.0, |t| (done as f64 / t as f64).min(1.0)),
        );
    }
    file.flush().map_err(|e| format!("could not write {}: {e}", path.display()))?;
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// A fresh temp folder for one fetch, removed with everything in it when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!("dissonant-sample-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).map_err(|e| format!("could not create a temp folder: {e}"))?;
        Ok(Scratch(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// ─── Tauri commands ─────────────────────────────────────────────────────────────────────────

/// Fetch `url` onto a new sampler track. Progress goes out on [`EVENT`], ending in exactly one
/// of `done`, `failed` or `cancelled`.
///
/// A second fetch while one runs is refused without an event: the running fetch owns the
/// progress display, and a `failed` from the refusal would read as the running one failing.
#[tauri::command]
pub async fn fetch_sample(app: AppHandle, state: State<'_, AppState>, url: String) -> Result<Snapshot, String> {
    let control = Arc::clone(&state.sample_fetch);
    if !control.try_begin() {
        return Err("a sample is already being fetched; wait for it or cancel it first".into());
    }
    let result = fetch_sample_inner(&app, &state, &control, url).await;
    // Free before the last event, so a UI that retries on `failed` is not told it is busy.
    control.finish();
    let (phase, fraction, message) = match &result {
        Ok(_) => (SampleFetchPhase::Done, 1.0, None),
        Err(e) if e == CANCELLED => (SampleFetchPhase::Cancelled, -1.0, None),
        Err(e) => (SampleFetchPhase::Failed, -1.0, Some(e.clone())),
    };
    let _ = app.emit(EVENT, SampleFetchProgress { phase, fraction, title: control.title(), message });
    result
}

async fn fetch_sample_inner(
    app: &AppHandle,
    state: &State<'_, AppState>,
    control: &Arc<FetchControl>,
    url: String,
) -> Result<Snapshot, String> {
    let data_dir = state.data_dir().ok_or("the app data folder is not available")?;
    let store = audio_files::store_dir(&data_dir);
    let handle = app.clone();
    let worker = Arc::clone(control);
    // Minutes of network and subprocess work, then decoding and analysis: none of it may hold
    // up the thread commands run on.
    let fetched = tauri::async_runtime::spawn_blocking(move || {
        fetch(&url, &data_dir, &worker, &mut |progress| {
            let _ = handle.emit(EVENT, progress);
        })
    })
    .await
    .map_err(|e| format!("the fetch thread failed: {e}"))??;

    let Fetched { source, buffer, analysis } = fetched;
    let source_id = source.id;
    let discard = || {
        state.audio_cache.lock().unwrap().remove(&source_id);
        let _ = fs::remove_file(audio_files::source_path(&store, source_id));
    };
    if let Err(e) = control.check() {
        discard();
        return Err(e);
    }
    // Into the cache before the edit, so the pool sync finds it in memory instead of reading
    // back the file we just wrote.
    state.audio_cache.lock().unwrap().insert(source_id, Arc::new(buffer));
    let applied = {
        let mut doc = state.doc.lock().unwrap();
        // The grid tools count bars in the song's meter; a fraction of a beat rounds.
        let bpb = doc.model().time_signature.beats_per_bar().round().max(1.0) as u32;
        doc.apply_labeled(add_sampler_track(source, analysis, bpb), false, Some(LABEL.into()))
    };
    if let Err(e) = applied {
        discard();
        return Err(e.to_string());
    }
    state.sync_engine();
    state.sync_audio_pool();
    Ok(state.snapshot())
}

#[tauri::command]
pub fn cancel_sample_fetch(state: State<'_, AppState>) {
    state.sample_fetch.cancel();
}

#[cfg(test)]
mod tests {
    use super::*;
    use dissonant_core::SampleRegion;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dissonant-{tag}-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn only_http_links_with_a_host_are_accepted() {
        assert_eq!(
            validate_url("  https://www.youtube.com/watch?v=jNQXAC9IVRw \n").as_deref(),
            Ok("https://www.youtube.com/watch?v=jNQXAC9IVRw")
        );
        assert!(validate_url("HTTP://youtu.be/jNQXAC9IVRw").is_ok());
        for bad in [
            "",
            "youtube.com/watch?v=x",
            "file:///etc/passwd",
            "ftp://example.com/a.mp3",
            "-o/tmp/x",
            "https://",
            "https:///path",
            "https://example.com/a b",
            "https://example.com/\u{7}",
        ] {
            assert!(validate_url(bad).is_err(), "{bad:?} was accepted");
        }
    }

    #[test]
    fn progress_lines_parse() {
        assert_eq!(
            parse_line("META Me at the zoo|19|m4a"),
            Some(YtLine::Meta { title: "Me at the zoo".into(), duration: Some(19.0) })
        );
        assert_eq!(
            parse_line("META a | b | c|NA|webm"),
            Some(YtLine::Meta { title: "a | b | c".into(), duration: None })
        );
        assert_eq!(parse_line("PROG 130048 309288 NA"), Some(YtLine::Progress(Some(130048.0 / 309288.0))));
        assert_eq!(parse_line("PROG 100 NA 400.0"), Some(YtLine::Progress(Some(0.25))));
        assert_eq!(parse_line("PROG 100 NA NA"), Some(YtLine::Progress(None)));
        assert_eq!(parse_line("PROG 500 400 NA"), Some(YtLine::Progress(Some(1.0))));
        assert_eq!(
            parse_line("FILE /private/tmp/ytt/jNQXAC9IVRw.m4a"),
            Some(YtLine::File(PathBuf::from("/private/tmp/ytt/jNQXAC9IVRw.m4a")))
        );
        assert_eq!(parse_line("[youtube] Extracting URL"), None);
    }

    #[test]
    fn the_last_error_line_is_what_the_user_sees() {
        let stderr = "WARNING: [youtube] something minor\n\
                      ERROR: [youtube] abc: first\n\
                      ERROR: [youtube] jNQXAC9IVRw: Video unavailable. This video is private\n";
        assert_eq!(ytdlp_error(stderr).as_deref(), Some("Video unavailable. This video is private"));
        assert_eq!(
            ytdlp_error("ERROR: Unsupported URL: https://example.com/").as_deref(),
            Some("Unsupported URL: https://example.com/")
        );
        assert_eq!(
            ytdlp_error("ERROR: [generic] Unable to download webpage: HTTP Error 404").as_deref(),
            Some("Unable to download webpage: HTTP Error 404")
        );
        assert_eq!(ytdlp_error("WARNING: hm\nTraceback: boom\nWARNING: later\n").as_deref(), Some("Traceback: boom"));
        assert_eq!(ytdlp_error("  \n"), None);
    }

    #[test]
    fn checksums_are_found_by_exact_asset_name() {
        let a = "e51e69e89489f199530f13d32a4342da3ddb2cf34842f364603a066db05afbf3";
        let b = "0F922C15CCC0B83EFA1C8BC9104E434318C4BF16E408E1EE93B7745A6EFE0AA";
        let sums = format!("{a}  yt-dlp\n{b}0 *yt-dlp_macos\n20d98fa608998eff87d1d35a6e9fb65e4b874639a6cc89addc19da3becde3ef1  yt-dlp_macos.zip\nnot-a-hash  yt-dlp_linux\n");
        assert_eq!(checksum_for(&sums, "yt-dlp").as_deref(), Some(a));
        assert_eq!(checksum_for(&sums, "yt-dlp_macos"), Some(format!("{b}0").to_ascii_lowercase()));
        assert_eq!(checksum_for(&sums, "yt-dlp_linux"), None);
        assert_eq!(checksum_for(&sums, "yt-dlp.exe"), None);
    }

    #[test]
    fn every_supported_platform_has_its_release_asset() {
        assert_eq!(release_asset("macos", "aarch64"), Some("yt-dlp_macos"));
        assert_eq!(release_asset("macos", "x86_64"), Some("yt-dlp_macos"));
        assert_eq!(release_asset("linux", "x86_64"), Some("yt-dlp_linux"));
        assert_eq!(release_asset("linux", "aarch64"), Some("yt-dlp_linux_aarch64"));
        assert_eq!(release_asset("windows", "x86_64"), Some("yt-dlp.exe"));
        assert_eq!(release_asset("windows", "aarch64"), Some("yt-dlp_arm64.exe"));
        assert_eq!(release_asset("freebsd", "x86_64"), None);
    }

    #[test]
    fn analysis_maps_onto_the_model() {
        let mapped = to_sample_analysis(Analysis {
            bpm: Some(92.5),
            bpm_confidence: 0.7,
            key_root: Some(9),
            key_minor: true,
            key_confidence: 0.4,
            onsets: vec![(0.5, 1.0), (1.25, 0.3)],
        });
        assert_eq!(mapped.bpm, Some(92.5));
        assert_eq!(mapped.bpm_confidence, 0.7);
        assert_eq!(mapped.key_root, Some(9));
        assert_eq!(mapped.key_scale, ScaleType::Minor);
        assert_eq!(mapped.key_confidence, 0.4);
        assert_eq!(
            mapped.onsets,
            vec![Onset { seconds: 0.5, strength: 1.0 }, Onset { seconds: 1.25, strength: 0.3 }]
        );
        assert_eq!(to_sample_analysis(Analysis::default()).key_scale, ScaleType::Major);
    }

    #[test]
    fn a_fetched_source_becomes_a_sampler_chopped_inside_its_suggested_region() {
        let source = AudioSource {
            id: Uuid::new_v4(),
            name: "Me at the zoo".into(),
            frames: 48_000 * 12,
            sample_rate: 48_000,
            channels: 2,
            origin_url: Some("https://youtu.be/jNQXAC9IVRw".into()),
        };
        // No tempo: 8 s from the first strong hit (the 0.3 at 0.5 s is pre-roll).
        let analysis = SampleAnalysis {
            onsets: vec![
                Onset { seconds: 0.5, strength: 0.3 },
                Onset { seconds: 1.0, strength: 0.9 },
                Onset { seconds: 2.5, strength: 0.5 },
                Onset { seconds: 10.0, strength: 1.0 },
            ],
            ..SampleAnalysis::default()
        };
        let Command::AddSamplerTrack { name, source: added, sampler } = add_sampler_track(source.clone(), analysis.clone(), 4) else {
            panic!("not an addSamplerTrack");
        };
        assert_eq!(name, "Me at the zoo");
        assert_eq!(added.id, source.id);
        assert_eq!(sampler.source_id, source.id);
        assert_eq!(sampler.analysis, Some(analysis));
        assert_eq!(sampler.region, Some(SampleRegion { start_seconds: 1.0, end_seconds: 9.0 }));
        assert_eq!(sampler.chopper, Some(Chopper::Auto { count: 16 }));
        let bounds: Vec<(f64, f64)> = sampler.chops.iter().map(|c| (c.start_seconds, c.end_seconds)).collect();
        assert_eq!(bounds, vec![(1.0, 2.5), (2.5, 9.0)]);
    }

    #[test]
    fn channels_past_stereo_are_dropped() {
        let mut audio = DecodedAudio { sample_rate: 48_000, channels: 3, data: vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0] };
        keep_two_channels(&mut audio);
        assert_eq!(audio.channels, 2);
        assert_eq!(audio.data, vec![1.0, 2.0, 4.0, 5.0]);
        let mut mono = DecodedAudio { sample_rate: 48_000, channels: 1, data: vec![1.0, 2.0] };
        keep_two_channels(&mut mono);
        assert_eq!((mono.channels, mono.data), (1, vec![1.0, 2.0]));
    }

    #[cfg(unix)]
    #[test]
    fn our_own_bin_is_searched_last() {
        use std::os::unix::fs::PermissionsExt;
        let data = temp_dir("locate");
        let name = format!("dissonant-no-such-tool-{}", Uuid::new_v4().simple());
        assert_eq!(locate(&name, &data), None);
        let tool = data.join("bin").join(&name);
        fs::create_dir_all(tool.parent().unwrap()).unwrap();
        fs::write(&tool, "#!/bin/sh\n").unwrap();
        // Not executable yet: a half-installed download must not be mistaken for a tool.
        assert_eq!(locate(&name, &data), None);
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(locate(&name, &data), Some(tool));
        let _ = fs::remove_dir_all(&data);
    }

    #[cfg(unix)]
    #[test]
    fn a_cancel_kills_the_running_process() {
        let control = Arc::new(FetchControl::default());
        let canceller = Arc::clone(&control);
        let mut cmd = tool_command(Path::new("/bin/sh"));
        cmd.args(["-c", "echo started; sleep 30; echo never"]);
        let begun = std::time::Instant::now();
        let result = control.run(cmd, &mut |line| {
            assert_ne!(line, "never");
            canceller.cancel();
            Ok(())
        });
        assert_eq!(result.err().as_deref(), Some(CANCELLED));
        assert!(begun.elapsed() < std::time::Duration::from_secs(10));
    }

    #[cfg(unix)]
    #[test]
    fn an_error_from_a_line_stops_the_process() {
        let control = FetchControl::default();
        let mut cmd = tool_command(Path::new("/bin/sh"));
        cmd.args(["-c", "echo 'META long|3600|m4a'; sleep 30"]);
        let result = control.run(cmd, &mut |line| match parse_line(line) {
            Some(YtLine::Meta { duration: Some(d), .. }) if d > MAX_SAMPLE_SECONDS => Err(TOO_LONG.into()),
            _ => Ok(()),
        });
        assert_eq!(result.err().as_deref(), Some(TOO_LONG));
    }

    #[test]
    fn the_reporter_throttles_within_a_phase() {
        let control = FetchControl::default();
        let mut seen = Vec::new();
        let mut sink = |p: SampleFetchProgress| seen.push((p.phase, p.fraction));
        let mut reporter = Reporter { control: &control, sink: &mut sink, last: None };
        for i in 0..=1000 {
            reporter.report(SampleFetchPhase::Decoding, i as f64 / 1000.0);
        }
        reporter.report(SampleFetchPhase::Analyzing, -1.0);
        reporter.report(SampleFetchPhase::Analyzing, -1.0);
        assert!(seen.len() <= 103, "{} events", seen.len());
        assert_eq!(seen.iter().rev().nth(1), Some(&(SampleFetchPhase::Decoding, 1.0)));
        assert_eq!(seen.last(), Some(&(SampleFetchPhase::Analyzing, -1.0)));
    }

    #[test]
    fn progress_serialises_as_the_ui_expects() {
        let json = serde_json::to_value(SampleFetchProgress {
            phase: SampleFetchPhase::Downloading,
            fraction: 0.5,
            title: Some("t".into()),
            message: None,
        })
        .unwrap();
        assert_eq!(json, serde_json::json!({ "phase": "downloading", "fraction": 0.5, "title": "t", "message": null }));
    }

    /// The whole pipeline against the real network. Run with
    /// `cargo test -p dissonant-app sample_fetch -- --ignored --nocapture`.
    #[test]
    #[ignore = "downloads from YouTube"]
    fn fetches_a_real_video() {
        let data = temp_dir("fetch");
        let control = FetchControl::default();
        let mut phases: Vec<SampleFetchPhase> = Vec::new();
        let fetched = fetch("https://www.youtube.com/watch?v=jNQXAC9IVRw", &data, &control, &mut |p| {
            if phases.last() != Some(&p.phase) {
                println!("phase {:?} title {:?}", p.phase, p.title);
                phases.push(p.phase);
            }
        })
        .expect("fetch");
        let s = &fetched.source;
        let a = &fetched.analysis;
        println!(
            "source {:?}: {} frames @ {} Hz, {} ch ({:.2} s), origin {:?}",
            s.name,
            s.frames,
            s.sample_rate,
            s.channels,
            s.frames as f64 / s.sample_rate as f64,
            s.origin_url
        );
        println!(
            "analysis: bpm {:?} (conf {:.2}), key {:?} {:?} (conf {:.2}), {} onsets",
            a.bpm,
            a.bpm_confidence,
            a.key_root,
            a.key_scale,
            a.key_confidence,
            a.onsets.len()
        );
        assert!(audio_files::source_path(&audio_files::store_dir(&data), s.id).is_file());
        assert_eq!(fetched.buffer.frames as u64, s.frames);
        let Command::AddSamplerTrack { sampler, .. } = add_sampler_track(fetched.source.clone(), fetched.analysis.clone(), 4) else {
            unreachable!()
        };
        println!("chops: {}", sampler.chops.len());
        assert!(s.frames > 0 && !sampler.chops.is_empty());
        assert!(phases.contains(&SampleFetchPhase::Downloading) && phases.contains(&SampleFetchPhase::Analyzing));
        let _ = fs::remove_dir_all(&data);
    }

    /// The first-use installer against the real release. Run as above.
    #[test]
    #[ignore = "downloads yt-dlp from GitHub"]
    fn installs_the_standalone_build() {
        let data = temp_dir("install");
        let control = FetchControl::default();
        let mut sink = |p: SampleFetchProgress| assert_eq!(p.phase, SampleFetchPhase::Installing);
        let mut reporter = Reporter { control: &control, sink: &mut sink, last: None };
        let path = install_ytdlp(&data, &control, &mut reporter).expect("install");
        assert_eq!(path, data.join("bin").join(exe_name("yt-dlp")));
        let version = tool_command(&path).arg("--version").output().expect("run");
        println!("installed {} reports {}", path.display(), String::from_utf8_lossy(&version.stdout).trim());
        assert!(version.status.success());
        let _ = fs::remove_dir_all(&data);
    }
}
