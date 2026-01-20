use std::fmt::Display;
use std::io;
use std::path::Path;

use iced::futures::channel::mpsc;
use serde::Deserialize;

use crate::log_debug;

pub const DATA_POLLING_INTERVAL_MS: u64 = 100;

#[derive(Debug)]
pub enum OsuCommand {
    RequestBeatmapData,
    UpdateEventForwardSender(mpsc::Sender<MemoryEvent>),
}

#[derive(Debug, Clone)]
pub enum MemoryEvent {
    StatusChanged(OsuStatus),
    BeatmapChanged(Option<BeatmapData>),
    BeatmapDataResponse(Option<BeatmapData>),
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum MemoryError {
    ReadFailed(String),
    InvalidString,
    ProcessNotFound,
    PatternNotFound,
    AccessDenied,
    IoError(io::Error),
}

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            MemoryError::ReadFailed(msg) => write!(f, "Failed to read memory: {}", msg),
            MemoryError::InvalidString => write!(f, "Invalid string data"),
            MemoryError::ProcessNotFound => write!(f, "Process not found"),
            MemoryError::PatternNotFound => write!(f, "Pattern not found in memory"),
            MemoryError::AccessDenied => write!(f, "Access denied to process"),
            MemoryError::IoError(e) => write!(f, "IO error: {}", e),
        }
    }
}

impl std::error::Error for MemoryError {}

impl From<io::Error> for MemoryError {
    fn from(e: io::Error) -> Self {
        MemoryError::IoError(e)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsuClient {
    Stable,
    Lazer,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum OsuStatus {
    #[default]
    Disconnected,
    Scanning,
    Initializing,
    Connected(String),
}

impl Display for OsuStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OsuStatus::Disconnected => write!(f, "Disconnected"),
            OsuStatus::Scanning => write!(f, "Scanning..."),
            OsuStatus::Initializing => write!(f, "Initializing..."),
            OsuStatus::Connected(s) => write!(f, "{}", s),
        }
    }
}

#[derive(Debug, Clone)]
pub enum BeatmapStatus {
    Unknown,
    NotSubmitted,
    Wip,
    Pending,
    Ranked,
    Approved,
    Qualified,
    Loved,
    Graveyard,
    StablePending,
}

impl Display for BeatmapStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BeatmapStatus::Unknown => write!(f, "Unknown"),
            BeatmapStatus::NotSubmitted => write!(f, "Local/Not Submitted"),
            BeatmapStatus::Wip => write!(f, "WIP"),
            BeatmapStatus::Pending => write!(f, "Pending"),
            BeatmapStatus::Ranked => write!(f, "Ranked"),
            BeatmapStatus::Approved => write!(f, "Approved"),
            BeatmapStatus::Qualified => write!(f, "Qualified"),
            BeatmapStatus::Loved => write!(f, "Loved"),
            BeatmapStatus::Graveyard => write!(f, "Graveyard"),
            BeatmapStatus::StablePending => write!(f, "Pending/Graveyard"),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
pub struct ModInfo {
    pub acronym: String,
    #[serde(default)]
    pub settings: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GameplayMods {
    pub mods: Vec<ModInfo>,
    pub mods_string: String,
}

#[derive(Debug, Clone)]
pub struct BeatmapData {
    pub id: i32,
    pub artist: String,
    pub title: String,
    pub difficulty_name: String,
    pub creator: String,
    pub status: BeatmapStatus,
    pub mods: Option<GameplayMods>,
    pub osu_file_path: Option<String>,
    pub songs_folder: Option<String>,
}

#[cfg(windows)]
mod platform {
    use super::MemoryError;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
    };

    pub struct ProcessHandle {
        pub(super) handle: HANDLE,
    }

    impl ProcessHandle {
        pub fn open(pid: u32) -> Result<Self, MemoryError> {
            unsafe {
                let handle = OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, false, pid)
                    .map_err(|_| MemoryError::AccessDenied)?;

                Ok(Self { handle })
            }
        }

        pub fn read_bytes(&self, addr: usize, size: usize) -> Result<Vec<u8>, MemoryError> {
            let mut buffer = vec![0u8; size];
            let mut bytes_read = 0;

            unsafe {
                ReadProcessMemory(
                    self.handle,
                    addr as *const _,
                    buffer.as_mut_ptr() as *mut _,
                    size,
                    Some(&mut bytes_read),
                )
                .map_err(|e| {
                    MemoryError::ReadFailed(format!("ReadProcessMemory failed: {:?}", e))
                })?;
            }

            if bytes_read != size {
                return Err(MemoryError::ReadFailed(format!(
                    "Expected {} bytes, read {}",
                    size, bytes_read
                )));
            }

            Ok(buffer)
        }
    }

    impl Drop for ProcessHandle {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.handle);
            }
        }
    }
}

#[cfg(unix)]
mod platform {
    use super::MemoryError;
    use std::fs::File;
    use std::io::{Read, Seek, SeekFrom};

    pub struct ProcessHandle {
        mem_file: File,
    }

    impl ProcessHandle {
        pub fn open(pid: u32) -> Result<Self, MemoryError> {
            let path = format!("/proc/{}/mem", pid);
            let mem_file = File::open(&path).map_err(|e| {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    MemoryError::AccessDenied
                } else {
                    MemoryError::ProcessNotFound
                }
            })?;

            Ok(Self { mem_file })
        }

        pub fn read_bytes(&self, addr: usize, size: usize) -> Result<Vec<u8>, MemoryError> {
            let mut file = &self.mem_file;
            let mut buffer = vec![0u8; size];

            file.seek(SeekFrom::Start(addr as u64))?;
            file.read_exact(&mut buffer)
                .map_err(|e| MemoryError::ReadFailed(format!("Failed to read memory: {}", e)))?;

            Ok(buffer)
        }
    }
}

// should be fine to send between threads? ai says:
// ProcessHandle wraps OS-level handles that don't have interior mutability.
// The underlying file descriptor/handle is only accessed through &self methods.
unsafe impl Send for platform::ProcessHandle {}
unsafe impl Sync for platform::ProcessHandle {}

#[allow(dead_code)]
pub struct ProcessMemory {
    pid: u32,
    handle: platform::ProcessHandle,
}

impl ProcessMemory {
    pub fn new(pid: u32) -> Result<Self, MemoryError> {
        let handle = platform::ProcessHandle::open(pid)?;
        Ok(Self { pid, handle })
    }

    fn read_bytes(&self, addr: usize, size: usize) -> Result<Vec<u8>, MemoryError> {
        self.handle.read_bytes(addr, size)
    }

    pub fn read_ptr(&self, addr: usize) -> Result<usize, MemoryError> {
        let bytes = self.read_bytes(addr, std::mem::size_of::<usize>())?;
        Ok(usize::from_le_bytes(bytes.try_into().unwrap()))
    }

    pub fn read_ptr32(&self, addr: usize) -> Result<usize, MemoryError> {
        let bytes = self.read_bytes(addr, 4)?;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()) as usize)
    }

    pub fn read_i32(&self, addr: usize) -> Result<i32, MemoryError> {
        let bytes = self.read_bytes(addr, 4)?;
        Ok(i32::from_le_bytes(bytes.try_into().unwrap()))
    }

    pub fn read_u16(&self, addr: usize) -> Result<u16, MemoryError> {
        let bytes = self.read_bytes(addr, 2)?;
        Ok(u16::from_le_bytes(bytes.try_into().unwrap()))
    }

    pub fn pattern_scan(&self, pattern: &[u8], mask: &[bool]) -> Result<usize, MemoryError> {
        #[cfg(windows)]
        {
            use windows::Win32::System::Memory::{
                MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE,
                PAGE_READONLY, PAGE_READWRITE, VirtualQueryEx,
            };

            unsafe {
                let mut address: usize = 0;
                let mut mbi: MEMORY_BASIC_INFORMATION = std::mem::zeroed();

                while VirtualQueryEx(
                    self.handle.handle,
                    Some(address as *const _),
                    &mut mbi,
                    std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
                ) != 0
                {
                    if mbi.State == MEM_COMMIT
                        && (mbi.Protect == PAGE_READONLY
                            || mbi.Protect == PAGE_READWRITE
                            || mbi.Protect == PAGE_EXECUTE_READ
                            || mbi.Protect == PAGE_EXECUTE_READWRITE)
                    {
                        let region_size = mbi.RegionSize;

                        if let Ok(data) = self.read_bytes(address, region_size) {
                            if let Some(offset) = find_pattern(&data, pattern, mask) {
                                return Ok(address + offset);
                            }
                        }
                    }

                    address = (mbi.BaseAddress as usize) + mbi.RegionSize;
                }
            }
        }

        #[cfg(unix)]
        {
            let maps_path = format!("/proc/{}/maps", self.pid);
            let maps_content = std::fs::read_to_string(&maps_path)?;

            for line in maps_content.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() < 2 {
                    continue;
                }

                if !parts[1].starts_with('r') {
                    continue;
                }

                let addr_parts: Vec<&str> = parts[0].split('-').collect();
                if addr_parts.len() != 2 {
                    continue;
                }

                let start = usize::from_str_radix(addr_parts[0], 16).unwrap_or(0);
                let end = usize::from_str_radix(addr_parts[1], 16).unwrap_or(0);

                if start == 0 || end == 0 || end <= start {
                    continue;
                }

                let size = end - start;

                if let Ok(data) = self.read_bytes(start, size)
                    && let Some(offset) = find_pattern(&data, pattern, mask)
                {
                    return Ok(start + offset);
                }
            }
        }

        Err(MemoryError::PatternNotFound)
    }
}

fn find_pattern(data: &[u8], pattern: &[u8], mask: &[bool]) -> Option<usize> {
    if pattern.len() != mask.len() || data.len() < pattern.len() {
        return None;
    }

    for i in 0..=(data.len() - pattern.len()) {
        let mut found = true;
        for j in 0..pattern.len() {
            if mask[j] && data[i + j] != pattern[j] {
                found = false;
                break;
            }
        }
        if found {
            return Some(i);
        }
    }

    None
}

pub fn detect_lazer_version(exe_path: &Path) -> Option<String> {
    let version_file = exe_path.parent()?.join("sq.version");
    let content = std::fs::read_to_string(&version_file).ok()?;

    // parse XML
    let version_start = content.find("<version>")? + "<version>".len();
    let version_end = content[version_start..].find("</version>")?;
    let version_str = &content[version_start..version_start + version_end];

    // remove "-lazer" suffix
    let version = version_str
        .strip_suffix("-lazer")
        .unwrap_or(version_str)
        .to_string();

    Some(version)
}

#[derive(Debug, Clone)]
pub struct DetectedProcess {
    pub client: OsuClient,
    pub pid: u32,
    pub version: Option<String>,
    pub songs_folder: Option<String>,
}

pub fn detect_osu_processes() -> Vec<DetectedProcess> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::everything(),
    );

    let mut result = Vec::new();
    let mut found = [false; 2];

    for (pid, process) in system.processes() {
        let name = process.name().to_string_lossy().to_ascii_lowercase();

        let is_osu = name.contains("osu!") || name == "osu!.exe" || name == "osu!";
        if !is_osu {
            continue;
        }

        let exe_path = process.exe();

        let is_lazer = exe_path
            .and_then(|path| path.to_str())
            .map(|s| s.to_lowercase().contains("lazer"))
            .unwrap_or(false);

        let client = if is_lazer {
            OsuClient::Lazer
        } else {
            OsuClient::Stable
        };

        let version = if is_lazer {
            exe_path.and_then(detect_lazer_version)
        } else {
            None
        };

        if !is_lazer {
            log_debug!(
                "process",
                "Detected osu! stable at exe_path: {:?}",
                exe_path
            );
        }

        let songs_folder = if !is_lazer {
            let folder = exe_path.and_then(|p| p.parent()).map(|p| p.join("Songs"));

            if let Some(ref path) = folder
                && !path.exists()
            {
                log_debug!(
                    "process",
                    "Songs folder does not exist at expected path: {:?}",
                    path
                );
            }

            folder
                .filter(|p| p.exists())
                .and_then(|p| p.to_str().map(|s| s.to_string()))
        } else {
            // for lazer we handle this through memory
            None
        };

        let index = if is_lazer { 1 } else { 0 };
        if !found[index] {
            result.push(DetectedProcess {
                client,
                pid: pid.as_u32(),
                version,
                songs_folder,
            });
            found[index] = true;
        }
    }

    result
}

pub fn order_mods(mods_string: &str) -> String {
    const MOD_ORDER: &[&str] = &[
        "EZ", "HD", "FL", "HR", "DT", "NC", "HT", "DC", "SD", "PF", "NF", "RX", "AP",
    ];

    let mut found: Vec<&str> = Vec::new();
    let mut remaining = mods_string.to_uppercase();

    for &m in MOD_ORDER {
        if remaining.contains(m) {
            found.push(m);
            remaining = remaining.replacen(m, "", 1);
        }
    }

    found.push(&remaining);
    found.concat()
}

pub fn parse_pattern(pattern_str: &str) -> (Vec<u8>, Vec<bool>) {
    let parts: Vec<&str> = pattern_str.split_whitespace().collect();
    let mut pattern = Vec::with_capacity(parts.len());
    let mut mask = Vec::with_capacity(parts.len());

    for part in parts {
        if part == "??" {
            pattern.push(0x00);
            mask.push(false);
        } else {
            pattern.push(u8::from_str_radix(part, 16).unwrap_or(0));
            mask.push(true);
        }
    }

    (pattern, mask)
}
