use std::{io, os::windows::ffi::OsStrExt, path::Path};
use windows_sys::Win32::{
    Globalization::{CSTR_EQUAL, CompareStringOrdinal},
    System::Registry::RegFlushKey,
};
use winreg::{RegKey, RegValue, enums::*};

const SETTINGS: &str = r"Software\Memori\Settings";
const WIDGET: &str = "Widget";
const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const LOGIN_NAME: &str = "Memori";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preferences {
    pub pinned: bool,
    pub compact: bool,
    pub position: Option<[i32; 2]>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            pinned: true,
            compact: false,
            position: None,
        }
    }
}

fn error(context: &str, e: impl std::fmt::Display) -> String {
    format!("{context}: {e}")
}

fn encoded(preferences: &Preferences) -> RegValue {
    let mut bytes = Vec::with_capacity(16);
    bytes.extend_from_slice(b"MEM2");
    bytes.extend_from_slice(&[
        u8::from(preferences.pinned),
        u8::from(preferences.position.is_some()),
        u8::from(preferences.compact),
        0,
    ]);
    for coordinate in preferences.position.unwrap_or([0, 0]) {
        bytes.extend_from_slice(&coordinate.to_le_bytes());
    }
    RegValue {
        bytes,
        vtype: REG_BINARY,
    }
}

fn decoded(value: &RegValue) -> io::Result<Preferences> {
    let b = &value.bytes;
    if value.vtype != REG_BINARY
        || b.len() != 16
        || (&b[..4] != b"MEM1" && &b[..4] != b"MEM2")
        || b[4] > 1
        || b[5] > 1
        || b[7] != 0
        || (&b[..4] == b"MEM1" && b[6] != 0)
        || (&b[..4] == b"MEM2" && b[6] > 1)
        || (b[5] == 0 && b[8..16] != [0; 8])
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "저장된 위젯 설정 형식이 올바르지 않습니다",
        ));
    }
    Ok(Preferences {
        pinned: b[4] == 1,
        compact: b[6] == 1,
        position: (b[5] == 1).then(|| {
            [
                i32::from_le_bytes(b[8..12].try_into().unwrap()),
                i32::from_le_bytes(b[12..16].try_into().unwrap()),
            ]
        }),
    })
}

fn optional_value(key: &RegKey, name: &str) -> io::Result<Option<RegValue>> {
    match key.get_raw_value(name) {
        Ok(value) => Ok(Some(value)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
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

fn load_from(key: &RegKey) -> io::Result<Preferences> {
    match optional_value(key, WIDGET)? {
        Some(value) => decoded(&value),
        None => Ok(Preferences::default()),
    }
}

fn save_to(key: &RegKey, preferences: &Preferences) -> io::Result<()> {
    let value = encoded(preferences);
    // One value keeps position and pin state together if a write is interrupted.
    key.set_raw_value(WIDGET, &value)?;
    flush(key)?;
    if load_from(key)? != *preferences {
        return Err(io::Error::other(
            "위젯 설정 저장 결과를 확인하지 못했습니다",
        ));
    }
    Ok(())
}

pub fn load() -> Result<Preferences, String> {
    let root = RegKey::predef(HKEY_CURRENT_USER);
    match root.open_subkey_with_flags(SETTINGS, KEY_READ | KEY_WOW64_64KEY) {
        Ok(key) => load_from(&key).map_err(|e| error("위젯 설정을 읽지 못했습니다", e)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Preferences::default()),
        Err(e) => Err(error("위젯 설정을 읽지 못했습니다", e)),
    }
}

pub fn save(preferences: &Preferences) -> Result<(), String> {
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey_with_flags(SETTINGS, KEY_READ | KEY_WRITE | KEY_WOW64_64KEY)
        .map_err(|e| error("위젯 설정 저장 위치를 열지 못했습니다", e))?;
    save_to(&key, preferences).map_err(|e| error("위젯 설정을 저장하지 못했습니다", e))
}

pub fn startup_command(exe: &Path) -> Result<String, String> {
    let wide: Vec<u16> = exe.as_os_str().encode_wide().collect();
    if !exe.is_absolute() || wide.contains(&0) || wide.contains(&(b'"' as u16)) {
        return Err("자동 실행에는 따옴표나 NUL 문자가 없는 절대 실행 경로가 필요합니다".into());
    }
    let path = exe
        .to_str()
        .ok_or("자동 실행 경로를 유니코드 문자열로 표현하지 못했습니다")?;
    let command = format!("\"{path}\" --widget");
    if command.encode_utf16().count() > 260 {
        return Err(
            "자동 실행 명령이 Windows Run의 260자 한도를 넘습니다. 앱을 더 짧은 경로로 옮겨 주세요"
                .into(),
        );
    }
    Ok(command)
}

fn current_command() -> Result<String, String> {
    let exe =
        std::env::current_exe().map_err(|e| error("Memori 실행 경로를 확인하지 못했습니다", e))?;
    startup_command(&exe)
}

fn command_value(command: &str) -> RegValue {
    RegValue {
        bytes: command
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect(),
        vtype: REG_SZ,
    }
}

fn registration(key: &RegKey, command: &str) -> io::Result<bool> {
    let Some(existing) = optional_value(key, LOGIN_NAME)? else {
        return Ok(false);
    };
    if !same_registration(&existing, command) {
        return Err(io::Error::other(
            "같은 이름의 다른 Memori 시작 항목이 있습니다. 기존 항목은 보존했습니다. 시작 프로그램 화면에서 확인해 주세요",
        ));
    }
    Ok(true)
}

fn same_registration(existing: &RegValue, command: &str) -> bool {
    let (pairs, remainder) = existing.bytes.as_chunks::<2>();
    if existing.vtype != REG_SZ || !remainder.is_empty() {
        return false;
    }
    let actual: Vec<u16> = pairs.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
    let expected: Vec<u16> = command.encode_utf16().chain(Some(0)).collect();
    let suffix: Vec<u16> = "\" --widget\0".encode_utf16().collect();
    if actual.len() > 261
        || !actual.starts_with(&[34])
        || !actual.ends_with(&suffix)
        || !expected.ends_with(&suffix)
    {
        return false;
    }
    // Windows may return different path casing; keep the argument comparison exact.
    unsafe {
        CompareStringOrdinal(
            actual.as_ptr(),
            (actual.len() - suffix.len()) as i32,
            expected.as_ptr(),
            (expected.len() - suffix.len()) as i32,
            1,
        ) == CSTR_EQUAL
    }
}

fn set_registration(key: &RegKey, command: &str, enabled: bool) -> io::Result<()> {
    if registration(key, command)? == enabled {
        return Ok(());
    }
    if enabled {
        key.set_raw_value(LOGIN_NAME, &command_value(command))?;
    } else {
        key.delete_value(LOGIN_NAME)?;
    }
    flush(key)?;
    if registration(key, command)? != enabled {
        return Err(io::Error::other(
            "자동 실행 등록 결과를 확인하지 못했습니다. 시작 프로그램 화면을 새로 고쳐 주세요",
        ));
    }
    Ok(())
}

/// Reports this executable's Run registration; Windows may independently block startup.
pub fn launch_at_login() -> Result<bool, String> {
    let command = current_command()?;
    let root = RegKey::predef(HKEY_CURRENT_USER);
    match root.open_subkey_with_flags(RUN, KEY_READ | KEY_WOW64_64KEY) {
        Ok(key) => {
            registration(&key, &command).map_err(|e| error("자동 실행 상태를 읽지 못했습니다", e))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(error("자동 실행 상태를 읽지 못했습니다", e)),
    }
}

pub fn set_launch_at_login(enabled: bool) -> Result<(), String> {
    let command = current_command()?;
    let root = RegKey::predef(HKEY_CURRENT_USER);
    let flags = KEY_READ | KEY_WRITE | KEY_WOW64_64KEY;
    let key = if enabled {
        root.create_subkey_with_flags(RUN, flags)
            .map(|(key, _)| key)
    } else {
        root.open_subkey_with_flags(RUN, flags)
    };
    match key {
        Ok(key) => set_registration(&key, &command, enabled)
            .map_err(|e| error("자동 실행 설정을 변경하지 못했습니다", e)),
        Err(e) if !enabled && e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(error("자동 실행 등록 위치를 열지 못했습니다", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Fixture {
        path: String,
        key: RegKey,
    }
    impl Fixture {
        fn new() -> Self {
            let path = format!(
                r"Software\Memori\Tests\preferences-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            );
            let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
                .create_subkey_with_flags(&path, KEY_ALL_ACCESS | KEY_WOW64_64KEY)
                .unwrap();
            Self { path, key }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(&self.path);
        }
    }

    #[test]
    fn preferences_roundtrip_and_strict_validation() {
        for prefs in [
            Preferences::default(),
            Preferences {
                pinned: false,
                compact: true,
                position: Some([-1920, 487]),
            },
            Preferences {
                pinned: true,
                compact: false,
                position: Some([0, 0]),
            },
        ] {
            assert_eq!(decoded(&encoded(&prefs)).unwrap(), prefs);
        }
        let mut invalid = encoded(&Preferences::default());
        invalid.bytes[4] = 2;
        assert!(decoded(&invalid).is_err());
        invalid = encoded(&Preferences::default());
        invalid.bytes[0] = b'X';
        assert!(decoded(&invalid).is_err());
        invalid = encoded(&Preferences::default());
        invalid.bytes[8] = 1;
        assert!(decoded(&invalid).is_err());
        invalid.bytes.truncate(4);
        assert!(decoded(&invalid).is_err());
        invalid = encoded(&Preferences::default());
        invalid.bytes[6] = 2;
        assert!(decoded(&invalid).is_err());
        invalid = encoded(&Preferences::default());
        invalid.bytes[7] = 1;
        assert!(decoded(&invalid).is_err());
    }

    #[test]
    fn legacy_mem1_migrates_without_compact_mode() {
        let expected = Preferences {
            pinned: false,
            compact: false,
            position: Some([-1920, 487]),
        };
        let mut legacy = encoded(&expected);
        legacy.bytes[..4].copy_from_slice(b"MEM1");
        assert_eq!(decoded(&legacy).unwrap(), expected);
        let updated = encoded(&decoded(&legacy).unwrap());
        assert_eq!(&updated.bytes[..4], b"MEM2");
        legacy.bytes[6] = 1;
        assert!(decoded(&legacy).is_err());
    }

    #[test]
    fn preferences_persist_together_in_an_isolated_key() {
        let fixture = Fixture::new();
        assert_eq!(load_from(&fixture.key).unwrap(), Preferences::default());
        let prefs = Preferences {
            pinned: false,
            compact: true,
            position: Some([2048, -300]),
        };
        save_to(&fixture.key, &prefs).unwrap();
        assert_eq!(load_from(&fixture.key).unwrap(), prefs);
    }

    #[test]
    fn login_command_quotes_spaces_and_unicode_and_rejects_invalid_paths() {
        assert_eq!(
            startup_command(Path::new(r"D:\한글 폴더\memori.exe")).unwrap(),
            "\"D:\\한글 폴더\\memori.exe\" --widget"
        );
        assert!(startup_command(Path::new("memori.exe")).is_err());
        assert!(startup_command(Path::new("C:\\bad\"name\\memori.exe")).is_err());
        assert!(
            startup_command(Path::new(&format!("C:\\{}\\memori.exe", "a".repeat(260)))).is_err()
        );
    }

    #[test]
    fn login_registration_accepts_path_casing_but_preserves_exact_arguments() {
        let fixture = Fixture::new();
        let command = startup_command(Path::new(r"D:\Mémori\Memori.exe")).unwrap();
        let original = command_value("\"d:\\MÉMORI\\memori.EXE\" --widget");
        fixture.key.set_raw_value(LOGIN_NAME, &original).unwrap();
        assert!(registration(&fixture.key, &command).unwrap());
        set_registration(&fixture.key, &command, true).unwrap();
        assert_eq!(fixture.key.get_raw_value(LOGIN_NAME).unwrap(), original);
        for foreign in [
            "\"d:\\MÉMORI\\memori.EXE\" --WIDGET",
            "\"d:\\MÉMORI\\other.exe\" --widget",
            "\"d:\\MÉMORI\\memori.EXE\" --widget --full",
        ] {
            fixture.key.set_value(LOGIN_NAME, &foreign).unwrap();
            assert!(registration(&fixture.key, &command).is_err());
        }
        assert!(!same_registration(
            &RegValue {
                bytes: vec![34],
                vtype: REG_SZ
            },
            &command
        ));
    }

    #[test]
    fn login_toggle_preserves_other_values_and_refuses_conflicts() {
        let fixture = Fixture::new();
        let command = startup_command(Path::new(r"D:\Memori\memori.exe")).unwrap();
        fixture.key.set_value("OtherApp", &"keep me").unwrap();
        assert!(!registration(&fixture.key, &command).unwrap());
        set_registration(&fixture.key, &command, true).unwrap();
        assert!(registration(&fixture.key, &command).unwrap());
        set_registration(&fixture.key, &command, false).unwrap();
        assert!(!registration(&fixture.key, &command).unwrap());
        assert_eq!(
            fixture.key.get_value::<String, _>("OtherApp").unwrap(),
            "keep me"
        );
        fixture.key.set_value(LOGIN_NAME, &"foreign.exe").unwrap();
        assert!(registration(&fixture.key, &command).is_err());
        assert!(set_registration(&fixture.key, &command, true).is_err());
        assert!(set_registration(&fixture.key, &command, false).is_err());
        assert_eq!(
            fixture.key.get_value::<String, _>(LOGIN_NAME).unwrap(),
            "foreign.exe"
        );
    }
}
