//! `cargo run -p xtask -- scroll-probe`: whether the end of a library taller than the
//! browser's layout cap can be reached in the grid. Serves the built UI with `mock.js`
//! answering a 300,000-photo library, drives it in headless Chromium, and reads the result
//! the page writes into its title. Not in CI: it needs Chromium, like `screenshots`.

use crate::screenshots::build_and_serve;
use std::path::Path;
use std::process::{Command, ExitCode};

struct Case {
    name: &'static str,
    query: &'static str,
    window: &'static str,
    scale: Option<&'static str>,
    /// What the page must report.
    check: fn(&serde_json::Value) -> Result<(), String>,
}

fn last_is(v: &serde_json::Value, want: i64) -> Result<(), String> {
    match v["last"].as_i64() {
        Some(last) if last == want => Ok(()),
        other => Err(format!("last mounted offset {other:?}, want {want}")),
    }
}

fn one_column(v: &serde_json::Value) -> Result<(), String> {
    match v["cols"].as_i64() {
        Some(1) => Ok(()),
        other => Err(format!("{other:?} columns: the case is meant to have one")),
    }
}

const CASES: &[Case] = &[
    Case {
        name: "end, 1 column",
        query: "huge=300000&folders=20000&tile=large&do=probe-end",
        window: "660,800",
        scale: None,
        check: |v| one_column(v).and_then(|()| last_is(v, 299_999)),
    },
    Case {
        name: "scrollbar to the bottom, 1 column",
        query: "huge=300000&folders=20000&tile=large&do=probe-bottom",
        window: "660,800",
        scale: None,
        check: |v| one_column(v).and_then(|()| last_is(v, 299_999)),
    },
    Case {
        name: "end, 1 column, display scale 2",
        query: "huge=300000&folders=20000&tile=large&do=probe-end",
        window: "660,800",
        scale: Some("2"),
        check: |v| {
            let canvas = v["canvas"].as_f64().unwrap_or(-1.0);
            if canvas > 16_777_214.0 {
                return Err(format!(
                    "canvas {canvas} px is past Chromium's cap at scale 2"
                ));
            }
            last_is(v, 299_999)
        },
    },
    Case {
        name: "a small library, scrollbar to the bottom",
        query: "do=probe-bottom",
        window: "1280,800",
        scale: None,
        check: |v| last_is(v, 90),
    },
];

fn run_case(chromium: &Path, port: u16, case: &Case) -> Result<(), String> {
    let mut args = vec![
        "--headless".to_owned(),
        "--disable-gpu".to_owned(),
        format!("--window-size={}", case.window),
        "--virtual-time-budget=30000".to_owned(),
        "--user-agent=Mozilla/5.0 (Windows NT 10.0; Win64; x64) photon-screenshots".to_owned(),
        format!("--host-resolver-rules=MAP photon.localhost 127.0.0.1:{port}"),
        "--dump-dom".to_owned(),
    ];
    if let Some(scale) = case.scale {
        args.push(format!("--force-device-scale-factor={scale}"));
    }
    args.push(format!(
        "http://127.0.0.1:{port}/?theme=light&{}",
        case.query
    ));
    let out = Command::new(chromium)
        .args(&args)
        .output()
        .map_err(|e| format!("cannot start {}: {e}", chromium.display()))?;
    let dom = String::from_utf8_lossy(&out.stdout);
    let title = dom
        .split("<title>PROBE ")
        .nth(1)
        .and_then(|rest| rest.split("</title>").next())
        .ok_or("the page wrote no PROBE title: the action did not run")?;
    let value: serde_json::Value = serde_json::from_str(&title.replace("&quot;", "\""))
        .map_err(|e| format!("{e}: {title}"))?;
    (case.check)(&value).map_err(|why| format!("{why} ({value})"))
}

pub fn run(root: &Path, args: &[String]) -> ExitCode {
    let (chromium, port) = match build_and_serve(root, args) {
        Ok(served) => served,
        Err(code) => return code,
    };
    let mut failed = 0;
    for case in CASES {
        match run_case(&chromium, port, case) {
            Ok(()) => println!("ok    {}", case.name),
            Err(why) => {
                failed += 1;
                println!("FAIL  {}: {why}", case.name);
            }
        }
    }
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
