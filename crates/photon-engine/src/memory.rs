//! How much memory photon is using, for Settings → About.
//!
//! The web view does not run inside photon's process. On Linux (WebKitGTK) and Windows
//! (WebView2) its processes are photon's descendants, so the figure is the whole process tree.
//! On macOS WKWebView's processes are XPC services started by launchd, not by photon: nothing
//! public ties them to the app that asked for them (Activity Monitor uses a private API), and
//! matching them by name would count Safari's too. There the figure is photon's own process,
//! and `includes_webview` says so, so the UI never presents a partial number as the whole.
//!
//! Each platform is measured the way its own task manager measures, not by adding up resident
//! sizes, which counts every shared library once per process: PSS on Linux, the private
//! working set on Windows, the physical footprint on macOS.

use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryUsage {
    pub bytes: u64,
    /// How many processes `bytes` covers: photon's own plus the web view's.
    pub processes: u32,
    pub includes_webview: bool,
}

/// `root` and every process descended from it, given each process's `(pid, parent)`.
///
/// A pid is taken once however it is reached: a parent pid is not always live (Windows keeps
/// the pid of a parent that has exited, and may hand it to another process), so the table can
/// hold a cycle, and its idle process is listed as its own parent.
#[cfg(any(target_os = "linux", target_os = "windows", test))]
fn descendants(root: u32, procs: &[(u32, u32)]) -> Vec<u32> {
    let mut found = vec![root];
    let mut next = 0;
    while next < found.len() {
        let parent = found[next];
        for &(pid, ppid) in procs {
            if ppid == parent && !found.contains(&pid) {
                found.push(pid);
            }
        }
        next += 1;
    }
    found
}

#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "windows")]
use windows as platform;

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub fn usage() -> std::io::Result<MemoryUsage> {
    let tree = process_tree()?;
    Ok(MemoryUsage {
        // A process that exits between the listing and the read, or cannot be opened, is
        // left out rather than failing the whole figure.
        bytes: tree
            .iter()
            .filter_map(|&pid| platform::footprint(pid))
            .sum(),
        processes: tree.len() as u32,
        includes_webview: true,
    })
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn process_tree() -> std::io::Result<Vec<u32>> {
    Ok(descendants(std::process::id(), &platform::processes()?))
}

#[cfg(target_os = "macos")]
pub fn usage() -> std::io::Result<MemoryUsage> {
    macos::usage()
}

#[cfg(target_os = "linux")]
mod linux {
    use std::{fs, io};

    /// Every process's `(pid, parent)`.
    pub fn processes() -> io::Result<Vec<(u32, u32)>> {
        Ok(fs::read_dir("/proc")?
            .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<u32>().ok())
            // A process can exit between the listing and the read; it is simply not listed.
            .filter_map(|pid| {
                let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
                Some((pid, parent_of(&stat)?))
            })
            .collect())
    }

    /// The parent pid from `/proc/<pid>/stat`. The command name before it is in parentheses
    /// and may itself hold spaces and parentheses, so the fields are read after the *last* `)`.
    pub(super) fn parent_of(stat: &str) -> Option<u32> {
        let rest = &stat[stat.rfind(')')? + 1..];
        // state, then ppid.
        rest.split_whitespace().nth(1)?.parse().ok()
    }

    /// PSS, which shares each page between the processes mapping it. `smaps_rollup` is read
    /// under ptrace's read check, which a process sandboxed under different credentials can
    /// fail; its resident size from the world-readable `statm` stands in rather than the
    /// process dropping out of the total.
    pub fn footprint(pid: u32) -> Option<u64> {
        if let Some(pss) = fs::read_to_string(format!("/proc/{pid}/smaps_rollup"))
            .ok()
            .and_then(|s| pss_of(&s))
        {
            return Some(pss);
        }
        let statm = fs::read_to_string(format!("/proc/{pid}/statm")).ok()?;
        let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
        Some(pages * page_size())
    }

    pub(super) fn pss_of(rollup: &str) -> Option<u64> {
        let line = rollup.lines().find(|l| l.starts_with("Pss:"))?;
        let kb: u64 = line["Pss:".len()..]
            .trim()
            .strip_suffix("kB")?
            .trim()
            .parse()
            .ok()?;
        Some(kb * 1024)
    }

    fn page_size() -> u64 {
        // SAFETY: sysconf reads a constant and has no preconditions.
        let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if size > 0 { size as u64 } else { 4096 }
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use std::{io, mem};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
            ProcessStatus::{
                GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2,
            },
            Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ},
        },
    };

    /// Every process's `(pid, parent)`.
    pub fn processes() -> io::Result<Vec<(u32, u32)>> {
        // SAFETY: a snapshot handle is closed on every path; the entry's `dwSize` is set
        // before the first call, as the API requires.
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }
            let mut entry: PROCESSENTRY32W = mem::zeroed();
            entry.dwSize = mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut procs = Vec::new();
            let mut more = Process32FirstW(snap, &mut entry) != 0;
            while more {
                procs.push((entry.th32ProcessID, entry.th32ParentProcessID));
                more = Process32NextW(snap, &mut entry) != 0;
            }
            CloseHandle(snap);
            Ok(procs)
        }
    }

    /// The private working set: Task Manager's "Memory" column.
    pub fn footprint(pid: u32) -> Option<u64> {
        // SAFETY: the handle is closed on every path; `cb` names the struct's real size.
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid);
            if handle.is_null() {
                return None;
            }
            let mut counters: PROCESS_MEMORY_COUNTERS_EX2 = mem::zeroed();
            let ok = GetProcessMemoryInfo(
                handle,
                (&raw mut counters).cast::<PROCESS_MEMORY_COUNTERS>(),
                mem::size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32,
            ) != 0;
            CloseHandle(handle);
            ok.then_some(counters.PrivateWorkingSetSize as u64)
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::MemoryUsage;
    use std::{io, mem};

    pub fn usage() -> io::Result<MemoryUsage> {
        // SAFETY: the buffer is a `rusage_info_v2`, the struct the V2 flavour writes.
        let footprint = unsafe {
            let mut info: libc::rusage_info_v2 = mem::zeroed();
            let rc = libc::proc_pid_rusage(
                std::process::id() as libc::c_int,
                libc::RUSAGE_INFO_V2,
                (&raw mut info).cast::<libc::rusage_info_t>(),
            );
            if rc != 0 {
                return Err(io::Error::last_os_error());
            }
            info.ri_phys_footprint
        };
        Ok(MemoryUsage {
            bytes: footprint,
            processes: 1,
            includes_webview: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tree_is_the_root_and_everything_below_it() {
        // 10 is photon, 11 the web view's network process, 12 a web process under a
        // sandbox launcher 13; 20 is another app and 21 its child.
        let procs = [
            (1, 0),
            (10, 1),
            (11, 10),
            (13, 10),
            (12, 13),
            (20, 1),
            (21, 20),
        ];
        let mut tree = descendants(10, &procs);
        tree.sort_unstable();
        assert_eq!(tree, [10, 11, 12, 13]);
    }

    #[test]
    fn a_cycle_in_the_table_is_walked_once() {
        // 0 is its own parent; 5 has taken the pid of 4's exited parent.
        let procs = [(0, 0), (4, 5), (5, 4), (6, 5)];
        let mut tree = descendants(4, &procs);
        tree.sort_unstable();
        assert_eq!(tree, [4, 5, 6]);
        assert_eq!(descendants(0, &procs), [0]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_stat_line_is_read_after_the_last_parenthesis() {
        assert_eq!(linux::parent_of("42 (a) b) (c) S 7 42 42 0 -1"), Some(7));
        assert_eq!(linux::parent_of("42 (WebKitWebProcess) S 10 1 1"), Some(10));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn pss_is_read_from_the_rollup() {
        let rollup = "00400000-7fff [rollup]\nRss:              204800 kB\nPss:              102400 kB\nPss_Anon:          51200 kB\n";
        assert_eq!(linux::pss_of(rollup), Some(102_400 * 1024));
    }

    /// Against the real process table: a process photon starts is part of its tree, as the
    /// web view's are.
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[test]
    fn a_child_process_is_in_the_tree() {
        #[cfg(target_os = "linux")]
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        #[cfg(target_os = "windows")]
        let mut child = std::process::Command::new("ping")
            .args(["-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let tree = process_tree();
        let with = usage();
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(tree.unwrap().contains(&child.id()));
        let with = with.unwrap();
        assert!(with.bytes > 0 && with.processes > 1, "{with:?}");
        assert!(with.includes_webview);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_reports_its_own_process_only() {
        let u = usage().unwrap();
        assert!(u.bytes > 0);
        assert_eq!(u.processes, 1);
        assert!(!u.includes_webview);
    }
}
