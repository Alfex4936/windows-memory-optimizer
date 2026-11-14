use crate::{memory::Handle, startup};
use std::{
    io,
    mem::{offset_of, size_of},
    os::windows::ffi::OsStrExt,
    path::Path,
    ptr::{null, null_mut},
    time::{SystemTime, UNIX_EPOCH},
};
use windows_sys::{
    Wdk::{
        Foundation::OBJECT_ATTRIBUTES,
        Storage::FileSystem::{
            FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_NO_RECALL,
            FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
        },
    },
    Win32::{
        Foundation::{
            ERROR_NO_MORE_FILES, HANDLE, INVALID_HANDLE_VALUE, OBJ_CASE_INSENSITIVE,
            OBJ_DONT_REPARSE, RtlNtStatusToDosError, UNICODE_STRING,
        },
        Storage::FileSystem::{
            CreateFileW, DELETE, FILE_ATTRIBUTE_DEVICE, FILE_ATTRIBUTE_DIRECTORY,
            FILE_ATTRIBUTE_OFFLINE, FILE_ATTRIBUTE_READONLY, FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS,
            FILE_ATTRIBUTE_RECALL_ON_OPEN, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_SYSTEM,
            FILE_BASIC_INFO, FILE_DISPOSITION_INFO, FILE_FLAG_BACKUP_SEMANTICS,
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_FULL_DIR_INFO, FILE_ID_INFO,
            FILE_INFO_BY_HANDLE_CLASS, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_READ,
            FILE_SHARE_WRITE, FILE_STANDARD_INFO, FileBasicInfo, FileDispositionInfo,
            FileFullDirectoryInfo, FileIdInfo, FileStandardInfo, GetDriveTypeW,
            GetFileInformationByHandleEx, GetVolumeNameForVolumeMountPointW, OPEN_EXISTING,
            SYNCHRONIZE, SetFileInformationByHandle,
        },
        System::{IO::IO_STATUS_BLOCK, WindowsProgramming::DRIVE_FIXED},
        UI::Shell::{FOLDERID_LocalAppData, FOLDERID_Profile},
    },
};

const MAX_ENTRIES: usize = 20_000;
const MAX_ITEMS: usize = 5_000;
const MAX_DEPTH: usize = 16;
const DIRECTORY_ACCESS: u32 = FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES;
const DIRECTORY_SHARE: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE;
const UNAVAILABLE: u32 = FILE_ATTRIBUTE_REPARSE_POINT
    | FILE_ATTRIBUTE_DEVICE
    | FILE_ATTRIBUTE_OFFLINE
    | FILE_ATTRIBUTE_RECALL_ON_OPEN
    | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS;
const EXCLUDED_FILE: u32 =
    UNAVAILABLE | FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_READONLY | FILE_ATTRIBUTE_SYSTEM;

type Name = Vec<u16>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Category {
    Temp,
    Shader,
}

impl Category {
    fn folder(self) -> Name {
        match self {
            Self::Temp => "Temp",
            Self::Shader => "D3DSCache",
        }
        .encode_utf16()
        .collect()
    }
}

#[derive(Clone, Debug)]
pub struct Item {
    pub name: String,
    pub category: Category,
    pub bytes: u64,
}

pub struct ScanReport {
    pub items: Vec<Item>,
    pub skipped: usize,
    pub limited: bool,
}

pub struct CleanupResult {
    pub removed: usize,
    pub bytes: u64,
    pub skipped: usize,
}

pub struct Catalog {
    location: Location,
    roots: Vec<(Category, Identity)>,
    entries: Vec<Entry>,
    skipped: usize,
    limited: bool,
}

struct Entry {
    root: usize,
    path: Vec<Name>,
    item: Item,
    metadata: Metadata,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Identity {
    volume: u64,
    file: [u8; 16],
}

#[derive(Debug, Eq, PartialEq)]
struct Metadata {
    identity: Identity,
    attributes: u32,
    creation: i64,
    modified: i64,
    changed: i64,
    accessed: i64,
    size: i64,
    allocation: u64,
}

struct Location {
    volume: Name,
    parts: Vec<Name>,
}

fn invalid_path() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "지원하지 않는 정리 경로입니다")
}

fn valid_name(name: &[u16]) -> bool {
    !name.is_empty()
        && !name.last().is_some_and(|&unit| matches!(unit, 32 | 46))
        && char::decode_utf16(name.iter().copied()).all(|value| {
            value.is_ok_and(|c| {
                !c.is_control()
                    && !matches!(c, '/' | ':' | '\\' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            })
        })
}

impl Location {
    fn new(path: &Path) -> io::Result<Self> {
        let wide: Vec<_> = path.as_os_str().encode_wide().collect();
        let [letter, 58, 92, tail @ ..] = wide.as_slice() else {
            return Err(invalid_path());
        };
        if !u8::try_from(*letter).is_ok_and(|value| value.is_ascii_alphabetic()) {
            return Err(invalid_path());
        }
        let mut parts = Vec::new();
        for part in tail.split(|&unit| unit == 92) {
            if !valid_name(part) {
                return Err(invalid_path());
            }
            parts.push(part.to_vec());
        }
        let mount = [*letter, 58, 92, 0];
        if unsafe { GetDriveTypeW(mount.as_ptr()) } != DRIVE_FIXED {
            return Err(invalid_path());
        }
        // Resolve drive letters once. Reopening the GUID cannot follow a reassigned drive letter.
        let mut volume = vec![0; 64];
        if unsafe { GetVolumeNameForVolumeMountPointW(mount.as_ptr(), volume.as_mut_ptr(), 64) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        let len = volume
            .iter()
            .position(|&unit| unit == 0)
            .ok_or_else(invalid_path)?;
        volume.truncate(len + 1);
        Ok(Self { volume, parts })
    }

    fn open(&self) -> io::Result<Vec<Handle>> {
        let raw = unsafe {
            CreateFileW(
                self.volume.as_ptr(),
                DIRECTORY_ACCESS,
                DIRECTORY_SHARE,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                null_mut(),
            )
        };
        if raw == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let mut held = vec![checked_directory(Handle(raw))?];
        for name in &self.parts {
            held.push(open_child(held.last().unwrap().0, name, true, true)?);
        }
        Ok(held)
    }
}

fn information<T: Default>(handle: HANDLE, class: FILE_INFO_BY_HANDLE_CLASS) -> io::Result<T> {
    let mut value = T::default();
    if unsafe {
        GetFileInformationByHandleEx(
            handle,
            class,
            (&raw mut value).cast(),
            size_of::<T>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(value)
}

fn identity(handle: HANDLE) -> io::Result<Identity> {
    let id: FILE_ID_INFO = information(handle, FileIdInfo)?;
    if id.FileId.Identifier == [0; 16] {
        return Err(invalid_path());
    }
    Ok(Identity {
        volume: id.VolumeSerialNumber,
        file: id.FileId.Identifier,
    })
}

fn checked_directory(handle: Handle) -> io::Result<Handle> {
    let basic: FILE_BASIC_INFO = information(handle.0, FileBasicInfo)?;
    if basic.FileAttributes & (UNAVAILABLE | FILE_ATTRIBUTE_DIRECTORY) != FILE_ATTRIBUTE_DIRECTORY {
        return Err(invalid_path());
    }
    Ok(handle)
}

fn open_child(
    parent: HANDLE,
    name: &[u16],
    directory: bool,
    ignore_case: bool,
) -> io::Result<Handle> {
    let len = u16::try_from(name.len().saturating_mul(2)).map_err(|_| invalid_path())?;
    if !valid_name(name) {
        return Err(invalid_path());
    }
    let name = UNICODE_STRING {
        Length: len,
        MaximumLength: len,
        Buffer: name.as_ptr().cast_mut(),
    };
    // A single literal component is opened relative to a held directory, with reparses disabled.
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent,
        ObjectName: &name,
        Attributes: OBJ_DONT_REPARSE | if ignore_case { OBJ_CASE_INSENSITIVE } else { 0 },
        SecurityDescriptor: null(),
        SecurityQualityOfService: null(),
    };
    let (access, share, options) = if directory {
        (DIRECTORY_ACCESS, DIRECTORY_SHARE, FILE_DIRECTORY_FILE)
    } else {
        (
            FILE_READ_ATTRIBUTES | DELETE,
            0,
            FILE_NON_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_OPEN_NO_RECALL,
        )
    };
    let mut file = null_mut();
    let mut status = IO_STATUS_BLOCK::default();
    let result = unsafe {
        NtCreateFile(
            &mut file,
            access | SYNCHRONIZE,
            &attributes,
            &mut status,
            null(),
            0,
            share,
            FILE_OPEN,
            options | FILE_SYNCHRONOUS_IO_NONALERT,
            null(),
            0,
        )
    };
    if result < 0 {
        let error = unsafe { RtlNtStatusToDosError(result) };
        return Err(io::Error::from_raw_os_error(error as i32));
    }
    if file.is_null() || file == INVALID_HANDLE_VALUE {
        return Err(invalid_path());
    }
    let handle = Handle(file);
    if directory {
        checked_directory(handle)
    } else {
        Ok(handle)
    }
}

fn age_cutoff() -> Result<i64, String> {
    const UNIX_EPOCH_FILETIME: i64 = 116_444_736_000_000_000;
    const DAY: i64 = 24 * 60 * 60 * 10_000_000;
    let ticks = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|now| i64::try_from(now.as_nanos() / 100).ok())
        .and_then(|ticks| ticks.checked_add(UNIX_EPOCH_FILETIME - DAY));
    ticks.ok_or_else(|| "시스템 시간으로 정리 기준을 계산하지 못했습니다".into())
}

fn old_enough(cutoff: i64, times: [i64; 3]) -> bool {
    times.into_iter().all(|value| value > 0 && value <= cutoff)
}

fn file_metadata(handle: HANDLE, cutoff: i64) -> io::Result<Metadata> {
    let basic: FILE_BASIC_INFO = information(handle, FileBasicInfo)?;
    let standard: FILE_STANDARD_INFO = information(handle, FileStandardInfo)?;
    if basic.FileAttributes & EXCLUDED_FILE != 0
        || !old_enough(
            cutoff,
            [basic.CreationTime, basic.LastWriteTime, basic.ChangeTime],
        )
        || standard.Directory
        || standard.DeletePending
        || standard.NumberOfLinks != 1
        || standard.EndOfFile < 0
        || standard.AllocationSize <= 0
    {
        return Err(invalid_path());
    }
    Ok(Metadata {
        identity: identity(handle)?,
        attributes: basic.FileAttributes,
        creation: basic.CreationTime,
        modified: basic.LastWriteTime,
        changed: basic.ChangeTime,
        accessed: basic.LastAccessTime,
        size: standard.EndOfFile,
        allocation: standard.AllocationSize as u64,
    })
}

struct DirectoryEntry {
    next: usize,
    name: Name,
    attributes: u32,
    times: [i64; 3],
}

fn directory_entry(buf: &[u8]) -> Option<DirectoryEntry> {
    let read_u32 = |offset| {
        Some(u32::from_le_bytes(
            buf.get(offset..offset + 4)?.try_into().ok()?,
        ))
    };
    let read_i64 = |offset| {
        Some(i64::from_le_bytes(
            buf.get(offset..offset + 8)?.try_into().ok()?,
        ))
    };
    let next = read_u32(offset_of!(FILE_FULL_DIR_INFO, NextEntryOffset))? as usize;
    let len = read_u32(offset_of!(FILE_FULL_DIR_INFO, FileNameLength))? as usize;
    let start = offset_of!(FILE_FULL_DIR_INFO, FileName);
    let end = start.checked_add(len)?;
    if len == 0
        || !len.is_multiple_of(2)
        || next != 0 && (next < end || next >= buf.len() || !next.is_multiple_of(8))
    {
        return None;
    }
    let name = buf
        .get(start..end)?
        .as_chunks::<2>()
        .0
        .iter()
        .copied()
        .map(u16::from_le_bytes)
        .collect();
    Some(DirectoryEntry {
        next,
        name,
        attributes: read_u32(offset_of!(FILE_FULL_DIR_INFO, FileAttributes))?,
        times: [
            read_i64(offset_of!(FILE_FULL_DIR_INFO, CreationTime))?,
            read_i64(offset_of!(FILE_FULL_DIR_INFO, LastWriteTime))?,
            read_i64(offset_of!(FILE_FULL_DIR_INFO, ChangeTime))?,
        ],
    })
}

struct Scan<'a> {
    catalog: &'a mut Catalog,
    root: usize,
    cutoff: i64,
    seen: usize,
    added: usize,
}

impl Scan<'_> {
    fn walk(&mut self, dir: HANDLE, path: &mut Vec<Name>) {
        // The API requires an 8-byte-aligned buffer. Parse only bounded slices.
        let mut buffer = vec![0u64; 1024];
        loop {
            let size = size_of::<u64>() * buffer.len();
            let ptr = buffer.as_mut_ptr().cast();
            if unsafe { GetFileInformationByHandleEx(dir, FileFullDirectoryInfo, ptr, size as u32) }
                == 0
            {
                if io::Error::last_os_error().raw_os_error() != Some(ERROR_NO_MORE_FILES as i32) {
                    self.catalog.skipped += 1;
                }
                return;
            }
            let mut bytes = unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), size) };
            loop {
                let Some(entry) = directory_entry(bytes) else {
                    self.catalog.skipped += 1;
                    return;
                };
                if self.seen >= MAX_ENTRIES || self.added >= MAX_ITEMS {
                    self.catalog.limited = true;
                    return;
                }
                self.seen += 1;
                self.visit(dir, path, &entry);
                if entry.next == 0 {
                    break;
                }
                bytes = &bytes[entry.next..];
            }
        }
    }

    fn visit(&mut self, dir: HANDLE, path: &mut Vec<Name>, entry: &DirectoryEntry) {
        if entry.name == [46] || entry.name == [46, 46] {
            return;
        }
        if !valid_name(&entry.name) || entry.attributes & UNAVAILABLE != 0 {
            self.catalog.skipped += 1;
            return;
        }
        if entry.attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
            if path.len() >= MAX_DEPTH {
                self.catalog.limited = true;
                self.catalog.skipped += 1;
                return;
            }
            match open_child(dir, &entry.name, true, false) {
                Ok(child) => {
                    path.push(entry.name.clone());
                    self.walk(child.0, path);
                    path.pop();
                }
                Err(_) => self.catalog.skipped += 1,
            }
            return;
        }
        if entry.attributes & EXCLUDED_FILE != 0 || !old_enough(self.cutoff, entry.times) {
            self.catalog.skipped += 1;
            return;
        }
        let result = open_child(dir, &entry.name, false, false)
            .and_then(|file| file_metadata(file.0, self.cutoff));
        let Ok(metadata) = result else {
            self.catalog.skipped += 1;
            return;
        };
        path.push(entry.name.clone());
        let name = path
            .iter()
            .map(|part| String::from_utf16_lossy(part).replace(char::is_control, "\u{fffd}"))
            .collect::<Vec<_>>()
            .join("\\");
        self.catalog.entries.push(Entry {
            root: self.root,
            path: path.clone(),
            item: Item {
                name,
                category: self.catalog.roots[self.root].0,
                bytes: metadata.allocation,
            },
            metadata,
        });
        self.added += 1;
        path.pop();
    }
}

pub fn scan() -> Result<Catalog, String> {
    let local = startup::known_folder(&FOLDERID_LocalAppData)
        .map_err(|_| "현재 계정의 로컬 앱 데이터 폴더를 읽지 못했습니다")?;
    let profile =
        startup::known_folder(&FOLDERID_Profile).map_err(|_| "현재 계정 폴더를 읽지 못했습니다")?;
    let location = profile_location(&profile, &local)
        .map_err(|error| format!("정리 경로를 확인하지 못했습니다: {error}"))?;
    scan_location(location, age_cutoff()?)
}

fn profile_location(profile: &Path, local: &Path) -> io::Result<Location> {
    if !local
        .strip_prefix(profile)
        .is_ok_and(|suffix| !suffix.as_os_str().is_empty())
    {
        return Err(invalid_path());
    }
    // A redirected profile on a network share or device path is outside cleanup scope.
    Location::new(profile)?;
    Location::new(local)
}

fn scan_location(location: Location, cutoff: i64) -> Result<Catalog, String> {
    let parents = location
        .open()
        .map_err(|error| format!("정리 폴더를 열지 못했습니다: {error}"))?;
    let parent = parents.last().unwrap().0;
    let mut catalog = Catalog {
        location,
        roots: Vec::new(),
        entries: Vec::new(),
        skipped: 0,
        limited: false,
    };
    for category in [Category::Temp, Category::Shader] {
        let directory = match open_child(parent, &category.folder(), true, true) {
            Ok(directory) => directory,
            Err(error) => {
                if error.kind() != io::ErrorKind::NotFound {
                    catalog.skipped += 1;
                }
                continue;
            }
        };
        let Ok(root_id) = identity(directory.0) else {
            catalog.skipped += 1;
            continue;
        };
        let root = catalog.roots.len();
        catalog.roots.push((category, root_id));
        let mut scanner = Scan {
            catalog: &mut catalog,
            root,
            cutoff,
            seen: 0,
            added: 0,
        };
        scanner.walk(directory.0, &mut Vec::new());
    }
    catalog.entries.sort_unstable_by(|a, b| {
        b.item
            .bytes
            .cmp(&a.item.bytes)
            .then_with(|| a.item.name.cmp(&b.item.name))
            .then_with(|| a.item.category.cmp(&b.item.category))
            .then_with(|| a.path.cmp(&b.path))
    });
    Ok(catalog)
}

impl Catalog {
    pub fn report(&self) -> ScanReport {
        ScanReport {
            items: self
                .entries
                .iter()
                .map(|entry| entry.item.clone())
                .collect(),
            skipped: self.skipped,
            limited: self.limited,
        }
    }

    pub fn clean(&self, indices: &[usize]) -> Result<CleanupResult, String> {
        self.clean_before(indices, age_cutoff()?)
    }

    fn clean_before(&self, indices: &[usize], cutoff: i64) -> Result<CleanupResult, String> {
        let mut selected = indices.to_vec();
        selected.sort_unstable();
        selected.dedup();
        if selected
            .last()
            .is_some_and(|&index| index >= self.entries.len())
        {
            return Err("미리보기에 없는 파일이 선택되었습니다. 다시 검사하세요".into());
        }
        let mut result = CleanupResult {
            removed: 0,
            bytes: 0,
            skipped: 0,
        };
        if selected.is_empty() {
            return Ok(result);
        }
        let ancestors = self
            .location
            .open()
            .map_err(|error| format!("정리 경로를 다시 열지 못했습니다: {error}"))?;
        let root_parent = ancestors.last().unwrap().0;
        let roots: Vec<_> = self
            .roots
            .iter()
            .map(|&(category, expected)| {
                let root = open_child(root_parent, &category.folder(), true, true).ok()?;
                (identity(root.0).ok()? == expected).then_some(root)
            })
            .collect();
        for index in selected {
            let entry = &self.entries[index];
            let bytes = roots[entry.root]
                .as_ref()
                .and_then(|root| remove_entry(root, entry, cutoff).ok());
            if let Some(bytes) = bytes {
                result.removed += 1;
                result.bytes = result.bytes.saturating_add(bytes);
            } else {
                result.skipped += 1;
            }
        }
        Ok(result)
    }
}

fn remove_entry(root: &Handle, entry: &Entry, cutoff: i64) -> io::Result<u64> {
    let (name, parents) = entry.path.split_last().ok_or_else(invalid_path)?;
    let mut held: Vec<Handle> = Vec::new();
    for part in parents {
        let parent = held.last().unwrap_or(root);
        held.push(open_child(parent.0, part, true, false)?);
    }
    let file = open_child(held.last().unwrap_or(root).0, name, false, false)?;
    if file_metadata(file.0, cutoff)? != entry.metadata {
        return Err(invalid_path());
    }
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    if unsafe {
        SetFileInformationByHandle(
            file.0,
            FileDispositionInfo,
            (&raw const disposition).cast(),
            size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    drop(file);
    Ok(entry.metadata.allocation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::{self, OpenOptions},
        os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "memori-cleanup-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            let fixture = Self(root);
            fs::create_dir(fixture.0.join("Temp")).unwrap();
            fs::create_dir(fixture.0.join("D3DSCache")).unwrap();
            fixture
        }

        fn file(&self, relative: &str, bytes: usize) -> PathBuf {
            let path = self.0.join(relative);
            fs::write(&path, vec![1; bytes]).unwrap();
            path
        }

        fn scan(&self) -> Catalog {
            scan_location(Location::new(&self.0).unwrap(), i64::MAX).unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn preview_selects_only_chosen_files_and_uses_allocated_bytes() {
        let fixture = Fixture::new();
        let nested = fixture.0.join(r"Temp\중첩");
        fs::create_dir(&nested).unwrap();
        let selected = fixture.file(r"Temp\중첩\오래된.tmp", 131_100);
        let untouched = fixture.file(r"D3DSCache\keep.bin", 8192);
        let empty = fixture.file(r"Temp\empty", 0);
        let shared = fixture.file(r"Temp\shared", 8192);
        fs::hard_link(&shared, fixture.0.join("shared-outside")).unwrap();
        let cutoff = age_cutoff().unwrap();
        assert!(
            scan_location(Location::new(&fixture.0).unwrap(), cutoff)
                .unwrap()
                .report()
                .items
                .is_empty(),
            "fresh files are not eligible"
        );
        let catalog = fixture.scan();
        let report = catalog.report();
        assert_eq!(report.items.len(), 2);
        assert_eq!(report.items[0].name, r"중첩\오래된.tmp");
        assert_eq!(report.items[0].category, Category::Temp);
        assert_eq!(report.items[1].category, Category::Shader);
        assert!(report.items[0].bytes >= 131_100);
        assert!(report.skipped >= 2);
        assert!(catalog.clean_before(&[0, 2], i64::MAX).is_err());
        assert!(selected.is_file());
        let result = catalog.clean_before(&[0, 0], i64::MAX).unwrap();
        assert_eq!(result.removed, 1);
        assert_eq!(result.bytes, report.items[0].bytes);
        assert_eq!(result.skipped, 0);
        assert!(!selected.exists());
        assert!(untouched.is_file() && empty.is_file() && shared.is_file() && nested.is_dir());
    }

    #[test]
    fn cleanup_skips_replaced_changed_and_locked_files() {
        let fixture = Fixture::new();
        let replaced = fixture.file(r"Temp\replace.tmp", 8192);
        let edited = fixture.file(r"Temp\edit.tmp", 8192);
        let opened = fixture.file(r"Temp\open.tmp", 8192);
        let eligible = fixture.file(r"Temp\eligible.tmp", 8192);
        let prelocked = fixture.file(r"Temp\prelocked.tmp", 8192);
        let _prelock = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&prelocked)
            .unwrap();
        let catalog = fixture.scan();
        assert_eq!(catalog.report().items.len(), 4);
        assert!(catalog.report().skipped >= 1);

        let moved = fixture.0.join("original.tmp");
        fs::rename(&replaced, &moved).unwrap();
        fs::write(&replaced, vec![1; 8192]).unwrap();
        fs::write(&edited, vec![2; 16384]).unwrap();
        let _lock = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&opened)
            .unwrap();
        let result = catalog.clean_before(&[0, 1, 2, 3], i64::MAX).unwrap();
        assert_eq!((result.removed, result.skipped), (1, 3));
        assert!(moved.is_file() && replaced.is_file() && edited.is_file() && opened.is_file());
        assert!(!eligible.exists());
    }

    fn make_junction(path: &Path, target: &Path) {
        // [MS-FSCC] Mount Point Reparse Data Buffer with an NT path.
        const FSCTL_SET_REPARSE_POINT: u32 = 0x0009_00a4;
        const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xa000_0003;
        fs::create_dir(path).unwrap();
        let handle = OpenOptions::new()
            .write(true)
            .share_mode(0)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .unwrap();
        let substitute: Vec<_> = r"\??\"
            .encode_utf16()
            .chain(target.as_os_str().encode_wide())
            .collect();
        let length = u16::try_from(substitute.len() * 2).unwrap();
        let mut buffer = Vec::new();
        buffer.extend(IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
        for half in [8 + length + 4, 0, 0, length, length + 2, 0] {
            buffer.extend(half.to_le_bytes());
        }
        for unit in substitute.into_iter().chain([0, 0]) {
            buffer.extend(unit.to_le_bytes());
        }
        let mut written = 0;
        let result = unsafe {
            DeviceIoControl(
                handle.as_raw_handle(),
                FSCTL_SET_REPARSE_POINT,
                buffer.as_ptr().cast(),
                buffer.len() as u32,
                null_mut(),
                0,
                &mut written,
                null_mut(),
            )
        };
        assert_ne!(
            result,
            0,
            "junction fixture: {}",
            io::Error::last_os_error()
        );
    }

    #[test]
    fn scan_and_cleanup_refuse_junctions_and_changed_roots() {
        let fixture = Fixture::new();
        let outside = fixture.0.join("outside");
        fs::create_dir(&outside).unwrap();
        let outside_file = fixture.file(r"outside\keep.bin", 8192);
        let temp = fixture.0.join("Temp");
        let link = temp.join("link");
        make_junction(&link, &outside);
        assert!(link.join("keep.bin").is_file());
        assert!(fixture.scan().report().items.is_empty());

        // Move the actual scanned file outside the root, then redirect its old parent to it.
        let nested = temp.join("nested");
        fs::create_dir(&nested).unwrap();
        fixture.file(r"Temp\nested\target.bin", 8192);
        let catalog = fixture.scan();
        assert_eq!(catalog.report().items.len(), 1);
        let moved = outside.join("nested");
        fs::rename(&nested, &moved).unwrap();
        make_junction(&nested, &moved);
        let result = catalog.clean_before(&[0], i64::MAX).unwrap();
        assert_eq!((result.removed, result.skipped), (0, 1));
        assert!(outside_file.is_file() && moved.join("target.bin").is_file());

        fs::rename(&temp, fixture.0.join("old-temp")).unwrap();
        fs::create_dir(&temp).unwrap();
        let new = fixture.file(r"Temp\new.bin", 8192);
        assert_eq!(catalog.clean_before(&[0], i64::MAX).unwrap().skipped, 1);
        assert!(new.is_file());
    }

    #[test]
    fn cleanup_path_scope_rejects_network_device_and_nonprofile_folders() {
        for path in [
            r"\\server\share\Temp",
            r"\\?\C:\Temp",
            r"C:\Temp\..\Windows",
            r"C:\",
        ] {
            assert!(Location::new(Path::new(path)).is_err(), "{path}");
        }
        let fixture = Fixture::new();
        assert!(profile_location(&fixture.0, &fixture.0.join("Temp")).is_ok());
        assert!(profile_location(&fixture.0, &fixture.0).is_err());
        assert!(profile_location(&fixture.0.join("Temp"), &fixture.0.join("D3DSCache")).is_err());
        assert!(old_enough(200, [100, 200, 150]));
        assert!(!old_enough(200, [100, 201, 150]));
        assert!(!old_enough(200, [0, 100, 100]));
    }
}
