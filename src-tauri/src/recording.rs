//! Writing recorded takes to disk.
//!
//! The engine pushes captured samples into a lock-free ring from the audio callback; this
//! thread drains it. Idle, it drains and discards so the ring never backs up and the engine
//! never has to care whether anyone is listening. Recording, it appends to a WAV.
//!
//! Mono, 32-bit float, at the device rate — the same format the audio store uses, so a take
//! needs no conversion to become a source.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Mutex;
use std::time::Duration;

enum TakeMsg {
    Start { path: PathBuf, sample_rate: u32 },
    Stop { reply: Sender<Result<u64, String>> },
    Shutdown,
}

/// How long the writer sleeps between drains. Short enough that a full ring never builds up,
/// long enough that an idle session costs nothing.
const DRAIN_INTERVAL: Duration = Duration::from_millis(10);

pub struct TakeWriter {
    tx: Mutex<Sender<TakeMsg>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl TakeWriter {
    /// Start draining `capture`. The thread lives until the writer is dropped.
    pub fn spawn(mut capture: rtrb::Consumer<f32>) -> TakeWriter {
        let (tx, rx) = mpsc::channel::<TakeMsg>();
        let thread = std::thread::Builder::new()
            .name("dissonant-takes".into())
            .spawn(move || run(&mut capture, rx))
            .ok();
        TakeWriter { tx: Mutex::new(tx), thread }
    }

    /// Begin a take at `path`. Anything already captured is discarded first, so a take starts
    /// at the sample the engine starts pushing and not at whatever was left in the ring.
    pub fn start(&self, path: PathBuf, sample_rate: u32) -> Result<(), String> {
        self.send(TakeMsg::Start { path, sample_rate })
    }

    /// Finish the take and return how many frames it holds.
    pub fn stop(&self) -> Result<u64, String> {
        let (reply, answer) = mpsc::channel();
        self.send(TakeMsg::Stop { reply })?;
        answer
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "the take writer stopped responding".to_string())?
    }

    fn send(&self, msg: TakeMsg) -> Result<(), String> {
        self.tx
            .lock()
            .map_err(|_| "the take writer is poisoned".to_string())?
            .send(msg)
            .map_err(|_| "the take writer is gone".to_string())
    }
}

impl Drop for TakeWriter {
    fn drop(&mut self) {
        let _ = self.send(TakeMsg::Shutdown);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

type Writer = hound::WavWriter<std::io::BufWriter<std::fs::File>>;

fn run(capture: &mut rtrb::Consumer<f32>, rx: Receiver<TakeMsg>) {
    let mut writer: Option<(Writer, u64)> = None;
    loop {
        match rx.recv_timeout(DRAIN_INTERVAL) {
            Ok(TakeMsg::Start { path, sample_rate }) => {
                // Discard the backlog: it predates the take.
                while capture.pop().is_ok() {}
                let spec = hound::WavSpec {
                    channels: 1,
                    sample_rate,
                    bits_per_sample: 32,
                    sample_format: hound::SampleFormat::Float,
                };
                match path.parent().map(std::fs::create_dir_all).unwrap_or(Ok(())).map_err(|e| e.to_string()) {
                    Ok(()) => match hound::WavWriter::create(&path, spec) {
                        Ok(w) => writer = Some((w, 0)),
                        Err(e) => log::error!("could not start a take at {}: {e}", path.display()),
                    },
                    Err(e) => log::error!("could not create the audio store: {e}"),
                }
            }
            Ok(TakeMsg::Stop { reply }) => {
                drain(capture, &mut writer);
                let result = match writer.take() {
                    Some((w, frames)) => w.finalize().map(|()| frames).map_err(|e| e.to_string()),
                    None => Ok(0),
                };
                let _ = reply.send(result);
            }
            Ok(TakeMsg::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                if let Some((w, _)) = writer.take() {
                    let _ = w.finalize();
                }
                return;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => drain(capture, &mut writer),
        }
    }
}

/// Move everything waiting in the ring into the take, or to the floor when idle.
fn drain(capture: &mut rtrb::Consumer<f32>, writer: &mut Option<(Writer, u64)>) {
    match writer {
        Some((w, frames)) => {
            while let Ok(s) = capture.pop() {
                if let Err(e) = w.write_sample(s) {
                    log::error!("a take stopped recording: {e}");
                    break;
                }
                *frames += 1;
            }
        }
        None => while capture.pop().is_ok() {},
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_wav() -> PathBuf {
        std::env::temp_dir()
            .join("dissonant-takes-test")
            .join(format!("{}.wav", uuid::Uuid::new_v4()))
    }

    /// Wait for the writer thread to drain; it polls on `DRAIN_INTERVAL`.
    fn settle() {
        std::thread::sleep(DRAIN_INTERVAL * 4);
    }

    #[test]
    fn a_take_lands_on_disk_with_what_was_captured() {
        let (mut producer, consumer) = rtrb::RingBuffer::<f32>::new(4096);
        let writer = TakeWriter::spawn(consumer);
        let path = temp_wav();

        // Anything already in the ring predates the take and must not end up in it.
        for _ in 0..100 {
            producer.push(0.9).unwrap();
        }
        settle();
        writer.start(path.clone(), 48_000).expect("start");
        settle();

        let wanted: Vec<f32> = (0..1_000).map(|n| (n as f32 / 1_000.0) - 0.5).collect();
        for &s in &wanted {
            producer.push(s).unwrap();
        }
        settle();
        let frames = writer.stop().expect("stop");
        assert_eq!(frames, wanted.len() as u64, "the take is the wrong length");

        let mut reader = hound::WavReader::open(&path).expect("the take was written");
        let spec = reader.spec();
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.sample_rate, 48_000);
        assert_eq!(spec.bits_per_sample, 32);
        let got: Vec<f32> = reader.samples::<f32>().map(Result::unwrap).collect();
        assert_eq!(got.len(), wanted.len());
        for (i, (a, b)) in wanted.iter().zip(got.iter()).enumerate() {
            assert!((a - b).abs() < 1e-6, "sample {i} changed: {a} became {b}");
        }

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_idle_writer_discards_the_ring_instead_of_letting_it_fill() {
        let (mut producer, consumer) = rtrb::RingBuffer::<f32>::new(256);
        let _writer = TakeWriter::spawn(consumer);
        // Far more than the ring holds. If nothing drained, this would stop pushing at 256.
        let mut pushed = 0;
        for _ in 0..20 {
            for _ in 0..200 {
                if producer.push(0.1).is_ok() {
                    pushed += 1;
                }
            }
            settle();
        }
        assert!(pushed > 1_000, "the idle writer let the ring back up after {pushed} samples");
    }

    #[test]
    fn stopping_without_starting_is_a_no_op_rather_than_an_error() {
        let (_producer, consumer) = rtrb::RingBuffer::<f32>::new(16);
        let writer = TakeWriter::spawn(consumer);
        assert_eq!(writer.stop().expect("stop"), 0);
    }

    #[test]
    fn a_second_take_does_not_carry_the_first_ones_audio() {
        let (mut producer, consumer) = rtrb::RingBuffer::<f32>::new(4096);
        let writer = TakeWriter::spawn(consumer);

        let first = temp_wav();
        writer.start(first.clone(), 48_000).expect("start");
        settle();
        for _ in 0..500 {
            producer.push(0.5).unwrap();
        }
        settle();
        assert_eq!(writer.stop().expect("stop"), 500);

        let second = temp_wav();
        writer.start(second.clone(), 48_000).expect("start");
        settle();
        for _ in 0..300 {
            producer.push(-0.25).unwrap();
        }
        settle();
        assert_eq!(writer.stop().expect("stop"), 300);

        let samples: Vec<f32> = hound::WavReader::open(&second)
            .expect("the second take exists")
            .samples::<f32>()
            .map(Result::unwrap)
            .collect();
        assert_eq!(samples.len(), 300);
        assert!(samples.iter().all(|s| (*s + 0.25).abs() < 1e-6), "the second take holds the first's audio");

        let _ = std::fs::remove_file(&first);
        let _ = std::fs::remove_file(&second);
    }
}
