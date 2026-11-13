//! Who eats the memory: Claude with everything it started, Chrome, Python, the rest.
//!
//! Sizes are private working sets, the figure Task Manager shows in its
//! "Memory" column, so the numbers here match what the person already knows.

use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proc {
    pub pid: u32,
    pub parent: u32,
    /// Lowercase image name with `.exe`.
    pub name: String,
    pub private_ws: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemGroups {
    pub claude: u64,
    pub chrome: u64,
    pub python: u64,
    pub other: u64,
    pub used: u64,
    pub total: u64,
}

impl MemGroups {
    pub fn percent(&self) -> u8 {
        let rounded = self.used.saturating_mul(100).saturating_add(self.total / 2);
        rounded.checked_div(self.total).map_or(0, |p| p.min(100) as u8)
    }

    /// The strip fills its memory chip from here on.
    pub fn alert(&self) -> bool {
        self.total > 0 && self.used.saturating_mul(100) >= self.total.saturating_mul(85)
    }
}

pub fn group(procs: &[Proc], used: u64, total: u64) -> MemGroups {
    let mut children: HashMap<u32, Vec<usize>> = HashMap::new();
    for (i, p) in procs.iter().enumerate() {
        if p.parent != p.pid {
            children.entry(p.parent).or_default().push(i);
        }
    }
    let tree = |roots: Vec<usize>, taken: &HashSet<usize>| -> HashSet<usize> {
        let mut seen: HashSet<usize> = HashSet::new();
        let mut queue: VecDeque<usize> = roots.into_iter().filter(|i| !taken.contains(i)).collect();
        while let Some(i) = queue.pop_front() {
            if taken.contains(&i) || !seen.insert(i) {
                continue;
            }
            if let Some(kids) = children.get(&procs[i].pid) {
                queue.extend(kids.iter().copied());
            }
        }
        seen
    };
    let roots = |name: &str| procs.iter().enumerate().filter(|(_, p)| p.name == name).map(|(i, _)| i).collect::<Vec<_>>();
    let sum = |set: &HashSet<usize>| set.iter().map(|&i| procs[i].private_ws).sum::<u64>();

    let claude = tree(roots("claude.exe"), &HashSet::new());
    let chrome = tree(roots("chrome.exe"), &claude);
    let python: HashSet<usize> = procs
        .iter()
        .enumerate()
        .filter(|(i, p)| is_python(&p.name) && !claude.contains(i) && !chrome.contains(i))
        .map(|(i, _)| i)
        .collect();

    let (claude, chrome, python) = (sum(&claude), sum(&chrome), sum(&python));
    let other = used.saturating_sub(claude + chrome + python);
    MemGroups { claude, chrome, python, other, used, total }
}

fn is_python(name: &str) -> bool {
    name.ends_with(".exe") && (name.starts_with("python") || name.starts_with("pythonw"))
}

/// Every process with its parent and private working set, plus physical memory.
pub fn sample() -> MemGroups {
    let (used, total) = physical();
    group(&processes(), used, total)
}

fn physical() -> (u64, u64) {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status = MEMORYSTATUSEX { dwLength: size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    // SAFETY: dwLength is set and the struct outlives the call.
    if unsafe { GlobalMemoryStatusEx(&mut status) }.is_err() {
        return (0, 0);
    }
    (status.ullTotalPhys.saturating_sub(status.ullAvailPhys), status.ullTotalPhys)
}

/// One `NtQuerySystemInformation(SystemProcessInformation)` call: name, PID,
/// parent and private working set for every process, no per-process handles.
fn processes() -> Vec<Proc> {
    use windows::Wdk::System::SystemInformation::{NtQuerySystemInformation, SYSTEM_INFORMATION_CLASS};
    const SYSTEM_PROCESS_INFORMATION: SYSTEM_INFORMATION_CLASS = SYSTEM_INFORMATION_CLASS(5);
    const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004_u32 as i32;

    let mut buf: Vec<u64> = vec![0; 1 << 17]; // 1 MiB, 8-byte aligned
    loop {
        let mut needed = 0u32;
        let bytes = (buf.len() * 8) as u32;
        // SAFETY: the buffer is writable for `bytes` bytes and outlives the call.
        let status = unsafe { NtQuerySystemInformation(SYSTEM_PROCESS_INFORMATION, buf.as_mut_ptr().cast(), bytes, &mut needed) };
        if status.0 == STATUS_INFO_LENGTH_MISMATCH {
            buf = vec![0; (needed as usize / 8) * 2 + 4096];
            continue;
        }
        if status.is_err() {
            return Vec::new();
        }
        break;
    }
    // SAFETY: the kernel filled the buffer with a chain of SYSTEM_PROCESS_INFORMATION records.
    unsafe { parse_records(buf.as_ptr().cast(), buf.len() * 8) }
}

/// Offsets of the x64 `SYSTEM_PROCESS_INFORMATION` record (see phnt `ntexapi.h`).
mod spi {
    pub const NEXT: usize = 0;
    pub const PRIVATE_WS: usize = 8;
    pub const NAME_LEN: usize = 56;
    pub const NAME_PTR: usize = 64;
    pub const PID: usize = 80;
    pub const PARENT: usize = 88;
    pub const MIN_SIZE: usize = 96;
}

/// SAFETY: `base` points to `len` readable bytes holding a record chain.
unsafe fn parse_records(base: *const u8, len: usize) -> Vec<Proc> {
    let read = |at: usize, size: usize| -> Option<usize> { (at + size <= len).then_some(at) };
    let mut out = Vec::new();
    let mut at = 0usize;
    while read(at, spi::MIN_SIZE).is_some() {
        // SAFETY: bounds were checked against `len` just above.
        let (next, private_ws, name_len, name_ptr, pid, parent) = unsafe {
            let field = |off: usize| base.add(at + off);
            (
                field(spi::NEXT).cast::<u32>().read_unaligned() as usize,
                field(spi::PRIVATE_WS).cast::<i64>().read_unaligned().max(0) as u64,
                field(spi::NAME_LEN).cast::<u16>().read_unaligned() as usize,
                field(spi::NAME_PTR).cast::<*const u16>().read_unaligned(),
                field(spi::PID).cast::<usize>().read_unaligned() as u32,
                field(spi::PARENT).cast::<usize>().read_unaligned() as u32,
            )
        };
        let name = if name_ptr.is_null() || name_len == 0 {
            String::new()
        } else {
            // SAFETY: the name buffer lives inside the same kernel-filled block.
            String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(name_ptr, name_len / 2) }).to_lowercase()
        };
        out.push(Proc { pid, parent, name, private_ws });
        if next == 0 {
            break;
        }
        at += next;
    }
    out
}
