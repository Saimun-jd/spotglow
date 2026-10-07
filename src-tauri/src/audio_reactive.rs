use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use realfft::RealFftPlanner;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tracing::{info, warn};

use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

/// Real-time audio reactivity metrics broadcast to the overlay frontend at ~60 FPS.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AudioBeatPayload {
    /// Normalized bass energy (25 Hz - 250 Hz kick drum & bass zone)
    pub bass: f32,
    /// Normalized mid-frequency energy (250 Hz - 2500 Hz rhythm/snare/vocals)
    pub mid: f32,
    /// Normalized treble energy (2500 Hz - 16000 Hz hi-hats/clarity)
    pub treble: f32,
    /// Overall normalized loudness / RMS volume: 0.0 .. 1.0
    pub volume: f32,
    /// Beat pulse intensity: peaks on detected beat, decays smoothly: 0.0 .. 1.0
    pub beat: f32,
    /// True on the exact frame where a transient beat onset is triggered
    pub is_beat: bool,
    /// Estimated tempo in beats per minute, or 0.0 if not locked
    pub bpm: f32,
}

impl Default for AudioBeatPayload {
    fn default() -> Self {
        Self {
            bass: 0.0,
            mid: 0.0,
            treble: 0.0,
            volume: 0.0,
            beat: 0.0,
            is_beat: false,
            bpm: 0.0,
        }
    }
}

pub struct AudioReactiveEngine {
    running: Arc<AtomicBool>,
    latest_state: Arc<Mutex<AudioBeatPayload>>,
}

impl AudioReactiveEngine {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            latest_state: Arc::new(Mutex::new(AudioBeatPayload::default())),
        }
    }

    /// Spawns the dedicated background audio capture and DSP thread.
    pub fn start(&self, app_handle: AppHandle) {
        if self.running.swap(true, Ordering::SeqCst) {
            return; // Already running
        }

        let running = Arc::clone(&self.running);
        let latest_state = Arc::clone(&self.latest_state);

        std::thread::Builder::new()
            .name("spotglow-audio-engine".to_string())
            .spawn(move || {
                info!("[SpotGlow Audio] Initializing native Windows WASAPI loopback capture & DSP engine...");
                unsafe {
                    run_audio_capture_thread(app_handle, running, latest_state);
                }
            })
            .expect("failed to spawn audio-engine thread");
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    pub fn get_latest(&self) -> AudioBeatPayload {
        *self.latest_state.lock().unwrap()
    }
}

const FFT_SIZE: usize = 1024;
const HOP_SIZE: usize = 512;

/// Dedicated WASAPI loopback capture and real-time DSP loop running on a single high-priority thread.
/// Keeping all Win32 COM interfaces and event handles strictly on this thread avoids any thread-safety / Send violations.
unsafe fn run_audio_capture_thread(
    app_handle: AppHandle,
    running: Arc<AtomicBool>,
    latest_state: Arc<Mutex<AudioBeatPayload>>,
) {
    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

    while running.load(Ordering::SeqCst) {
        let setup_res = (|| -> Result<_, Box<dyn std::error::Error>> {
            let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
            let id = device.GetId()?.to_string();
            info!("[SpotGlow Audio] Connected to default render endpoint: {:?}", id);

            let audio_client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
            let pwfx = audio_client.GetMixFormat()?;
            let channels = (*pwfx).nChannels as usize;
            let sample_rate = (*pwfx).nSamplesPerSec;
            let bits_per_sample = (*pwfx).wBitsPerSample;

            let event_handle = CreateEventW(None, false, false, None)?;

            // 50ms buffer in 100-nanosecond units
            let buffer_duration = 50 * 10000;
            audio_client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                buffer_duration,
                0,
                pwfx,
                None,
            )?;

            audio_client.SetEventHandle(event_handle)?;
            let capture_client: IAudioCaptureClient = audio_client.GetService()?;
            audio_client.Start()?;

            Ok((audio_client, capture_client, event_handle, sample_rate, channels, bits_per_sample))
        })();

        let (audio_client, capture_client, event_handle, sample_rate, channels, bits_per_sample) = match setup_res {
            Ok(tuple) => tuple,
            Err(e) => {
                warn!("[SpotGlow Audio] Failed to initialize WASAPI loopback: {:?}. Retrying in 2s...", e);
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }
        };

        info!(
            "[SpotGlow Audio] WASAPI Loopback active ({} Hz, {} channels, {} bit). Starting real-time DSP.",
            sample_rate, channels, bits_per_sample
        );

        // Run DSP processing loop with direct audio packet pulling
        run_dsp_stream(
            &capture_client,
            event_handle,
            sample_rate,
            channels,
            bits_per_sample,
            &app_handle,
            &running,
            &latest_state,
        );

        let _ = audio_client.Stop();
        let _ = CloseHandle(event_handle);

        std::thread::sleep(Duration::from_millis(500));
    }

    CoUninitialize();
    info!("[SpotGlow Audio] Audio engine thread exiting.");
}

unsafe fn run_dsp_stream(
    capture_client: &IAudioCaptureClient,
    event_handle: windows::Win32::Foundation::HANDLE,
    sample_rate: u32,
    channels: usize,
    bits_per_sample: u16,
    app_handle: &AppHandle,
    running: &Arc<AtomicBool>,
    latest_state: &Arc<Mutex<AudioBeatPayload>>,
) {
    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = planner.plan_fft_forward(FFT_SIZE);

    let mut input_buffer = vec![0.0f32; FFT_SIZE];
    let mut fft_input = vec![0.0f32; FFT_SIZE];
    let mut fft_output = r2c.make_output_vec();

    // Hann window
    let mut hann = vec![0.0f32; FFT_SIZE];
    for (i, h) in hann.iter_mut().enumerate() {
        *h = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / FFT_SIZE as f32).cos());
    }

    let bin_freq = sample_rate as f32 / FFT_SIZE as f32;
    // Low-end Bass & Kick zone (25 Hz - 250 Hz): includes kick thump and bass guitar body
    let bass_start = (25.0 / bin_freq).max(1.0) as usize;
    let bass_end = (250.0 / bin_freq).min((FFT_SIZE / 2) as f32) as usize;

    // Mid punch & snare zone (250 Hz - 2500 Hz): includes snare hits, rhythm guitar, synths
    let mid_start = bass_end;
    let mid_end = (2500.0 / bin_freq).min((FFT_SIZE / 2) as f32) as usize;

    // Treble clarity zone (2500 Hz - 16000 Hz)
    let treble_start = mid_end;
    let treble_end = (16000.0 / bin_freq).min((FFT_SIZE / 2) as f32) as usize;

    let mut prev_bass_energy = 0.0f32;
    let mut prev_mid_energy = 0.0f32;
    let mut flux_history = VecDeque::<f32>::with_capacity(20);
    let mut last_beat_instant = Instant::now();
    let mut beat_intervals = VecDeque::<f32>::with_capacity(8);

    // Adaptive Automatic Gain Control (AGC) state for songs with subtle or quiet mastering
    let mut rolling_peak_volume = 0.05f32;
    let mut rolling_peak_bass = 0.01f32;
    let mut rolling_peak_mid = 0.01f32;
    let mut rolling_peak_treble = 0.01f32;

    let mut current_beat = 0.0f32;
    let mut smoothed_bass = 0.0f32;
    let mut smoothed_mid = 0.0f32;
    let mut smoothed_treble = 0.0f32;
    let mut smoothed_volume = 0.0f32;

    let mut last_emit_instant = Instant::now();
    let emit_interval = Duration::from_millis(16); // ~60 Hz emit rate

    let mut samples_collected = 0usize;

    while running.load(Ordering::SeqCst) {
        // Wait up to 35ms for the next Windows audio buffer packet
        let wait_res = WaitForSingleObject(event_handle, 35);

        if wait_res == WAIT_OBJECT_0 {
            let mut p_data = std::ptr::null_mut();
            let mut num_frames = 0u32;
            let mut flags = 0u32;

            while capture_client.GetNextPacketSize().unwrap_or(0) > 0 {
                if capture_client
                    .GetBuffer(&mut p_data, &mut num_frames, &mut flags, None, None)
                    .is_ok()
                {
                    if num_frames > 0 && !p_data.is_null() {
                        let is_silent = (flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32)) != 0;

                        // Helper closure to process a mono sample into the FFT window buffer
                        let mut process_sample = |mono: f32| {
                            input_buffer[samples_collected] = mono;
                            samples_collected += 1;

                            if samples_collected >= FFT_SIZE {
                                // 1. Hann windowing
                                for i in 0..FFT_SIZE {
                                    fft_input[i] = input_buffer[i] * hann[i];
                                }

                                // 2. Forward FFT
                                if r2c.process(&mut fft_input, &mut fft_output).is_ok() {
                                    let fft_norm = (FFT_SIZE as f32) / 2.0;
                                    let mut magnitudes = Vec::with_capacity(fft_output.len());
                                    let mut sum_squares = 0.0f32;

                                    for c in &fft_output {
                                        let mag = (c.re * c.re + c.im * c.im).sqrt() / fft_norm;
                                        magnitudes.push(mag);
                                    }

                                    for s in &input_buffer {
                                        sum_squares += s * s;
                                    }
                                    let raw_volume = (sum_squares / FFT_SIZE as f32).sqrt().min(1.0);

                                    let raw_bass = if bass_end > bass_start {
                                        magnitudes[bass_start..=bass_end].iter().sum::<f32>()
                                            / (bass_end - bass_start + 1) as f32
                                    } else {
                                        0.0
                                    };

                                    let raw_mid = if mid_end > mid_start {
                                        magnitudes[mid_start..=mid_end].iter().sum::<f32>()
                                            / (mid_end - mid_start + 1) as f32
                                    } else {
                                        0.0
                                    };

                                    let raw_treble = if treble_end > treble_start {
                                        magnitudes[treble_start..=treble_end].iter().sum::<f32>()
                                            / (treble_end - treble_start + 1) as f32
                                    } else {
                                        0.0
                                    };

                                    // Dynamic AGC tracking: smoothly adapt to track loudness
                                    // Slow release (~2.5s) allows both soft rock (The Cure) and heavy EDM to dynamically calibrate
                                    rolling_peak_volume = (rolling_peak_volume * 0.996).max(raw_volume).max(0.008);
                                    rolling_peak_bass = (rolling_peak_bass * 0.996).max(raw_bass).max(0.001);
                                    rolling_peak_mid = (rolling_peak_mid * 0.996).max(raw_mid).max(0.001);
                                    rolling_peak_treble = (rolling_peak_treble * 0.996).max(raw_treble).max(0.001);

                                    // Fully normalized band energies (0.0 .. 1.0)
                                    let norm_bass = (raw_bass / (rolling_peak_bass * 1.15)).clamp(0.0, 1.0);
                                    let norm_mid = (raw_mid / (rolling_peak_mid * 1.15)).clamp(0.0, 1.0);
                                    let norm_treble = (raw_treble / (rolling_peak_treble * 1.15)).clamp(0.0, 1.0);
                                    let norm_volume = (raw_volume / (rolling_peak_volume * 1.10)).clamp(0.0, 1.0);

                                    // Multi-band Onset Spectral Flux: 75% low-end kick/bass + 25% mid-frequency snare/rhythm
                                    let bass_diff = (norm_bass - prev_bass_energy).max(0.0);
                                    let mid_diff = (norm_mid - prev_mid_energy).max(0.0);
                                    prev_bass_energy = norm_bass;
                                    prev_mid_energy = norm_mid;

                                    let onset_flux = bass_diff * 0.75 + mid_diff * 0.25;

                                    flux_history.push_back(onset_flux);
                                    if flux_history.len() > 16 {
                                        flux_history.pop_front();
                                    }

                                    let avg_flux = if !flux_history.is_empty() {
                                        flux_history.iter().sum::<f32>() / flux_history.len() as f32
                                    } else {
                                        0.0
                                    };

                                    // Adaptive beat detection limit:
                                    // Multiplier 1.18x over local moving average flux with very low floor (0.003)
                                    // Never locks out subtle tracks or 80s rock
                                    let threshold = (avg_flux * 1.18).max(0.003);
                                    let now = Instant::now();
                                    let time_since_last_beat = now.duration_since(last_beat_instant);

                                    // 160ms minimum interval (~375 BPM ceiling) to prevent false double-hits
                                    let is_beat = onset_flux > threshold
                                        && norm_volume > 0.05
                                        && time_since_last_beat >= Duration::from_millis(160);

                                    if is_beat {
                                        // Pulse punch scales with onset impact: 0.65 floor up to 1.0 peak
                                        let punch = (onset_flux / (threshold + 0.001)).clamp(0.65, 1.0);
                                        // Smooth attack rather than abrupt snap
                                        current_beat = current_beat * 0.20 + punch * 0.80;

                                        let interval_sec = time_since_last_beat.as_secs_f32();
                                        last_beat_instant = now;

                                        // Update BPM estimate if interval is within musical range (50 - 200 BPM)
                                        if interval_sec >= 0.30 && interval_sec <= 1.20 {
                                            beat_intervals.push_back(interval_sec);
                                            if beat_intervals.len() > 6 {
                                                beat_intervals.pop_front();
                                            }
                                        }
                                    } else {
                                        // Musical smooth exponential decay (~220ms release)
                                        current_beat = (current_beat * 0.91).max(0.0);
                                    }

                                    // Visual smoothing with gentle low-pass filtering
                                    smoothed_bass = smoothed_bass * 0.70 + norm_bass * 0.30;
                                    smoothed_mid = smoothed_mid * 0.74 + norm_mid * 0.26;
                                    smoothed_treble = smoothed_treble * 0.75 + norm_treble * 0.25;
                                    smoothed_volume = smoothed_volume * 0.68 + norm_volume * 0.32;

                                    let estimated_bpm = if beat_intervals.len() >= 3 {
                                        let avg_interval =
                                            beat_intervals.iter().sum::<f32>() / beat_intervals.len() as f32;
                                        (60.0 / avg_interval).round().clamp(60.0, 180.0)
                                    } else {
                                        0.0
                                    };

                                    let payload = AudioBeatPayload {
                                        bass: (smoothed_bass * 100.0).round() / 100.0,
                                        mid: (smoothed_mid * 100.0).round() / 100.0,
                                        treble: (smoothed_treble * 100.0).round() / 100.0,
                                        volume: (smoothed_volume * 100.0).round() / 100.0,
                                        beat: (current_beat * 100.0).round() / 100.0,
                                        is_beat,
                                        bpm: estimated_bpm,
                                    };

                                    *latest_state.lock().unwrap() = payload;

                                    if now.duration_since(last_emit_instant) >= emit_interval {
                                        last_emit_instant = now;
                                        let _ = app_handle.emit("audio_beat", &payload);
                                    }
                                }

                                // Slide buffer by HOP_SIZE
                                input_buffer.copy_within(HOP_SIZE..FFT_SIZE, 0);
                                samples_collected = FFT_SIZE - HOP_SIZE;
                            }
                        };

                        if is_silent {
                            for _ in 0..num_frames {
                                process_sample(0.0);
                            }
                        } else if bits_per_sample == 32 {
                            let total_samples = (num_frames as usize) * channels;
                            let float_slice =
                                std::slice::from_raw_parts(p_data as *const f32, total_samples);
                            for frame in float_slice.chunks_exact(channels) {
                                let mono: f32 = frame.iter().sum::<f32>() / channels as f32;
                                process_sample(mono);
                            }
                        } else if bits_per_sample == 16 {
                            let total_samples = (num_frames as usize) * channels;
                            let i16_slice =
                                std::slice::from_raw_parts(p_data as *const i16, total_samples);
                            for frame in i16_slice.chunks_exact(channels) {
                                let mono: f32 = frame.iter().map(|&s| s as f32 / 32768.0).sum::<f32>()
                                    / channels as f32;
                                process_sample(mono);
                            }
                        }

                        let _ = capture_client.ReleaseBuffer(num_frames);
                    }
                } else {
                    break;
                }
            }
        } else if wait_res == WAIT_TIMEOUT {
            // Audio device is idle or paused (no application outputting sound)
            current_beat = (current_beat * 0.80).max(0.0);
            smoothed_bass = (smoothed_bass * 0.80).max(0.0);
            smoothed_mid = (smoothed_mid * 0.80).max(0.0);
            smoothed_treble = (smoothed_treble * 0.80).max(0.0);
            smoothed_volume = (smoothed_volume * 0.80).max(0.0);

            let payload = AudioBeatPayload {
                bass: smoothed_bass,
                mid: smoothed_mid,
                treble: smoothed_treble,
                volume: smoothed_volume,
                beat: current_beat,
                is_beat: false,
                bpm: 0.0,
            };

            *latest_state.lock().unwrap() = payload;

            let now = Instant::now();
            if now.duration_since(last_emit_instant) >= emit_interval {
                last_emit_instant = now;
                let _ = app_handle.emit("audio_beat", &payload);
            }
        }
    }
}
