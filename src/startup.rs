use std::{
    collections::HashSet,
    ffi::OsString,
    fs::{self, File},
    hash::{Hash, Hasher},
    io::{self, Read},
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        fs::MetadataExt,
    },
    path::{Path, PathBuf},
};
use windows_sys::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT},
        Storage::FileSystem::MoveFileW,
        System::{
            Com::CoTaskMemFree,
            Registry::RegFlushKey,
            Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject},
        },
        UI::Shell::{
            FOLDERID_CommonStartup, FOLDERID_LocalAppData, FOLDERID_Startup, SHGetKnownFolderPath,
        },
    },
    core::GUID,
};
use winreg::{RegKey, RegValue, enums::*};

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const BACKUP: &str = r"Software\Memori\DisabledRun";
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";
const DISABLED_FOLDER: &str = "MemoriDisabledStartup";
const REPARSE_POINT: u32 = 0x400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupState {
    Enabled,
    Disabled,
    WindowsDisabled,
    Unknown,
}

#[derive(Clone, Debug)]
pub struct StartupEntry {
    pub id: String,
    pub name: String,
    pub command: String,
    pub source: String,
    pub state: StartupState,
    pub manageable: bool,
    pub detail: String,
    target: Target,
}

#[derive(Clone, Debug, PartialEq)]
enum Target {
    Registry {
        user: bool,
        view: u32,
        saved: bool,
        value: RawValue,
    },
    Folder {
        user: bool,
        saved: bool,
        name: OsString,
        stamp: FileStamp,
    },
    Unsupported,
}

#[derive(Clone, Debug, PartialEq)]
struct RawValue {
    bytes: Vec<u8>,
    kind: RegType,
}

impl From<RegValue> for RawValue {
    fn from(v: RegValue) -> Self {
        Self {
            bytes: v.bytes,
            kind: v.vtype,
        }
    }
}
impl RawValue {
    fn value(&self) -> RegValue {
        RegValue {
            bytes: self.bytes.clone(),
            vtype: self.kind.clone(),
        }
    }
    fn command(&self) -> Option<String> {
        if !matches!(self.kind, REG_SZ | REG_EXPAND_SZ) || !self.bytes.len().is_multiple_of(2) {
            return None;
        }
        let mut chars: Vec<u16> = self
            .bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u16::from_le_bytes([p[0], p[1]]))
            .collect();
        while chars.last() == Some(&0) {
            chars.pop();
        }
        let text = String::from_utf16(&chars).ok()?;
        if text.contains('\0') || text.is_empty() {
            None
        } else {
            Some(text)
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct FileStamp {
    len: u64,
    written: u64,
    digest: u64,
}

fn error(context: &str, e: impl std::fmt::Display) -> String {
    format!("{context}: {e}")
}

fn optional_key(root: &RegKey, path: &str, flags: u32) -> io::Result<Option<RegKey>> {
    match root.open_subkey_with_flags(path, flags) {
        Ok(key) => Ok(Some(key)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn optional_value(key: &RegKey, name: &str) -> io::Result<Option<RawValue>> {
    match key.get_raw_value(name) {
        Ok(v) => Ok(Some(v.into())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn approved(user: bool, subkey: &str, name: &str) -> io::Result<StartupState> {
    let roots = if user {
        vec![HKEY_CURRENT_USER]
    } else {
        vec![HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE]
    };
    let mut state = StartupState::Enabled;
    for root in roots {
        let root = RegKey::predef(root);
        let Some(key) = optional_key(
            &root,
            &format!(r"{APPROVED}\{subkey}"),
            KEY_READ | KEY_WOW64_64KEY,
        )?
        else {
            continue;
        };
        let Some(value) = optional_value(&key, name)? else {
            continue;
        };
        // StartupApproved has no public write contract. Unknown formats remain read-only.
        let current = match (value.kind, value.bytes.as_slice()) {
            (REG_BINARY, bytes) if bytes.len() == 12 => {
                match u32::from_le_bytes(bytes[..4].try_into().unwrap()) {
                    2 => StartupState::Enabled,
                    3 => StartupState::WindowsDisabled,
                    _ => StartupState::Unknown,
                }
            }
            _ => StartupState::Unknown,
        };
        if current == StartupState::WindowsDisabled {
            return Ok(current);
        }
        if current == StartupState::Unknown {
            state = current;
        }
    }
    Ok(state)
}

pub fn scan() -> Result<Vec<StartupEntry>, String> {
    let mut entries = Vec::new();
    scan_registry(true, KEY_WOW64_64KEY, &mut entries)?;
    scan_registry(false, KEY_WOW64_64KEY, &mut entries)?;
    scan_registry(false, KEY_WOW64_32KEY, &mut entries)?;
    scan_folder(true, &mut entries)?;
    scan_folder(false, &mut entries)?;
    entries.sort_by_key(|e| {
        (
            e.state == StartupState::Disabled,
            e.name.to_lowercase(),
            e.id.clone(),
        )
    });
    Ok(entries)
}

fn scan_registry(user: bool, view: u32, out: &mut Vec<StartupEntry>) -> Result<(), String> {
    let root = RegKey::predef(if user {
        HKEY_CURRENT_USER
    } else {
        HKEY_LOCAL_MACHINE
    });
    let run = optional_key(&root, RUN, KEY_READ | view)
        .map_err(|e| error("시작 프로그램을 읽지 못했습니다. 새로 고침해 주세요", e))?;
    let backup = if user {
        optional_key(&root, BACKUP, KEY_READ | KEY_WOW64_64KEY)
            .map_err(|e| error("보관된 시작 항목을 읽지 못했습니다", e))?
    } else {
        None
    };
    let source = if user {
        "내 계정 · 레지스트리"
    } else if view == KEY_WOW64_32KEY {
        "모든 사용자 · 32비트"
    } else {
        "모든 사용자 · 레지스트리"
    };
    let mut seen = HashSet::new();
    if let Some(run) = run {
        for item in run.enum_values() {
            let (name, value) = item.map_err(|e| error("시작 항목을 읽지 못했습니다", e))?;
            seen.insert(name.to_lowercase());
            let value: RawValue = value.into();
            let saved = backup
                .as_ref()
                .map(|b| optional_value(b, &name))
                .transpose()
                .map_err(|e| error("백업을 확인하지 못했습니다", e))?
                .flatten();
            let conflict = saved.as_ref().is_some_and(|v| v != &value);
            let state = if conflict {
                StartupState::Unknown
            } else {
                approved(
                    user,
                    if view == KEY_WOW64_32KEY {
                        "Run32"
                    } else {
                        "Run"
                    },
                    &name,
                )
                .map_err(|e| error("Windows 사용 상태를 읽지 못했습니다", e))?
            };
            let command = value.command();
            let manageable = user && command.is_some() && state == StartupState::Enabled;
            let detail = if conflict {
                "보관된 원본과 현재 값이 다릅니다. 자동으로 덮어쓰지 않습니다."
            } else if state == StartupState::WindowsDisabled {
                "Windows에서 꺼진 항목입니다. Windows 시작 앱 설정에서 변경해 주세요."
            } else if state == StartupState::Unknown {
                "Windows 상태 형식을 확인할 수 없습니다. Windows 설정에서 확인해 주세요."
            } else if !user {
                "모든 사용자에게 적용되는 항목입니다. Windows 시작 앱 설정에서 변경해 주세요."
            } else if command.is_none() {
                "실행 명령 형식을 해석할 수 없어 읽기 전용입니다."
            } else if saved.is_some() {
                "이전 변경의 원본이 보관되어 있습니다. 끄기를 누르면 보관을 마칩니다."
            } else {
                "끄면 원본 명령을 보관합니다. 다시 켜서 복원할 수 있습니다."
            };
            out.push(StartupEntry {
                id: format!("reg:{user}:{view}:{name}"),
                name,
                command: command.unwrap_or_else(|| "지원하지 않는 레지스트리 값".into()),
                source: source.into(),
                state,
                manageable,
                detail: detail.into(),
                target: Target::Registry {
                    user,
                    view,
                    saved: false,
                    value,
                },
            });
        }
    }
    if let Some(backup) = backup {
        for item in backup.enum_values() {
            let (name, value) = item.map_err(|e| error("보관된 시작 항목을 읽지 못했습니다", e))?;
            if seen.contains(&name.to_lowercase()) {
                continue;
            }
            let value: RawValue = value.into();
            let command = value.command();
            out.push(StartupEntry {
                id: format!("reg:{user}:{view}:{name}"),
                name,
                command: command.unwrap_or_else(|| "원본 레지스트리 값".into()),
                source: source.into(),
                state: StartupState::Disabled,
                manageable: true,
                detail: "Memori가 원본을 보관 중입니다. 켜면 원래 위치로 복원합니다.".into(),
                target: Target::Registry {
                    user,
                    view,
                    saved: true,
                    value,
                },
            });
        }
    }
    Ok(())
}

fn known_folder(id: &GUID) -> Result<PathBuf, String> {
    let mut pointer = std::ptr::null_mut();
    // KF_FLAG_DONT_VERIFY: scanning a missing folder must not create it.
    let result = unsafe { SHGetKnownFolderPath(id, 0x4000, std::ptr::null_mut(), &mut pointer) };
    if result < 0 {
        return Err(format!(
            "Windows 시작 폴더 위치를 읽지 못했습니다 (0x{:08X})",
            result as u32
        ));
    }
    if pointer.is_null() {
        return Err("Windows 시작 폴더 위치가 비어 있습니다".into());
    }
    let mut len = 0;
    unsafe {
        while *pointer.add(len) != 0 {
            len += 1;
        }
    }
    let path = PathBuf::from(OsString::from_wide(unsafe {
        std::slice::from_raw_parts(pointer, len)
    }));
    unsafe {
        CoTaskMemFree(pointer.cast());
    }
    Ok(path)
}

fn folder_paths(user: bool) -> Result<(PathBuf, PathBuf), String> {
    let active = known_folder(if user {
        &FOLDERID_Startup
    } else {
        &FOLDERID_CommonStartup
    })?;
    let parent = active
        .parent()
        .ok_or("시작 폴더 경로가 올바르지 않습니다")?;
    let disabled = parent.join(DISABLED_FOLDER);
    Ok((active, disabled))
}

fn regular_directory(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && meta.file_attributes() & REPARSE_POINT == 0 => Ok(true),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "연결된 폴더 또는 일반 폴더가 아닌 경로는 변경하지 않습니다",
        )),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

fn file_stamp(path: &Path) -> io::Result<FileStamp> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.file_attributes() & REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "연결된 파일이나 폴더는 이동하지 않습니다",
        ));
    }
    // Hash from a handle opened without following reparse points.
    use std::os::windows::fs::OpenOptionsExt;
    let mut file = File::options()
        .read(true)
        .custom_flags(0x00200000)
        .open(path)?;
    if file.metadata()?.file_attributes() & REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "연결된 파일은 이동하지 않습니다",
        ));
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let len = file.read(&mut buffer)?;
        if len == 0 {
            break;
        }
        buffer[..len].hash(&mut hasher);
    }
    let after = file.metadata()?;
    if after.file_size() != meta.file_size() || after.last_write_time() != meta.last_write_time() {
        return Err(io::Error::other(
            "파일이 읽는 동안 변경되었습니다. 새로 고침해 주세요",
        ));
    }
    Ok(FileStamp {
        len: meta.file_size(),
        written: meta.last_write_time(),
        digest: hasher.finish(),
    })
}

fn scan_folder(user: bool, out: &mut Vec<StartupEntry>) -> Result<(), String> {
    let (active, disabled) = folder_paths(user)?;
    scan_folder_pair(user, &active, &disabled, out)
}

fn scan_folder_pair(
    user: bool,
    active: &Path,
    disabled: &Path,
    out: &mut Vec<StartupEntry>,
) -> Result<(), String> {
    for (folder, saved) in [(active, false), (disabled, true)] {
        if saved && !user {
            continue;
        }
        if !regular_directory(folder).map_err(|e| error("시작 폴더를 확인하지 못했습니다", e))?
        {
            continue;
        }
        for item in fs::read_dir(folder).map_err(|e| error("시작 폴더를 읽지 못했습니다", e))?
        {
            let item = item.map_err(|e| error("시작 파일을 읽지 못했습니다", e))?;
            let name = item.file_name();
            if name.to_string_lossy().eq_ignore_ascii_case("desktop.ini") {
                continue;
            }
            let counterpart = if saved { active } else { disabled }.join(&name);
            let conflict = if user {
                match fs::symlink_metadata(&counterpart) {
                    Ok(_) => true,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => false,
                    Err(e) => return Err(error("같은 이름의 보관 파일을 확인하지 못했습니다", e)),
                }
            } else {
                false
            };
            let stamp = file_stamp(&item.path());
            let windows_state = approved(user, "StartupFolder", &name.to_string_lossy())
                .map_err(|e| error("Windows 사용 상태를 읽지 못했습니다", e))?;
            let state = if conflict {
                StartupState::Unknown
            } else if saved {
                StartupState::Disabled
            } else if stamp.is_err() {
                StartupState::Unknown
            } else {
                windows_state
            };
            let manageable =
                user && !conflict && stamp.is_ok() && (saved || state == StartupState::Enabled);
            let detail = if conflict {
                "시작 폴더와 보관 폴더에 같은 이름의 파일이 있습니다. 두 원본을 보존하며 자동으로 덮어쓰지 않습니다.".into()
            } else if let Err(e) = &stamp {
                format!("읽기 전용: {e}")
            } else if !user {
                "모든 사용자 시작 폴더입니다. Windows 설정에서 변경해 주세요.".into()
            } else if saved {
                "원본 파일을 시작 폴더 밖에 보관 중입니다. 켜면 복원합니다.".into()
            } else if state == StartupState::WindowsDisabled {
                "Windows에서 꺼진 항목입니다. Windows 설정에서 변경해 주세요.".into()
            } else if state == StartupState::Unknown {
                "Windows 상태 형식을 확인할 수 없습니다. Windows 설정에서 확인해 주세요.".into()
            } else {
                "끄면 원본 파일을 시작 폴더 밖으로 이동합니다. 다시 켜서 복원할 수 있습니다.".into()
            };
            out.push(StartupEntry {
                id: folder_id(user, saved, &name),
                name: item
                    .path()
                    .file_stem()
                    .unwrap_or(&name)
                    .to_string_lossy()
                    .into_owned(),
                command: if saved {
                    active.join(&name)
                } else {
                    item.path()
                }
                .display()
                .to_string(),
                source: if user {
                    "내 계정 · 시작 폴더"
                } else {
                    "모든 사용자 · 시작 폴더"
                }
                .into(),
                state,
                manageable,
                detail,
                target: stamp
                    .map(|stamp| Target::Folder {
                        user,
                        saved,
                        name,
                        stamp,
                    })
                    .unwrap_or(Target::Unsupported),
            });
        }
    }
    Ok(())
}

fn folder_id(user: bool, saved: bool, name: &std::ffi::OsStr) -> String {
    // UTF-16 encoding keeps even non-Unicode Windows filenames distinct.
    let name: String = name
        .encode_wide()
        .map(|unit| format!("{unit:04x}"))
        .collect();
    format!("folder:{user}:{saved}:{name}")
}

struct MutationLock(HANDLE);
impl Drop for MutationLock {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.0);
            CloseHandle(self.0);
        }
    }
}

fn lock_name(profile: &Path) -> Vec<u16> {
    // Stable per-user namespace; Global coordinates the same user across sessions.
    let hash = profile
        .to_string_lossy()
        .to_lowercase()
        .as_bytes()
        .iter()
        .fold(0xcbf29ce484222325u64, |hash, b| {
            (hash ^ u64::from(*b)).wrapping_mul(0x100000001b3)
        });
    format!(r"Global\Memori.Startup.{hash:016x}")
        .encode_utf16()
        .chain(Some(0))
        .collect()
}

fn mutation_lock() -> Result<MutationLock, String> {
    let name = lock_name(&known_folder(&FOLDERID_LocalAppData)?);
    acquire_mutation_lock(&name)
}

fn acquire_mutation_lock(name: &[u16]) -> Result<MutationLock, String> {
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    if handle.is_null() {
        return Err(error(
            "시작 항목 변경 잠금을 만들지 못했습니다",
            io::Error::last_os_error(),
        ));
    }
    match unsafe { WaitForSingleObject(handle, 0) } {
        WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(MutationLock(handle)),
        WAIT_TIMEOUT => {
            unsafe {
                CloseHandle(handle);
            }
            Err("다른 Memori 창이 시작 항목을 변경 중입니다. 잠시 후 다시 시도해 주세요".into())
        }
        _ => {
            let err = io::Error::last_os_error();
            unsafe {
                CloseHandle(handle);
            }
            Err(error("시작 항목 변경 잠금을 얻지 못했습니다", err))
        }
    }
}

fn flush(key: &RegKey) -> io::Result<()> {
    let status = unsafe { RegFlushKey(key.raw_handle()) };
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(status as i32))
    }
}

fn disable_registry(
    run: &RegKey,
    backup: &RegKey,
    name: &str,
    expected: &RawValue,
) -> io::Result<()> {
    if optional_value(run, name)?.as_ref() != Some(expected) {
        return Err(io::Error::other(
            "시작 항목이 변경되었습니다. 새로 고침해 주세요",
        ));
    }
    match optional_value(backup, name)? {
        Some(value) if value != *expected => {
            return Err(io::Error::other(
                "다른 원본이 보관되어 있습니다. 덮어쓰지 않았습니다",
            ));
        }
        Some(_) => (),
        None => backup.set_raw_value(name, &expected.value())?,
    }
    flush(backup)?;
    if optional_value(backup, name)?.as_ref() != Some(expected) {
        return Err(io::Error::other(
            "원본 보관 확인에 실패했습니다. 시작 항목을 유지했습니다",
        ));
    }
    if optional_value(run, name)?.as_ref() != Some(expected) {
        return Err(io::Error::other(
            "시작 항목이 변경되었습니다. 원본을 보존했습니다",
        ));
    }
    run.delete_value(name)?;
    flush(run)
}

fn enable_registry(
    run: &RegKey,
    backup: &RegKey,
    name: &str,
    expected: &RawValue,
) -> io::Result<()> {
    if optional_value(backup, name)?.as_ref() != Some(expected) {
        return Err(io::Error::other(
            "보관된 원본이 변경되었습니다. 새로 고침해 주세요",
        ));
    }
    match optional_value(run, name)? {
        Some(value) if value != *expected => {
            return Err(io::Error::other(
                "같은 이름의 다른 시작 항목이 있습니다. 덮어쓰지 않았습니다",
            ));
        }
        Some(_) => (),
        None => run.set_raw_value(name, &expected.value())?,
    }
    flush(run)?;
    if optional_value(run, name)?.as_ref() != Some(expected) {
        return Err(io::Error::other(
            "복원 확인에 실패했습니다. 보관된 원본을 유지했습니다",
        ));
    }
    backup.delete_value(name)?;
    flush(backup)
}

pub fn toggle(entry: &StartupEntry) -> Result<String, String> {
    if !entry.manageable {
        return Err("이 항목은 Windows 시작 앱 설정에서 변경해 주세요".into());
    }
    let _lock = mutation_lock()?;
    let fresh = scan()?
        .into_iter()
        .find(|e| {
            e.id == entry.id && e.target == entry.target && e.state == entry.state && e.manageable
        })
        .ok_or("시작 항목이 변경되었습니다. 새로 고침한 뒤 다시 시도해 주세요")?;
    match &fresh.target {
        Target::Registry {
            user: true,
            view,
            saved,
            value,
        } if *view == KEY_WOW64_64KEY => {
            let root = RegKey::predef(HKEY_CURRENT_USER);
            let (run, _) = root
                .create_subkey_with_flags(RUN, KEY_READ | KEY_WRITE | *view)
                .map_err(|e| error("시작 항목에 접근하지 못했습니다", e))?;
            let (backup, _) = root
                .create_subkey_with_flags(BACKUP, KEY_READ | KEY_WRITE | KEY_WOW64_64KEY)
                .map_err(|e| error("원본 보관 위치에 접근하지 못했습니다", e))?;
            if *saved {
                enable_registry(&run, &backup, &fresh.name, value)
            } else {
                disable_registry(&run, &backup, &fresh.name, value)
            }
            .map_err(|e| {
                error(
                    "변경하지 못했습니다. 새로 고침하여 현재 상태를 확인해 주세요",
                    e,
                )
            })?;
        }
        Target::Folder {
            user: true,
            saved,
            name,
            stamp,
        } => {
            let (active, disabled) = folder_paths(true)?;
            let (source, destination) = if *saved {
                (&disabled, &active)
            } else {
                (&active, &disabled)
            };
            if !regular_directory(source)
                .map_err(|e| error("원본 폴더를 확인하지 못했습니다", e))?
            {
                return Err("원본 시작 폴더가 사라졌습니다. 새로 고침해 주세요".into());
            }
            if !regular_directory(destination)
                .map_err(|e| error("복원 위치를 확인하지 못했습니다", e))?
            {
                fs::create_dir(destination)
                    .map_err(|e| error("보관 또는 복원 폴더를 만들지 못했습니다", e))?;
            }
            let from = source.join(name);
            let to = destination.join(name);
            if file_stamp(&from).map_err(|e| error("원본 파일을 확인하지 못했습니다", e))? != *stamp
            {
                return Err("파일이 변경되었습니다. 새로 고침한 뒤 다시 시도해 주세요".into());
            }
            move_no_replace(&from, &to).map_err(|e| {
                error(
                    "파일을 이동하지 못했습니다. 같은 이름의 파일이 있는지 확인해 주세요",
                    e,
                )
            })?;
        }
        _ => return Err("지원하지 않는 항목입니다. Windows 설정에서 변경해 주세요".into()),
    }
    Ok(if fresh.state == StartupState::Disabled {
        format!(
            "{} · 시작 항목을 원래 위치로 복원했습니다. Windows 사용 상태는 새로 고침하여 확인해 주세요.",
            fresh.name
        )
    } else {
        format!(
            "{} · 시작을 껐습니다. 원본은 보관 중이며 다시 켤 수 있습니다.",
            fresh.name
        )
    })
}

fn move_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    // MoveFileW never replaces an existing target and refuses cross-volume moves.
    if unsafe { MoveFileW(from.as_ptr(), to.as_ptr()) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Fixture {
        path: String,
        root: RegKey,
    }
    impl Fixture {
        fn new() -> Self {
            let path = format!(
                r"Software\Memori\Tests\{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            );
            let (root, _) = RegKey::predef(HKEY_CURRENT_USER)
                .create_subkey_with_flags(&path, KEY_ALL_ACCESS | KEY_WOW64_64KEY)
                .unwrap();
            Self { path, root }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(&self.path);
        }
    }

    #[test]
    fn registry_roundtrip_preserves_raw_type_and_bytes_and_recovers_duplicate() {
        let fixture = Fixture::new();
        let (run, _) = fixture.root.create_subkey("Run").unwrap();
        let (backup, _) = fixture.root.create_subkey("Backup").unwrap();
        let expected: RawValue = RegValue {
            vtype: REG_EXPAND_SZ,
            bytes: "%TEMP%\\한글.exe\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect(),
        }
        .into();
        run.set_raw_value("한글 프로그램", &expected.value())
            .unwrap();
        disable_registry(&run, &backup, "한글 프로그램", &expected).unwrap();
        assert!(optional_value(&run, "한글 프로그램").unwrap().is_none());
        assert_eq!(
            optional_value(&backup, "한글 프로그램").unwrap().as_ref(),
            Some(&expected)
        );
        // Simulate a crash after restore but before backup deletion.
        run.set_raw_value("한글 프로그램", &expected.value())
            .unwrap();
        enable_registry(&run, &backup, "한글 프로그램", &expected).unwrap();
        assert_eq!(
            optional_value(&run, "한글 프로그램").unwrap().as_ref(),
            Some(&expected)
        );
        assert!(optional_value(&backup, "한글 프로그램").unwrap().is_none());
    }

    #[test]
    fn registry_conflict_and_stale_snapshots_do_not_overwrite() {
        let fixture = Fixture::new();
        let (run, _) = fixture.root.create_subkey("Run").unwrap();
        let (backup, _) = fixture.root.create_subkey("Backup").unwrap();
        let original: RawValue = RegValue {
            vtype: REG_SZ,
            bytes: vec![65, 0, 0, 0],
        }
        .into();
        let different: RawValue = RegValue {
            vtype: REG_SZ,
            bytes: vec![66, 0, 0, 0],
        }
        .into();
        run.set_raw_value("Example", &different.value()).unwrap();
        assert!(disable_registry(&run, &backup, "Example", &original).is_err());
        assert!(optional_value(&backup, "Example").unwrap().is_none());
        backup.set_raw_value("Example", &original.value()).unwrap();
        assert!(disable_registry(&run, &backup, "Example", &different).is_err());
        assert!(enable_registry(&run, &backup, "Example", &original).is_err());
        assert_eq!(
            optional_value(&run, "Example").unwrap().as_ref(),
            Some(&different)
        );
        assert_eq!(
            optional_value(&backup, "Example").unwrap().as_ref(),
            Some(&original)
        );
    }

    #[test]
    fn command_decode_rejects_invalid_and_preserves_expansion() {
        let value: RawValue = RegValue {
            vtype: REG_EXPAND_SZ,
            bytes: "%APPDATA%\\한글.exe\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect(),
        }
        .into();
        assert_eq!(value.command().unwrap(), "%APPDATA%\\한글.exe");
        assert!(
            RawValue {
                bytes: vec![0],
                kind: REG_SZ
            }
            .command()
            .is_none()
        );
        assert!(
            RawValue {
                bytes: vec![0, 0],
                kind: REG_BINARY
            }
            .command()
            .is_none()
        );
    }

    #[test]
    fn folder_conflicts_have_distinct_ids_and_are_read_only() {
        let root = std::env::temp_dir().join(format!(
            "memori-duplicate-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let active = root.join("Startup");
        let disabled = root.join(DISABLED_FOLDER);
        fs::create_dir(&root).unwrap();
        fs::create_dir(&active).unwrap();
        fs::create_dir(&disabled).unwrap();
        fs::write(active.join("example.lnk"), b"original").unwrap();
        fs::write(disabled.join("example.lnk"), b"different").unwrap();
        let mut entries = Vec::new();
        scan_folder_pair(true, &active, &disabled, &mut entries).unwrap();
        assert_eq!(entries.len(), 2);
        assert_ne!(entries[0].id, entries[1].id);
        assert!(
            entries
                .iter()
                .all(|e| !e.manageable && e.state == StartupState::Unknown)
        );
        assert_eq!(fs::read(active.join("example.lnk")).unwrap(), b"original");
        assert_eq!(
            fs::read(disabled.join("example.lnk")).unwrap(),
            b"different"
        );
        fs::remove_file(active.join("example.lnk")).unwrap();
        fs::remove_file(disabled.join("example.lnk")).unwrap();
        fs::remove_dir(&active).unwrap();
        fs::remove_dir(&disabled).unwrap();
        fs::remove_dir(&root).unwrap();
        let a = OsString::from_wide(&[0xd800]);
        let b = OsString::from_wide(&[0xd801]);
        assert_eq!(a.to_string_lossy(), b.to_string_lossy());
        assert_ne!(folder_id(true, false, &a), folder_id(true, false, &b));
    }

    #[test]
    fn mutation_mutex_excludes_other_threads_and_releases() {
        let profile = PathBuf::from(format!(
            r"C:\MemoriMutexTest\{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let name = lock_name(&profile);
        let guard = acquire_mutation_lock(&name).unwrap();
        let other_name = name.clone();
        assert!(
            std::thread::spawn(move || acquire_mutation_lock(&other_name).is_err())
                .join()
                .unwrap()
        );
        drop(guard);
        assert!(
            std::thread::spawn(move || acquire_mutation_lock(&name).is_ok())
                .join()
                .unwrap()
        );
        assert_eq!(
            lock_name(Path::new(r"C:\Users\Example")),
            lock_name(Path::new(r"c:\users\example"))
        );
    }

    #[test]
    fn file_move_does_not_clobber_and_roundtrips() {
        let root = std::env::temp_dir().join(format!(
            "memori-startup-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let from = root.join("original.lnk");
        let to = root.join("disabled.lnk");
        fs::write(&from, b"original bytes").unwrap();
        fs::write(&to, b"conflicting bytes").unwrap();
        assert!(move_no_replace(&from, &to).is_err());
        assert_eq!(fs::read(&to).unwrap(), b"conflicting bytes");
        fs::remove_file(&to).unwrap();
        let before = file_stamp(&from).unwrap();
        move_no_replace(&from, &to).unwrap();
        assert_eq!(file_stamp(&to).unwrap(), before);
        move_no_replace(&to, &from).unwrap();
        assert_eq!(fs::read(&from).unwrap(), b"original bytes");
        fs::remove_file(&from).unwrap();
        fs::remove_dir(&root).unwrap();
    }
}
