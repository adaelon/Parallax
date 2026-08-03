//! Minimal native foreground sampler.
//!
//! This is the only module allowed to call Win32 directly. It returns bounded
//! metadata or an explicit gap signal and never captures pixels or keystrokes.

#![allow(unsafe_code)]

use std::time::Duration;

use crate::{ActivitySnapshot, IdleState};

pub const DEFAULT_IDLE_THRESHOLD: Duration = Duration::from_mins(5);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeCaptureSample {
    Foreground(ActivitySnapshot),
    SessionLocked,
    SourceUnavailable,
}

#[must_use]
pub fn sample_foreground_activity(idle_threshold: Duration) -> NativeCaptureSample {
    imp::sample(idle_threshold)
}

fn idle_state_from_millis(idle_millis: u64, threshold: Duration) -> IdleState {
    if u128::from(idle_millis) >= threshold.as_millis() {
        IdleState::Idle
    } else {
        IdleState::Active
    }
}

#[cfg(not(windows))]
mod imp {
    use super::{Duration, NativeCaptureSample};

    pub(super) fn sample(_idle_threshold: Duration) -> NativeCaptureSample {
        NativeCaptureSample::SourceUnavailable
    }
}

#[cfg(windows)]
mod imp {
    use std::{ffi::OsString, mem, os::windows::ffi::OsStringExt, path::PathBuf, ptr};

    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, HWND},
        System::{
            StationsAndDesktops::{
                CloseDesktop, DESKTOP_SWITCHDESKTOP, OpenInputDesktop, SwitchDesktop,
            },
            SystemInformation::GetTickCount64,
            Threading::{
                OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
                QueryFullProcessImageNameW,
            },
        },
        UI::{
            Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
            WindowsAndMessaging::{
                GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
            },
        },
    };

    use super::{ActivitySnapshot, Duration, NativeCaptureSample, idle_state_from_millis};

    const MAX_PROCESS_PATH_UTF16: usize = 32_768;
    const MAX_WINDOW_TITLE_UTF16: usize = 4_096;

    pub(super) fn sample(idle_threshold: Duration) -> NativeCaptureSample {
        if session_is_locked() {
            return NativeCaptureSample::SessionLocked;
        }
        foreground_snapshot(idle_threshold).map_or(
            NativeCaptureSample::SourceUnavailable,
            NativeCaptureSample::Foreground,
        )
    }

    fn foreground_snapshot(idle_threshold: Duration) -> Option<ActivitySnapshot> {
        // SAFETY: `GetForegroundWindow` takes no pointers and returns a borrowed
        // HWND that remains valid for the immediately following metadata calls.
        let window = unsafe { GetForegroundWindow() };
        if window.is_null() {
            return None;
        }
        let application = process_name(window)?;
        let title = window_title(window)?;
        let idle_state = idle_state_from_millis(last_input_millis()?, idle_threshold);
        ActivitySnapshot::new(application, title, idle_state).ok()
    }

    fn session_is_locked() -> bool {
        // SAFETY: no borrowed pointers cross the call. The returned desktop
        // handle is closed on every non-null path before this function returns.
        let desktop = unsafe { OpenInputDesktop(0, 0, DESKTOP_SWITCHDESKTOP) };
        if desktop.is_null() {
            return true;
        }
        // SAFETY: `desktop` is the live handle returned immediately above.
        let switchable = unsafe { SwitchDesktop(desktop) } != 0;
        // SAFETY: `desktop` is owned by this function and closed exactly once.
        let _ = unsafe { CloseDesktop(desktop) };
        !switchable
    }

    fn window_title(window: HWND) -> Option<String> {
        // SAFETY: `window` is a borrowed HWND from `GetForegroundWindow`.
        let reported = unsafe { GetWindowTextLengthW(window) };
        if reported < 0 {
            return None;
        }
        let capacity = usize::try_from(reported)
            .ok()?
            .saturating_add(1)
            .clamp(1, MAX_WINDOW_TITLE_UTF16);
        let mut buffer = vec![0_u16; capacity];
        // SAFETY: the buffer is writable for `capacity` UTF-16 code units and
        // remains live for the duration of the call.
        let copied =
            unsafe { GetWindowTextW(window, buffer.as_mut_ptr(), i32::try_from(capacity).ok()?) };
        if copied < 0 {
            return None;
        }
        Some(String::from_utf16_lossy(
            &buffer[..usize::try_from(copied).ok()?],
        ))
    }

    fn process_name(window: HWND) -> Option<String> {
        let mut process_id = 0_u32;
        // SAFETY: `process_id` is a live writable output and `window` is a
        // borrowed foreground HWND.
        let thread_id = unsafe { GetWindowThreadProcessId(window, &raw mut process_id) };
        if thread_id == 0 || process_id == 0 {
            return None;
        }
        // SAFETY: the call takes values only; the returned handle is wrapped
        // immediately and closed by `OwnedHandle::drop`.
        let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
        let process = OwnedHandle::new(raw)?;
        let mut buffer = vec![0_u16; MAX_PROCESS_PATH_UTF16];
        let mut length = u32::try_from(buffer.len()).ok()?;
        // SAFETY: `process` owns a live process handle and `buffer` is
        // writable for the length advertised through `length`.
        let succeeded = unsafe {
            QueryFullProcessImageNameW(
                process.raw(),
                PROCESS_NAME_WIN32,
                buffer.as_mut_ptr(),
                &raw mut length,
            )
        };
        if succeeded == 0 || length == 0 {
            return None;
        }
        let path = PathBuf::from(OsString::from_wide(
            &buffer[..usize::try_from(length).ok()?],
        ));
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.trim().is_empty())
    }

    fn last_input_millis() -> Option<u64> {
        let mut input = LASTINPUTINFO {
            cbSize: u32::try_from(mem::size_of::<LASTINPUTINFO>()).ok()?,
            dwTime: 0,
        };
        // SAFETY: `input` is fully initialized and writable for the Win32
        // structure size declared in `cbSize`.
        if unsafe { GetLastInputInfo(&raw mut input) } == 0 {
            return None;
        }
        // SAFETY: `GetTickCount64` takes no pointers and has no preconditions.
        let now = u32::try_from(unsafe { GetTickCount64() } & u64::from(u32::MAX)).ok()?;
        Some(u64::from(now.wrapping_sub(input.dwTime)))
    }

    struct OwnedHandle(HANDLE);

    impl OwnedHandle {
        fn new(handle: HANDLE) -> Option<Self> {
            (!handle.is_null()).then_some(Self(handle))
        }

        const fn raw(&self) -> HANDLE {
            self.0
        }
    }

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            // SAFETY: this wrapper owns one non-null process handle and drops
            // exactly once after all Win32 calls using it have completed.
            let _ = unsafe { CloseHandle(self.0) };
            self.0 = ptr::null_mut();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_IDLE_THRESHOLD, IdleState, idle_state_from_millis};

    #[test]
    fn idle_threshold_is_inclusive_and_does_not_collect_input_content() {
        assert_eq!(
            idle_state_from_millis(299_999, DEFAULT_IDLE_THRESHOLD),
            IdleState::Active
        );
        assert_eq!(
            idle_state_from_millis(300_000, DEFAULT_IDLE_THRESHOLD),
            IdleState::Idle
        );
    }
}
