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
    thread: Option<std::thread::JoinHandle<()>>,
    pub sample_rate: u32,
}

impl AudioDevice {
    /// Open the default output device and start rendering. `master` seeds the master chain.
    pub fn start(master: MasterSettings) -> Result<AudioDevice, DeviceError> {
        let (producer, consumer) = rtrb::RingBuffer::<EngineCommand>::new(1024);
        let shared = Arc::new(Shared::default());
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::channel::<Result<u32, DeviceError>>();

        let thread_shared = shared.clone();
        let thread_stop = stop.clone();
        let thread = std::thread::Builder::new()
            .name("dissonant-audio".into())
            .spawn(move || {
                let result = build_stream(consumer, thread_shared, master);
                match result {
                    Ok((stream, sample_rate)) => {
                        if let Err(e) = stream.play() {
                            let _ = ready_tx.send(Err(DeviceError::Cpal(e.to_string())));
                            return;
                        }
                        let _ = ready_tx.send(Ok(sample_rate));
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

        let sample_rate = ready_rx
            .recv()
            .map_err(|_| DeviceError::Cpal("audio thread died".into()))??;

        Ok(AudioDevice {
            producer: Mutex::new(producer),
            shared,
            stop,
            thread: Some(thread),
            sample_rate,
        })
    }

    pub fn shared(&self) -> &Arc<Shared> {
        &self.shared
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
) -> Result<(cpal::Stream, u32), DeviceError> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or(DeviceError::NoDevice)?;
    let config = device
        .default_output_config()
        .map_err(|e| DeviceError::Cpal(e.to_string()))?;
    let sample_rate = config.sample_rate();
    let channels = config.channels() as usize;
    let format = config.sample_format();
    let stream_config: cpal::StreamConfig = config.into();
    let engine = Engine::new(sample_rate as f32, shared, &master);

    let stream = match format {
        SampleFormat::F32 => make_stream::<f32>(&device, &stream_config, channels, engine, consumer),
        SampleFormat::I16 => make_stream::<i16>(&device, &stream_config, channels, engine, consumer),
        SampleFormat::U16 => make_stream::<u16>(&device, &stream_config, channels, engine, consumer),
        SampleFormat::I32 => make_stream::<i32>(&device, &stream_config, channels, engine, consumer),
        other => return Err(DeviceError::Cpal(format!("unsupported sample format {other:?}"))),
    }?;
    Ok((stream, sample_rate))
}

fn make_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    mut engine: Engine,
    mut consumer: rtrb::Consumer<EngineCommand>,
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
            |err| log::error!("audio stream error: {err}"),
            None,
        )
        .map_err(|e| DeviceError::Cpal(e.to_string()))
}
