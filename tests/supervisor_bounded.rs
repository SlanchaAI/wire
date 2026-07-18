use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

fn wire_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_wire"))
}

fn make_stale_homes(root: &Path, count: usize) {
    for index in 0..count {
        let home = root
            .join("sessions")
            .join("by-key")
            .join(format!("{index:016x}"));
        let config = home.join("config").join("wire");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(
            config.join("agent-card.json"),
            serde_json::to_vec(&serde_json::json!({
                "did": format!("did:wire:stale-{index:04}-00000000"),
                "handle": format!("stale-{index:04}"),
            }))
            .unwrap(),
        )
        .unwrap();
    }
}

fn spawn_supervisor(root: &Path) -> Child {
    Command::new(wire_bin())
        .args([
            "daemon",
            "--all-sessions",
            "--interval",
            "60",
            "--max-workers",
            "4",
        ])
        .env("WIRE_HOME", root)
        .env("WIRE_HOME_FORCE", "1")
        .env_remove("WIRE_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("CODEX_SESSION_ID")
        .env_remove("COPILOT_AGENT_SESSION_ID")
        .env_remove("VSCODE_GIT_REPOSITORY_ROOT")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn isolated supervisor")
}

fn direct_children(pid: u32) -> usize {
    let output = Command::new("ps")
        .args(["-axo", "ppid="])
        .output()
        .expect("read process parents");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .filter(|parent| *parent == pid)
        .count()
}

fn rss_kib(pid: u32) -> u64 {
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .expect("read supervisor RSS");
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap_or(u64::MAX)
}

#[test]
fn stale_655_home_supervisor_is_bounded_across_restart() {
    let temp = tempfile::tempdir().unwrap();
    make_stale_homes(temp.path(), 655);

    for restart in 0..2 {
        let mut supervisor = spawn_supervisor(temp.path());
        std::thread::sleep(Duration::from_millis(900));
        assert!(supervisor.try_wait().unwrap().is_none());
        let children = direct_children(supervisor.id());
        let rss = rss_kib(supervisor.id());
        eprintln!("restart={restart} children={children} rss_kib={rss}");
        assert!(children <= 4, "worker count exceeded configured cap");
        assert!(rss < 256 * 1024, "supervisor RSS exceeded 256 MiB");
        supervisor.kill().unwrap();
        supervisor.wait().unwrap();
        std::thread::sleep(Duration::from_millis(100));
    }
}
