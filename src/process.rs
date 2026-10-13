use crate::AppResult;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(40);
const TERMINATION_GRACE: Duration = Duration::from_millis(500);

#[derive(Debug)]
pub struct CommandResult {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub cancelled: bool,
}

pub fn run_with_timeout(
    command: &mut Command,
    input: Option<Vec<u8>>,
    timeout: Duration,
    maximum_output_bytes: usize,
) -> AppResult<CommandResult> {
    run_cancellable(command, input, timeout, maximum_output_bytes, || false)
}

pub fn run_cancellable(
    command: &mut Command,
    input: Option<Vec<u8>>,
    timeout: Duration,
    maximum_output_bytes: usize,
    cancelled: impl Fn() -> bool,
) -> AppResult<CommandResult> {
    use std::os::unix::process::CommandExt;

    if cancelled() {
        return Err("Maeklog work cancelled before model launch".into());
    }

    command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);

    let mut child = command.spawn()?;
    let process_id = child.id();

    let input_thread = input.map(|payload| {
        let mut stdin = child.stdin.take().expect("piped stdin is available");
        thread::spawn(move || {
            let _ = stdin.write_all(&payload);
        })
    });
    let stdout_thread = child
        .stdout
        .take()
        .map(|stdout| thread::spawn(move || read_bounded(stdout, maximum_output_bytes)));
    let stderr_thread = child
        .stderr
        .take()
        .map(|stderr| thread::spawn(move || read_bounded(stderr, maximum_output_bytes)));

    let started = Instant::now();
    let mut timed_out = false;
    let mut was_cancelled = false;
    let status = 'wait: loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if cancelled() || started.elapsed() >= timeout {
            was_cancelled = cancelled();
            timed_out = !was_cancelled;
            terminate_process_group(process_id, "TERM");
            let deadline = Instant::now() + TERMINATION_GRACE;
            loop {
                if let Some(status) = child.try_wait()? {
                    break 'wait status;
                }
                if Instant::now() >= deadline {
                    terminate_process_group(process_id, "KILL");
                    let _ = child.kill();
                    break 'wait child.wait()?;
                }
                thread::sleep(POLL_INTERVAL);
            }
        }
        thread::sleep(POLL_INTERVAL);
    };

    if was_cancelled || timed_out {
        // A child can ignore TERM after its parent exits and keep a pipe open.
        // This is only the process group created for this command, never Herdr/Codex's pane.
        terminate_process_group(process_id, "KILL");
    }

    if let Some(handle) = input_thread {
        let _ = handle.join();
    }
    let stdout = join_reader(stdout_thread)?;
    let stderr = join_reader(stderr_thread)?;

    Ok(CommandResult {
        success: status.success() && !timed_out && !was_cancelled,
        code: status.code(),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        timed_out,
        cancelled: was_cancelled,
    })
}

fn read_bounded<R: Read>(mut reader: R, maximum_output_bytes: usize) -> Vec<u8> {
    let mut kept = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                let remaining = maximum_output_bytes.saturating_sub(kept.len());
                kept.extend_from_slice(&buffer[..read.min(remaining)]);
            }
        }
    }
    kept
}

fn join_reader(handle: Option<thread::JoinHandle<Vec<u8>>>) -> AppResult<Vec<u8>> {
    match handle {
        Some(handle) => handle
            .join()
            .map_err(|_| "output reader thread panicked".into()),
        None => Ok(Vec::new()),
    }
}

fn terminate_process_group(process_id: u32, signal: &str) {
    let _ = Command::new("/bin/kill")
        .arg(format!("-{signal}"))
        .arg(format!("-{process_id}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_output() {
        let result = run_with_timeout(
            Command::new("sh").arg("-c").arg("printf ok"),
            None,
            Duration::from_secs(2),
            1024,
        )
        .unwrap();
        assert!(result.success);
        assert_eq!(result.stdout, "ok");
    }

    #[test]
    fn times_out_without_hanging() {
        let result = run_with_timeout(
            Command::new("sh").arg("-c").arg("sleep 5"),
            None,
            Duration::from_millis(100),
            1024,
        )
        .unwrap();
        assert!(result.timed_out);
        assert!(!result.success);
    }

    #[test]
    fn cancellation_is_not_timeout_and_stops_term_ignoring_descendants() {
        let started = Instant::now();
        let result = run_cancellable(
            Command::new("sh")
                .arg("-c")
                .arg("sh -c 'trap \"\" TERM; sleep 10' & wait"),
            None,
            Duration::from_secs(5),
            1024,
            || started.elapsed() >= Duration::from_millis(100),
        )
        .unwrap();
        assert!(result.cancelled);
        assert!(!result.timed_out);
        assert!(!result.success);
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
