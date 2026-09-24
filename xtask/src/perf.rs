//! `cargo xtask perf`: the performance, memory and start-up gates
//! (phase 12 A4, A6, B5, C2).
//!
//! The measuring is `xarast-cli bench <scenario>`, which prints one JSON
//! object per run; this module runs those scenarios, reads the numbers and
//! judges them against `xtask/perf-budgets.txt`. It exits non-zero when a
//! gate is breached, and says which one, by how much, and against what.
//!
//! # Noise control
//!
//! CI runners are shared, slower than the reference machine and have fewer
//! cores, so the gates are built not to cry wolf:
//!
//! - every time is a **median** (or a stated percentile) of several runs;
//! - **time limits are scaled by a calibration** measured right before
//!   each scenario, on the same machine, under the same load: a fixed
//!   CPU workload (`xarast-cli bench calibrate`), single-threaded (`st`)
//!   and on every core (`mt`). A machine that runs it 1.8× slower than
//!   the reference machine gets 1.8× the limit. The factor is never
//!   below 1, so a faster machine does not tighten a gate;
//! - the limits carry a **margin** over the reference-machine measurement
//!   (the budgets file says which and why);
//! - a failing scenario is **run again once**, and only a breach both
//!   times fails the gate.
//!
//! Memory (peak RSS) and the leak measure are not scaled: they do not
//! depend on the machine's speed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use serde_json::Value;

const USAGE: &str = "\
cargo xtask perf [--tier pr|nightly] [--cli PATH] [--budgets FILE]
                 [--only ID,...] [--out FILE.json] [--no-retry] [--list]

Runs the `xarast-cli bench` scenarios of every gate in the tier (default
pr) and judges them against the budgets file (default
xtask/perf-budgets.txt). Without --cli, builds xarast-cli in release first.
Gates whose document is under $CORPUS are skipped when XARAST_XAR_CORPUS
is not set. Exit status 1 when any gate fails.";

/// One gate: a metric of a scenario against a limit.
#[derive(Debug, Clone)]
struct Gate {
    id: String,
    tiers: Vec<String>,
    metric: String,
    limit: f64,
    scale: Scale,
    budget: Option<f64>,
    reference: Option<f64>,
    note: String,
    args: Vec<String>,
}

/// How a limit follows the machine's speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scale {
    /// By the single-threaded calibration.
    St,
    /// By the all-cores calibration.
    Mt,
    /// Not at all (memory).
    None,
}

/// The parsed budgets file.
#[derive(Debug)]
struct Budgets {
    st_ref_ms: f64,
    mt_ref_ms: f64,
    gates: Vec<Gate>,
}

fn parse_budgets(text: &str) -> Result<Budgets, String> {
    let mut calib = None;
    let mut gates = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let at = |e: &str| format!("budgets line {}: {e}: {raw}", n + 1);
        let (body, note) = match line.split_once(" #") {
            Some((b, c)) => (b.trim(), c.trim().to_owned()),
            None => (line, String::new()),
        };
        if let Some(rest) = body.strip_prefix("calibration ") {
            let v: Vec<f64> = rest
                .split_whitespace()
                .map(str::parse)
                .collect::<Result<_, _>>()
                .map_err(|_| at("calibration needs two numbers"))?;
            let [st, mt] = v[..] else {
                return Err(at("calibration needs <st ms> <mt ms>"));
            };
            calib = Some((st, mt));
            continue;
        }
        let (head, args) = body.split_once(" -- ").ok_or_else(|| at("no ` -- `"))?;
        let f: Vec<&str> = head.split_whitespace().collect();
        let [id, tiers, metric, limit, scale, budget, reference] = f[..] else {
            return Err(at(
                "expected <id> <tiers> <metric> <limit> <scale> <budget> <reference> -- <args>",
            ));
        };
        let num = |s: &str, what: &str| -> Result<Option<f64>, String> {
            if s == "-" {
                Ok(None)
            } else {
                s.parse::<f64>()
                    .map(Some)
                    .map_err(|_| at(&format!("{what} `{s}` is not a number")))
            }
        };
        gates.push(Gate {
            id: id.to_owned(),
            tiers: tiers.split(',').map(str::to_owned).collect(),
            metric: metric.to_owned(),
            limit: num(limit, "limit")?.ok_or_else(|| at("a gate needs a limit"))?,
            scale: match scale {
                "st" => Scale::St,
                "mt" => Scale::Mt,
                "none" => Scale::None,
                _ => return Err(at("scale is st, mt or none")),
            },
            budget: num(budget, "budget")?,
            reference: num(reference, "reference")?,
            note,
            args: args.split_whitespace().map(str::to_owned).collect(),
        });
    }
    let (st_ref_ms, mt_ref_ms) = calib.ok_or("the budgets file has no `calibration` line")?;
    Ok(Budgets {
        st_ref_ms,
        mt_ref_ms,
        gates,
    })
}

/// What one scenario run produced.
#[derive(Debug, Clone)]
struct Measured {
    metrics: BTreeMap<String, f64>,
    /// Single- and multi-threaded calibration factors (≥ 1).
    st: f64,
    mt: f64,
    seconds: f64,
}

/// One gate's verdict.
#[derive(Debug, Clone)]
struct Verdict {
    value: Option<f64>,
    limit: f64,
    factor: f64,
    pass: bool,
}

fn judge(g: &Gate, m: &Measured) -> Verdict {
    let factor = match g.scale {
        Scale::St => m.st,
        Scale::Mt => m.mt,
        Scale::None => 1.0,
    };
    let value = m.metrics.get(&g.metric).copied();
    let limit = g.limit * factor;
    Verdict {
        value,
        limit,
        factor,
        pass: value.is_some_and(|v| v <= limit),
    }
}

struct Options {
    tier: String,
    cli: Option<PathBuf>,
    budgets: PathBuf,
    only: Vec<String>,
    out: Option<PathBuf>,
    retry: bool,
    list: bool,
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut o = Options {
        tier: "pr".into(),
        cli: None,
        budgets: crate::repo_root().join("xtask/perf-budgets.txt"),
        only: Vec::new(),
        out: None,
        retry: true,
        list: false,
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = || it.next().cloned().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--tier" => o.tier = value()?,
            "--cli" => o.cli = Some(PathBuf::from(value()?)),
            "--budgets" => o.budgets = PathBuf::from(value()?),
            "--only" => o.only = value()?.split(',').map(str::to_owned).collect(),
            "--out" => o.out = Some(PathBuf::from(value()?)),
            "--no-retry" => o.retry = false,
            "--list" => o.list = true,
            "-h" | "--help" => return Err(USAGE.into()),
            other => return Err(format!("unknown argument {other}\n\n{USAGE}")),
        }
    }
    if o.tier != "pr" && o.tier != "nightly" {
        return Err(format!("--tier is pr or nightly, not {}", o.tier));
    }
    Ok(o)
}

/// Runs `cargo xtask perf`.
pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let o = parse_args(args)?;
    let budgets = parse_budgets(
        &std::fs::read_to_string(&o.budgets)
            .map_err(|e| format!("{}: {e}", o.budgets.display()))?,
    )?;
    let gates: Vec<&Gate> = budgets
        .gates
        .iter()
        .filter(|g| g.tiers.contains(&o.tier))
        .filter(|g| o.only.is_empty() || o.only.contains(&g.id))
        .collect();
    if o.list {
        for g in &gates {
            println!(
                "{:<22} {:<24} ≤ {:<10} {}",
                g.id,
                g.metric,
                g.limit,
                g.args.join(" ")
            );
        }
        return Ok(());
    }
    if gates.is_empty() {
        return Err(format!("no gate in tier {} matches", o.tier).into());
    }
    let cli = match &o.cli {
        Some(c) => c.clone(),
        None => build_cli()?,
    };
    let corpus = std::env::var_os("XARAST_XAR_CORPUS").filter(|c| !c.is_empty());

    println!(
        "perf: tier {}, {} gates, {} ({} cores), load {}",
        o.tier,
        gates.len(),
        cli.display(),
        std::thread::available_parallelism().map_or(1, std::num::NonZero::get),
        load_average().unwrap_or_else(|| "unknown".into())
    );

    // One run per distinct scenario command line, shared by its gates.
    let mut results: BTreeMap<Vec<String>, Result<Measured, String>> = BTreeMap::new();
    let mut skipped = Vec::new();
    for g in &gates {
        let Some(args) = expand_corpus(&g.args, corpus.as_deref()) else {
            skipped.push(g.id.clone());
            continue;
        };
        if let std::collections::btree_map::Entry::Vacant(e) = results.entry(args) {
            let r = measure(&cli, e.key(), &budgets);
            e.insert(r);
        }
    }

    // Judge, then run every failing scenario once more (A6's noise
    // control: one bad run never fails the build on its own).
    let mut first = BTreeMap::new();
    let failing = |results: &BTreeMap<Vec<String>, Result<Measured, String>>| {
        let mut keys = Vec::new();
        for g in &gates {
            let Some(args) = expand_corpus(&g.args, corpus.as_deref()) else {
                continue;
            };
            let ok = matches!(&results[&args], Ok(m) if judge(g, m).pass);
            if !ok && !keys.contains(&args) {
                keys.push(args);
            }
        }
        keys
    };
    if o.retry {
        for args in failing(&results) {
            println!(
                "perf: breach on first run, measuring again: bench {}",
                args.join(" ")
            );
            let again = measure(&cli, &args, &budgets);
            if let Some(old) = results.insert(args.clone(), again) {
                first.insert(args, old);
            }
        }
    }

    // Report.
    println!();
    println!(
        "{:<22} {:<22} {:>11} {:>11} {:>7} {:>11} {:>11}  verdict",
        "gate", "metric", "measured", "limit", "×calib", "budget", "reference"
    );
    let (mut passed, mut failed) = (0usize, Vec::new());
    let mut breaches = Vec::new();
    let mut json_gates = Vec::new();
    for g in &gates {
        let Some(args) = expand_corpus(&g.args, corpus.as_deref()) else {
            println!(
                "{:<22} {:<22} {:>11}  skipped: no XARAST_XAR_CORPUS",
                g.id, g.metric, "-"
            );
            json_gates.push(serde_json::json!({"id": g.id, "metric": g.metric, "skipped": true}));
            continue;
        };
        match &results[&args] {
            Err(e) => {
                println!(
                    "{:<22} {:<22} {:>11}  FAIL: the scenario failed: {e}",
                    g.id, g.metric, "-"
                );
                failed.push(format!("{}: the scenario failed: {e}", g.id));
                json_gates.push(serde_json::json!({"id": g.id, "metric": g.metric, "error": e}));
            }
            Ok(m) => {
                let v = judge(g, m);
                let budget_scaled = g.budget.map(|b| b * v.factor);
                let over_budget = matches!((v.value, budget_scaled), (Some(x), Some(b)) if x > b);
                let verdict = if !v.pass {
                    "FAIL"
                } else if over_budget {
                    "ok, over budget"
                } else {
                    "ok"
                };
                println!(
                    "{:<22} {:<22} {:>11} {:>11} {:>7} {:>11} {:>11}  {verdict}{}",
                    g.id,
                    g.metric,
                    v.value.map_or("missing".into(), fmt),
                    fmt(v.limit),
                    format!("{:.2}", v.factor),
                    budget_scaled.map_or("-".into(), fmt),
                    g.reference.map_or("-".into(), fmt),
                    if g.note.is_empty() || v.pass && !over_budget {
                        String::new()
                    } else {
                        format!(" ({})", g.note)
                    }
                );
                if v.pass {
                    passed += 1;
                } else {
                    failed.push(regression_line(g, &v, first.get(&args)));
                }
                if over_budget {
                    breaches.push(format!(
                        "{} {} = {} over its budget {}{}",
                        g.id,
                        g.metric,
                        v.value.map_or("?".into(), fmt),
                        budget_scaled.map_or("?".into(), fmt),
                        if g.note.is_empty() {
                            String::new()
                        } else {
                            format!(" ({})", g.note)
                        }
                    ));
                }
                json_gates.push(serde_json::json!({
                    "id": g.id,
                    "metric": g.metric,
                    "value": v.value,
                    "limit": v.limit,
                    "factor": v.factor,
                    "budget": budget_scaled,
                    "reference": g.reference,
                    "pass": v.pass,
                    "args": args,
                }));
            }
        }
    }
    println!();
    for b in &breaches {
        println!("perf: known budget breach (gated on its regression limit): {b}");
    }
    for f in &failed {
        println!("perf: REGRESSION: {f}");
    }
    println!(
        "perf: {} gates: {passed} passed, {} failed, {} skipped",
        gates.len(),
        failed.len(),
        skipped.len()
    );

    if let Some(out) = &o.out {
        let runs: Vec<Value> = results
            .iter()
            .map(|(args, r)| match r {
                Ok(m) => serde_json::json!({
                    "args": args, "metrics": m.metrics, "st_factor": m.st,
                    "mt_factor": m.mt, "seconds": m.seconds,
                }),
                Err(e) => serde_json::json!({"args": args, "error": e}),
            })
            .collect();
        let doc = serde_json::json!({
            "tier": o.tier,
            "load": load_average(),
            "cores": std::thread::available_parallelism().map_or(1, std::num::NonZero::get),
            "gates": json_gates,
            "runs": runs,
        });
        if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(out, serde_json::to_string_pretty(&doc)?)?;
        println!("perf: results written to {}", out.display());
    }

    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} performance gate{} failed",
            failed.len(),
            if failed.len() == 1 { "" } else { "s" }
        )
        .into())
    }
}

/// "open-10k median_ms: 250.3 ms > limit 120.0 (+108.6 %); reference
/// 42.1 (+494 %); first run 243.0".
fn regression_line(g: &Gate, v: &Verdict, first: Option<&Result<Measured, String>>) -> String {
    let Some(value) = v.value else {
        return format!(
            "{} {}: metric missing from the scenario's output",
            g.id, g.metric
        );
    };
    let mut s = format!(
        "{} {} = {} > limit {} (+{:.1} % over the limit",
        g.id,
        g.metric,
        fmt(value),
        fmt(v.limit),
        (value / v.limit - 1.0) * 100.0
    );
    if v.factor > 1.0 {
        s += &format!(", scaled ×{:.2} for this machine", v.factor);
    }
    s += ")";
    if let Some(r) = g.reference {
        s += &format!(
            "; reference machine {} ({:+.1} %)",
            fmt(r),
            (value / r - 1.0) * 100.0
        );
    }
    if let Some(Ok(m)) = first
        && let Some(x) = m.metrics.get(&g.metric)
    {
        s += &format!("; first run {}", fmt(*x));
    }
    if !g.note.is_empty() {
        s += &format!("; {}", g.note);
    }
    s
}

fn fmt(v: f64) -> String {
    if v.abs() >= 100.0 {
        format!("{v:.0}")
    } else if v.abs() >= 1.0 {
        format!("{v:.2}")
    } else {
        format!("{v:.4}")
    }
}

/// Replaces `$CORPUS` by the corpus root; `None` when a gate needs the
/// corpus and there is none.
fn expand_corpus(args: &[String], corpus: Option<&std::ffi::OsStr>) -> Option<Vec<String>> {
    args.iter()
        .map(|a| {
            if a.contains("$CORPUS") {
                corpus.map(|c| a.replace("$CORPUS", &c.to_string_lossy()))
            } else {
                Some(a.clone())
            }
        })
        .collect()
}

/// Calibrates, then runs one scenario. `startup` is run as `--runs`
/// separate processes, each timed from spawn to exit.
fn measure(cli: &Path, args: &[String], budgets: &Budgets) -> Result<Measured, String> {
    let t = Instant::now();
    let calib = bench(cli, &["calibrate".into(), "--runs".into(), "5".into()])?;
    let factor =
        |key: &str, reference: f64| calib.get(key).map_or(1.0, |v| (v / reference).max(1.0));
    let st = factor("st_median_ms", budgets.st_ref_ms);
    let mt = factor("mt_median_ms", budgets.mt_ref_ms);
    let metrics = if args.first().is_some_and(|a| a == "startup") {
        startup(cli, args)?
    } else {
        bench(cli, args)?
    };
    Ok(Measured {
        metrics,
        st,
        mt,
        seconds: t.elapsed().as_secs_f64(),
    })
}

/// `--runs N` cold processes of `bench startup`: the median and 95th
/// percentile of spawn → exit (`process_*`), of the process's own entry →
/// first frame (`frame_*`), and the largest peak RSS.
fn startup(cli: &Path, args: &[String]) -> Result<BTreeMap<String, f64>, String> {
    let mut runs = 20usize;
    let mut rest = Vec::new();
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        if a == "--runs" {
            runs = it
                .next()
                .and_then(|v| v.parse().ok())
                .ok_or("startup: --runs needs a number")?;
        } else {
            rest.push(a.clone());
        }
    }
    let mut process = Vec::new();
    let mut frame = Vec::new();
    let mut rss = 0.0f64;
    for _ in 0..runs.max(1) {
        let mut cmd_args = vec!["startup".to_owned(), "--runs".into(), "1".into()];
        cmd_args.extend(rest.iter().cloned());
        let t = Instant::now();
        let m = bench(cli, &cmd_args)?;
        process.push(t.elapsed().as_secs_f64() * 1000.0);
        if let Some(v) = m.get("entry_to_frame_ms") {
            frame.push(*v);
        }
        rss = rss.max(m.get("peak_rss_mib").copied().unwrap_or(0.0));
    }
    let mut out = BTreeMap::new();
    for (name, v) in [("process", &mut process), ("frame", &mut frame)] {
        if v.is_empty() {
            continue;
        }
        v.sort_by(f64::total_cmp);
        out.insert(format!("{name}_median_ms"), percentile(v, 50.0));
        out.insert(format!("{name}_p95_ms"), percentile(v, 95.0));
    }
    out.insert("peak_rss_mib".into(), rss);
    Ok(out)
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

/// Runs `xarast-cli bench ARGS` and returns its `metrics`.
fn bench(cli: &Path, args: &[String]) -> Result<BTreeMap<String, f64>, String> {
    let out = Command::new(cli)
        .arg("bench")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{}: {e}", cli.display()))?;
    if !out.status.success() {
        return Err(format!(
            "bench {} exited with {}: {}",
            args.join(" "),
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout
        .lines()
        .rev()
        .find(|l| l.starts_with('{'))
        .ok_or_else(|| format!("bench {} printed no JSON", args.join(" ")))?;
    let v: Value = serde_json::from_str(line).map_err(|e| format!("bench output: {e}"))?;
    let metrics = v
        .get("metrics")
        .and_then(Value::as_object)
        .ok_or("bench output has no metrics")?;
    Ok(metrics
        .iter()
        .filter_map(|(k, v)| v.as_f64().map(|x| (k.clone(), x)))
        .collect())
}

fn build_cli() -> Result<PathBuf, String> {
    let root = crate::repo_root();
    println!("perf: building xarast-cli (release)");
    let status = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .args([
            "build",
            "--release",
            "-p",
            "xarast-cli",
            "--bin",
            "xarast-cli",
        ])
        .current_dir(&root)
        .status()
        .map_err(|e| format!("cargo: {e}"))?;
    if !status.success() {
        return Err("building xarast-cli failed".into());
    }
    let target =
        std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);
    Ok(target.join("release/xarast-cli"))
}

/// The 1-, 5- and 15-minute load averages, where the system reports them.
fn load_average() -> Option<String> {
    let s = std::fs::read_to_string("/proc/loadavg").ok()?;
    Some(s.split_whitespace().take(3).collect::<Vec<_>>().join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# comment
calibration 20 10
open-10k pr,nightly median_ms 60 st 500 30 -- open --nodes 10000 # a note
rss pr peak_rss_mib 200 none - 90 -- open --nodes 10000
";

    #[test]
    fn the_budgets_file_parses() {
        let b = parse_budgets(SAMPLE).unwrap();
        assert_eq!((b.st_ref_ms, b.mt_ref_ms), (20.0, 10.0));
        assert_eq!(b.gates.len(), 2);
        let g = &b.gates[0];
        assert_eq!(g.id, "open-10k");
        assert_eq!(g.tiers, ["pr", "nightly"]);
        assert_eq!(
            (g.limit, g.budget, g.reference),
            (60.0, Some(500.0), Some(30.0))
        );
        assert_eq!(g.scale, Scale::St);
        assert_eq!(g.note, "a note");
        assert_eq!(g.args, ["open", "--nodes", "10000"]);
        assert_eq!(b.gates[1].budget, None);
        assert!(
            parse_budgets("open pr x 1 st - - -- open").is_err(),
            "no calibration"
        );
        assert!(parse_budgets("calibration 1 1\nopen pr x 1 fast - - -- open").is_err());
    }

    #[test]
    fn limits_scale_with_the_calibration_but_never_tighten() {
        let b = parse_budgets(SAMPLE).unwrap();
        let mut m = Measured {
            metrics: BTreeMap::from([("median_ms".to_owned(), 100.0)]),
            st: 2.0,
            mt: 1.0,
            seconds: 0.0,
        };
        let v = judge(&b.gates[0], &m);
        assert_eq!(v.limit, 120.0);
        assert!(v.pass);
        m.st = 1.0;
        assert!(!judge(&b.gates[0], &m).pass);
        m.metrics.clear();
        let v = judge(&b.gates[0], &m);
        assert!(!v.pass && v.value.is_none(), "a missing metric fails");
    }

    #[test]
    fn a_regression_says_what_and_by_how_much() {
        let b = parse_budgets(SAMPLE).unwrap();
        let v = Verdict {
            value: Some(90.0),
            limit: 60.0,
            factor: 1.0,
            pass: false,
        };
        let s = regression_line(&b.gates[0], &v, None);
        assert!(
            s.contains("open-10k median_ms = 90.00 > limit 60.00 (+50.0 %"),
            "{s}"
        );
        assert!(s.contains("reference machine 30.00 (+200.0 %)"), "{s}");
    }

    #[test]
    fn corpus_gates_are_skipped_without_the_corpus() {
        let args = vec!["open".to_owned(), "--doc".into(), "$CORPUS/a.xar".into()];
        assert_eq!(expand_corpus(&args, None), None);
        assert_eq!(
            expand_corpus(&args, Some(std::ffi::OsStr::new("/c"))).unwrap()[2],
            "/c/a.xar"
        );
    }
}
