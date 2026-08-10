use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use serde_json::{Value, json};

fn wire_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_wire"))
}

fn wire(root: &Path, args: &[&str]) -> std::process::Output {
    let output = Command::new(wire_bin())
        .args(args)
        .env("WIRE_HOME", root)
        .env("WIRE_HOME_FORCE", "1")
        .env_remove("RUST_LOG")
        .output()
        .expect("spawn wire");
    assert!(
        output.status.success(),
        "wire {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn session_home(root: &Path, name: &str) -> PathBuf {
    root.join("sessions")
        .join("by-key")
        .join(wire::session::by_key_dir_name(
            &wire::session::sanitize_name(name),
        ))
}

fn add_live_lease(home: &Path, source: &str) {
    wire::session_lifecycle::write_lease_at(
        home,
        "mcp",
        std::process::id(),
        time::OffsetDateTime::now_utc(),
        Duration::from_secs(90),
        env!("CARGO_PKG_VERSION"),
        &wire_bin(),
        source,
        Some(Path::new("/work/operator-proof")),
    )
    .unwrap();
}

struct Dashboard(Child);

impl Drop for Dashboard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dashboard_links_two_and_materializes_one_shared_group() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("operator-root");
    std::fs::create_dir_all(&root).unwrap();

    let relay = wire::relay_server::Relay::new(temp.path().join("relay"))
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let relay_address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, relay.router()).await.ok() });
    let relay_url = format!("http://{relay_address}");

    for name in ["alice", "bob", "carol"] {
        wire(
            &root,
            &[
                "session",
                "new",
                name,
                "--relay",
                &relay_url,
                "--no-daemon",
                "--json",
            ],
        );
    }
    add_live_lease(&session_home(&root, "alice"), "codex-cli");
    add_live_lease(&session_home(&root, "bob"), "goose");
    add_live_lease(&session_home(&root, "carol"), "claude-code");

    let mut child = Command::new(wire_bin())
        .args(["dash", "--web", "--no-open"])
        .env("WIRE_HOME", &root)
        .env("WIRE_HOME_FORCE", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut first_line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut first_line)
        .unwrap();
    let url = first_line
        .split_once("dashboard: ")
        .map(|(_, url)| url.trim().to_string())
        .expect("dashboard URL");
    let parsed_url = reqwest::Url::parse(&url).unwrap();
    assert_eq!(parsed_url.host_str(), Some("127.0.0.1"));
    let token = parsed_url
        .query_pairs()
        .find(|(key, _)| key == "token")
        .map(|(_, value)| value.into_owned())
        .unwrap();
    let origin = format!(
        "{}://{}:{}",
        parsed_url.scheme(),
        parsed_url.host_str().unwrap(),
        parsed_url.port().unwrap()
    );
    let _dashboard = Dashboard(child);
    let client = reqwest::Client::new();

    let report: Value = client
        .get(format!("{origin}/api/sessions"))
        .header("X-Wire-Token", &token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let sessions = report["sessions"].as_array().unwrap();
    assert_eq!(report["schema"], "wire-live-sessions-v2");
    assert_eq!(sessions.len(), 3, "live inventory: {report}");
    assert!(
        sessions
            .iter()
            .any(|session| session["harness"]["kind"] == "goose")
    );
    assert!(
        sessions
            .iter()
            .all(|session| session["machine"]["hostname"].is_string())
    );
    assert!(
        sessions
            .iter()
            .all(|session| session["identity"]["source"].is_string())
    );
    assert!(
        sessions
            .iter()
            .all(|session| session["project"]["cwd"] == "/work/operator-proof")
    );
    let serialized = report.to_string();
    for secret_field in [
        "thread_id",
        "session_key",
        "command_line",
        "slot_token",
        "private_key",
    ] {
        assert!(!serialized.contains(secret_field));
    }
    let ids: Vec<String> = sessions
        .iter()
        .map(|session| session["id"].as_str().unwrap().to_string())
        .collect();

    let linked = client
        .post(format!("{origin}/api/links"))
        .header("X-Wire-Token", &token)
        .json(&json!({"sessions": [&ids[0], &ids[1]]}))
        .send()
        .await
        .unwrap();
    assert!(
        linked.status().is_success(),
        "link failed: {}",
        linked.text().await.unwrap()
    );

    let grouped = client
        .post(format!("{origin}/api/groups"))
        .header("X-Wire-Token", &token)
        .json(&json!({
            "name": "operator-proof",
            "creator": &ids[0],
            "members": &ids,
        }))
        .send()
        .await
        .unwrap();
    assert!(
        grouped.status().is_success(),
        "group failed: {}",
        grouped.text().await.unwrap()
    );

    for name in ["alice", "bob", "carol"] {
        let groups = session_home(&root, name).join("config/wire/groups");
        assert_eq!(
            std::fs::read_dir(groups).unwrap().count(),
            1,
            "{name} should hold the shared group"
        );
    }

    let second_home = sessions
        .iter()
        .find(|session| session["id"] == ids[1])
        .and_then(|_| {
            ["alice", "bob", "carol"].iter().find(|name| {
                let output = wire(&session_home(&root, name), &["whoami", "--json"]);
                let who: Value = serde_json::from_slice(&output.stdout).unwrap();
                who["handle"] == ids[1]
            })
        })
        .unwrap();
    let third_id = &ids[2];
    let peers: Value = serde_json::from_slice(
        &wire(&session_home(&root, second_home), &["peers", "--json"]).stdout,
    )
    .unwrap();
    assert!(
        !peers.to_string().contains(third_id),
        "group creation must not directly pair every member: {peers}"
    );
}
