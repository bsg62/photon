//! `cargo run -p xtask -- grid-gate`: the native grid against the Svelte grid, over the same
//! 300,000-photo library, on this machine's screen.
//!
//! It is the gate of the native UI (spec `2026-10-09-photon-native-grid-slice-design.md`):
//! whether egui draws photon's grid well enough to build the rest on. Both applications run
//! the same scroll programme fullscreen, one after the other, and write what they measured;
//! this lays the two reports side by side and says which lines pass.
//!
//! **It opens two fullscreen windows for about two minutes**, which is why it does nothing
//! without `--go`: photon's conventions forbid launching the application to verify a change,
//! and this is the one exception, because the interval between two frames does not exist
//! without a compositor. It needs a desktop that is awake and unlocked for that long.
//! `--dry-run` builds everything and launches nothing.
//!
//! The Svelte grid has no probe of its own. `gate/svelte-probe.patch` gives it one, applied
//! to a throwaway worktree (`target/gate-svelte`) that is built apart from everything else
//! (`target/gate-svelte-target`): the patch is never merged and the working tree is never
//! touched.
//!
//! The pure part - reading two reports and judging them - is tested. Building and running
//! two applications is not.

use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    process::{Command, ExitCode},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const PATCH: &str = "crates/xtask/gate/svelte-probe.patch";
/// How long one application may take over the programme before it is given up on: the
/// programme is half a minute, a launch over 300,000 photos a few seconds more.
const RUN_TIMEOUT: Duration = Duration::from_secs(360);
/// A measure within this share of the other's is a tie. The two clocks are not one
/// instrument: an interval read in a frame callback and one read in the application's own
/// frame are both the compositor's cadence, seen from two places.
const TIE: f64 = 0.05;

#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    go: bool,
    dry_run: bool,
    native_only: bool,
    refresh_hz: Option<f64>,
    photos: usize,
}

impl Options {
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let mut options = Self {
            go: false,
            dry_run: false,
            native_only: false,
            refresh_hz: None,
            photos: 300_000,
        };
        let mut args = args.iter().skip(1);
        while let Some(arg) = args.next() {
            let mut number = |name: &str| -> Result<f64, String> {
                let value = args
                    .next()
                    .ok_or_else(|| format!("{name} needs a number"))?;
                value
                    .parse::<f64>()
                    .ok()
                    .filter(|number| *number > 0.0)
                    .ok_or_else(|| format!("{name} {value} is not a number above zero"))
            };
            match arg.as_str() {
                "--go" => options.go = true,
                "--dry-run" => options.dry_run = true,
                "--native-only" => options.native_only = true,
                "--refresh-hz" => options.refresh_hz = Some(number("--refresh-hz")?),
                "--photos" => options.photos = number("--photos")? as usize,
                other => return Err(format!("unknown argument {other}")),
            }
        }
        if options.go == options.dry_run {
            return Err(
                "say which: --go opens two fullscreen windows for about two minutes; \
                 --dry-run builds everything and launches nothing"
                    .to_owned(),
            );
        }
        if options.go && options.refresh_hz.is_none() {
            return Err("--go needs --refresh-hz, the screen's refresh rate \
                 (hyprctl monitors, xrandr, or the display settings say it)"
                .to_owned());
        }
        Ok(options)
    }
}

/// How one line of the comparison came out.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Pass,
    Fail,
    /// Reported, and judged by a person: the spec gives it no pass line.
    Shown,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub what: &'static str,
    pub native: Option<f64>,
    pub svelte: Option<f64>,
    pub outcome: Outcome,
    pub why: String,
}

fn number(report: &Value, path: &[&str]) -> Option<f64> {
    path.iter()
        .try_fold(report, |value, key| value.get(key))?
        .as_f64()
}

/// The comparison the spec sets: in the two scrolls the native grid's frames are within one
/// refresh at the 95th percentile, and it is no worse than the Svelte grid in time to
/// picture, in memory and in launch.
pub fn judge(native: &Value, svelte: Option<&Value>, refresh_hz: f64) -> Vec<Line> {
    let refresh_ms = 1000.0 / refresh_hz;
    let of = |report: Option<&Value>, path: &[&str]| report.and_then(|r| number(r, path));
    let mut lines = Vec::new();

    // A frame that missed its refresh shows as an interval of two. Half a refresh of
    // slack is the jitter of a frame that did not.
    for (what, step) in [
        ("steady scroll, p95 frame", "steady"),
        ("fast scroll, p95 frame", "fast"),
    ] {
        let p95 = number(native, &[step, "p95_ms"]);
        let (outcome, why) = match p95 {
            Some(p95) if p95 <= refresh_ms * 1.5 => (
                Outcome::Pass,
                format!("within a refresh ({refresh_ms:.1} ms)"),
            ),
            Some(_) => (
                Outcome::Fail,
                format!("a frame in twenty misses its refresh ({refresh_ms:.1} ms)"),
            ),
            None => (Outcome::Fail, "not measured".to_owned()),
        };
        lines.push(Line {
            what,
            native: p95,
            svelte: of(svelte, &[step, "p95_ms"]),
            outcome,
            why,
        });
    }

    for (what, key) in [
        ("jump to the end, to pictures", "jump_end_ms"),
        ("jump to the middle, to pictures", "jump_middle_ms"),
        ("after a sweep, to pictures", "sweep_settle_ms"),
        ("launch, to pictures", "launch_ms"),
        ("memory", "memory_bytes"),
    ] {
        let (ours, theirs) = (number(native, &[key]), of(svelte, &[key]));
        // A frame's grace on a time: both are read once a frame.
        let grace = if key == "memory_bytes" {
            0.0
        } else {
            refresh_ms
        };
        let (outcome, why) = match (ours, theirs) {
            (None, _) => (
                Outcome::Fail,
                "the native grid did not get there".to_owned(),
            ),
            (Some(_), None) => (Outcome::Shown, "nothing to compare with".to_owned()),
            (Some(ours), Some(theirs)) if ours <= theirs * (1.0 + TIE) + grace => {
                let tie = ours >= theirs * (1.0 - TIE) - grace;
                (
                    Outcome::Pass,
                    if tie { "a tie" } else { "better" }.to_owned(),
                )
            }
            (Some(_), Some(_)) => (Outcome::Fail, "worse than the Svelte grid".to_owned()),
        };
        lines.push(Line {
            what,
            native: ours,
            svelte: theirs,
            outcome,
            why,
        });
    }

    // A still grid that repaints is a bug in the wiring: a finding, the spec says, that
    // weighs as much as a failed number.
    let idle = number(native, &["idle_frames"]);
    lines.push(Line {
        what: "frames drawn in five idle seconds",
        native: idle,
        svelte: None,
        outcome: if idle == Some(0.0) {
            Outcome::Pass
        } else {
            Outcome::Fail
        },
        why: "a still grid draws nothing".to_owned(),
    });
    for (what, step) in [
        ("sweep, p95 frame", "sweep"),
        ("steady scroll, longest frame", "steady"),
    ] {
        let key = if what.contains("longest") {
            "longest_ms"
        } else {
            "p95_ms"
        };
        lines.push(Line {
            what,
            native: number(native, &[step, key]),
            svelte: of(svelte, &[step, key]),
            outcome: Outcome::Shown,
            why: String::new(),
        });
    }
    lines
}

fn shown(value: Option<f64>, what: &str) -> String {
    match value {
        None => "-".to_owned(),
        Some(bytes) if what == "memory" => format!("{:.0} MB", bytes / 1_048_576.0),
        Some(frames) if what.starts_with("frames") => format!("{frames:.0}"),
        Some(ms) => format!("{ms:.1} ms"),
    }
}

pub fn table(lines: &[Line]) -> String {
    let mut out = format!("{:<36}{:>12}{:>12}   \n", "", "native", "svelte");
    for line in lines {
        let mark = match line.outcome {
            Outcome::Pass => "pass",
            Outcome::Fail => "FAIL",
            Outcome::Shown => "",
        };
        out.push_str(&format!(
            "{:<36}{:>12}{:>12}   {mark:<5}{}\n",
            line.what,
            shown(line.native, line.what),
            shown(line.svelte, line.what),
            line.why
        ));
    }
    out
}

fn step(what: &str, command: &mut Command) -> Result<(), String> {
    println!("== {what}");
    match command.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("{what}: {status}")),
        Err(err) => Err(format!("{what}: {err}")),
    }
}

/// cargo in the repository, building into its own `target`: the binaries are looked for
/// there, so a `CARGO_TARGET_DIR` this was started under must not send them elsewhere.
fn cargo(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO"));
    command.current_dir(root).env_remove("CARGO_TARGET_DIR");
    command
}

/// The throwaway worktree with the probe patched in, built: the path of its binary.
fn build_svelte(root: &Path) -> Result<PathBuf, String> {
    let tree = root.join("target/gate-svelte");
    let head = Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "HEAD"])
        .output()
        .map_err(|err| format!("git: {err}"))?;
    let head = String::from_utf8_lossy(&head.stdout).trim().to_owned();
    let marker = tree.join(".gate-built-from");
    // Made from another commit, or half made: start again.
    if tree.exists() && std::fs::read_to_string(&marker).ok().as_deref() != Some(head.as_str()) {
        let _ = Command::new("git")
            .current_dir(root)
            .args(["worktree", "remove", "--force"])
            .arg(&tree)
            .status();
        let _ = std::fs::remove_dir_all(&tree);
    }
    if !tree.exists() {
        step(
            "a worktree for the Svelte grid",
            Command::new("git")
                .current_dir(root)
                .args(["worktree", "add", "--detach"])
                .arg(&tree)
                .arg("HEAD"),
        )?;
        step(
            "the probe patched into it",
            Command::new("git")
                .current_dir(&tree)
                .arg("apply")
                .arg(root.join(PATCH)),
        )?;
        step(
            "its packages",
            Command::new("npm").current_dir(&tree).arg("ci"),
        )?;
        std::fs::write(&marker, &head).map_err(|err| format!("{}: {err}", marker.display()))?;
    }
    let target = root.join("target/gate-svelte-target");
    step(
        "the Svelte photon, release",
        Command::new("npm")
            .current_dir(&tree)
            .env("CARGO_TARGET_DIR", &target)
            .args(["run", "tauri", "build", "--", "--no-bundle"]),
    )?;
    Ok(target.join("release/photon"))
}

/// Reads the library into the page cache. The two applications run one after the other
/// over the same file, and the first would otherwise pay for reading it from the disk in
/// its launch while the second found it in memory.
fn warm(data: &Path) {
    for name in ["library.db", "library.db-wal"] {
        let _ = std::fs::read(data.join(name));
    }
}

/// Runs `command` to its end, or kills it at the timeout, and reads the report it wrote.
fn measure(what: &str, command: &mut Command, report: &Path) -> Result<Value, String> {
    println!("== {what}: running");
    let _ = std::fs::remove_file(report);
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |since| since.as_secs_f64() * 1000.0);
    let mut child = command
        .env("PHOTON_PROBE_T0", format!("{started:.3}"))
        .spawn()
        .map_err(|err| format!("{what}: {err}"))?;
    let deadline = Instant::now() + RUN_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                return Err(format!("{what}: no report after {RUN_TIMEOUT:?}; killed"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(err) => return Err(format!("{what}: {err}")),
        }
    }
    let text = std::fs::read_to_string(report)
        .map_err(|err| format!("{what} wrote no report ({}): {err}", report.display()))?;
    serde_json::from_str(&text).map_err(|err| format!("{what}'s report: {err}"))
}

fn gate(root: &Path, options: &Options) -> Result<bool, String> {
    let fixture = root.join("target/gate-fixture");
    let data = fixture.join("data/io.github.bsg62.photon");
    let cache = fixture.join("cache/io.github.bsg62.photon");
    if !data.join("library.db").is_file() {
        step(
            "the fixture library",
            cargo(root)
                .args([
                    "run",
                    "--release",
                    "-p",
                    "photon-ui",
                    "--example",
                    "fixture",
                    "--",
                ])
                .args(["--photos", &options.photos.to_string(), "--out"])
                .arg(&fixture),
        )?;
    }
    step(
        "photon-native, release",
        cargo(root).args(["build", "--release", "-p", "photon-ui"]),
    )?;
    let native = root.join("target/release/photon-native");
    let svelte = if options.native_only {
        None
    } else {
        Some(build_svelte(root)?)
    };
    if options.dry_run {
        println!("== built; --dry-run launches nothing");
        return Ok(true);
    }

    let reports = root.join("target/gate");
    std::fs::create_dir_all(&reports).map_err(|err| format!("{}: {err}", reports.display()))?;
    // The native grid first. Whatever the first run finds cold - a directory entry, a page
    // of the library the warm-up did not reach - then counts against the grid that has
    // to pass, never for it.
    let out = reports.join("native.json");
    warm(&data);
    let native_report = measure(
        "the native grid",
        Command::new(&native)
            .arg("--data-dir")
            .arg(&data)
            .arg("--cache-dir")
            .arg(&cache)
            .arg("--fullscreen")
            .arg("--probe")
            .arg(&out),
        &out,
    )?;

    let svelte_report = match &svelte {
        Some(binary) => {
            let out = reports.join("svelte.json");
            // The Tauri photon finds its library, its cache and its window state under
            // these three; nothing of the user's own photon is read or written.
            warm(&data);
            let report = measure(
                "the Svelte grid",
                Command::new(binary)
                    .env("XDG_DATA_HOME", fixture.join("data"))
                    .env("XDG_CACHE_HOME", fixture.join("cache"))
                    .env("XDG_CONFIG_HOME", fixture.join("config"))
                    .env("PHOTON_PROBE_OUT", &out),
                &out,
            )?;
            Some(report)
        }
        None => None,
    };
    let refresh_hz = options.refresh_hz.unwrap_or(60.0);
    let lines = judge(&native_report, svelte_report.as_ref(), refresh_hz);
    // The fixture's own count: one built before with another `--photos` is used as it is.
    println!("\n{} photos, {refresh_hz} Hz", native_report["photos"]);
    for (name, report) in [
        ("native", Some(&native_report)),
        ("svelte", svelte_report.as_ref()),
    ] {
        if let Some(report) = report {
            println!(
                "{name}: window {} at scale {}, {}",
                report["window"], report["scale"], report["adapter"]
            );
        }
    }
    println!("\n{}", table(&lines));
    println!("reports: {}", reports.display());
    Ok(lines.iter().all(|line| line.outcome != Outcome::Fail))
}

pub fn run(root: &Path, args: &[String]) -> ExitCode {
    let options = match Options::parse(args) {
        Ok(options) => options,
        Err(problem) => {
            eprintln!(
                "{problem}\n\nusage: cargo run -p xtask -- grid-gate (--go --refresh-hz HZ | --dry-run) \
                 [--native-only] [--photos N]"
            );
            return ExitCode::from(2);
        }
    };
    match gate(root, &options) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => {
            println!("the gate did not pass");
            ExitCode::FAILURE
        }
        Err(problem) => {
            eprintln!("{problem}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn args(list: &[&str]) -> Vec<String> {
        std::iter::once("grid-gate")
            .chain(list.iter().copied())
            .map(str::to_owned)
            .collect()
    }

    /// A report in which everything took `ms`, at 60 Hz, in `mb` megabytes.
    fn report(ms: f64, settle: f64, mb: f64) -> Value {
        let cadence =
            json!({ "frames": 600, "median_ms": ms, "p95_ms": ms, "p99_ms": ms, "longest_ms": ms });
        json!({
            "steady": cadence, "fast": cadence, "sweep": cadence,
            "jump_end_ms": settle, "jump_middle_ms": settle, "sweep_settle_ms": settle,
            "launch_ms": 800.0, "memory_bytes": mb * 1_048_576.0, "idle_frames": 0,
        })
    }

    fn outcome(lines: &[Line], what: &str) -> Outcome {
        lines
            .iter()
            .find(|line| line.what == what)
            .unwrap()
            .outcome
            .clone()
    }

    // Opening two fullscreen windows is never something this does by default, or by a
    // flag that could mean something else.
    #[test]
    fn nothing_is_launched_without_being_told_to() {
        assert!(Options::parse(&args(&[])).is_err());
        assert!(Options::parse(&args(&["--refresh-hz", "60"])).is_err());
        assert!(Options::parse(&args(&["--go", "--dry-run", "--refresh-hz", "60"])).is_err());
        let dry = Options::parse(&args(&["--dry-run"])).unwrap();
        assert!(dry.dry_run && !dry.go);
        let go = Options::parse(&args(&["--go", "--refresh-hz", "144", "--native-only"])).unwrap();
        assert!(go.go && go.native_only);
        assert_eq!(go.refresh_hz, Some(144.0));
    }

    // The pass line is against the screen's refresh, so the screen's refresh is not guessed.
    #[test]
    fn a_run_needs_the_screens_refresh_rate() {
        let problem = Options::parse(&args(&["--go"])).unwrap_err();
        assert!(problem.contains("--refresh-hz"), "{problem}");
        assert!(Options::parse(&args(&["--go", "--refresh-hz", "0"])).is_err());
        assert!(Options::parse(&args(&["--go", "--refresh-hz", "fast"])).is_err());
    }

    #[test]
    fn a_grid_on_time_that_is_no_worse_passes_every_line() {
        let lines = judge(
            &report(16.7, 80.0, 300.0),
            Some(&report(16.7, 120.0, 600.0)),
            60.0,
        );
        assert!(
            lines.iter().all(|line| line.outcome != Outcome::Fail),
            "{lines:?}"
        );
        assert_eq!(outcome(&lines, "steady scroll, p95 frame"), Outcome::Pass);
        assert_eq!(outcome(&lines, "memory"), Outcome::Pass);
    }

    // At 60 Hz a refresh is 16.7 ms; a frame that misses one is 33.3 apart from the last.
    #[test]
    fn a_frame_in_twenty_that_misses_its_refresh_fails_the_scroll() {
        let mut native = report(16.7, 80.0, 300.0);
        native["fast"]["p95_ms"] = json!(33.3);
        let lines = judge(&native, Some(&report(16.7, 120.0, 600.0)), 60.0);
        assert_eq!(outcome(&lines, "fast scroll, p95 frame"), Outcome::Fail);
        assert_eq!(outcome(&lines, "steady scroll, p95 frame"), Outcome::Pass);
        // The same intervals at 30 Hz are every frame on time.
        let lines = judge(&native, None, 30.0);
        assert_eq!(outcome(&lines, "fast scroll, p95 frame"), Outcome::Pass);
    }

    #[test]
    fn worse_than_the_svelte_grid_fails_and_within_a_few_percent_is_a_tie() {
        let svelte = report(16.7, 200.0, 600.0);
        // 5% and a frame's grace: 200 * 1.05 + 16.7 = 226.7.
        let lines = judge(&report(16.7, 226.0, 620.0), Some(&svelte), 60.0);
        assert_eq!(
            outcome(&lines, "jump to the end, to pictures"),
            Outcome::Pass
        );
        assert_eq!(outcome(&lines, "memory"), Outcome::Pass);
        let lines = judge(&report(16.7, 228.0, 640.0), Some(&svelte), 60.0);
        assert_eq!(
            outcome(&lines, "jump to the end, to pictures"),
            Outcome::Fail
        );
        assert_eq!(
            outcome(&lines, "memory"),
            Outcome::Fail,
            "630 MB is the line"
        );
    }

    #[test]
    fn a_jump_that_never_showed_its_pictures_fails() {
        let mut native = report(16.7, 80.0, 300.0);
        native["jump_end_ms"] = Value::Null;
        let lines = judge(&native, Some(&report(16.7, 120.0, 600.0)), 60.0);
        assert_eq!(
            outcome(&lines, "jump to the end, to pictures"),
            Outcome::Fail
        );
    }

    #[test]
    fn a_grid_that_draws_while_idle_fails() {
        let mut native = report(16.7, 80.0, 300.0);
        native["idle_frames"] = json!(3);
        let lines = judge(&native, None, 60.0);
        assert_eq!(
            outcome(&lines, "frames drawn in five idle seconds"),
            Outcome::Fail
        );
    }

    // The patch is against files that go on changing, and nothing else reads it until
    // somebody runs the gate. Asked of the index, not of the working tree, so an edit in
    // progress to one of those files does not fail it. Linux only, where the gate is run:
    // a Windows checkout rewrites the patch's line endings.
    #[test]
    #[cfg_attr(not(target_os = "linux"), ignore = "the gate is run on Linux")]
    fn the_svelte_probe_still_applies() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let check = Command::new("git")
            .current_dir(&root)
            .args(["apply", "--check", "--cached", PATCH])
            .output()
            .expect("git");
        assert!(
            check.status.success(),
            "{PATCH} no longer applies: {}",
            String::from_utf8_lossy(&check.stderr)
        );
    }

    // `--native-only`, while tuning: the scrolls are still judged, and what has nothing to
    // be compared with is shown.
    #[test]
    fn without_a_svelte_report_only_the_native_lines_are_judged() {
        let lines = judge(&report(16.7, 80.0, 300.0), None, 60.0);
        assert_eq!(outcome(&lines, "steady scroll, p95 frame"), Outcome::Pass);
        assert_eq!(outcome(&lines, "memory"), Outcome::Shown);
        assert!(table(&lines).contains("300 MB"));
    }
}
