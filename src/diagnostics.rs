//! Fixed, reviewable diagnostic commands. No shell or user-supplied code is executed.
use crate::Result;
use std::{
    collections::VecDeque,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

pub const TASKS: &[&str] = &[
    "All validation checks (includes release build and real mining)",
    "Rust tests (all targets)",
    "Consensus tests",
    "Network tests",
    "Conformance vectors (Rust)",
    "Recovery tests",
    "Format check",
    "Clippy (warnings are errors)",
    "Independent Python/OpenSSL vectors",
    "Release build",
    "TCP fault injection (builds release first)",
    "Shutdown regression",
    "Offline/mining control regression",
    "Terminal interface smoke test (Unix PTY)",
    "Three-node workshop demo (builds release first)",
    "Hash-rate benchmark",
];

#[derive(Clone, Debug)]
struct Step {
    program: String,
    args: Vec<String>,
}
impl Step {
    fn new(program: &str, args: &[&str]) -> Self {
        Self {
            program: program.into(),
            args: args.iter().map(|s| (*s).into()).collect(),
        }
    }
    fn display(&self) -> String {
        format!("{} {}", self.program, self.args.join(" "))
    }
}
fn cargo(args: &[&str]) -> Step {
    Step::new("cargo", args)
}
fn python(script: &str) -> Step {
    Step::new("python3", &[script])
}
fn release_check(script: &str) -> Step {
    Step::new(
        "python3",
        &[script, "--binary", "target/release/baneworkshop"],
    )
}
fn build() -> Step {
    cargo(&["build", "--release", "--locked"])
}
fn plan(task: usize) -> Vec<Step> {
    match task {
        0 => vec![
            cargo(&["fmt", "--check"]),
            cargo(&[
                "clippy",
                "--locked",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ]),
            cargo(&["test", "--locked", "--all-targets"]),
            build(),
            python("scripts/verify-vectors.py"),
            python("scripts/network-fault-test.py"),
            release_check("scripts/test-network-settings.py"),
            release_check("scripts/test-peers.py"),
            release_check("scripts/test-compatibility.py"),
            release_check("scripts/test-shutdown.py"),
            release_check("scripts/test-controls.py"),
            release_check("scripts/test-tui.py"),
            python("scripts/workshop-demo"),
        ],
        1 => vec![cargo(&["test", "--locked", "--all-targets"])],
        2..=5 => vec![cargo(&[
            "test",
            "--locked",
            "--test",
            ["consensus", "network", "conformance", "recovery"][task - 2],
        ])],
        6 => vec![cargo(&["fmt", "--check"])],
        7 => vec![cargo(&[
            "clippy",
            "--locked",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ])],
        8 => vec![python("scripts/verify-vectors.py")],
        9 => vec![build()],
        10 => vec![
            build(),
            python("scripts/network-fault-test.py"),
            release_check("scripts/test-network-settings.py"),
            release_check("scripts/test-peers.py"),
            release_check("scripts/test-compatibility.py"),
        ],
        11..=13 => vec![
            cargo(&["build", "--locked"]),
            python(
                [
                    "scripts/test-shutdown.py",
                    "scripts/test-controls.py",
                    "scripts/test-tui.py",
                ][task - 11],
            ),
        ],
        14 => vec![build(), python("scripts/workshop-demo")],
        15 => vec![cargo(&["run", "--release", "--locked", "--", "benchmark"])],
        _ => Vec::new(),
    }
}

pub struct Runner {
    pub root: PathBuf,
    pub output: VecDeque<String>,
    pub status: String,
    pub log_path: Option<PathBuf>,
    child: Option<Child>,
    steps: VecDeque<Step>,
    log: Option<File>,
    reader: Option<File>,
    started: Option<Instant>,
    sequence: u64,
    partial: Vec<u8>,
}
impl Default for Runner {
    fn default() -> Self {
        Self {
            root: std::env::var_os("PIXEL_WORKSPACE")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR"))),
            output: VecDeque::new(),
            status: "Select a check and press Enter".into(),
            log_path: None,
            child: None,
            steps: VecDeque::new(),
            log: None,
            reader: None,
            started: None,
            sequence: 0,
            partial: Vec::new(),
        }
    }
}
impl Runner {
    pub fn running(&self) -> bool {
        self.child.is_some()
    }
    pub fn start(&mut self, task: usize, data_dir: &Path) -> Result<()> {
        if self.running() {
            return Err(crate::Error::Invalid(
                "a diagnostic is already running; cancel it first",
            ));
        }
        if !self.root.join("Cargo.toml").is_file() {
            return Err(crate::Error::Invalid(
                "source workspace missing; set PIXEL_WORKSPACE to the repository",
            ));
        }
        let steps = plan(task);
        if steps.is_empty() {
            return Err(crate::Error::Invalid("unknown diagnostic"));
        }
        let dir = data_dir.join("diagnostics");
        std::fs::create_dir_all(&dir)?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        self.sequence += 1;
        let path = dir.join(format!(
            "{stamp}-{}-{}.log",
            std::process::id(),
            self.sequence
        ));
        self.log = Some(
            OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)?,
        );
        self.reader = Some(File::open(&path)?);
        self.log_path = Some(path);
        self.output.clear();
        self.partial.clear();
        self.steps = steps.into();
        self.started = Some(Instant::now());
        self.next()
    }
    fn next(&mut self) -> Result<()> {
        let Some(step) = self.steps.pop_front() else {
            self.status = format!(
                "PASS • completed in {:.1}s",
                self.started.map_or(0.0, |s| s.elapsed().as_secs_f64())
            );
            if let Some(log) = &mut self.log {
                writeln!(log, "\n{}", self.status)?;
            }
            return Ok(());
        };
        self.status = format!("RUNNING • {}", step.display());
        let log = self.log.as_mut().expect("log created before starting");
        writeln!(log, "\n$ {}\n", step.display())?;
        let mut command = Command::new(&step.program);
        command
            .args(&step.args)
            .current_dir(&self.root)
            .env("CARGO_TERM_COLOR", "never")
            .env("NO_COLOR", "1")
            .env("RUST_BACKTRACE", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log.try_clone()?));
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        match command.spawn() {
            Ok(child) => {
                self.child = Some(child);
                Ok(())
            }
            Err(error) => {
                self.steps.clear();
                self.status = format!("FAILED to start {}: {error}", step.program);
                writeln!(log, "{}", self.status)?;
                Err(error.into())
            }
        }
    }
    pub fn poll(&mut self) -> Result<()> {
        if let Some(child) = &mut self.child
            && let Some(status) = child.try_wait()?
        {
            self.child = None;
            if status.success() {
                self.next()?;
            } else {
                self.steps.clear();
                self.status = format!("FAILED • {status} (remaining steps skipped)");
                if let Some(log) = &mut self.log {
                    writeln!(log, "\n{}", self.status)?;
                }
            }
        }
        if let Some(reader) = &mut self.reader {
            let mut bytes = vec![0; 64 * 1024];
            let count = reader.read(&mut bytes)?;
            self.partial.extend_from_slice(&bytes[..count]);
            let complete = if let Some(end) = self.partial.iter().rposition(|b| *b == b'\n') {
                self.partial.drain(..=end).collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            for line in String::from_utf8_lossy(&complete).lines() {
                self.output_line(line);
            }
            // Bound an unterminated line too, while retaining the full file.
            if self.partial.len() > 16 * 1024
                || (count == 0 && !self.running() && !self.partial.is_empty())
            {
                let partial = std::mem::take(&mut self.partial);
                self.output_line(&String::from_utf8_lossy(&partial));
            }
            while self.output.len() > 2000 {
                self.output.pop_front();
            }
        }
        Ok(())
    }
    fn output_line(&mut self, line: &str) {
        self.output.push_back(
            line.chars()
                .filter(|c| !c.is_control() || *c == '\t')
                .take(4000)
                .collect(),
        );
    }
    pub fn cancel(&mut self) {
        self.steps.clear();
        if let Some(mut child) = self.child.take() {
            #[cfg(unix)]
            {
                // Each child owns a fresh process group. Kill descendants too,
                // including test nodes, so no hidden miner survives cancellation.
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
            }
            let _ = child.kill();
            let _ = child.wait();
            self.status = "CANCELLED • diagnostic stopped".into();
            if let Some(log) = &mut self.log {
                let _ = writeln!(log, "\n{}", self.status);
            }
        }
    }
}
impl Drop for Runner {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_menu_item_has_a_plan_and_suite_contains_runtime_regressions() {
        for task in 0..TASKS.len() {
            assert!(!plan(task).is_empty());
        }
        let suite = plan(0)
            .iter()
            .map(Step::display)
            .collect::<Vec<_>>()
            .join("\n");
        for script in [
            "test-shutdown.py",
            "test-controls.py",
            "test-tui.py",
            "network-fault-test.py",
            "test-network-settings.py",
            "test-peers.py",
            "test-compatibility.py",
            "workshop-demo",
        ] {
            assert!(suite.contains(script));
        }
    }
}

#[cfg(all(test, unix))]
mod process_tests {
    use super::*;
    use std::{thread, time::Duration};
    fn runner(steps: Vec<Step>) -> (Runner, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "pixel-runner-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("output.log");
        let mut runner = Runner {
            root: root.clone(),
            log: Some(File::create(&path).unwrap()),
            reader: Some(File::open(&path).unwrap()),
            steps: steps.into(),
            output: VecDeque::new(),
            status: String::new(),
            log_path: Some(path),
            child: None,
            started: None,
            sequence: 0,
            partial: Vec::new(),
        };
        runner.next().unwrap();
        (runner, root)
    }
    fn until(runner: &mut Runner, predicate: impl Fn(&Runner) -> bool) {
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            runner.poll().unwrap();
            if predicate(runner) {
                return;
            }
            assert!(Instant::now() < end, "diagnostic did not complete");
            thread::sleep(Duration::from_millis(10));
        }
    }
    #[test]
    fn failure_captures_both_streams_and_skips_remaining_steps() {
        let (mut runner, root) = runner(vec![
            Step::new("sh", &["-c", "echo stdout; echo stderr >&2; exit 7"]),
            Step::new("sh", &["-c", "echo should-not-run"]),
        ]);
        until(&mut runner, |r| !r.running());
        assert!(runner.status.starts_with("FAILED"));
        let log = std::fs::read_to_string(root.join("output.log")).unwrap();
        assert!(log.contains("stdout") && log.contains("stderr"));
        assert!(!log.contains("should-not-run"));
        drop(runner);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn cancellation_stops_descendants_and_runner_can_report_success() {
        let (mut runner, root) = runner(vec![Step::new(
            "sh",
            &["-c", "(echo READY; sleep 0.3; touch leaked) & wait"],
        )]);
        until(&mut runner, |r| r.output.iter().any(|s| s == "READY"));
        runner.cancel();
        thread::sleep(Duration::from_millis(400));
        assert!(
            !root.join("leaked").exists(),
            "descendant survived cancellation"
        );
        assert!(runner.status.starts_with("CANCELLED"));
        runner.steps = vec![Step::new("sh", &["-c", "echo recovered"])].into();
        runner.next().unwrap();
        until(&mut runner, |r| !r.running());
        assert!(runner.status.starts_with("PASS"));
        drop(runner);
        std::fs::remove_dir_all(root).unwrap();
    }
}
