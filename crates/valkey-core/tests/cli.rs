//! End-to-end test for the valkey-demo binary. Requires Valkey at 127.0.0.1:6379
//! (run `make up` before `cargo test`).

use std::process::Command;

#[test]
fn cli_runs_all_four_contracts_against_live_valkey() {
    let out = Command::new(env!("CARGO_BIN_EXE_valkey-demo"))
        .output()
        .expect("spawn valkey-demo");
    assert!(
        out.status.success(),
        "valkey-demo exited non-zero. stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    for marker in ["[C1]", "[C2]", "[C3]", "[C4]", "[done]"] {
        assert!(
            stdout.contains(marker),
            "expected {marker} in demo stdout, got:\n{stdout}"
        );
    }
}

#[test]
fn cli_honors_valkey_url_env() {
    // Pointing at an unreachable host should non-zero exit. We bound the wait
    // with a 5s overall timeout because ConnectionManager retries.
    let out = Command::new(env!("CARGO_BIN_EXE_valkey-demo"))
        .env("VALKEY_URL", "redis://127.0.0.1:1")
        .spawn()
        .expect("spawn")
        .wait_timeout_or_kill(std::time::Duration::from_secs(5));
    // Either it errored out (exit non-zero) or we killed it after 5s — both
    // prove the demo respected VALKEY_URL and tried to reach the unreachable.
    assert!(out.is_err() || !out.unwrap().success());
}

// Tiny helper trait to time-bound the wait without pulling in `wait-timeout`.
trait WaitTimeoutOrKill {
    fn wait_timeout_or_kill(
        self,
        d: std::time::Duration,
    ) -> Result<std::process::ExitStatus, std::io::Error>;
}

impl WaitTimeoutOrKill for std::process::Child {
    fn wait_timeout_or_kill(
        mut self,
        d: std::time::Duration,
    ) -> Result<std::process::ExitStatus, std::io::Error> {
        let start = std::time::Instant::now();
        loop {
            if let Some(status) = self.try_wait()? {
                return Ok(status);
            }
            if start.elapsed() > d {
                let _ = self.kill();
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "child exceeded timeout — killed",
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}
