use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use tempfile::tempdir;

fn wire_bin() -> &'static str {
    env!("CARGO_BIN_EXE_wire")
}

#[test]
fn mcp_process_owns_lease_for_its_lifetime() {
    let home = tempdir().unwrap();
    let mut child = Command::new(wire_bin())
        .arg("mcp")
        .env("WIRE_HOME", home.path())
        .env("WIRE_HOME_FORCE", "1")
        .env("WIRE_AUTO_INIT", "0")
        .env("WIRE_MCP_SKIP_AUTO_UP", "1")
        .env_remove("WIRE_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("CODEX_SESSION_ID")
        .env_remove("CODEX_THREAD_ID")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let lease = home
        .path()
        .join("state/wire/leases")
        .join(format!("mcp-{}.json", child.id()));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !lease.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(lease.exists(), "MCP did not publish its lifecycle lease");

    drop(child.stdin.take());
    let status = child.wait().unwrap();
    assert!(status.success());
    assert!(!lease.exists(), "clean MCP shutdown left a stale lease");
}
