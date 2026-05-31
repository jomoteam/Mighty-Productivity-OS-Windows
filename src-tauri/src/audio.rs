use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use hound::{SampleFormat as HoundFormat, WavSpec, WavWriter};
use std::io::Cursor;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub fn list_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    host.input_devices()
        .map(|devs| devs.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}

pub struct AudioRecorder {
    is_recording: Arc<Mutex<bool>>,
    buffer: Arc<Mutex<Vec<f32>>>,
    sample_rate: Arc<Mutex<u32>>,
    channels: Arc<Mutex<u16>>,
    preferred_device: Arc<Mutex<Option<String>>>,
    // Persistent stream thread: stays alive so the mic never sleeps between recordings
    stream_active: Arc<Mutex<bool>>,
}

impl AudioRecorder {
    pub fn new() -> Self {
        Self {
            is_recording: Arc::new(Mutex::new(false)),
            buffer: Arc::new(Mutex::new(Vec::new())),
            sample_rate: Arc::new(Mutex::new(44_100)),
            channels: Arc::new(Mutex::new(1)),
            preferred_device: Arc::new(Mutex::new(None)),
            stream_active: Arc::new(Mutex::new(false)),
        }
    }

    pub fn set_preferred_device(&self, name: Option<String>) {
        // Kill the current stream thread so next start_recording spawns a fresh one
        *self.stream_active.lock().unwrap() = false;
        thread::sleep(Duration::from_millis(150));
        *self.preferred_device.lock().unwrap() = name;
    }

    pub fn start_recording(&self) -> Result<(), String> {
        self.buffer.lock().unwrap().clear();
        *self.is_recording.lock().unwrap() = true;

        // If the persistent stream thread isn't running, start it
        let mut active = self.stream_active.lock().unwrap();
        if !*active {
            *active = true;
            drop(active);
            self.spawn_stream_thread();
        }

        Ok(())
    }

    fn spawn_stream_thread(&self) {
        let is_recording = Arc::clone(&self.is_recording);
        let buffer = Arc::clone(&self.buffer);
        let sample_rate_arc = Arc::clone(&self.sample_rate);
        let channels_arc = Arc::clone(&self.channels);
        let preferred_device = self.preferred_device.lock().unwrap().clone();
        let stream_active = Arc::clone(&self.stream_active);

        thread::spawn(move || {
            let host = cpal::default_host();
            let device = if let Some(ref name) = preferred_device {
                host.input_devices()
                    .ok()
                    .and_then(|mut devs| devs.find(|d| d.name().ok().as_deref() == Some(name)))
                    .or_else(|| host.default_input_device())
            } else {
                host.default_input_device()
            };
            let device = match device {
                Some(dev) => dev,
                None => {
                    eprintln!("No input device found");
                    return;
                }
            };

            let config = match device.default_input_config() {
                Ok(cfg) => cfg,
                Err(err) => {
                    eprintln!("Failed to read input config: {err}");
                    return;
                }
            };

            let stream_config: cpal::StreamConfig = config.clone().into();
            *sample_rate_arc.lock().unwrap() = stream_config.sample_rate.0;
            *channels_arc.lock().unwrap() = stream_config.channels;

            // Warmup: discard first 200ms to let the device stabilize on first open
            let warmup_total = (stream_config.sample_rate.0 as f32 * 0.2) as usize
                * stream_config.channels as usize;
            let warmup_count = Arc::new(Mutex::new(0usize));

            let err_fn = |err| eprintln!("audio stream error: {}", err);

            let stream = match config.sample_format() {
                SampleFormat::F32 => {
                    let wc = Arc::clone(&warmup_count);
                    let buf = Arc::clone(&buffer);
                    let rec = Arc::clone(&is_recording);
                    device.build_input_stream(
                        &stream_config,
                        move |data: &[f32], _: &_| {
                            let mut wc = wc.lock().unwrap();
                            if *wc < warmup_total {
                                *wc += data.len();
                                return;
                            }
                            if *rec.lock().unwrap() {
                                buf.lock().unwrap().extend_from_slice(data);
                            }
                        },
                        err_fn,
                        None,
                    )
                }
                SampleFormat::I16 => {
                    let wc = Arc::clone(&warmup_count);
                    let buf = Arc::clone(&buffer);
                    let rec = Arc::clone(&is_recording);
                    device.build_input_stream(
                        &stream_config,
                        move |data: &[i16], _: &_| {
                            let mut wc = wc.lock().unwrap();
                            if *wc < warmup_total {
                                *wc += data.len();
                                return;
                            }
                            if *rec.lock().unwrap() {
                                let mut b = buf.lock().unwrap();
                                b.extend(data.iter().map(|s| *s as f32 / i16::MAX as f32));
                            }
                        },
                        err_fn,
                        None,
                    )
                }
                SampleFormat::U16 => {
                    let wc = Arc::clone(&warmup_count);
                    let buf = Arc::clone(&buffer);
                    let rec = Arc::clone(&is_recording);
                    device.build_input_stream(
                        &stream_config,
                        move |data: &[u16], _: &_| {
                            let mut wc = wc.lock().unwrap();
                            if *wc < warmup_total {
                                *wc += data.len();
                                return;
                            }
                            if *rec.lock().unwrap() {
                                let mut b = buf.lock().unwrap();
                                b.extend(
                                    data.iter()
                                        .map(|s| (*s as f32 / u16::MAX as f32) * 2.0 - 1.0),
                                );
                            }
                        },
                        err_fn,
                        None,
                    )
                }
                _ => {
                    eprintln!("Unsupported sample format");
                    return;
                }
            };

            let stream = match stream {
                Ok(s) => s,
                Err(err) => {
                    eprintln!("Failed to build input stream: {err}");
                    return;
                }
            };

            if let Err(err) = stream.play() {
                eprintln!("Failed to play stream: {err}");
                return;
            }

            // Keep stream alive until explicitly told to stop
            while *stream_active.lock().unwrap() {
                thread::sleep(Duration::from_millis(100));
            }
            // stream dropped here — Core Audio releases the device
        });
    }

    pub fn stop_recording_and_get_wav(&self) -> Result<Vec<u8>, String> {
        *self.is_recording.lock().unwrap() = false;
        thread::sleep(Duration::from_millis(100)); // let last callback flush

        let data = self.buffer.lock().unwrap().clone();
        if data.is_empty() {
            return Err("No audio recorded".to_string());
        }

        let channels = *self.channels.lock().unwrap();
        let sample_rate = *self.sample_rate.lock().unwrap();

        let mono_data: Vec<i16> = if channels > 1 {
            data.chunks(channels as usize)
                .map(|frame| {
                    let avg = frame.iter().sum::<f32>() / channels as f32;
                    (avg.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
                })
                .collect()
        } else {
            data.iter()
                .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                .collect()
        };

        let spec = WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: HoundFormat::Int,
        };

        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = WavWriter::new(&mut cursor, spec).map_err(|e| e.to_string())?;
            for sample in mono_data {
                writer.write_sample(sample).map_err(|e| e.to_string())?;
            }
            writer.finalize().map_err(|e| e.to_string())?;
        }

        Ok(cursor.into_inner())
    }
}
