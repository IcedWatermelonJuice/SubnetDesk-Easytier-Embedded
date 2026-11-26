use super::{PrivacyMode, INVALID_PRIVACY_MODE_CONN_ID};
use crate::{platform::windows::get_user_token, privacy_mode::PrivacyModeState};
use hbb_common::{allow_err, bail, log, ResultType};
use std::{
    ffi::CString,
    io::Error,
    mem::size_of,
    time::{Duration, Instant},
};
use winapi::{
    shared::{
        minwindef::{BOOL, FALSE, LPARAM, LRESULT, TRUE, UINT, WPARAM},
        ntdef::{HANDLE, NULL},
        windef::{HBRUSH, HDC, HMONITOR, HWND, RECT},
    },
    um::{
        handleapi::CloseHandle,
        wingdi::{
            CreateFontW, DeleteObject, GetStockObject, SelectObject, SetBkMode, SetTextColor,
            SetDCBrushColor, BLACK_BRUSH, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS,
            DC_BRUSH, DEFAULT_CHARSET, DEFAULT_GUI_FONT, DEFAULT_PITCH, FW_NORMAL,
            FW_SEMIBOLD, OUT_DEFAULT_PRECIS, TRANSPARENT,
        },
        libloaderapi::{FreeLibrary, GetModuleHandleA, GetProcAddress, LoadLibraryExA, LOAD_LIBRARY_SEARCH_SYSTEM32},
        memoryapi::{VirtualAllocEx, WriteProcessMemory},
        processthreadsapi::{
            CreateProcessAsUserW, QueueUserAPC, ResumeThread, TerminateProcess,
            PROCESS_INFORMATION, STARTUPINFOW,
        },
        winbase::{WTSGetActiveConsoleSessionId, CREATE_SUSPENDED, DETACHED_PROCESS},
        winnt::{MEM_COMMIT, PAGE_READWRITE},
        winuser::*,
    },
};

pub(super) const PRIVACY_MODE_IMPL: &str = "privacy_mode_impl_mag";

pub const ORIGIN_PROCESS_EXE: &'static str = "C:\\Windows\\System32\\RuntimeBroker.exe";
pub const WIN_TOPMOST_INJECTED_PROCESS_EXE: &'static str = "RuntimeBroker_rustdesk.exe";
pub const INJECTED_PROCESS_EXE: &'static str = WIN_TOPMOST_INJECTED_PROCESS_EXE;
pub(super) const PRIVACY_WINDOW_CLASS: &'static str = "RustDeskPrivacyWindowClass";
pub(super) const PRIVACY_WINDOW_NAME: &'static str = "RustDeskPrivacyWindow";
const PRIVACY_WINDOW_WAIT_MILLIS: u128 = 1_000;
const PRIVACY_WINDOW_WAIT_EXTRA_MONITOR_MILLIS: u128 = 500;
const PRIVACY_WINDOW_POLL_INTERVAL_MILLIS: u64 = 100;
const WM_RUSTDESK_SHOW_WINDOWS: u32 = WM_APP + 3;
const WM_RUSTDESK_HIDE_WINDOWS: u32 = WM_APP + 4;

struct WindowHandlers {
    hthread: u64,
    hprocess: u64,
}

impl Drop for WindowHandlers {
    fn drop(&mut self) {
        self.reset();
    }
}

impl WindowHandlers {
    fn reset(&mut self) {
        unsafe {
            if self.hprocess != 0 {
                let _res = TerminateProcess(self.hprocess as _, 0);
                CloseHandle(self.hprocess as _);
            }
            self.hprocess = 0;
            if self.hthread != 0 {
                CloseHandle(self.hthread as _);
            }
            self.hthread = 0;
        }
    }

    fn is_default(&self) -> bool {
        self.hthread == 0 && self.hprocess == 0
    }
}

pub struct PrivacyModeImpl {
    impl_key: String,
    conn_id: i32,
    handlers: WindowHandlers,
    hwnd: u64,
    native_hwnd: u64,
}

impl PrivacyMode for PrivacyModeImpl {
    fn is_async_privacy_mode(&self) -> bool {
        false
    }

    fn init(&self) -> ResultType<()> {
        Ok(())
    }

    fn clear(&mut self) {
        allow_err!(self.turn_off_privacy(self.conn_id, None));
    }

    fn turn_on_privacy(&mut self, conn_id: i32) -> ResultType<bool> {
        if self.check_on_conn_id(conn_id)? {
            log::debug!("Privacy mode of conn {} is already on", conn_id);
            return Ok(true);
        }

        if !crate::desktop_runtime::is_host() {
            let exe_file = std::env::current_exe()?;
            let cur_dir = exe_file.parent().ok_or_else(||
                hbb_common::anyhow::anyhow!("Invalid executable directory"))?;
            if !cur_dir.join("WindowInjection.dll").exists() {
                bail!("隐私屏组件 WindowInjection.dll 缺失，请更新完整应用包。");
            }
        }

        let should_start_broker = self.handlers.is_default() && self.native_hwnd == 0;
        if should_start_broker {
            log::info!("turn_on_privacy, broker not running, try start");
            self.start()?;
            std::thread::sleep(std::time::Duration::from_millis(1_000));
        }

        if let Err(e) = self.show_privacy_windows(conn_id, true) {
            self.stop();
            return Err(e);
        }
        Ok(true)
    }

    fn turn_off_privacy(
        &mut self,
        conn_id: i32,
        state: Option<PrivacyModeState>,
    ) -> ResultType<()> {
        self.check_off_conn_id(conn_id)?;
        super::win_input::unhook()?;
        let hwnds = find_privacy_hwnds()?;
        let hide_result = set_privacy_windows_visible(&hwnds, false);
        if hide_result.is_err() {
            self.stop();
        }

        // Continue local state cleanup even after stop(); the broker has
        // been torn down, so keeping conn_id/hwnd would leave stale state.
        if self.conn_id != INVALID_PRIVACY_MODE_CONN_ID {
            // Only publish the off state after the hide message was posted.
            // Otherwise the peer may receive a success-like state and then a
            // failed turn-off response for the same request.
            if hide_result.is_ok() {
                if let Some(state) = state {
                    allow_err!(super::set_privacy_mode_state(
                        conn_id,
                        state,
                        PRIVACY_MODE_IMPL.to_string(),
                        1_000
                    ));
                }
            }
            self.conn_id = INVALID_PRIVACY_MODE_CONN_ID.to_owned();
            self.hwnd = 0;
        }

        hide_result.map(|_| ())
    }

    #[inline]
    fn pre_conn_id(&self) -> i32 {
        self.conn_id
    }

    #[inline]
    fn get_impl_key(&self) -> &str {
        &self.impl_key
    }
}

impl PrivacyModeImpl {
    pub fn new(impl_key: &str) -> Self {
        Self {
            impl_key: impl_key.to_owned(),
            conn_id: INVALID_PRIVACY_MODE_CONN_ID,
            handlers: WindowHandlers {
                hthread: 0,
                hprocess: 0,
            },
            hwnd: 0,
            native_hwnd: 0,
        }
    }

    #[inline]
    pub fn get_hwnd(&self) -> u64 {
        self.hwnd
    }

    pub fn start(&mut self) -> ResultType<()> {
        if crate::desktop_runtime::is_host() {
            if self.native_hwnd == 0 {
                self.native_hwnd = start_native_privacy_window()?;
            }
            return Ok(());
        }
        if self.handlers.hprocess != 0 {
            return Ok(());
        }

        log::info!("Start privacy mode window broker, check_update_broker_process");
        if let Err(e) = crate::platform::windows::check_update_broker_process() {
            log::warn!(
                "Failed to check update broker process. Privacy mode may not work properly. {}",
                e
            );
        }

        let exe_file = std::env::current_exe()?;
        let Some(cur_dir) = exe_file.parent() else {
            bail!("Cannot get parent of current exe file");
        };

        let dll_file = cur_dir.join("WindowInjection.dll");
        if !dll_file.exists() {
            bail!(
                "Failed to find required file {}",
                dll_file.to_string_lossy().as_ref()
            );
        }

        if wait_find_privacy_hwnds(PRIVACY_WINDOW_WAIT_MILLIS).is_ok() {
            log::info!("Privacy window is ready");
            return Ok(());
        }

        // let cmdline = cur_dir.join("MiniBroker.exe").to_string_lossy().to_string();
        let cmdline = cur_dir
            .join(INJECTED_PROCESS_EXE)
            .to_string_lossy()
            .to_string();

        unsafe {
            let cmd_utf16: Vec<u16> = cmdline.encode_utf16().chain(Some(0).into_iter()).collect();

            let mut start_info = STARTUPINFOW {
                cb: 0,
                lpReserved: NULL as _,
                lpDesktop: NULL as _,
                lpTitle: NULL as _,
                dwX: 0,
                dwY: 0,
                dwXSize: 0,
                dwYSize: 0,
                dwXCountChars: 0,
                dwYCountChars: 0,
                dwFillAttribute: 0,
                dwFlags: 0,
                wShowWindow: 0,
                cbReserved2: 0,
                lpReserved2: NULL as _,
                hStdInput: NULL as _,
                hStdOutput: NULL as _,
                hStdError: NULL as _,
            };
            let mut proc_info = PROCESS_INFORMATION {
                hProcess: NULL as _,
                hThread: NULL as _,
                dwProcessId: 0,
                dwThreadId: 0,
            };

            let session_id = WTSGetActiveConsoleSessionId();
            let token = get_user_token(session_id, true);
            if token.is_null() {
                bail!("Failed to get token of current user");
            }

            let create_res = CreateProcessAsUserW(
                token,
                NULL as _,
                cmd_utf16.as_ptr() as _,
                NULL as _,
                NULL as _,
                FALSE,
                CREATE_SUSPENDED | DETACHED_PROCESS,
                NULL,
                NULL as _,
                &mut start_info,
                &mut proc_info,
            );
            CloseHandle(token);
            if 0 == create_res {
                bail!(
                    "Failed to create privacy window process {}, error {}",
                    cmdline,
                    Error::last_os_error()
                );
            };

            if let Err(e) = inject_dll(
                proc_info.hProcess,
                proc_info.hThread,
                dll_file.to_string_lossy().as_ref(),
            ) {
                TerminateProcess(proc_info.hProcess, 0);
                CloseHandle(proc_info.hThread);
                CloseHandle(proc_info.hProcess);
                return Err(e);
            }

            if 0xffffffff == ResumeThread(proc_info.hThread) {
                TerminateProcess(proc_info.hProcess, 0);
                CloseHandle(proc_info.hThread);
                CloseHandle(proc_info.hProcess);

                bail!(
                    "Failed to create privacy window process, error {}",
                    Error::last_os_error()
                );
            }

            self.handlers.hthread = proc_info.hThread as _;
            self.handlers.hprocess = proc_info.hProcess as _;

            if let Err(e) = wait_find_privacy_hwnds(PRIVACY_WINDOW_WAIT_MILLIS) {
                self.handlers.reset();
                return Err(e);
            }
        }

        Ok(())
    }

    #[inline]
    pub fn stop(&mut self) {
        if self.native_hwnd != 0 {
            unsafe {
                if FALSE == PostMessageA(self.native_hwnd as HWND, WM_CLOSE, 0, 0) {
                    log::error!("Failed to close native privacy window: {}", Error::last_os_error());
                }
            }
            self.native_hwnd = 0;
        }
        self.handlers.reset();
    }

    fn show_privacy_windows(&mut self, conn_id: i32, hook_input: bool) -> ResultType<()> {
        let hwnds = wait_find_privacy_hwnds(PRIVACY_WINDOW_WAIT_MILLIS)?;
        if hwnds.is_empty() {
            bail!("No privacy window created");
        }

        if hook_input {
            super::win_input::hook()?;
        }
        match set_privacy_windows_visible(&hwnds, true) {
            Ok(_) => {
                let visible_hwnds =
                    match wait_find_visible_privacy_hwnds(PRIVACY_WINDOW_WAIT_MILLIS) {
                        Ok(hwnds) => hwnds,
                        Err(e) => {
                            allow_err!(set_privacy_windows_visible(&hwnds, false));
                            if hook_input {
                                allow_err!(super::win_input::unhook());
                            }
                            return Err(e);
                        }
                    };
                let Some(hwnd) = visible_hwnds.first() else {
                    allow_err!(set_privacy_windows_visible(&hwnds, false));
                    if hook_input {
                        allow_err!(super::win_input::unhook());
                    }
                    bail!("No visible privacy window created");
                };
                self.conn_id = conn_id;
                self.hwnd = *hwnd as _;
                Ok(())
            }
            Err(e) => {
                allow_err!(set_privacy_windows_visible(&hwnds, false));
                if hook_input {
                    allow_err!(super::win_input::unhook());
                }
                Err(e)
            }
        }
    }
}

impl Drop for PrivacyModeImpl {
    fn drop(&mut self) {
        if self.conn_id != INVALID_PRIVACY_MODE_CONN_ID {
            allow_err!(self.turn_off_privacy(self.conn_id, None));
        }
        self.stop();
    }
}

// 交互宿主自己创建隐私窗，无需系统服务令牌或向 RuntimeBroker 注入 DLL。
fn start_native_privacy_window() -> ResultType<u64> {
    if !super::win_exclude_from_capture::is_supported() {
        bail!("当前 Windows 不支持便携隐私屏，需要 Windows 10 2004 或更新版本。");
    }
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new().name("privacy-window".into()).spawn(move || {
        let create = || -> ResultType<HWND> { unsafe {
            let instance = GetModuleHandleA(std::ptr::null());
            let class_name = CString::new(PRIVACY_WINDOW_CLASS)?;
            let window_name = CString::new(PRIVACY_WINDOW_NAME)?;
            let class = WNDCLASSA {
                lpfnWndProc: Some(native_privacy_window_proc),
                hInstance: instance,
                hbrBackground: GetStockObject(BLACK_BRUSH as _) as HBRUSH,
                lpszClassName: class_name.as_ptr(),
                ..std::mem::zeroed()
            };
            if RegisterClassA(&class) == 0
                && winapi::um::errhandlingapi::GetLastError() != winapi::shared::winerror::ERROR_CLASS_ALREADY_EXISTS
            {
                bail!("无法注册隐私屏窗口：{}", Error::last_os_error());
            }
            let hwnd = CreateWindowExA(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class_name.as_ptr(), window_name.as_ptr(), WS_POPUP,
                GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN),
                std::ptr::null_mut(), std::ptr::null_mut(), instance, std::ptr::null_mut(),
            );
            if hwnd.is_null() { bail!("无法创建隐私屏窗口：{}", Error::last_os_error()); }
            // 先确认排除屏幕采集成功，再允许隐私提示窗显示；失败时不遮挡屏幕。
            if FALSE == SetWindowDisplayAffinity(hwnd, 0x00000011) {
                let error = Error::last_os_error();
                DestroyWindow(hwnd);
                bail!("无法从屏幕采集中排除隐私屏：{error}");
            }
            Ok(hwnd)
        }};
        match create() {
            Ok(hwnd) => unsafe {
                if tx.send(Ok(hwnd as u64)).is_err() {
                    DestroyWindow(hwnd);
                    return;
                }
                let mut message = std::mem::zeroed();
                while GetMessageA(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
                    TranslateMessage(&message);
                    DispatchMessageA(&message);
                }
                if IsWindow(hwnd) != FALSE { DestroyWindow(hwnd); }
            },
            Err(error) => { let _ = tx.send(Err(error)); }
        }
    })?;
    rx.recv_timeout(Duration::from_secs(3))
        .map_err(|error| hbb_common::anyhow::anyhow!("创建隐私屏窗口超时：{error}"))?
}

unsafe extern "system" fn native_privacy_window_proc(hwnd: HWND, message: UINT, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_RUSTDESK_SHOW_WINDOWS | WM_DISPLAYCHANGE | WM_DPICHANGED => {
            SetWindowPos(hwnd, HWND_TOPMOST,
                GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN),
                SWP_NOACTIVATE);
            if message == WM_RUSTDESK_SHOW_WINDOWS { ShowWindow(hwnd, SW_SHOWNOACTIVATE); }
            InvalidateRect(hwnd, std::ptr::null(), FALSE);
            0
        },
        WM_PAINT => {
            paint_native_privacy_window(hwnd);
            0
        },
        WM_RUSTDESK_HIDE_WINDOWS => { ShowWindow(hwnd, SW_HIDE); 0 },
        WM_CLOSE => { DestroyWindow(hwnd); 0 },
        WM_DESTROY => { PostQuitMessage(0); 0 },
        _ => DefWindowProcA(hwnd, message, wparam, lparam),
    }
}

// 在同一个采集排除窗口内绘制，每块显示器分别居中，避免提示跨越屏幕边界。
unsafe fn paint_native_privacy_window(hwnd: HWND) {
    let mut paint: PAINTSTRUCT = std::mem::zeroed();
    let hdc = BeginPaint(hwnd, &mut paint);
    if hdc.is_null() {
        log::warn!("无法绘制隐私屏：{}", Error::last_os_error());
        return;
    }
    let mut client: RECT = std::mem::zeroed();
    if GetClientRect(hwnd, &mut client) == FALSE {
        log::warn!("无法获取隐私屏绘制区域：{}", Error::last_os_error());
        EndPaint(hwnd, &paint);
        return;
    }
    SetDCBrushColor(hdc, 0x00221C18);
    FillRect(hdc, &client, GetStockObject(DC_BRUSH as _) as HBRUSH);
    SetBkMode(hdc, TRANSPARENT as _);

    let mut window: RECT = std::mem::zeroed();
    if GetWindowRect(hwnd, &mut window) == FALSE {
        log::warn!("无法获取隐私屏窗口位置：{}", Error::last_os_error());
        EndPaint(hwnd, &paint);
        return;
    }
    let monitors = match get_monitor_rects() {
        Ok(rects) if !rects.is_empty() => rects,
        result => {
            if let Err(error) = result {
                log::warn!("无法获取隐私屏显示器布局：{error}");
            }
            vec![window]
        }
    };
    for monitor in monitors {
        let hmonitor = MonitorFromRect(&monitor, MONITOR_DEFAULTTONEAREST);
        let dpi_y = privacy_monitor_dpi(hmonitor);
        // 在小屏幕上缩小字号，长说明允许自动换行。
        let scale = (dpi_y as f32 / 96.0)
            .min((monitor.right - monitor.left) as f32 / 560.0)
            .min((monitor.bottom - monitor.top) as f32 / 320.0)
            .max(0.5);
        let center_y = (monitor.top + monitor.bottom) / 2 - window.top;
        let margin = (24.0 * scale) as i32;
        let left = monitor.left - window.left + margin;
        let right = monitor.right - window.left - margin;
        for (text, size, weight, color, top, bottom) in [
            ("SubnetDesk", 18.0, FW_NORMAL, 0x00C9B6A7, -106.0, -70.0),
            ("此电脑正在被远程控制", 30.0, FW_SEMIBOLD, 0x00F8F5F2, -48.0, 8.0),
            ("隐私模式已开启，本机屏幕内容暂时隐藏。\n远程会话结束或关闭隐私模式后恢复显示。", 16.0, FW_NORMAL, 0x00C9B6A7, 32.0, 116.0),
        ] {
            let mut rect = RECT {
                left, right,
                top: center_y + (top * scale) as i32,
                bottom: center_y + (bottom * scale) as i32,
            };
            draw_privacy_text(hdc, text, &mut rect, (size * scale) as i32, weight, color);
        }
    }
    EndPaint(hwnd, &paint);
}

unsafe fn draw_privacy_text(hdc: HDC, text: &str, rect: &mut RECT, height: i32, weight: i32, color: u32) {
    let face: Vec<u16> = "Microsoft YaHei UI\0".encode_utf16().collect();
    let font = CreateFontW(-height, 0, 0, 0, weight, 0, 0, 0,
        DEFAULT_CHARSET as _, OUT_DEFAULT_PRECIS as _, CLIP_DEFAULT_PRECIS as _,
        CLEARTYPE_QUALITY as _, DEFAULT_PITCH as _, face.as_ptr());
    if font.is_null() {
        log::warn!("无法创建隐私屏字体，使用系统字体：{}", Error::last_os_error());
    }
    let selected_font = if font.is_null() { GetStockObject(DEFAULT_GUI_FONT as _) } else { font as _ };
    let previous = SelectObject(hdc, selected_font);
    SetTextColor(hdc, color);
    let text: Vec<u16> = text.encode_utf16().collect();
    DrawTextW(hdc, text.as_ptr(), text.len() as i32, rect, DT_CENTER | DT_WORDBREAK | DT_NOPREFIX);
    SelectObject(hdc, previous);
    if !font.is_null() { DeleteObject(font as _); }
}

unsafe fn privacy_monitor_dpi(monitor: HMONITOR) -> u32 {
    // 隐私提示仅用于 Windows 10，但程序仍需能在 Windows 7 上加载。
    // 动态查询新系统接口，避免给整个 EXE 增加 Shcore.dll 的静态依赖。
    let module = LoadLibraryExA(b"shcore.dll\0".as_ptr() as _, std::ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32);
    if module.is_null() {
        log::warn!("无法加载显示器缩放接口，使用默认字号：{}", Error::last_os_error());
        return 96;
    }
    let address = GetProcAddress(module, b"GetDpiForMonitor\0".as_ptr() as _);
    let mut x = 96;
    let mut y = 96;
    if address.is_null() {
        log::warn!("显示器缩放接口不可用，使用默认字号：{}", Error::last_os_error());
    } else {
        let get_dpi: unsafe extern "system" fn(HMONITOR, i32, *mut u32, *mut u32) -> i32 = std::mem::transmute(address);
        let result = get_dpi(monitor, 0, &mut x, &mut y);
        if result < 0 || y == 0 {
            log::warn!("无法读取显示器缩放，使用默认字号：HRESULT={result:#x}");
            y = 96;
        }
    }
    FreeLibrary(module);
    y
}

unsafe fn inject_dll<'a>(hproc: HANDLE, hthread: HANDLE, dll_file: &'a str) -> ResultType<()> {
    let dll_file_utf16: Vec<u16> = dll_file.encode_utf16().chain(Some(0).into_iter()).collect();

    let buf = VirtualAllocEx(
        hproc,
        NULL as _,
        dll_file_utf16.len() * 2,
        MEM_COMMIT,
        PAGE_READWRITE,
    );
    if buf.is_null() {
        bail!("Failed VirtualAllocEx");
    }

    let mut written: usize = 0;
    if 0 == WriteProcessMemory(
        hproc,
        buf,
        dll_file_utf16.as_ptr() as _,
        dll_file_utf16.len() * 2,
        &mut written,
    ) {
        bail!("Failed WriteProcessMemory");
    }

    let kernel32_modulename = CString::new("kernel32")?;
    let hmodule = GetModuleHandleA(kernel32_modulename.as_ptr() as _);
    if hmodule.is_null() {
        bail!("Failed GetModuleHandleA");
    }

    let load_librarya_name = CString::new("LoadLibraryW")?;
    let load_librarya = GetProcAddress(hmodule, load_librarya_name.as_ptr() as _);
    if load_librarya.is_null() {
        bail!("Failed GetProcAddress of LoadLibraryW");
    }

    if 0 == QueueUserAPC(Some(std::mem::transmute(load_librarya)), hthread, buf as _) {
        bail!("Failed QueueUserAPC");
    }

    Ok(())
}

fn wait_find_privacy_hwnds(msecs: u128) -> ResultType<Vec<HWND>> {
    wait_find_privacy_hwnds_impl(msecs, false)
}

fn wait_find_visible_privacy_hwnds(msecs: u128) -> ResultType<Vec<HWND>> {
    wait_find_privacy_hwnds_impl(msecs, true)
}

fn privacy_window_wait_millis(base_millis: u128, monitor_count: usize) -> u128 {
    if base_millis == 0 {
        return 0;
    }
    // Privacy Mode 1 creates one overlay per monitor. Keep the single-monitor
    // wait as the base and add time for each extra overlay before coverage
    // verification times out.
    base_millis
        + (monitor_count.saturating_sub(1) as u128) * PRIVACY_WINDOW_WAIT_EXTRA_MONITOR_MILLIS
}

fn wait_find_privacy_hwnds_impl(msecs: u128, require_visible: bool) -> ResultType<Vec<HWND>> {
    // This verifies initial turn-on coverage. If displays change during this
    // short poll window, the DLL refreshes overlays asynchronously, while this
    // check may still time out against the geometry sampled here.
    let monitor_rects = get_monitor_rects()?;
    if monitor_rects.is_empty() {
        bail!("No privacy monitor found");
    }
    let msecs = privacy_window_wait_millis(msecs, monitor_rects.len());

    let tm_begin = Instant::now();
    loop {
        let hwnds = find_privacy_hwnds()?;
        let visible_hwnds = if require_visible {
            filter_visible_hwnds(&hwnds)
        } else {
            Vec::new()
        };
        let covered_hwnds = if require_visible {
            visible_hwnds.as_slice()
        } else {
            hwnds.as_slice()
        };
        let covered = count_covered_monitors(covered_hwnds, &monitor_rects);
        if covered == monitor_rects.len() {
            return Ok(if require_visible {
                visible_hwnds
            } else {
                hwnds
            });
        }

        if msecs == 0 || tm_begin.elapsed().as_millis() > msecs {
            let visible = if require_visible { "visible " } else { "" };
            bail!(
                "Expected {}privacy windows to cover {} monitors, covered {}, found {}",
                visible,
                monitor_rects.len(),
                covered,
                hwnds.len(),
            );
        }

        std::thread::sleep(Duration::from_millis(PRIVACY_WINDOW_POLL_INTERVAL_MILLIS));
    }
}

fn find_privacy_hwnds() -> ResultType<Vec<HWND>> {
    let class_name = CString::new(PRIVACY_WINDOW_CLASS)?;
    let wndname = CString::new(PRIVACY_WINDOW_NAME)?;
    let mut hwnds = Vec::new();
    unsafe {
        let mut after = NULL as _;
        loop {
            let hwnd = FindWindowExA(
                NULL as _,
                after,
                class_name.as_ptr() as _,
                wndname.as_ptr() as _,
            );
            if hwnd.is_null() {
                break;
            }
            hwnds.push(hwnd);
            after = hwnd;
        }
    }
    Ok(hwnds)
}

fn filter_visible_hwnds(hwnds: &[HWND]) -> Vec<HWND> {
    hwnds
        .iter()
        .copied()
        .filter(|hwnd| unsafe { FALSE != IsWindowVisible(*hwnd) })
        .collect()
}

fn set_privacy_windows_visible(hwnds: &[HWND], show: bool) -> ResultType<usize> {
    if hwnds.is_empty() {
        return Ok(0);
    };
    let message = if show {
        WM_RUSTDESK_SHOW_WINDOWS
    } else {
        WM_RUSTDESK_HIDE_WINDOWS
    };
    let mut posted = 0;
    let mut first_error = None;
    for &hwnd in hwnds {
        unsafe {
            if FALSE == PostMessageA(hwnd, message, 0, 0) {
                if first_error.is_none() {
                    first_error = Some(Error::last_os_error());
                }
            } else {
                posted += 1;
            }
        }
    }
    if let Some(error) = first_error {
        bail!(
            "Failed to post privacy window visibility message to all privacy windows, posted {}/{}, first error {}",
            posted,
            hwnds.len(),
            error,
        );
    }
    Ok(posted)
}

fn get_monitor_rects() -> ResultType<Vec<RECT>> {
    let mut rects = Vec::new();
    unsafe {
        if FALSE
            == EnumDisplayMonitors(
                NULL as _,
                NULL as _,
                Some(enum_monitor_rect_proc),
                &mut rects as *mut Vec<RECT> as LPARAM,
            )
        {
            bail!(
                "Failed EnumDisplayMonitors, error {}",
                Error::last_os_error()
            );
        }
    }
    Ok(rects)
}

unsafe extern "system" fn enum_monitor_rect_proc(
    hmon: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    let rects = &mut *(lparam as *mut Vec<RECT>);
    let mut monitor_info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as _,
        rcMonitor: RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        rcWork: RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        dwFlags: 0,
    };
    if FALSE == GetMonitorInfoA(hmon, &mut monitor_info) {
        return FALSE;
    }
    rects.push(monitor_info.rcMonitor);
    TRUE
}

fn count_covered_monitors(hwnds: &[HWND], monitor_rects: &[RECT]) -> usize {
    let mut covered = 0;
    for monitor_rect in monitor_rects {
        for hwnd in hwnds {
            let mut window_rect = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            unsafe {
                if FALSE == GetWindowRect(*hwnd, &mut window_rect) {
                    log::warn!(
                        "Failed GetWindowRect for privacy window, error {}",
                        Error::last_os_error()
                    );
                    continue;
                }
            }
            if rect_covers(&window_rect, monitor_rect) {
                covered += 1;
                break;
            }
        }
    }
    covered
}

fn rect_covers(window_rect: &RECT, monitor_rect: &RECT) -> bool {
    window_rect.left <= monitor_rect.left
        && window_rect.top <= monitor_rect.top
        && window_rect.right >= monitor_rect.right
        && window_rect.bottom >= monitor_rect.bottom
}
