#![cfg(target_os = "windows")]

use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_MENU, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_SHIFT, VK_SPACE};
use windows::Win32::UI::WindowsAndMessaging::{CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP};

#[derive(Clone, Debug)]
struct HotkeySpec {
    ctrl: bool,
    alt: bool,
    shift: bool,
    key_vk: u32,
}

impl Default for HotkeySpec {
    fn default() -> Self {
        Self { ctrl: true, alt: false, shift: true, key_vk: VK_SPACE.0 as u32 }
    }
}

struct HookState {
    tx: Option<mpsc::Sender<bool>>,
    spec: HotkeySpec,
    active: bool,
    active_since: Option<Instant>,
    last_trigger_down_at: Option<Instant>,
    released_samples: u8,
    suppress_until_released: bool,
    installed: bool,
}

static STATE: OnceLock<Mutex<HookState>> = OnceLock::new();

fn state() -> &'static Mutex<HookState> {
    STATE.get_or_init(|| Mutex::new(HookState {
        tx: None,
        spec: HotkeySpec::default(),
        active: false,
        active_since: None,
        last_trigger_down_at: None,
        released_samples: 0,
        suppress_until_released: false,
        installed: false,
    }))
}

fn is_down(vk: i32) -> bool {
    unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 }
}

fn mods_match(spec: &HotkeySpec) -> bool {
    let ctrl = is_down(VK_CONTROL.0 as i32) || is_down(VK_LCONTROL.0 as i32) || is_down(VK_RCONTROL.0 as i32);
    let alt = is_down(VK_MENU.0 as i32) || is_down(VK_LMENU.0 as i32) || is_down(VK_RMENU.0 as i32);
    let shift = is_down(VK_SHIFT.0 as i32) || is_down(VK_LSHIFT.0 as i32) || is_down(VK_RSHIFT.0 as i32);
    (!spec.ctrl || ctrl) && (!spec.alt || alt) && (!spec.shift || shift)
}

fn parse_hotkey(value: &str) -> HotkeySpec {
    let mut spec = HotkeySpec { ctrl: false, alt: false, shift: false, key_vk: VK_SPACE.0 as u32 };
    for part in value.split('+').map(|p| p.trim().to_ascii_lowercase()) {
        match part.as_str() {
            "cmdorctrl" | "ctrl" | "control" => spec.ctrl = true,
            "alt" | "option" => spec.alt = true,
            "shift" => spec.shift = true,
            "space" => spec.key_vk = VK_SPACE.0 as u32,
            s if s.len() == 1 => {
                let ch = s.as_bytes()[0];
                if ch.is_ascii_alphabetic() { spec.key_vk = ch.to_ascii_uppercase() as u32; }
                if ch.is_ascii_digit() { spec.key_vk = ch as u32; }
            }
            _ => {}
        }
    }
    spec
}

fn stop_if_trigger_released() -> bool {
    if let Ok(mut st) = state().lock() {
        if !st.active {
            return false;
        }

        let Some(started) = st.active_since else {
            return false;
        };
        let age = started.elapsed();
        if age < Duration::from_millis(600) {
            return false;
        }

        // GetAsyncKeyState can briefly report "up" while our low-level hook is
        // swallowing the trigger key. Do not stop while fresh key-down/repeat
        // events are still arriving; this prevents rapid false start/stop loops.
        let recent_trigger_down = st
            .last_trigger_down_at
            .is_some_and(|last| last.elapsed() < Duration::from_millis(350));
        if recent_trigger_down {
            st.released_samples = 0;
            return false;
        }

        if !is_down(st.spec.key_vk as i32) {
            st.released_samples = st.released_samples.saturating_add(1);
            if st.released_samples < 6 {
                return false;
            }
            st.active = false;
            st.active_since = None;
            st.last_trigger_down_at = None;
            st.released_samples = 0;
            st.suppress_until_released = true;
            crate::ocr::runtime_log("voice hotkey watchdog: trigger released, stopping recording");
            if let Some(tx) = &st.tx { let _ = tx.try_send(false); }
            return true;
        }

        st.released_samples = 0;
    }
    false
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let kb = *(lparam.0 as *const KBDLLHOOKSTRUCT);
        let vk = kb.vkCode;
        let is_key_down = wparam.0 as u32 == WM_KEYDOWN || wparam.0 as u32 == WM_SYSKEYDOWN;
        let is_key_up = wparam.0 as u32 == WM_KEYUP || wparam.0 as u32 == WM_SYSKEYUP;

        if let Ok(mut st) = state().lock() {
            if vk == st.spec.key_vk {
                if is_key_down {
                    st.last_trigger_down_at = Some(Instant::now());
                }
                if is_key_up {
                    st.suppress_until_released = false;
                    st.last_trigger_down_at = None;
                }
                if is_key_down && st.suppress_until_released {
                    // If the watchdog already stopped recording because Windows reported the
                    // trigger released, swallow repeat/down noise until we see a real key-up.
                    return LRESULT(1);
                }
                if is_key_down && !st.active && mods_match(&st.spec) {
                    st.active = true;
                    st.active_since = Some(Instant::now());
                    st.last_trigger_down_at = Some(Instant::now());
                    st.released_samples = 0;
                    st.suppress_until_released = false;
                    crate::ocr::runtime_log("voice hotkey pressed: starting recording");
                    if let Some(tx) = &st.tx { let _ = tx.try_send(true); }
                    return LRESULT(1);
                }
                if is_key_down && st.active {
                    st.last_trigger_down_at = Some(Instant::now());
                    // Critical behavior: once PTT starts, keep swallowing Space repeats even if
                    // Ctrl/Alt/Shift are released while Space stays physically held.
                    return LRESULT(1);
                }
                if is_key_up && st.active {
                    // Guard against spurious early key-up notifications while the trigger key
                    // is still physically down (seen on some Windows focus transitions).
                    if is_down(st.spec.key_vk as i32) {
                        st.released_samples = 0;
                        return LRESULT(1);
                    }
                    // Also ignore ultra-fast bounce right after activation.
                    if st.active_since.is_some_and(|started| started.elapsed() < Duration::from_millis(300)) {
                        return LRESULT(1);
                    }
                    st.active = false;
                    st.active_since = None;
                    st.last_trigger_down_at = None;
                    st.released_samples = 0;
                    crate::ocr::runtime_log("voice hotkey released: stopping recording");
                    if let Some(tx) = &st.tx { let _ = tx.try_send(false); }
                    return LRESULT(1);
                }
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

pub fn start(tx: mpsc::Sender<bool>, hotkey: &str) {
    {
        let mut st = state().lock().unwrap();
        st.tx = Some(tx);
        st.spec = parse_hotkey(hotkey);
        if st.installed { return; }
        st.installed = true;
    }

    thread::spawn(|| unsafe {
        if SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0).is_ok() {
            crate::ocr::runtime_log("voice hotkey hook installed");
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).into() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        } else {
            crate::ocr::runtime_log("voice hotkey hook failed to install");
        }
    });

    // Watchdog for missed key-up events. Focus/window switches can sometimes
    // prevent the hook from seeing release; polling the physical trigger key
    // keeps push-to-talk from getting stuck like hands-free mode.
    thread::spawn(|| loop {
        let _ = stop_if_trigger_released();
        thread::sleep(Duration::from_millis(25));
    });
}

pub fn update_hotkey(hotkey: &str) {
    if let Ok(mut st) = state().lock() {
        st.spec = parse_hotkey(hotkey);
        st.active = false;
        st.active_since = None;
        st.last_trigger_down_at = None;
        st.released_samples = 0;
        st.suppress_until_released = false;
        crate::ocr::runtime_log(format!("voice hotkey updated: {}", hotkey));
    }
}
