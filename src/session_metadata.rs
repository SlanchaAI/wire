use std::path::PathBuf;

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
        "claude-code" => harness(
            "claude-code",
            "Claude Code",
            Some("mcp-host"),
            MetadataConfidence::Explicit,
            "lease-source",
        ),
        "codex-cli" => harness(
            "codex-cli",
            "Codex CLI",
            Some("mcp-host"),
            MetadataConfidence::Explicit,
            "lease-source",
        ),
        "goose" => harness(
            "goose",
            "Goose",
            Some("mcp-host"),
            MetadataConfidence::Explicit,
            "lease-source",
        ),
        "copilot-cli" => harness(
            "copilot-cli",
            "GitHub Copilot CLI",
            Some("mcp-host"),
            MetadataConfidence::Explicit,
            "lease-source",
        ),
        "vscode-workspace" => harness(
            "vscode",
            "VS Code",
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
}
