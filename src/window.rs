use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::cell::Cell;
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect, ScreenToClient,
    },
    UI::{
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{
            GetWindowRect, HTCAPTION, IsIconic, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
            SetWindowPos, WM_NCHITTEST, WM_NCLBUTTONDBLCLK,
        },
    },
};

pub struct DragRegion {
    window: HWND,
    region: Box<Cell<RECT>>,
}

impl DragRegion {
    pub fn install(window: Option<HWND>) -> Option<Self> {
        let window = window?;
        let region = Box::new(Cell::new(RECT::default()));
        let pointer = (&*region as *const Cell<RECT>) as usize;
        // This subclass belongs only to this window and is removed before its storage is freed.
        (unsafe { SetWindowSubclass(window, Some(header_proc), 0x4d454d, pointer) } != 0)
            .then_some(Self { window, region })
    }
    pub fn set(&self, rect: Option<eframe::egui::Rect>, scale: f32) {
        self.region.set(rect.map_or(RECT::default(), |r| RECT {
            left: (r.left() * scale) as i32,
            top: (r.top() * scale) as i32,
            right: (r.right() * scale) as i32,
            bottom: (r.bottom() * scale) as i32,
        }));
    }
}
impl Drop for DragRegion {
    fn drop(&mut self) {
        unsafe {
            RemoveWindowSubclass(self.window, Some(header_proc), 0x4d454d);
        }
    }
}

fn contains(rect: RECT, point: POINT) -> bool {
    point.x >= rect.left && point.x < rect.right && point.y >= rect.top && point.y < rect.bottom
}

unsafe extern "system" fn header_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _: usize,
    data: usize,
) -> LRESULT {
    if message == WM_NCHITTEST && data != 0 {
        let mut point = POINT {
            x: (lparam as u32 as u16 as i16) as i32,
            y: ((lparam as u32 >> 16) as u16 as i16) as i32,
        };
        if unsafe { ScreenToClient(hwnd, &mut point) } != 0
            && contains(unsafe { &*(data as *const Cell<RECT>) }.get(), point)
        {
            return HTCAPTION as isize;
        }
    }
    if message == WM_NCLBUTTONDBLCLK
        && wparam == HTCAPTION as usize
        && data != 0
        && unsafe { &*(data as *const Cell<RECT>) }.get().right
            > unsafe { &*(data as *const Cell<RECT>) }.get().left
    {
        return 0;
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

pub fn handle(cc: &eframe::CreationContext<'_>) -> Option<HWND> {
    match cc.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(window) => Some(window.hwnd.get() as HWND),
        _ => None,
    }
}

fn rect(window: Option<HWND>) -> Option<RECT> {
    let window = window?;
    let mut rect = RECT::default();
    unsafe { (IsIconic(window) == 0 && GetWindowRect(window, &mut rect) != 0).then_some(rect) }
}

// Physical desktop pixels preserve negative monitor origins without mixing DPI coordinate systems.
pub fn position(window: Option<HWND>) -> Option<[i32; 2]> {
    rect(window).map(|r| [r.left, r.top])
}

fn clamp_position(position: [i32; 2], size: [i32; 2], work: RECT) -> [i32; 2] {
    [
        position[0].clamp(work.left, work.right.saturating_sub(size[0]).max(work.left)),
        position[1].clamp(work.top, work.bottom.saturating_sub(size[1]).max(work.top)),
    ]
}

pub fn restore(window: Option<HWND>, position: [i32; 2]) -> Result<[i32; 2], String> {
    let window = window.ok_or("위젯 창을 찾지 못했습니다")?;
    let current = rect(Some(window)).ok_or("위젯 위치를 읽지 못했습니다")?;
    let size = [
        (current.right - current.left).max(1),
        (current.bottom - current.top).max(1),
    ];
    let target = RECT {
        left: position[0],
        top: position[1],
        right: position[0].saturating_add(size[0]),
        bottom: position[1].saturating_add(size[1]),
    };
    unsafe {
        let monitor = MonitorFromRect(&target, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if monitor.is_null() || GetMonitorInfoW(monitor, &mut info) == 0 {
            return Err("모니터 작업 영역을 읽지 못했습니다".into());
        }
        let position = clamp_position(position, size, info.rcWork);
        if SetWindowPos(
            window,
            std::ptr::null_mut(),
            position[0],
            position[1],
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        ) == 0
        {
            return Err(format!(
                "위젯 위치를 복원하지 못했습니다: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn position_stays_on_screen_after_monitor_changes() {
        let work = RECT {
            left: -1920,
            top: -200,
            right: 0,
            bottom: 880,
        };
        assert_eq!(clamp_position([-1800, 10], [360, 340], work), [-1800, 10]);
        assert_eq!(
            clamp_position([5000, -5000], [360, 340], work),
            [-360, -200]
        );
        assert_eq!(
            clamp_position([i32::MAX, i32::MIN], [360, 340], work),
            [-360, -200]
        );
        assert_eq!(clamp_position([-10, 10], [3000, 2000], work), [-1920, -200]);
        let header = RECT {
            left: 30,
            top: 30,
            right: 220,
            bottom: 58,
        };
        assert!(contains(header, POINT { x: 40, y: 42 }));
        assert!(!contains(header, POINT { x: 221, y: 42 }));
    }
}
