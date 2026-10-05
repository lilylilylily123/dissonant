//! Real-time output through cpal. The stream lives on its own thread (cpal streams aren't
//! `Send` on every platform), and the application talks to the engine through a lock-free
//! ring buffer plus the [`Shared`] atomics.

use crate::engine::{Engine, EngineCommand, Shared};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SizedSample};
use dissonant_core::MasterSettings;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DeviceError {
    #[error("no default output device")]
    NoDevice,
    #[error("audio device error: {0}")]
    Cpal(String),
    #[error("engine command queue is full")]
    QueueFull,
}

/// Handle to a running output stream. Dropping it stops audio.
pub struct AudioDevice {
    producer: Mutex<rtrb::Producer<EngineCommand>>,
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
    /// Set from the stream's error callback (device unplugged, format lost). The application
    /// polls it and restarts on the current default device.
    failed: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    pub sample_rate: u32,
    /// Name of the device the stream was opened on.
    pub device_name: String,
}

/// The name of the host's current default output device, if any.
pub fn default_output_name() -> Option<String> {
    let device = cpal::default_host().default_output_device()?;
    Some(device_name(&device))
}

fn device_name(device: &cpal::Device) -> String {
    device
        .description()
        .map(|d| d.name().to_string())
        .unwrap_or_else(|_| "output".into())
}

impl AudioDevice {
    /// Open the default output device and start rendering. `master` seeds the master chain.
    pub fn start(master: MasterSettings) -> Result<AudioDevice, DeviceError> {
        let (producer, consumer) = rtrb::RingBuffer::<EngineCommand>::new(1024);
        let shared = Arc::new(Shared::default());
        let stop = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(u32, String), DeviceError>>();

        let thread_shared = shared.clone();
        let thread_stop = stop.clone();
        let thread_failed = failed.clone();
        let thread = std::thread::Builder::new()
            .name("dissonant-audio".into())
            .spawn(move || {
                let result = build_stream(consumer, thread_shared, master, thread_failed);
                match result {
                    Ok((stream, sample_rate, name)) => {
                        if let Err(e) = stream.play() {
                            let _ = ready_tx.send(Err(DeviceError::Cpal(e.to_string())));
                            return;
                        }
                        let _ = ready_tx.send(Ok((sample_rate, name)));
                        while !thread_stop.load(Ordering::Relaxed) {
                            std::thread::park_timeout(std::time::Duration::from_millis(250));
                        }
                        drop(stream);
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                    }
                }
            })
            .map_err(|e| DeviceError::Cpal(e.to_string()))?;

        let (sample_rate, device_name) = ready_rx
            .recv()
            .map_err(|_| DeviceError::Cpal("audio thread died".into()))??;

        Ok(AudioDevice {
            producer: Mutex::new(producer),
            shared,
            stop,
            failed,
            thread: Some(thread),
            sample_rate,
            device_name,
        })
    }

    pub fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }

    /// True once the stream reported an error (e.g. the device went away).
    pub fn has_failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }

    pub fn send(&self, cmd: EngineCommand) -> Result<(), DeviceError> {
        let mut p = self.producer.lock().map_err(|_| DeviceError::QueueFull)?;
        p.push(cmd).map_err(|_| DeviceError::QueueFull)
    }
}

impl Drop for AudioDevice {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            t.thread().unpark();
            let _ = t.join();
        }
    }
}

fn build_stream(
    consumer: rtrb::Consumer<EngineCommand>,
    shared: Arc<Shared>,
    master: MasterSettings,
    failed: Arc<AtomicBool>,
) -> Result<(cpal::Stream, u32, String), DeviceError> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or(DeviceError::NoDevice)?;
    let name = device_name(&device);
    let config = device
        .default_output_config()
        .map_err(|e| DeviceError::Cpal(e.to_string()))?;
    let sample_rate = config.sample_rate();
    let channels = config.channels() as usize;
    let format = config.sample_format();
    let stream_config: cpal::StreamConfig = config.into();
    let engine = Engine::new(sample_rate as f32, shared, &master);

    let stream = match format {
        SampleFormat::F32 => make_stream::<f32>(&device, &stream_config, channels, engine, consumer, failed),
        SampleFormat::I16 => make_stream::<i16>(&device, &stream_config, channels, engine, consumer, failed),
        SampleFormat::U16 => make_stream::<u16>(&device, &stream_config, channels, engine, consumer, failed),
        SampleFormat::I32 => make_stream::<i32>(&device, &stream_config, channels, engine, consumer, failed),
        other => return Err(DeviceError::Cpal(format!("unsupported sample format {other:?}"))),
    }?;
    Ok((stream, sample_rate, name))
}

fn make_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    mut engine: Engine,
    mut consumer: rtrb::Consumer<EngineCommand>,
    failed: Arc<AtomicBool>,
) -> Result<cpal::Stream, DeviceError>
where
    T: SizedSample + cpal::FromSample<f32>,
{
    let mut stereo: Vec<f32> = vec![0.0; 8192];
    device
        .build_output_stream(
            *config,
            move |data: &mut [T], _| {
                while let Ok(cmd) = consumer.pop() {
                    engine.handle(cmd);
                }
                let frames = data.len() / channels.max(1);
                if stereo.len() < frames * 2 {
                    stereo.resize(frames * 2, 0.0);
                }
                engine.process(&mut stereo[..frames * 2]);
                for (i, frame) in data.chunks_mut(channels.max(1)).enumerate() {
                    let (l, r) = (stereo[2 * i], stereo[2 * i + 1]);
                    match frame.len() {
                        1 => frame[0] = T::from_sample((l + r) * 0.5),
                        _ => {
                            frame[0] = T::from_sample(l);
                            frame[1] = T::from_sample(r);
                            for s in &mut frame[2..] {
                                *s = T::from_sample(0.0);
                            }
                        }
                    }
                }
            },
            move |err| {
                log::error!("audio stream error: {err}");
                failed.store(true, Ordering::Relaxed);
            },
            None,
        )
        .map_err(|e| DeviceError::Cpal(e.to_string()))
}
