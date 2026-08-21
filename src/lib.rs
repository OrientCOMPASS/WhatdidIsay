#![allow(non_snake_case, non_camel_case_types)]

const CHUNK: usize = 65536; // 64KB
const RING_CAP: usize = 2 * 1024 * 1024; // 2MB
const VAD_WINDOW_BYTES: usize = 12288;
const WAKE_THRESHOLD: usize = VAD_WINDOW_BYTES * 3; 

use chrono::prelude::*;
use sherpa_onnx::{
    OfflineModelConfig, OfflineQwen3ASRModelConfig, OfflineRecognizer, OfflineRecognizerConfig,
    SileroVadModelConfig, VadModelConfig, VoiceActivityDetector,
};
use std::cell::UnsafeCell;
use std::ffi::{c_char, c_void, CStr, CString};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Condvar, Mutex, OnceLock,
};
use std::time::{Duration, SystemTime};

// ==================== ABI ====================
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum mpl_result_t {
    MPL_OK = 0, MPL_ERR_NOT_IMPLEMENTED = 1, MPL_ERR_INVALID_ARG = 2,
    MPL_ERR_RUNTIME = 3, MPL_ERR_BUFFER_TOO_SMALL = 4, MPL_ERR_PERMISSION = 5,
}

#[repr(C)]
pub struct mpl_plugin_info_t {
    pub abi_version: u32, pub api_version: u32,
    pub id: *const c_char, pub version: *const c_char,
}
unsafe impl Sync for mpl_plugin_info_t {}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct mpl_host_api_t {
    pub log: unsafe extern "C" fn(*mut c_void, i32, *const c_char),
    pub get_config: unsafe extern "C" fn(*mut c_void, *const c_char, *mut c_char, *mut u32) -> mpl_result_t,
    pub set_config: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> mpl_result_t,
    pub emit_event: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> mpl_result_t,
    pub send_message: unsafe extern "C" fn(*mut c_void, *const c_char, *const u8, u32) -> mpl_result_t,
    pub audio_state: unsafe extern "C" fn(*mut c_void, *mut c_char, *mut u32) -> mpl_result_t,
    pub connected_devices: unsafe extern "C" fn(*mut c_void, *mut c_char, *mut u32) -> mpl_result_t,
    pub ctx: *mut c_void,
    pub play_sound: unsafe extern "C" fn(*mut c_void, *const c_char) -> mpl_result_t,
    pub plugin_dir: unsafe extern "C" fn(*mut c_void, *mut c_char, *mut u32) -> mpl_result_t,
    pub register_hotkey: unsafe extern "C" fn(*mut c_void, *const c_char, *mut u64) -> mpl_result_t,
    pub open_window: unsafe extern "C" fn(*mut c_void, *const c_char) -> mpl_result_t,
    pub fs_read: unsafe extern "C" fn(*mut c_void, *const c_char, *mut c_char, *mut u32) -> mpl_result_t,
    pub fs_write: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> mpl_result_t,
    pub set_timeout: unsafe extern "C" fn(*mut c_void, u64, *const c_char, *mut u64) -> mpl_result_t,
    pub clear_timeout: unsafe extern "C" fn(*mut c_void, u64) -> mpl_result_t,
    pub http_request: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, *const c_char, *const c_char, *mut u64) -> mpl_result_t,
    pub set_interval: unsafe extern "C" fn(*mut c_void, u64, *const c_char, *mut u64) -> mpl_result_t,
    pub clear_interval: unsafe extern "C" fn(*mut c_void, u64) -> mpl_result_t,
    pub open_url: unsafe extern "C" fn(*mut c_void, *const c_char) -> mpl_result_t,
    // 【注意】notify 字段必须保留以维持 ABI 内存布局对齐，但我们在代码中不再调用它
    pub notify: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> mpl_result_t, 
    pub locale: unsafe extern "C" fn(*mut c_void, *mut c_char, *mut u32) -> mpl_result_t,
    pub host_info: unsafe extern "C" fn(*mut c_void, *mut c_char, *mut u32) -> mpl_result_t,
    pub clipboard_read: unsafe extern "C" fn(*mut c_void, *mut c_char, *mut u32) -> mpl_result_t,
    pub clipboard_write: unsafe extern "C" fn(*mut c_void, *const c_char) -> mpl_result_t,
    pub set_panel_icon: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> mpl_result_t,
}
unsafe impl Send for mpl_host_api_t {}
unsafe impl Sync for mpl_host_api_t {}

// ==================== 零轮询无锁环形缓冲区 ====================
struct Ring {
    buf: UnsafeCell<Vec<u8>>,
    cap: usize,
    wp: AtomicUsize,
    rp: AtomicUsize,
    is_running: AtomicBool,
    is_ready: AtomicBool,
    is_io_alive: AtomicBool,
    cond_lock: Mutex<()>,
    cond: Condvar,
}
unsafe impl Send for Ring {}
unsafe impl Sync for Ring {}

impl Ring {
    fn new(cap: usize) -> Self {
        Ring {
            buf: UnsafeCell::new(vec![0u8; cap]), cap,
            wp: AtomicUsize::new(0), rp: AtomicUsize::new(0),
            is_running: AtomicBool::new(false),
            is_ready: AtomicBool::new(false),
            is_io_alive: AtomicBool::new(false),
            cond_lock: Mutex::new(()),
            cond: Condvar::new(),
        }
    }

    #[inline]
    fn available(&self) -> usize {
        let w = self.wp.load(Ordering::Acquire);
        let r = self.rp.load(Ordering::Acquire);
        if w >= r { w - r } else { self.cap - r + w }
    }

    #[inline]
    fn space(&self) -> usize { self.cap - self.available() - 1 }

    fn write(&self, data: &[u8]) {
        let len = data.len();
        if len == 0 || len > self.space() { return; }
        
        let w = self.wp.load(Ordering::Relaxed);
        let ptr = unsafe { (*self.buf.get()).as_mut_ptr() };
        let first = std::cmp::min(len, self.cap - w);
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr.add(w), first);
            if first < len {
                std::ptr::copy_nonoverlapping(data.as_ptr().add(first), ptr, len - first);
            }
        }
        self.wp.store((w + len) % self.cap, Ordering::Release);

        if let Ok(_guard) = self.cond_lock.try_lock() {
            self.cond.notify_one();
        }
    }

    fn read_chunk(&self, out: &mut Vec<u8>, n: usize) -> usize {
        if self.available() < n { return 0; }
        out.resize(n, 0);
        let r = self.rp.load(Ordering::Relaxed);
        let ptr = unsafe { (*self.buf.get()).as_ptr() };
        let first = std::cmp::min(n, self.cap - r);
        unsafe {
            std::ptr::copy_nonoverlapping(ptr.add(r), out.as_mut_ptr(), first);
            if first < n {
                std::ptr::copy_nonoverlapping(ptr, out.as_mut_ptr().add(first), n - first);
            }
        }
        self.rp.store((r + n) % self.cap, Ordering::Release);
        n
    }

    fn drain(&self, out: &mut Vec<u8>) -> usize {
        let a = self.available();
        if a == 0 { return 0; }
        self.read_chunk(out, a)
    }

    fn reset(&self) {
        self.wp.store(0, Ordering::SeqCst);
        self.rp.store(0, Ordering::SeqCst);
    }

    fn wait_for_data(&self) {
        let mut guard = self.cond_lock.lock().unwrap();
        while self.available() < WAKE_THRESHOLD && self.is_running.load(Ordering::SeqCst) {
            guard = self.cond.wait(guard).unwrap();
        }
    }
}

// ==================== 全局状态 ====================
static HOST_API: Mutex<Option<mpl_host_api_t>> = Mutex::new(None);
static RING: OnceLock<Arc<Ring>> = OnceLock::new();
static STATE: Mutex<Option<PluginState>> = Mutex::new(None);
static LOG_FILE: Mutex<Option<File>> = Mutex::new(None);

struct PluginState {
    plugin_dir: String,
    config: Config,
    asr_thread: Option<std::thread::JoinHandle<()>>,
}

#[derive(Debug, Clone)]
struct Config {
    load_timing: String,
    sample_rate: u32,
    channels: u32,
}

// ==================== 辅助 ====================
fn extract_json_number(json: &str, key: &str) -> Option<u32> {
    let s = format!("\"{}\":", key);
    json.find(&s).and_then(|i| {
        json[i + s.len()..].chars().take_while(|c| c.is_ascii_digit())
            .collect::<String>().parse().ok()
    })
}

fn unquote(s: &str) -> &str {
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') { &s[1..s.len() - 1] } else { s }
}

unsafe fn read_cfg(h: &mpl_host_api_t, ctx: *mut c_void, key: &str) -> String {
    let k = CString::new(key).unwrap();
    let mut b = [0u8; 256]; let mut sz = b.len() as u32;
    if (h.get_config)(ctx, k.as_ptr(), b.as_mut_ptr() as *mut c_char, &mut sz) == mpl_result_t::MPL_OK && sz > 0 {
        unquote(&String::from_utf8_lossy(&b[..std::cmp::min(sz as usize, b.len())])).to_string()
    } else { String::new() }
}

fn log_msg(msg: &str) {
    if let Some(h) = HOST_API.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        if let Ok(c) = CString::new(msg) { unsafe { (h.log)(h.ctx, 2, c.as_ptr()) }; }
    }
    if let Some(f) = LOG_FILE.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
        let _ = writeln!(f, "[{}] {}", ts, msg);
    }
}

fn init_log(dir: &str) {
    let p = if cfg!(windows) { format!("{}\\log.txt", dir) } else { format!("{}/log.txt", dir) };
    if let Ok(f) = OpenOptions::new().create(true).write(true).truncate(true).open(&p) {
        *LOG_FILE.lock().unwrap_or_else(|e| e.into_inner()) = Some(f);
    }
}

// ==================== Transcript Writer ====================
struct TranscriptWriter {
    save_dir: PathBuf,
    fh: Option<File>,
    cur_date: Option<String>,
    cur_minute: Option<String>,
}

impl TranscriptWriter {
    fn new(save_dir: PathBuf) -> Self {
        Self { save_dir, fh: None, cur_date: None, cur_minute: None }
    }

    fn write(&mut self, real_time: SystemTime, text: &str) {
        let dt: DateTime<Local> = real_time.into();
        let date_str = dt.format("%Y%m%d").to_string();
        let minute_str = dt.format("%H:%M").to_string();
        let sec = dt.second();

        if self.cur_date.as_deref() != Some(&date_str) {
            if let Some(mut f) = self.fh.take() { let _ = f.flush(); }
            let path = self.save_dir.join(format!("{}.txt", date_str));
            match OpenOptions::new().create(true).append(true).open(&path) {
                Ok(f) => {
                    self.fh = Some(f);
                    self.cur_date = Some(date_str);
                    self.cur_minute = None;
                }
                Err(e) => {
                    log_msg(&format!("[Writer] Failed to open file {}: {}", path.display(), e));
                    return;
                }
            }
        }

        if self.cur_minute.as_deref() != Some(&minute_str) {
            if let Some(f) = self.fh.as_mut() { let _ = writeln!(f, "{}", minute_str); }
            self.cur_minute = Some(minute_str);
        }

        if let Some(f) = self.fh.as_mut() {
            let _ = writeln!(f, "\t{} {}", sec, text);
            let _ = f.flush();
        }
    }
}

// ==================== Audio Processing Helpers ====================
fn process_audio_chunk(
    raw_f32: &[f32],
    channels: usize,
    down_ratio: usize,
    mono_leftover: &mut Vec<f32>,
    vad_buf: &mut Vec<f32>
) {
    let mut mono = Vec::with_capacity(raw_f32.len() / channels);
    if channels == 2 {
        for chunk in raw_f32.chunks_exact(2) {
            mono.push((chunk[0] + chunk[1]) * 0.5);
        }
    } else {
        mono.extend_from_slice(raw_f32);
    }

    let mut full_mono = mono_leftover.clone();
    full_mono.extend(mono);

    let trim = full_mono.len() - (full_mono.len() % down_ratio);
    let to_resample = &full_mono[..trim];
    mono_leftover.clear();
    mono_leftover.extend_from_slice(&full_mono[trim..]);

    if down_ratio > 1 {
        let resampled: Vec<f32> = to_resample.chunks_exact(down_ratio)
            .map(|c| c.iter().sum::<f32>() / down_ratio as f32)
            .collect();
        vad_buf.extend(resampled);
    } else {
        vad_buf.extend(to_resample);
    }
}

// ==================== 线程管理 ====================
fn spawn_asr_thread(dir: String, cfg: Config) -> Option<std::thread::JoinHandle<()>> {
    let ring = RING.get().unwrap().clone();
    let handle = std::thread::spawn(move || {
        log_msg("[ASR] Thread started. Loading models...");

        // 1. Initialize VAD
        let mut vad_config = VadModelConfig::default();
        let vad_model_path = format!("{}/silero_vad.onnx", dir);
        vad_config.silero_vad = SileroVadModelConfig {
            model: Some(vad_model_path.into()),
            threshold: 0.5,
            min_silence_duration: 0.5,
            min_speech_duration: 0.25,
            window_size: 512,
            ..Default::default()
        };
        vad_config.sample_rate = 16000;
        vad_config.num_threads = 1;

        let vad = match VoiceActivityDetector::create(&vad_config, 60.0) {
            Some(v) => v,
            None => {
                log_msg("[ASR] Failed to create VAD. Check silero_vad.onnx path.");
                return;
            }
        };
        log_msg("[ASR] VAD loaded.");

        // 2. Initialize ASR
        let mut model_config = OfflineModelConfig::default();
        let model_dir = format!("{}/sherpa-onnx-qwen3-asr-0.6B-int8", dir);
        model_config.qwen3_asr = OfflineQwen3ASRModelConfig {
            conv_frontend: Some(format!("{}/conv_frontend.onnx", model_dir).into()),
            encoder: Some(format!("{}/encoder.int8.onnx", model_dir).into()),
            decoder: Some(format!("{}/decoder.int8.onnx", model_dir).into()),
            tokenizer: Some(format!("{}/tokenizer", model_dir).into()),
            ..Default::default()
        };
        model_config.num_threads = 4;

        let mut config = OfflineRecognizerConfig::default();
        config.model_config = model_config;
        config.decoding_method = Some("greedy_search".into());

        let recognizer = match OfflineRecognizer::create(&config) {
            Some(r) => r,
            None => {
                log_msg("[ASR] Failed to create ASR recognizer. Check model paths.");
                return;
            }
        };
        
        log_msg("[ASR] ASR model loaded. Ready.");

        // 3. Setup TranscriptWriter
        let save_dir = format!("{}/records", dir);
        let _ = std::fs::create_dir_all(&save_dir);
        let mut writer = TranscriptWriter::new(PathBuf::from(&save_dir));

        // 4. Processing Loop
        let mut first_audio_ts: Option<SystemTime> = None;
        let mut mono_leftover = Vec::new();
        let mut vad_buf = Vec::new();
        let mut buf = Vec::with_capacity(CHUNK);

        let channels = cfg.channels as usize;
        let down_ratio = (cfg.sample_rate / 16000) as usize;
        if down_ratio == 0 {
            log_msg("[ASR] Invalid sample rate, cannot resample.");
            return;
        }

        ring.is_running.store(true, Ordering::SeqCst);
        ring.is_io_alive.store(true, Ordering::SeqCst);

        loop {
            if !ring.is_running.load(Ordering::SeqCst) {
                let n = ring.drain(&mut buf);
                if n > 0 {
                    let samples_read = buf.len() / 4;
                    let mut raw_f32 = Vec::with_capacity(samples_read);
                    for i in 0..samples_read {
                        let bytes = [buf[i * 4], buf[i * 4 + 1], buf[i * 4 + 2], buf[i * 4 + 3]];
                        raw_f32.push(f32::from_le_bytes(bytes));
                    }
                    process_audio_chunk(&raw_f32, channels, down_ratio, &mut mono_leftover, &mut vad_buf);
                }
                break;
            }

            let n = ring.drain(&mut buf);
            if n == 0 {
                ring.wait_for_data();
                continue;
            }

            if first_audio_ts.is_none() {
                first_audio_ts = Some(SystemTime::now());
            }

            let samples_read = buf.len() / 4;
            let mut raw_f32 = Vec::with_capacity(samples_read);
            for i in 0..samples_read {
                let bytes = [buf[i * 4], buf[i * 4 + 1], buf[i * 4 + 2], buf[i * 4 + 3]];
                raw_f32.push(f32::from_le_bytes(bytes));
            }

            process_audio_chunk(&raw_f32, channels, down_ratio, &mut mono_leftover, &mut vad_buf);

            while vad_buf.len() >= 512 {
                let chunk: Vec<f32> = vad_buf.drain(..512).collect();
                vad.accept_waveform(&chunk);

                while !vad.is_empty() {
                    if let Some(segment) = vad.front() {
                        let samples = segment.samples();
                        let start_sample = segment.start();

                        if !samples.is_empty() {
                            let stream = recognizer.create_stream();
                            stream.accept_waveform(16000, samples);
                            recognizer.decode(&stream);
                            if let Some(result) = stream.get_result() {
                                let text = result.text.trim();
                                if !text.is_empty() {
                                    let elapsed_secs = start_sample as f64 / 16000.0;
                                    let real_time = first_audio_ts.unwrap() + Duration::from_secs_f64(elapsed_secs);
                                    // 仅记录到文件，不在日志中打印识别结果
                                    writer.write(real_time, text);
                                }
                            }
                        }
                    }
                    vad.pop();
                }
            }
        }

        // Flush VAD on exit
        vad.flush();
        while !vad.is_empty() {
            if let Some(segment) = vad.front() {
                let samples = segment.samples();
                let start_sample = segment.start();
                if !samples.is_empty() {
                    let stream = recognizer.create_stream();
                    stream.accept_waveform(16000, samples);
                    recognizer.decode(&stream);
                    if let Some(result) = stream.get_result() {
                        let text = result.text.trim();
                        if !text.is_empty() {
                            let elapsed_secs = start_sample as f64 / 16000.0;
                            let real_time = first_audio_ts.unwrap() + Duration::from_secs_f64(elapsed_secs);
                            writer.write(real_time, text);
                        }
                    }
                }
            }
            vad.pop();
        }

        ring.is_io_alive.store(false, Ordering::SeqCst);
        log_msg("[ASR] Thread exited.");
    });
    Some(handle)
}

fn start_asr_thread() {
    let (dir, cfg) = {
        let lk = STATE.lock().unwrap_or_else(|e| e.into_inner());
        match lk.as_ref() {
            Some(s) if s.asr_thread.is_some() && RING.get().map_or(false, |r| r.is_io_alive.load(Ordering::SeqCst)) => return,
            Some(s) => (s.plugin_dir.clone(), s.config.clone()),
            None => return,
        }
    };

    if let Some(r) = RING.get() { r.reset(); r.is_running.store(false, Ordering::SeqCst); r.is_ready.store(false, Ordering::SeqCst); }
    stop_asr_thread();

    let handle = spawn_asr_thread(dir, cfg);

    if handle.is_some() {
        let mut lk = STATE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = lk.as_mut() {
            s.asr_thread = handle;
            log_msg("ASR thread started.");
        }
    }
}

fn stop_asr_thread() {
    let handle = {
        let mut lk = STATE.lock().unwrap_or_else(|e| e.into_inner());
        lk.as_mut().map(|s| s.asr_thread.take()).unwrap_or(None)
    };

    if let Some(r) = RING.get() { 
        r.is_running.store(false, Ordering::SeqCst); 
        r.is_ready.store(false, Ordering::SeqCst);
        if let Ok(_guard) = r.cond_lock.lock() {
            r.cond.notify_one();
        }
    }

    let had_thread = handle.is_some();
    if let Some(h) = handle { let _ = h.join(); }
    if had_thread { log_msg("ASR thread stopped."); }
}

// ==================== 导出函数 ====================
#[no_mangle]
pub extern "C" fn micyou_plugin_info() -> *const mpl_plugin_info_t {
    static I: mpl_plugin_info_t = mpl_plugin_info_t {
        abi_version: 1, api_version: 1,
        id: b"opss.whatdidisay\0" as *const u8 as *const c_char,
        version: b"0.3.0\0" as *const u8 as *const c_char
    };
    &I
}

#[no_mangle]
pub extern "C" fn micyou_plugin_init(host: *const mpl_host_api_t) -> mpl_result_t {
    unsafe {
        if host.is_null() { return mpl_result_t::MPL_ERR_INVALID_ARG; }
        stop_asr_thread();
        let h = &*host;
        *HOST_API.lock().unwrap_or_else(|e| e.into_inner()) = Some(*h);
        let _ = RING.set(Arc::new(Ring::new(RING_CAP)));

        let mut db = [0u8; 512]; let mut ds = db.len() as u32;
        let mut dir = String::new();
        if (h.plugin_dir)(h.ctx, db.as_mut_ptr() as *mut c_char, &mut ds) == mpl_result_t::MPL_OK {
            dir = String::from_utf8_lossy(&db[..ds as usize]).to_string();
        }
        init_log(&dir);

        let timing = read_cfg(h, h.ctx, "loadTiming");

        let (mut sr, mut ch) = (48000u32, 2u32);
        let mut b = [0u8; 256]; let mut sz = b.len() as u32;
        if (h.audio_state)(h.ctx, b.as_mut_ptr() as *mut c_char, &mut sz) == mpl_result_t::MPL_OK {
            let j = String::from_utf8_lossy(&b[..sz as usize]);
            if let Some(v) = extract_json_number(&j, "sampleRate") { sr = v; }
            if let Some(v) = extract_json_number(&j, "channels") { ch = v; }
        }

        let cfg = Config {
            load_timing: if timing.is_empty() { "device_connect".into() } else { timing },
            sample_rate: sr,
            channels: ch,
        };

        *STATE.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(PluginState { plugin_dir: dir, config: cfg.clone(), asr_thread: None });

        if cfg.load_timing == "micyou_start" { start_asr_thread(); }
    }
    mpl_result_t::MPL_OK
}

#[no_mangle]
pub extern "C" fn micyou_plugin_deinit() { stop_asr_thread(); }

#[no_mangle]
pub extern "C" fn micyou_plugin_process(
    data: *mut f32, samples: u32, _channels: u32, _: f64, bypass: *mut u32,
) -> mpl_result_t {
    unsafe {
        if !bypass.is_null() { *bypass = 0; }
        if data.is_null() || samples == 0 { return mpl_result_t::MPL_OK; }
        if let Some(ring) = RING.get() {
            let bytes_len = (samples as usize) * 4;
            let slice = std::slice::from_raw_parts(data as *const u8, bytes_len);
            ring.write(slice);
        }
    }
    mpl_result_t::MPL_OK
}

#[no_mangle]
pub extern "C" fn micyou_plugin_handle_event(ev: *const c_char, _: *const c_char) -> mpl_result_t {
    unsafe {
        if ev.is_null() { return mpl_result_t::MPL_ERR_INVALID_ARG; }
        let s = CStr::from_ptr(ev).to_str().unwrap_or("");
        let go = STATE.lock().unwrap_or_else(|e| e.into_inner())
            .as_ref().map(|x| x.config.load_timing == "device_connect").unwrap_or(false);
        match s {
            "device_connected" if go => start_asr_thread(),
            "device_disconnected" => stop_asr_thread(),
            _ => {}
        }
    }
    mpl_result_t::MPL_OK
}

#[no_mangle]
pub extern "C" fn micyou_plugin_handle_message(_: *const c_char, topic: *const c_char, _: *const u8, _: u32) -> mpl_result_t {
    unsafe {
        if topic.is_null() { return mpl_result_t::MPL_ERR_INVALID_ARG; }
        if CStr::from_ptr(topic).to_str().unwrap_or("") == "config:changed" {
            if let Some(h) = HOST_API.lock().unwrap_or_else(|e| e.into_inner()).clone() {
                let nt = read_cfg(&h, h.ctx, "loadTiming");
                let mut restart = false;
                {
                    let mut lk = STATE.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(s) = lk.as_mut() {
                        if !nt.is_empty() && nt != s.config.load_timing { 
                            s.config.load_timing = nt; 
                            restart = true; 
                        }
                    }
                }
                if restart { log_msg("Config changed, restarting..."); stop_asr_thread(); start_asr_thread(); }
            }
        }
    }
    mpl_result_t::MPL_OK
}