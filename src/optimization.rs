use crate::memory::{self, Process};
use std::{
    io,
    mem::size_of,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::HANDLE,
    Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives},
    System::{
        Threading::{
            GetProcessInformation, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
            PROCESS_SET_INFORMATION, ProcessPowerThrottling, SetProcessInformation,
        },
        WindowsProgramming::DRIVE_FIXED,
    },
    UI::Shell::{SHQUERYRBINFO, SHQueryRecycleBinW},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerPolicy {
    Automatic,
    Eco,
    Performance,
}

const SPEED: u32 = PROCESS_POWER_THROTTLING_EXECUTION_SPEED;
const POWER_STATE_SIZE: u32 = size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32;

fn error(context: &str) -> String {
    format!("{context}: {}", io::Error::last_os_error())
}

fn read_power(handle: HANDLE) -> Result<PROCESS_POWER_THROTTLING_STATE, String> {
    let mut state = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: 0,
        StateMask: 0,
    };
    if unsafe {
        GetProcessInformation(
            handle,
            ProcessPowerThrottling,
            (&raw mut state).cast(),
            POWER_STATE_SIZE,
        )
    } == 0
    {
        return Err(error("효율 모드 상태를 읽지 못했습니다"));
    }
    Ok(state)
}

fn write_power(handle: HANDLE, state: &PROCESS_POWER_THROTTLING_STATE) -> Result<(), String> {
    if unsafe {
        SetProcessInformation(
            handle,
            ProcessPowerThrottling,
            (state as *const PROCESS_POWER_THROTTLING_STATE).cast(),
            POWER_STATE_SIZE,
        )
    } == 0
    {
        return Err(error("효율 모드를 변경하지 못했습니다"));
    }
    Ok(())
}

fn policy(state: &PROCESS_POWER_THROTTLING_STATE) -> PowerPolicy {
    if state.ControlMask & SPEED == 0 {
        PowerPolicy::Automatic
    } else if state.StateMask & SPEED != 0 {
        PowerPolicy::Eco
    } else {
        PowerPolicy::Performance
    }
}

pub fn power_policy(process: &Process) -> Result<PowerPolicy, String> {
    let handle = memory::checked_process(process, 0, false)?;
    read_power(handle.0).map(|state| policy(&state))
}

pub fn set_power_policy(
    process: &Process,
    expected: PowerPolicy,
    desired: PowerPolicy,
) -> Result<PowerPolicy, String> {
    let handle = memory::checked_process(
        process,
        PROCESS_SET_INFORMATION,
        desired == PowerPolicy::Eco,
    )?;
    let mut state = read_power(handle.0)?;
    if policy(&state) != expected {
        return Err("효율 모드 상태가 바뀌었습니다. 새로 확인한 뒤 다시 시도하세요".into());
    }
    if expected == desired {
        return Ok(expected);
    }
    state.ControlMask &= !SPEED;
    state.StateMask &= !SPEED;
    match desired {
        PowerPolicy::Automatic => {}
        PowerPolicy::Eco => {
            state.ControlMask |= SPEED;
            state.StateMask |= SPEED;
        }
        PowerPolicy::Performance => state.ControlMask |= SPEED,
    }
    write_power(handle.0, &state)?;
    let actual = read_power(handle.0)?;
    if actual.ControlMask != state.ControlMask || actual.StateMask != state.StateMask {
        return Err("효율 모드 변경 결과가 요청과 다릅니다. 현재 상태를 다시 확인하세요".into());
    }
    Ok(policy(&actual))
}

pub struct Drive {
    pub letter: char,
    pub space: Option<DriveSpace>,
}

pub struct DriveSpace {
    pub available: u64,
    pub total: u64,
}

pub fn recycle_bin_usage() -> Result<(u64, u64), String> {
    let mut info = SHQUERYRBINFO {
        cbSize: size_of::<SHQUERYRBINFO>() as u32,
        ..Default::default()
    };
    let result = unsafe { SHQueryRecycleBinW(null(), &mut info) };
    if result != 0 {
        return Err(format!(
            "휴지통 용량을 읽지 못했습니다 (0x{:08X}).",
            result as u32
        ));
    }
    match (u64::try_from(info.i64NumItems), u64::try_from(info.i64Size)) {
        (Ok(count), Ok(bytes)) => Ok((count, bytes)),
        _ => Err("Windows가 휴지통 용량을 반환하지 않았습니다.".into()),
    }
}

pub fn storage() -> Result<Vec<Drive>, String> {
    let mask = unsafe { GetLogicalDrives() };
    if mask == 0 {
        return Err(error("드라이브 목록을 읽지 못했습니다"));
    }
    let mut drives = Vec::new();
    for index in 0..26 {
        if mask & (1 << index) == 0 {
            continue;
        }
        let letter = b'A' + index;
        let root = [u16::from(letter), b':' as u16, b'\\' as u16, 0];
        if unsafe { GetDriveTypeW(root.as_ptr()) } != DRIVE_FIXED {
            continue;
        }
        let (mut available, mut total) = (0, 0);
        let read =
            unsafe { GetDiskFreeSpaceExW(root.as_ptr(), &mut available, &mut total, null_mut()) }
                != 0;
        drives.push(Drive {
            letter: char::from(letter),
            space: (read && total > 0 && available <= total)
                .then_some(DriveSpace { available, total }),
        });
    }
    Ok(drives)
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::Threading::{
        GetPriorityClass, PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
    };

    #[test]
    fn efficiency_roundtrip_preserves_other_flags_and_refuses_stale_updates() {
        let (_child, process) = memory::tests::disposable_process();
        let handle = memory::checked_process(&process, PROCESS_SET_INFORMATION, false).unwrap();
        let priority = unsafe { GetPriorityClass(handle.0) };
        assert_ne!(priority, 0);

        let timer = PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION;
        let performance = PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: timer | SPEED,
            StateMask: timer,
        };
        write_power(handle.0, &performance).unwrap();
        assert_eq!(power_policy(&process).unwrap(), PowerPolicy::Performance);

        let mut stale = process.clone();
        stale.created ^= 1;
        assert!(
            power_policy(&stale)
                .unwrap_err()
                .contains("프로세스가 바뀌었습니다")
        );
        assert!(
            set_power_policy(&stale, PowerPolicy::Performance, PowerPolicy::Eco)
                .unwrap_err()
                .contains("프로세스가 바뀌었습니다")
        );
        assert!(
            set_power_policy(&process, PowerPolicy::Automatic, PowerPolicy::Eco)
                .unwrap_err()
                .contains("효율 모드 상태가 바뀌었습니다")
        );
        assert_eq!(power_policy(&process).unwrap(), PowerPolicy::Performance);

        assert_eq!(
            set_power_policy(&process, PowerPolicy::Performance, PowerPolicy::Eco).unwrap(),
            PowerPolicy::Eco
        );
        let eco = read_power(handle.0).unwrap();
        assert_eq!(
            (eco.ControlMask, eco.StateMask),
            (timer | SPEED, timer | SPEED)
        );
        assert_eq!(
            set_power_policy(&process, PowerPolicy::Eco, PowerPolicy::Automatic).unwrap(),
            PowerPolicy::Automatic
        );
        let automatic = read_power(handle.0).unwrap();
        assert_eq!((automatic.ControlMask, automatic.StateMask), (timer, timer));

        write_power(
            handle.0,
            &PROCESS_POWER_THROTTLING_STATE {
                Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
                ControlMask: 0,
                StateMask: 0,
            },
        )
        .unwrap();
        set_power_policy(&process, PowerPolicy::Automatic, PowerPolicy::Eco).unwrap();
        set_power_policy(&process, PowerPolicy::Eco, PowerPolicy::Automatic).unwrap();
        let defaults = read_power(handle.0).unwrap();
        assert_eq!((defaults.ControlMask, defaults.StateMask), (0, 0));
        assert_eq!(unsafe { GetPriorityClass(handle.0) }, priority);
    }

    #[test]
    fn storage_can_read_space_on_the_system_drive() {
        let system_drive = std::env::var("SystemDrive").unwrap();
        let letter = system_drive.chars().next().unwrap().to_ascii_uppercase();
        let drives = storage().unwrap();
        let space = drives
            .iter()
            .find(|drive| drive.letter == letter)
            .and_then(|drive| drive.space.as_ref())
            .expect("system drive capacity");
        assert!(space.total > 0 && space.available <= space.total);
    }
}
