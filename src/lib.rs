#![allow(non_snake_case)]

const CHUNK: usize = 65536;
const RING_CAP: usize = 2 * 1024 * 1024;
const POLL_MS: u64 = 10;

use std::cell::UnsafeCell;
use std::ffi::{c_char, c_void, CStr, CString};
use std::fs::{File, OpenOptions};
use std::io::{Write, BufRead};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex, OnceLock,
};
use std::time::{Duration, Instant};

// ==================== ABI ====================

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
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
    pub notify: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> mpl_result_t,
    pub locale: unsafe extern "C" fn(*mut c_void, *mut c_char, *mut u32) -> mpl_result_t,
    pub host_info: unsafe extern "C" fn(*mut c_void, *mut c_char, *mut u32) -> mpl_result_t,
    pub clipboard_read: unsafe extern "C" fn(*mut c_void, *mut c_char, *mut u32) -> mpl_result_t,
    pub clipboard_write: unsafe extern "C" fn(*mut c_void, *const c_char) -> mpl_result_t,
    pub set_panel_icon: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> mpl_result_t,
}
unsafe impl Send for mpl_host_api_t {}
unsafe impl Sync for mpl_host_api_t {}

// ==================== 无锁环形缓冲区 ====================

struct Ring {
    buf: UnsafeCell<Vec<u8>>,
    cap: usize,
    wp: AtomicUsize,
    rp: AtomicUsize,
    is_running: AtomicBool,
    is_ready: AtomicBool,
    is_io_alive: AtomicBool,
}
unsafe impl Send for Ring {}
unsafe impl Sync for Ring {}

impl Ring {
    fn new(cap: usize) -> Self {
        Ring { buf: UnsafeCell::new(vec![0u8; cap]), cap,
            wp: AtomicUsize::new(0), rp: AtomicUsize::new(0),
            is_running: AtomicBool::new(false),
            is_ready: AtomicBool::new(false),
            is_io_alive: AtomicBool::new(false) }
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
}

// ==================== 全局状态 ====================

static HOST_API: Mutex<Option<mpl_host_api_t>> = Mutex::new(None);
static RING: OnceLock<Arc<Ring>> = OnceLock::new();
static STATE: Mutex<Option<PluginState>> = Mutex::new(None);
static LOG_FILE: Mutex<Option<File>> = Mutex::new(None);

struct PluginState {
    plugin_dir: String, config: Config,
    child: Option<std::process::Child>,
    io_thread: Option<std::thread::JoinHandle<()>>,
}
#[derive(Debug, Clone)]
struct Config { load_timing: String, target_language: String }

// ==================== 辅助 ====================

fn extract_json_number(json: &str, key: &str) -> Option<u32> {
    let s = format!("\"{}\":", key);
    json.find(&s).and_then(|i| {
        json[i+s.len()..].chars().take_while(|c| c.is_ascii_digit())
            .collect::<String>().parse().ok()
    })
}
fn unquote(s: &str) -> &str {
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') { &s[1..s.len()-1] } else { s }
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

// ==================== 进程管理 ====================

fn spawn_process(dir: &str, cfg: &Config) -> (Option<std::process::Child>, Option<std::thread::JoinHandle<()>>) {
    let host = match HOST_API.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        Some(h) => h, None => return (None, None) };
    let ctx = host.ctx;

    let mut b = [0u8; 256]; let mut sz = b.len() as u32;
    let (mut sr, mut ch) = (48000u32, 2u32);
    unsafe {
        if (host.audio_state)(ctx, b.as_mut_ptr() as *mut c_char, &mut sz) == mpl_result_t::MPL_OK {
            let j = String::from_utf8_lossy(&b[..sz as usize]);
            if let Some(v) = extract_json_number(&j, "sampleRate") { sr = v; }
            if let Some(v) = extract_json_number(&j, "channels") { ch = v; }
        }
    }

    let (py, script, save_dir, model) = if cfg!(windows) {
        (format!("{}\\runtime\\Scripts\\python.exe", dir), format!("{}\\main.py", dir),
         format!("{}\\records", dir), format!("{}\\faster-whisper-large-v2", dir))
    } else {
        (format!("{}/runtime/bin/python", dir), format!("{}/main.py", dir),
         format!("{}/records", dir), format!("{}/faster-whisper-large-v2", dir))
    };
    let _ = std::fs::create_dir_all(&save_dir);
    log_msg(&format!("Run: {} {}", py, script));

    let mut cmd = std::process::Command::new(&py);
    cmd.arg(&script)
        .arg("--model").arg(&model)
        .arg("--language").arg(&cfg.target_language)
        .arg("--save-dir").arg(&save_dir)
        .arg("--sample-rate").arg(sr.to_string())
        .arg("--channels").arg(ch.to_string())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(target_os = "windows")]
    { use std::os::windows::process::CommandExt; cmd.creation_flags(0x08000000); }

    let mut child = match cmd.spawn() {
        Ok(c) => c, Err(e) => { log_msg(&format!("Spawn err: {}", e)); return (None, None); } };

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    std::thread::spawn(move || {
        for l in std::io::BufReader::new(stderr).lines() {
            if let Ok(l) = l { log_msg(&format!("[Py] {}", l)); } } });

    let ring = RING.get().unwrap().clone();
    ring.is_running.store(true, Ordering::SeqCst);
    ring.is_io_alive.store(true, Ordering::SeqCst);

    let handle = std::thread::spawn(move || {
        log_msg("[IO] Waiting READY...");
        let mut ready = false;
        let mut lb = String::new();
        let mut rd = std::io::BufReader::new(stdout);
        while let Ok(n) = rd.read_line(&mut lb) {
            if n == 0 { break; }
            if lb.contains("READY") { ring.is_ready.store(true, Ordering::Release); ready = true; break; }
            lb.clear();
        }
        if !ready { ring.is_io_alive.store(false, Ordering::SeqCst); return; }
        log_msg("[IO] READY. Streaming 64KB chunks.");

        let mut buf = Vec::with_capacity(CHUNK);
        loop {
            if !ring.is_running.load(Ordering::SeqCst) {
                let n = ring.drain(&mut buf);
                if n > 0 { let _ = stdin.write_all(&buf[..n]); }
                break;
            }
            if ring.read_chunk(&mut buf, CHUNK) > 0 {
                if stdin.write_all(&buf).is_err() { log_msg("[IO] write err!"); break; }
            } else {
                std::thread::sleep(Duration::from_millis(POLL_MS));
            }
        }
        ring.is_io_alive.store(false, Ordering::SeqCst);
        log_msg("[IO] Exited.");
    });

    (Some(child), Some(handle))
}

fn start_process() {
    let (dir, cfg) = {
        let lk = STATE.lock().unwrap_or_else(|e| e.into_inner());
        match lk.as_ref() {
            Some(s) if s.child.is_some() && RING.get().map_or(false, |r| r.is_io_alive.load(Ordering::SeqCst)) => return,
            Some(s) => (s.plugin_dir.clone(), s.config.clone()),
            None => return,
        } };
    if let Some(r) = RING.get() { r.reset(); r.is_running.store(false, Ordering::SeqCst); r.is_ready.store(false, Ordering::SeqCst); }
    let (child, handle) = spawn_process(&dir, &cfg);
    if child.is_some() {
        let mut lk = STATE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = lk.as_mut() {
            if let Some(mut o) = s.child.take() { let _ = o.kill(); let _ = o.wait(); }
            s.child = child; s.io_thread = handle;
            log_msg("Python started.");
        } }
}

fn stop_process() {
    let (mut child, handle) = {
        let mut lk = STATE.lock().unwrap_or_else(|e| e.into_inner());
        lk.as_mut().map(|s| (s.child.take(), s.io_thread.take())).unwrap_or((None, None)) };
    if let Some(r) = RING.get() { r.is_running.store(false, Ordering::SeqCst); r.is_ready.store(false, Ordering::SeqCst); }
    if let Some(h) = handle { let _ = h.join(); }
    if let Some(ref mut c) = child {
        let t = Instant::now();
        loop { match c.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if t.elapsed() < Duration::from_secs(2) => std::thread::sleep(Duration::from_millis(100)),
            _ => { let _ = c.kill(); let _ = c.wait(); break; } } } }
    if child.is_some() { log_msg("Python stopped."); }
}

// ==================== 导出函数 ====================

#[no_mangle]
pub extern "C" fn micyou_plugin_info() -> *const mpl_plugin_info_t {
    static I: mpl_plugin_info_t = mpl_plugin_info_t {
        abi_version: 1, api_version: 1,
        id: b"opss.whatdidisay\0" as *const u8 as *const c_char,
        version: b"0.1.0\0" as *const u8 as *const c_char };
    &I
}

#[no_mangle]
pub extern "C" fn micyou_plugin_init(host: *const mpl_host_api_t) -> mpl_result_t {
    unsafe {
        if host.is_null() { return mpl_result_t::MPL_ERR_INVALID_ARG; }
        stop_process();
        let h = &*host;
        *HOST_API.lock().unwrap_or_else(|e| e.into_inner()) = Some(*h);
        let _ = RING.set(Arc::new(Ring::new(RING_CAP)));

        let mut db = [0u8; 512]; let mut ds = db.len() as u32;
        let mut dir = String::new();
        if (h.plugin_dir)(h.ctx, db.as_mut_ptr() as *mut c_char, &mut ds) == mpl_result_t::MPL_OK {
            dir = String::from_utf8_lossy(&db[..ds as usize]).to_string(); }
        init_log(&dir);

        let mut lang = read_cfg(h, h.ctx, "targetLanguage");
        if lang.is_empty() || lang == "auto" {
            let mut lb = [0u8; 32]; let mut ls = lb.len() as u32;
            let mut fl = "en";
            if (h.locale)(h.ctx, lb.as_mut_ptr() as *mut c_char, &mut ls) == mpl_result_t::MPL_OK {
                if String::from_utf8_lossy(&lb[..ls as usize]).starts_with("zh") { fl = "zh"; } }
            lang = fl.to_string();
            let jv = CString::new(format!("\"{}\"", lang)).unwrap();
            let kk = CString::new("targetLanguage").unwrap();
            (h.set_config)(h.ctx, kk.as_ptr(), jv.as_ptr());
        }
        let timing = read_cfg(h, h.ctx, "loadTiming");
        let cfg = Config {
            load_timing: if timing.is_empty() { "device_connect".into() } else { timing },
            target_language: lang };
        *STATE.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(PluginState { plugin_dir: dir, config: cfg.clone(), child: None, io_thread: None });
        if cfg.load_timing == "micyou_start" { start_process(); }
    }
    mpl_result_t::MPL_OK
}

#[no_mangle]
pub extern "C" fn micyou_plugin_deinit() { stop_process(); }

#[no_mangle]
pub extern "C" fn micyou_plugin_process(
    data: *mut f32, samples: u32, _channels: u32, _: f64, bypass: *mut u32,
) -> mpl_result_t {
    unsafe {
        if !bypass.is_null() { *bypass = 0; }
        if data.is_null() || samples == 0 { return mpl_result_t::MPL_OK; }
        if let Some(ring) = RING.get() {
            if !ring.is_ready.load(Ordering::Acquire) { return mpl_result_t::MPL_OK; }

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
            "device_connected" if go => start_process(),
            "device_disconnected" => stop_process(),
            _ => {} }
    }
    mpl_result_t::MPL_OK
}

#[no_mangle]
pub extern "C" fn micyou_plugin_handle_message(_: *const c_char, topic: *const c_char, _: *const u8, _: u32) -> mpl_result_t {
    unsafe {
        if topic.is_null() { return mpl_result_t::MPL_ERR_INVALID_ARG; }
        if CStr::from_ptr(topic).to_str().unwrap_or("") == "config:changed" {
            if let Some(h) = HOST_API.lock().unwrap_or_else(|e| e.into_inner()).clone() {
                let nl = read_cfg(&h, h.ctx, "targetLanguage");
                let nt = read_cfg(&h, h.ctx, "loadTiming");
                let mut restart = false;
                { let mut lk = STATE.lock().unwrap_or_else(|e| e.into_inner());
                  if let Some(s) = lk.as_mut() {
                      if !nl.is_empty() && nl != s.config.target_language { s.config.target_language = nl; restart = true; }
                      if !nt.is_empty() && nt != s.config.load_timing { s.config.load_timing = nt; } } }
                if restart { log_msg("Config changed, restarting..."); stop_process(); start_process(); }
            } }
    }
    mpl_result_t::MPL_OK
}