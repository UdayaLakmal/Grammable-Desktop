// Grammable — Copyright (C) 2026 Udaya Lakmal — GPL-3.0-or-later
//
// Windows: Win32 for the foreground window and its process, UI Automation for
// the selection and password fields. Apps running as administrator can't be
// read or typed into from a normal process; that is a Windows rule.

use std::ffi::c_void;

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE,
};

use super::{Probe, Target};

pub fn foreground() -> Option<Target> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_null() {
        return None;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    let pid = (pid != 0).then_some(pid);
    Some(Target { handle: hwnd as usize as u64, pid, process: pid.and_then(process_name) })
}

fn process_name(pid: u32) -> Option<String> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut buffer = [0u16; 1024];
        let mut len = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut len);
        CloseHandle(process);
        if ok == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buffer[..len as usize]);
        let file = path.rsplit(['\\', '/']).next()?;
        Some(file.strip_suffix(".exe").or_else(|| file.strip_suffix(".EXE")).unwrap_or(file).to_string())
    }
}

pub fn activate(target: &Target) -> bool {
    let hwnd = target.handle as usize as *mut c_void;
    unsafe {
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        }
        SetForegroundWindow(hwnd) != 0
    }
}

pub fn modifiers_held() -> bool {
    [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN]
        .iter()
        .any(|vk| unsafe { GetAsyncKeyState(*vk as i32) } as u16 & 0x8000 != 0)
}

/// UI Automation: password check and the focused control's selected text.
/// Runs on its own thread so COM is initialised there and nowhere else.
pub fn probe_selection() -> Probe {
    std::thread::spawn(|| {
        use uiautomation::patterns::UITextPattern;
        use uiautomation::UIAutomation;

        let Ok(automation) = UIAutomation::new() else { return Probe::Unknown };
        let Ok(element) = automation.get_focused_element() else { return Probe::Unknown };
        if element.is_password().unwrap_or(false) {
            return Probe::Password;
        }
        let Ok(pattern) = element.get_pattern::<UITextPattern>() else { return Probe::Unknown };
        let Ok(ranges) = pattern.get_selection() else { return Probe::Unknown };
        let text: Vec<String> = ranges.iter().filter_map(|r| r.get_text(-1).ok()).collect();
        let text = text.join("\n");
        if text.trim().is_empty() { Probe::Unknown } else { Probe::Text(text) }
    })
    .join()
    .unwrap_or(Probe::Unknown)
}
