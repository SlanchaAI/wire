use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MetadataConfidence {
    Explicit,
    Inferred,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct MachineDescriptor {
    pub fingerprint: Option<String>,
    pub hostname: String,
    pub os: String,
    pub arch: String,
    pub wire_version: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct HarnessDescriptor {
    pub kind: String,
    pub label: String,
    pub mode: Option<String>,
    pub confidence: MetadataConfidence,
    pub evidence: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct IdentityDescriptor {
    pub source: String,
    pub class: String,
    pub warning: Option<String>,
}

pub fn machine_descriptor(wire_version: &str) -> MachineDescriptor {
    let fingerprint = crate::platform::machine_id_raw()
        .zip(crate::platform::os_user_id_bytes())
        .map(|(machine, user)| {
            hex::encode(crate::same_machine::machine_fingerprint(&machine, &user))
        });
    let hostname = hostname::get()
        .ok()
        .and_then(|value| value.into_string().ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "Unknown".to_string());
    MachineDescriptor {
        fingerprint,
        hostname,
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        wire_version: wire_version.to_string(),
    }
}

pub fn identity_descriptor(source: &str) -> IdentityDescriptor {
    let (class, warning) = match source {
        "override" => ("explicit-override", None),
        "claude-code" | "codex-cli" | "goose" | "copilot-cli" | "vscode-workspace" => {
            ("session-keyed", None)
        }
        "cwd-registry" | "registry" => ("registry-fallback", None),
        "machine-default" => (
            "machine-default",
            Some("Identity propagation missing: this agent uses the machine-default session."),
        ),
        "minted" => (
            "machine-default",
            Some("Identity propagation missing: this agent uses a minted fallback session."),
        ),
        _ => ("unknown", None),
    };
    IdentityDescriptor {
        source: source.to_string(),
        class: class.to_string(),
        warning: warning.map(str::to_string),
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ProjectDescriptor {
    pub name: Option<String>,
    pub root: Option<String>,
    pub cwd: Option<String>,
    pub relative_cwd: Option<String>,
    pub branch: Option<String>,
    pub revision: Option<String>,
    pub worktree_name: Option<String>,
    pub worktree_path: Option<String>,
    pub remote: Option<String>,
    pub confidence: MetadataConfidence,
    pub evidence: String,
}

impl ProjectDescriptor {
    pub fn unknown(cwd: Option<PathBuf>) -> Self {
        Self {
            name: None,
            root: None,
            cwd: cwd.map(|path| path.to_string_lossy().into_owned()),
            relative_cwd: None,
            branch: None,
            revision: None,
            worktree_name: None,
            worktree_path: None,
            remote: None,
            confidence: MetadataConfidence::Unknown,
            evidence: "unavailable".to_string(),
        }
    }
}

fn read_trimmed(path: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn origin_remote(config: &std::path::Path) -> Option<String> {
    let body = std::fs::read_to_string(config).ok()?;
    let mut in_origin = false;
    for raw_line in body.lines() {
        let line = raw_line.trim();
        if line.starts_with('[') {
            in_origin = line == "[remote \"origin\"]";
        } else if in_origin {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            if key.trim() == "url" {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

fn repository_name(remote: Option<&str>, root: &std::path::Path) -> Option<String> {
    remote
        .and_then(|value| value.trim_end_matches('/').rsplit(['/', ':']).next())
        .map(|value| value.trim_end_matches(".git"))
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            root.file_name()
                .and_then(|value| value.to_str())
                .map(str::to_string)
        })
}

pub fn describe_project(cwd: &std::path::Path) -> ProjectDescriptor {
    let cwd = cwd.to_path_buf();
    let Some(root) = cwd
        .ancestors()
        .find(|candidate| candidate.join(".git").exists())
        .map(std::path::Path::to_path_buf)
    else {
        return ProjectDescriptor::unknown(Some(cwd));
    };

    let dot_git = root.join(".git");
    let (gitdir, worktree_name) = if dot_git.is_dir() {
        (dot_git.clone(), None)
    } else {
        let Some(pointer) = read_trimmed(&dot_git).and_then(|value| {
            value
                .strip_prefix("gitdir:")
                .map(str::trim)
                .map(str::to_string)
        }) else {
            return ProjectDescriptor::unknown(Some(cwd));
        };
        let path = std::path::PathBuf::from(pointer);
        let gitdir = if path.is_absolute() {
            path
        } else {
            root.join(path)
        };
        let name = gitdir
            .file_name()
            .and_then(|value| value.to_str())
            .map(str::to_string);
        (gitdir, name)
    };
    let common_dir = read_trimmed(&gitdir.join("commondir"))
        .map(std::path::PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                gitdir.join(path)
            }
        })
        .unwrap_or_else(|| gitdir.clone());
    let head = read_trimmed(&gitdir.join("HEAD"));
    let (branch, revision) = match head.as_deref() {
        Some(value) if value.starts_with("ref: refs/heads/") => (
            Some(value.trim_start_matches("ref: refs/heads/").to_string()),
            None,
        ),
        Some(value) => (None, Some(value.to_string())),
        None => (None, None),
    };
    let remote = origin_remote(&common_dir.join("config"));
    let relative_cwd = cwd.strip_prefix(&root).ok().map(|path| {
        if path.as_os_str().is_empty() {
            ".".to_string()
        } else {
            path.to_string_lossy().into_owned()
        }
    });

    ProjectDescriptor {
        name: repository_name(remote.as_deref(), &root),
        root: Some(root.to_string_lossy().into_owned()),
        cwd: Some(cwd.to_string_lossy().into_owned()),
        relative_cwd,
        branch,
        revision,
        worktree_name,
        worktree_path: dot_git
            .is_file()
            .then(|| root.to_string_lossy().into_owned()),
        remote,
        confidence: MetadataConfidence::Inferred,
        evidence: "git-filesystem".to_string(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProcessObservation {
    pub pid: u32,
    pub parent_pid: Option<u32>,
    pub executable: String,
    pub arguments: Vec<String>,
    pub cwd: Option<PathBuf>,
}

pub(crate) const MAX_ANCESTORS: usize = 8;

#[derive(Clone, Debug, Default)]
pub(crate) struct ProcessSnapshot {
    observations: HashMap<u32, ProcessObservation>,
}

impl ProcessSnapshot {
    #[cfg(test)]
    fn from_observations(observations: Vec<ProcessObservation>) -> Self {
        Self {
            observations: observations
                .into_iter()
                .map(|observation| (observation.pid, observation))
                .collect(),
        }
    }

    pub(crate) fn ancestry(&self, pid: u32) -> Vec<ProcessObservation> {
        let mut rows = Vec::new();
        let mut current = Some(pid);
        while let Some(pid) = current {
            if rows.len() == MAX_ANCESTORS {
                break;
            }
            let Some(observation) = self.observations.get(&pid) else {
                break;
            };
            rows.push(observation.clone());
            current = observation.parent_pid.filter(|parent| *parent != pid);
        }
        rows
    }

    pub(crate) fn cwd(&self, pid: u32) -> Option<PathBuf> {
        self.observations
            .get(&pid)
            .and_then(|observation| observation.cwd.clone())
    }
}

#[derive(Default)]
struct ProcessSnapshotCache {
    pids: Vec<u32>,
    snapshot: ProcessSnapshot,
    initialized: bool,
}

impl ProcessSnapshotCache {
    fn get_or_refresh(
        &mut self,
        pids: &[u32],
        mut probe: impl FnMut(&[u32]) -> Result<ProcessSnapshot, String>,
    ) -> ProcessSnapshot {
        let mut key = pids.to_vec();
        key.sort_unstable();
        key.dedup();
        if !self.initialized || self.pids != key {
            self.snapshot = probe(&key).unwrap_or_default();
            self.pids = key;
            self.initialized = true;
        }
        self.snapshot.clone()
    }
}

static PROCESS_SNAPSHOT_CACHE: OnceLock<Mutex<ProcessSnapshotCache>> = OnceLock::new();

pub(crate) fn process_snapshot(pids: &[u32]) -> ProcessSnapshot {
    PROCESS_SNAPSHOT_CACHE
        .get_or_init(|| Mutex::new(ProcessSnapshotCache::default()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get_or_refresh(pids, capture_process_snapshot)
}

pub(crate) fn harness_from_snapshot(
    snapshot: &ProcessSnapshot,
    pid: u32,
    session_source: &str,
) -> HarnessDescriptor {
    infer_harness(session_source, &snapshot.ancestry(pid))
}

pub(crate) fn project_from_snapshot(
    snapshot: &ProcessSnapshot,
    pid: u32,
    fallback_cwd: Option<&std::path::Path>,
) -> ProjectDescriptor {
    if let Some(cwd) = fallback_cwd {
        describe_project(cwd)
    } else if let Some(cwd) = snapshot.cwd(pid) {
        describe_project(&cwd)
    } else {
        ProjectDescriptor::unknown(None)
    }
}

#[cfg(target_os = "macos")]
fn capture_process_snapshot(pids: &[u32]) -> Result<ProcessSnapshot, String> {
    if pids.is_empty() {
        return Ok(ProcessSnapshot::default());
    }
    let mut ps = Command::new("ps");
    ps.args(["-axo", "pid=,ppid=,comm=,args="]);
    let output = crate::platform::run_with_timeout(ps, Duration::from_secs(5))
        .filter(|output| output.status.success())
        .ok_or_else(|| "process table unavailable".to_string())?;
    let body = String::from_utf8_lossy(&output.stdout);
    let mut all = HashMap::new();
    for line in body.lines() {
        let mut fields = line.split_whitespace();
        let (Some(pid), Some(parent_pid), Some(executable)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let (Ok(pid), Ok(parent_pid)) = (pid.parse::<u32>(), parent_pid.parse::<u32>()) else {
            continue;
        };
        all.insert(
            pid,
            ProcessObservation {
                pid,
                parent_pid: (parent_pid != 0).then_some(parent_pid),
                executable: executable.to_string(),
                arguments: fields.map(str::to_string).collect(),
                cwd: None,
            },
        );
    }

    let pid_list = pids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let mut lsof = Command::new("lsof");
    lsof.args(["-a", "-d", "cwd", "-p", &pid_list, "-Fn"]);
    if let Some(output) = crate::platform::run_with_timeout(lsof, Duration::from_secs(5)) {
        let mut current_pid = None;
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            if let Some(value) = line.strip_prefix('p') {
                current_pid = value.parse::<u32>().ok();
            } else if let (Some(pid), Some(path)) = (current_pid, line.strip_prefix('n'))
                && let Some(observation) = all.get_mut(&pid)
            {
                observation.cwd = Some(PathBuf::from(path));
            }
        }
    }

    let mut selected = HashMap::new();
    for pid in pids {
        let mut current = Some(*pid);
        for _ in 0..MAX_ANCESTORS {
            let Some(process_pid) = current else { break };
            let Some(observation) = all.get(&process_pid).cloned() else {
                break;
            };
            current = observation.parent_pid;
            selected.entry(process_pid).or_insert(observation);
        }
    }
    Ok(ProcessSnapshot {
        observations: selected,
    })
}

#[cfg(target_os = "linux")]
fn capture_process_snapshot(pids: &[u32]) -> Result<ProcessSnapshot, String> {
    let mut observations = HashMap::new();
    for root_pid in pids {
        let mut current = Some(*root_pid);
        for depth in 0..MAX_ANCESTORS {
            let Some(pid) = current else { break };
            if observations.contains_key(&pid) {
                break;
            }
            let proc_dir = PathBuf::from(format!("/proc/{pid}"));
            let status = std::fs::read_to_string(proc_dir.join("status"))
                .map_err(|error| format!("reading process {pid}: {error}"))?;
            let parent_pid = status
                .lines()
                .find_map(|line| line.strip_prefix("PPid:"))
                .and_then(|value| value.trim().parse::<u32>().ok())
                .filter(|value| *value != 0);
            let executable = std::fs::read_link(proc_dir.join("exe"))
                .ok()
                .and_then(|path| {
                    path.file_name()
                        .map(|value| value.to_string_lossy().into_owned())
                })
                .unwrap_or_else(|| "unknown".to_string());
            let arguments = std::fs::read(proc_dir.join("cmdline"))
                .unwrap_or_default()
                .split(|byte| *byte == 0)
                .filter(|value| !value.is_empty())
                .map(|value| String::from_utf8_lossy(value).into_owned())
                .collect();
            let cwd = (depth == 0)
                .then(|| std::fs::read_link(proc_dir.join("cwd")).ok())
                .flatten();
            observations.insert(
                pid,
                ProcessObservation {
                    pid,
                    parent_pid,
                    executable,
                    arguments,
                    cwd,
                },
            );
            current = parent_pid;
        }
    }
    Ok(ProcessSnapshot { observations })
}

#[cfg(windows)]
fn capture_process_snapshot(pids: &[u32]) -> Result<ProcessSnapshot, String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct WindowsProcess {
        process_id: u32,
        parent_process_id: u32,
        executable_path: Option<String>,
        command_line: Option<String>,
    }

    let mut command = Command::new("powershell.exe");
    command.args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,ExecutablePath,CommandLine | ConvertTo-Json -Compress",
    ]);
    let output = crate::platform::run_with_timeout(command, Duration::from_secs(5))
        .filter(|output| output.status.success())
        .ok_or_else(|| "process table unavailable".to_string())?;
    let mut rows: Vec<WindowsProcess> = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("parsing process table: {error}"))?;
    let all: HashMap<u32, WindowsProcess> =
        rows.drain(..).map(|row| (row.process_id, row)).collect();
    let mut observations = HashMap::new();
    for root_pid in pids {
        let mut current = Some(*root_pid);
        for _ in 0..MAX_ANCESTORS {
            let Some(pid) = current else { break };
            let Some(row) = all.get(&pid) else { break };
            let parent_pid = (row.parent_process_id != 0).then_some(row.parent_process_id);
            observations.insert(
                pid,
                ProcessObservation {
                    pid,
                    parent_pid,
                    executable: row
                        .executable_path
                        .clone()
                        .unwrap_or_else(|| "unknown".to_string()),
                    arguments: row
                        .command_line
                        .as_deref()
                        .unwrap_or_default()
                        .split_whitespace()
                        .map(str::to_string)
                        .collect(),
                    cwd: None,
                },
            );
            current = parent_pid;
        }
    }
    Ok(ProcessSnapshot { observations })
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn capture_process_snapshot(_pids: &[u32]) -> Result<ProcessSnapshot, String> {
    Ok(ProcessSnapshot::default())
}

fn harness(
    kind: &str,
    label: &str,
    mode: Option<&str>,
    confidence: MetadataConfidence,
    evidence: &str,
) -> HarnessDescriptor {
    HarnessDescriptor {
        kind: kind.to_string(),
        label: label.to_string(),
        mode: mode.map(str::to_string),
        confidence,
        evidence: evidence.to_string(),
    }
}

pub(crate) fn infer_harness(
    session_source: &str,
    ancestry: &[ProcessObservation],
) -> HarnessDescriptor {
    let explicit = match session_source {
        "claude-code" => Some(("claude-code", "Claude Code")),
        "goose" => Some(("goose", "Goose")),
        "copilot-cli" => Some(("copilot-cli", "GitHub Copilot CLI")),
        "vscode-workspace" => Some(("vscode", "VS Code")),
        _ => None,
    };
    if let Some((kind, label)) = explicit {
        return harness(
            kind,
            label,
            Some("mcp-host"),
            MetadataConfidence::Explicit,
            "lease-source",
        );
    }
    for process in ancestry {
        let executable = std::path::Path::new(&process.executable)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&process.executable)
            .to_ascii_lowercase();
        match executable.as_str() {
            "codex" if process.arguments.iter().any(|arg| arg == "app-server") => {
                return harness(
                    "chatgpt-codex",
                    "ChatGPT Codex",
                    Some("app-server"),
                    MetadataConfidence::Inferred,
                    "process-executable",
                );
            }
            "codex" => {
                let mode = if process.arguments.iter().any(|arg| arg == "resume") {
                    "resume"
                } else {
                    "interactive"
                };
                return harness(
                    "codex-cli",
                    "Codex CLI",
                    Some(mode),
                    MetadataConfidence::Inferred,
                    "process-executable",
                );
            }
            "claude" => {
                return harness(
                    "claude-code",
                    "Claude Code",
                    Some("interactive"),
                    MetadataConfidence::Inferred,
                    "process-executable",
                );
            }
            "goose" => {
                return harness(
                    "goose",
                    "Goose",
                    Some("interactive"),
                    MetadataConfidence::Inferred,
                    "process-executable",
                );
            }
            "cursor" | "cursor.exe" => {
                return harness(
                    "cursor",
                    "Cursor",
                    Some("app-server"),
                    MetadataConfidence::Inferred,
                    "process-executable",
                );
            }
            "code" | "code.exe" => {
                return harness(
                    "vscode",
                    "VS Code",
                    Some("app-server"),
                    MetadataConfidence::Inferred,
                    "process-executable",
                );
            }
            _ => {}
        }
    }

    match session_source {
        "codex-cli" => harness(
            "codex-cli",
            "Codex CLI",
            Some("mcp-host"),
            MetadataConfidence::Explicit,
            "lease-source",
        ),
        _ => harness(
            "unknown",
            "Unknown",
            None,
            MetadataConfidence::Unknown,
            "unavailable",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn process(
        pid: u32,
        parent_pid: Option<u32>,
        executable: &str,
        arguments: &[&str],
    ) -> ProcessObservation {
        ProcessObservation {
            pid,
            parent_pid,
            executable: executable.to_string(),
            arguments: arguments.iter().map(|value| (*value).to_string()).collect(),
            cwd: None,
        }
    }

    #[test]
    fn harness_prefers_explicit_source() {
        let ancestry = vec![
            process(20, Some(10), "wire", &["mcp"]),
            process(10, None, "python", &[]),
        ];
        let harness = infer_harness("goose", &ancestry);

        assert_eq!(harness.kind, "goose");
        assert_eq!(harness.label, "Goose");
        assert_eq!(harness.confidence, MetadataConfidence::Explicit);
        assert_eq!(harness.evidence, "lease-source");
    }

    #[test]
    fn harness_infers_supported_executable_boundaries_and_modes() {
        let cases = [
            (
                process(1, None, "codex", &["resume", "thread"]),
                "codex-cli",
                "resume",
            ),
            (
                process(1, None, "codex", &["app-server"]),
                "chatgpt-codex",
                "app-server",
            ),
            (
                process(1, None, "claude", &["--dangerously-skip-permissions"]),
                "claude-code",
                "interactive",
            ),
            (
                process(1, None, "goose", &["session"]),
                "goose",
                "interactive",
            ),
            (
                process(1, None, "Cursor", &["--type=utility"]),
                "cursor",
                "app-server",
            ),
            (
                process(1, None, "Code", &["--ms-enable-electron-run-as-node"]),
                "vscode",
                "app-server",
            ),
        ];

        for (observation, expected_kind, expected_mode) in cases {
            let harness = infer_harness("machine-default", &[observation]);
            assert_eq!(harness.kind, expected_kind);
            assert_eq!(harness.mode.as_deref(), Some(expected_mode));
            assert_eq!(harness.confidence, MetadataConfidence::Inferred);
        }
    }

    #[test]
    fn harness_does_not_match_arguments_as_executables() {
        let ancestry = vec![process(
            1,
            None,
            "python",
            &["worker.py", "codex", "claude"],
        )];
        let harness = infer_harness("machine-default", &ancestry);

        assert_eq!(harness.kind, "unknown");
        assert_eq!(harness.confidence, MetadataConfidence::Unknown);
        assert_eq!(harness.evidence, "unavailable");
    }

    fn write(path: &std::path::Path, body: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, body).unwrap();
    }

    #[test]
    fn project_discovers_normal_repository_and_nested_cwd() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("descriptive-repo");
        let cwd = root.join("crates/wire/src");
        fs::create_dir_all(&cwd).unwrap();
        write(
            &root.join(".git/HEAD"),
            "ref: refs/heads/feature/provenance\n",
        );
        write(
            &root.join(".git/config"),
            "[remote \"origin\"]\n\turl = git@github.com:SlanchaAI/wire.git\n",
        );

        let project = describe_project(&cwd);

        assert_eq!(project.name.as_deref(), Some("wire"));
        assert_eq!(project.root.as_deref(), root.to_str());
        assert_eq!(project.relative_cwd.as_deref(), Some("crates/wire/src"));
        assert_eq!(project.branch.as_deref(), Some("feature/provenance"));
        assert_eq!(
            project.remote.as_deref(),
            Some("git@github.com:SlanchaAI/wire.git")
        );
        assert_eq!(project.confidence, MetadataConfidence::Inferred);
        assert_eq!(project.evidence, "git-filesystem");
    }

    #[test]
    fn project_discovers_linked_worktree() {
        let temp = tempdir().unwrap();
        let common = temp.path().join("repo/.git");
        let worktree = temp.path().join("operator-dashboard");
        let gitdir = common.join("worktrees/operator-dashboard");
        fs::create_dir_all(&worktree).unwrap();
        write(
            &worktree.join(".git"),
            &format!("gitdir: {}\n", gitdir.display()),
        );
        write(
            &gitdir.join("HEAD"),
            "ref: refs/heads/feat/operator-dashboard\n",
        );
        write(&gitdir.join("commondir"), "../..\n");
        write(
            &common.join("config"),
            "[remote \"origin\"]\n\turl = https://github.com/SlanchaAI/wire.git\n",
        );

        let project = describe_project(&worktree);

        assert_eq!(project.name.as_deref(), Some("wire"));
        assert_eq!(project.worktree_name.as_deref(), Some("operator-dashboard"));
        assert_eq!(project.worktree_path.as_deref(), worktree.to_str());
        assert_eq!(project.branch.as_deref(), Some("feat/operator-dashboard"));
    }

    #[test]
    fn project_handles_detached_missing_remote_and_non_git_directory() {
        let temp = tempdir().unwrap();
        let detached = temp.path().join("detached");
        fs::create_dir_all(&detached).unwrap();
        write(&detached.join(".git/HEAD"), "0123456789abcdef\n");
        write(&detached.join(".git/config"), "[core]\n\tbare = false\n");

        let project = describe_project(&detached);
        assert_eq!(project.branch, None);
        assert_eq!(project.revision.as_deref(), Some("0123456789abcdef"));
        assert_eq!(project.remote, None);

        let plain = temp.path().join("plain");
        fs::create_dir_all(&plain).unwrap();
        let unknown = describe_project(&plain);
        assert_eq!(unknown.cwd.as_deref(), plain.to_str());
        assert_eq!(unknown.name, None);
        assert_eq!(unknown.confidence, MetadataConfidence::Unknown);
    }

    #[test]
    fn process_snapshot_cache_refreshes_only_when_pid_set_changes() {
        let mut cache = ProcessSnapshotCache::default();
        let probes = std::cell::Cell::new(0);
        let mut probe = |pids: &[u32]| {
            probes.set(probes.get() + 1);
            Ok(ProcessSnapshot::from_observations(
                pids.iter()
                    .map(|pid| process(*pid, None, "wire", &["mcp"]))
                    .collect(),
            ))
        };

        cache.get_or_refresh(&[20, 10, 20], &mut probe);
        cache.get_or_refresh(&[10, 20], &mut probe);
        assert_eq!(probes.get(), 1);

        cache.get_or_refresh(&[10, 30], &mut probe);
        assert_eq!(probes.get(), 2);
    }

    #[test]
    fn process_ancestry_is_bounded() {
        let observations = (1..=20)
            .map(|pid| process(pid, (pid > 1).then_some(pid - 1), "parent", &[]))
            .collect();
        let snapshot = ProcessSnapshot::from_observations(observations);

        let ancestry = snapshot.ancestry(20);

        assert_eq!(ancestry.len(), MAX_ANCESTORS);
        assert_eq!(ancestry.first().map(|row| row.pid), Some(20));
        assert_eq!(ancestry.last().map(|row| row.pid), Some(13));
    }

    #[test]
    fn process_probe_failure_fails_open() {
        let mut cache = ProcessSnapshotCache::default();
        let snapshot = cache.get_or_refresh(&[42], |_| Err("probe failed".to_string()));

        assert!(snapshot.ancestry(42).is_empty());
        assert_eq!(snapshot.cwd(42), None);
        assert_eq!(infer_harness("machine-default", &[]).kind, "unknown");
    }
}
