use std::{
    mem::{size_of, zeroed},
    path::Path,
    time::SystemTime,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, FILETIME, HANDLE},
    Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser},
    System::{
        ProcessStatus::{
            K32EmptyWorkingSet, K32EnumProcesses, K32GetPerformanceInfo, K32GetProcessMemoryInfo,
            PERFORMANCE_INFORMATION, PROCESS_MEMORY_COUNTERS,
        },
        RemoteDesktop::ProcessIdToSessionId,
        SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX},
        Threading::{
            GetCurrentProcess, GetCurrentProcessId, GetProcessTimes, OpenProcess, OpenProcessToken,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA, PROCESS_VM_READ,
            QueryFullProcessImageNameW,
        },
    },
    UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
};

pub struct Handle(pub HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

#[derive(Clone, Debug)]
pub struct Process {
    pub pid: u32,
    pub name: String,
    pub path: String,
    pub bytes: u64,
    pub created: u64,
    pub eligible: bool,
    pub reason: String,
}

#[derive(Clone)]
pub struct Snapshot {
    pub total: u64,
    pub available: u64,
    pub commit: u64,
    pub commit_limit: u64,
    pub processes: Vec<Process>,
    pub at: SystemTime,
}
impl Snapshot {
    pub fn percent(&self) -> f32 {
        (self.total.saturating_sub(self.available)) as f32 / self.total.max(1) as f32
    }
}

#[derive(Default)]
pub struct TrimResult {
    pub count: usize,
    pub reclaimed: u64,
    pub errors: Vec<String>,
}

fn error(context: &str) -> String {
    format!("{context}: {}", std::io::Error::last_os_error())
}

fn sid(handle: HANDLE) -> Option<Vec<u8>> {
    unsafe {
        let mut raw = std::ptr::null_mut();
        if OpenProcessToken(handle, TOKEN_QUERY, &mut raw) == 0 {
            return None;
        }
        let token = Handle(raw);
        let mut len = 0;
        GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &mut len);
        if len == 0 {
            return None;
        }
        // usize storage keeps TOKEN_USER aligned; copy only the SID bytes out.
        let mut storage = vec![0usize; (len as usize).div_ceil(size_of::<usize>())];
        if GetTokenInformation(
            token.0,
            TokenUser,
            storage.as_mut_ptr().cast(),
            len,
            &mut len,
        ) == 0
        {
            return None;
        }
        let user = &*(storage.as_ptr().cast::<TOKEN_USER>());
        let sid_len = windows_sys::Win32::Security::GetLengthSid(user.User.Sid);
        if sid_len == 0 {
            return None;
        }
        Some(std::slice::from_raw_parts(user.User.Sid.cast::<u8>(), sid_len as usize).to_vec())
    }
}

fn working_set(handle: HANDLE) -> Option<u64> {
    unsafe {
        let mut counters: PROCESS_MEMORY_COUNTERS = zeroed();
        (K32GetProcessMemoryInfo(
            handle,
            &mut counters,
            size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ) != 0)
            .then_some(counters.WorkingSetSize as u64)
    }
}

fn created(handle: HANDLE) -> Option<u64> {
    unsafe {
        let mut times: [FILETIME; 4] = zeroed();
        let [c, e, k, u] = &mut times;
        (GetProcessTimes(handle, c, e, k, u) != 0)
            .then_some(((c.dwHighDateTime as u64) << 32) | c.dwLowDateTime as u64)
    }
}

fn foreground_pid() -> u32 {
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(GetForegroundWindow(), &mut pid);
    }
    pid
}

pub(crate) fn protected_foreground_pid() -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static LAST_EXTERNAL: AtomicU32 = AtomicU32::new(0);
    let pid = foreground_pid();
    if pid != 0 && pid != unsafe { GetCurrentProcessId() } {
        LAST_EXTERNAL.store(pid, Ordering::Relaxed);
        pid
    } else {
        LAST_EXTERNAL.load(Ordering::Relaxed)
    }
}

fn blocked_reason(pid: u32, name: &str, path: &str, foreground: u32) -> Option<&'static str> {
    let name = name.to_ascii_lowercase();
    if pid == unsafe { GetCurrentProcessId() } {
        return Some("이 앱");
    }
    if pid == foreground {
        return Some("현재 사용 중인 앱");
    }
    let windows = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    if Path::new(&path.to_ascii_lowercase()).starts_with(windows.to_ascii_lowercase()) {
        return Some("Windows 구성 요소");
    }
    if [
        "explorer.exe",
        "msmpeng.exe",
        "nissrv.exe",
        "securityhealthsystray.exe",
        "lsass.exe",
        "dwm.exe",
        "csrss.exe",
        "wininit.exe",
        "winlogon.exe",
        "services.exe",
        "smss.exe",
        "registry",
        "system",
        "memory compression",
    ]
    .contains(&name.as_str())
        || name.contains("antimalware")
        || name.contains("defender")
    {
        return Some("시스템 또는 보안 프로세스");
    }
    None
}

pub fn snapshot() -> Result<Snapshot, String> {
    unsafe {
        let mut mem: MEMORYSTATUSEX = zeroed();
        mem.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
        if GlobalMemoryStatusEx(&mut mem) == 0 {
            return Err(error("메모리 정보를 읽지 못했습니다"));
        }
        let mut perf: PERFORMANCE_INFORMATION = zeroed();
        if K32GetPerformanceInfo(&mut perf, size_of::<PERFORMANCE_INFORMATION>() as u32) == 0 {
            return Err(error("커밋 정보를 읽지 못했습니다"));
        }
        let owner = sid(GetCurrentProcess()).ok_or("현재 사용자 확인 실패")?;
        let mut own_session = 0;
        if ProcessIdToSessionId(GetCurrentProcessId(), &mut own_session) == 0 {
            return Err(error("세션 확인 실패"));
        }
        let mut ids = vec![0u32; 2048];
        let mut needed = 0;
        loop {
            if K32EnumProcesses(ids.as_mut_ptr(), (ids.len() * 4) as u32, &mut needed) == 0 {
                return Err(error("프로세스 목록 확인 실패"));
            }
            if needed as usize / 4 < ids.len() {
                break;
            }
            ids.resize(ids.len() * 2, 0);
        }
        ids.truncate(needed as usize / 4);
        let foreground = protected_foreground_pid();
        let mut processes = Vec::new();
        for pid in ids {
            let mut session = 0;
            if ProcessIdToSessionId(pid, &mut session) == 0 || session != own_session {
                continue;
            }
            let raw = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid);
            if raw.is_null() {
                continue;
            }
            let handle = Handle(raw);
            if sid(handle.0).as_ref() != Some(&owner) {
                continue;
            }
            let mut path = vec![0u16; 32768];
            let mut len = path.len() as u32;
            if QueryFullProcessImageNameW(handle.0, 0, path.as_mut_ptr(), &mut len) == 0 {
                continue;
            }
            let path = String::from_utf16_lossy(&path[..len as usize]);
            let name = Path::new(&path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let Some(bytes) = working_set(handle.0) else {
                continue;
            };
            let Some(created) = created(handle.0) else {
                continue;
            };
            let reason =
                blocked_reason(pid, &name, &path, foreground).or(if bytes < 16 * 1024 * 1024 {
                    Some("16 MB 미만")
                } else {
                    None
                });
            processes.push(Process {
                pid,
                name,
                path,
                bytes,
                created,
                eligible: reason.is_none(),
                reason: reason.unwrap_or("선택하여 정리").into(),
            });
        }
        processes.sort_by_key(|p| std::cmp::Reverse(p.bytes));
        Ok(Snapshot {
            total: mem.ullTotalPhys,
            available: mem.ullAvailPhys,
            commit: perf.CommitTotal as u64 * perf.PageSize as u64,
            commit_limit: perf.CommitLimit as u64 * perf.PageSize as u64,
            processes,
            at: SystemTime::now(),
        })
    }
}

fn trim_one(process: &Process) -> Result<u64, String> {
    unsafe {
        if !process.eligible {
            return Err("정리 대상이 아닌 프로세스입니다".into());
        }
        if let Some(reason) = blocked_reason(
            process.pid,
            &process.name,
            &process.path,
            protected_foreground_pid(),
        ) {
            return Err(reason.into());
        }
        let raw = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_QUOTA | PROCESS_VM_READ,
            0,
            process.pid,
        );
        if raw.is_null() {
            return Err(error("앱이 종료되었거나 접근할 수 없습니다"));
        }
        let handle = Handle(raw);
        if created(handle.0) != Some(process.created) {
            return Err("프로세스가 바뀌었습니다. 새로 고친 뒤 다시 선택하세요".into());
        }
        let owner = sid(GetCurrentProcess()).ok_or("현재 사용자 확인 실패")?;
        if sid(handle.0).as_ref() != Some(&owner) {
            return Err("프로세스 소유자를 확인할 수 없습니다".into());
        }
        let (mut current_session, mut target_session) = (0, 0);
        if ProcessIdToSessionId(GetCurrentProcessId(), &mut current_session) == 0
            || ProcessIdToSessionId(process.pid, &mut target_session) == 0
            || current_session != target_session
        {
            return Err("현재 세션의 프로세스가 아닙니다".into());
        }
        let before = working_set(handle.0).ok_or("작업 집합을 확인하지 못했습니다")?;
        if before < 16 * 1024 * 1024 {
            return Err("작업 집합이 이미 16 MB 미만입니다".into());
        }
        if K32EmptyWorkingSet(handle.0) == 0 {
            return Err(error("작업 집합 정리 실패"));
        }
        let after = working_set(handle.0).ok_or("정리는 요청했지만 결과를 측정하지 못했습니다")?;
        Ok(before.saturating_sub(after))
    }
}

pub fn trim(processes: &[Process]) -> TrimResult {
    let mut result = TrimResult::default();
    for p in processes {
        match trim_one(p) {
            Ok(bytes) => {
                result.count += 1;
                result.reclaimed += bytes;
            }
            Err(e) => result.errors.push(format!("{} ({}): {e}", p.name, p.pid)),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_memory_and_protected_processes() {
        let s = snapshot().unwrap();
        assert!(s.total > 0 && s.available <= s.total);
        assert!(s.commit_limit >= s.commit);
        assert!(
            blocked_reason(
                unsafe { GetCurrentProcessId() },
                "memori.exe",
                "D:\\memori.exe",
                0
            )
            .is_some()
        );
        assert!(
            blocked_reason(99, "anything.exe", "C:\\Windows\\System32\\anything.exe", 0).is_some()
        );
        assert!(blocked_reason(123, "app.exe", "D:\\app.exe", 123).is_some());
    }
    #[test]
    fn refuses_own_process_even_if_marked_eligible() {
        let mut p = snapshot()
            .unwrap()
            .processes
            .into_iter()
            .find(|p| p.pid == unsafe { GetCurrentProcessId() })
            .unwrap();
        assert!(trim_one(&p).is_err());
        p.eligible = true;
        assert!(trim_one(&p).is_err());
    }

    #[test]
    #[ignore = "helper process used only by trim_only_a_disposable_child"]
    fn trim_child_helper() {
        let mut data = vec![0u8; 64 * 1024 * 1024];
        for i in (0..data.len()).step_by(4096) {
            data[i] = 1;
        }
        std::hint::black_box(&data);
        std::thread::sleep(std::time::Duration::from_secs(30));
    }

    #[test]
    fn trim_only_a_disposable_child() {
        struct Child(std::process::Child);
        impl Drop for Child {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let child = Child(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "memory::tests::trim_child_helper",
                    "--ignored",
                    "--nocapture",
                ])
                .stdout(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        );
        let start = std::time::Instant::now();
        let process = loop {
            if let Some(p) = snapshot()
                .unwrap()
                .processes
                .into_iter()
                .find(|p| p.pid == child.0.id() && p.bytes > 48 * 1024 * 1024)
            {
                break p;
            }
            assert!(
                start.elapsed().as_secs() < 10,
                "test child failed to allocate its working set"
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        };
        assert!(process.eligible);
        let mut stale = process.clone();
        stale.created ^= 1;
        assert!(
            trim_one(&stale)
                .unwrap_err()
                .contains("프로세스가 바뀌었습니다")
        );
        let result = trim(&[process]);
        assert_eq!(result.count, 1, "{:?}", result.errors);
        assert!(result.reclaimed > 32 * 1024 * 1024);
    }
}
