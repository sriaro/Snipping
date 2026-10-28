use std::{
    cell::RefCell,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
        System::{
            LibraryLoader::GetModuleHandleW,
            Shutdown::{ShutdownBlockReasonCreate, ShutdownBlockReasonDestroy},
        },
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DispatchMessageW, ENDSESSION_CLOSEAPP, GetMessageW, LoadIconW, MSG, RegisterClassExW, TranslateMessage, WM_CLOSE, WM_ENDSESSION, WM_QUERYENDSESSION,
            WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
        },
    },
    core::{PCWSTR, w},
};

const GRACE: Duration = Duration::from_secs(25);

const LOOK: Duration = Duration::from_millis(50);

const SAVERS: usize = 4;

const REASON: PCWSTR = w!("Saving a screen recording.");

struct Hooks {
    busy: fn() -> bool,
    end: Box<dyn Fn(bool) + Send>,
    step: fn(),
    grace: Duration,
}

thread_local! {
    static HOOKS: RefCell<Option<Hooks>> = const { RefCell::new(None) };
}

static ENDING: AtomicBool = AtomicBool::new(false);

pub fn ending() -> bool {
    ENDING.load(Ordering::Acquire)
}

pub fn watch(busy: fn() -> bool, end: impl Fn(bool) + Send + 'static, step: fn()) {
    start(Hooks { busy, end: Box::new(end), step, grace: GRACE }, |_| {});
}

fn start(hooks: Hooks, made: impl FnOnce(Option<isize>) + Send + 'static) {
    let _ = std::thread::Builder::new().name("snipping-session".into()).spawn(move || {
        HOOKS.with(|h| *h.borrow_mut() = Some(hooks));
        let hwnd = window();
        made(hwnd.map(|h| h.0 as isize));
        if hwnd.is_some() {
            pump();
        }
    });
}

fn window() -> Option<HWND> {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    let hinst = unsafe { GetModuleHandleW(None) }.ok().map(|h| HINSTANCE(h.0))?;
    ONCE.call_once(|| unsafe {
        let icon = LoadIconW(Some(hinst), PCWSTR(std::ptr::without_provenance(1))).unwrap_or_default();
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst,
            hIcon: icon,
            hIconSm: icon,
            lpszClassName: w!("SnippingSession"),
            ..Default::default()
        };
        RegisterClassExW(&wc);
    });
    unsafe { CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, w!("SnippingSession"), w!("Snipping"), WS_POPUP, 0, 0, 0, 0, None, None, Some(hinst), None) }.ok()
}

fn pump() {
    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn with<R>(f: impl FnOnce(&Hooks) -> R) -> Option<R> {
    HOOKS.with(|c| {
        let h = c.try_borrow().ok()?;
        h.as_ref().map(f)
    })
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_QUERYENDSESSION => {
            if with(|h| (h.busy)()).unwrap_or(false) {
                let _ = unsafe { ShutdownBlockReasonCreate(hwnd, REASON) };
            }
            LRESULT(1)
        }
        WM_ENDSESSION => {
            if wparam.0 != 0 {
                with(|h| end_of_session(hwnd, h, lparam.0 as u32 & ENDSESSION_CLOSEAPP != 0));
            }
            let _ = unsafe { ShutdownBlockReasonDestroy(hwnd) };
            LRESULT(0)
        }
        WM_CLOSE => LRESULT(0),
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

struct Saver {
    go: std::sync::mpsc::Sender<()>,
    done: std::sync::mpsc::Receiver<()>,
    busy: bool,
}

impl Saver {
    fn start(step: fn()) -> Option<Saver> {
        let (go, going) = std::sync::mpsc::channel::<()>();
        let (done, finished) = std::sync::mpsc::channel::<()>();
        std::thread::Builder::new()
            .name("snipping-session-save".into())
            .spawn(move || {
                while going.recv().is_ok() {
                    step();
                    if done.send(()).is_err() {
                        break;
                    }
                }
            })
            .ok()?;
        Some(Saver { go, done: finished, busy: false })
    }

    fn settled(&mut self) -> bool {
        if self.busy && self.done.try_recv().is_ok() {
            self.busy = false;
        }
        !self.busy
    }

    fn run(&mut self) -> Option<bool> {
        self.go.send(()).ok()?;
        self.busy = self.done.recv_timeout(LOOK).is_err();
        Some(!self.busy)
    }
}

fn end_of_session(hwnd: HWND, h: &Hooks, closing_app: bool) {
    let busy = (h.busy)();
    if busy {
        let _ = unsafe { ShutdownBlockReasonCreate(hwnd, REASON) };
        ENDING.store(true, Ordering::Release);
    }
    (h.end)(closing_app);
    if !busy {
        return;
    }
    let until = Instant::now() + h.grace;
    let mut savers: Vec<Saver> = Vec::new();
    loop {
        let quick = match savers.iter_mut().position(|s| s.settled()) {
            Some(i) => match savers[i].run() {
                Some(quick) => quick,
                None => {
                    savers.swap_remove(i);
                    false
                }
            },
            None if savers.len() < SAVERS => match Saver::start(h.step) {
                Some(mut saver) => {
                    let quick = saver.run().unwrap_or(false);
                    savers.push(saver);
                    quick
                }
                None => {
                    (h.step)();
                    true
                }
            },
            None => true,
        };
        if !(h.busy)() || Instant::now() >= until {
            break;
        }
        if quick {
            std::thread::sleep(LOOK);
        }
    }
    ENDING.store(false, Ordering::Release);
}
