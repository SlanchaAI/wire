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

    fn process(pid: u32, parent_pid: Option<u32>, executable: &str, arguments: &[&str]) -> ProcessObservation {
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
        let ancestry = vec![process(20, Some(10), "wire", &["mcp"]), process(10, None, "python", &[])];
        let harness = infer_harness("goose", &ancestry);

        assert_eq!(harness.kind, "goose");
        assert_eq!(harness.label, "Goose");
        assert_eq!(harness.confidence, MetadataConfidence::Explicit);
        assert_eq!(harness.evidence, "lease-source");
    }

    #[test]
    fn harness_infers_supported_executable_boundaries_and_modes() {
        let cases = [
            (process(1, None, "codex", &["resume", "thread"]), "codex-cli", "resume"),
            (process(1, None, "codex", &["app-server"]), "chatgpt-codex", "app-server"),
            (process(1, None, "claude", &["--dangerously-skip-permissions"]), "claude-code", "interactive"),
            (process(1, None, "goose", &["session"]), "goose", "interactive"),
            (process(1, None, "Cursor", &["--type=utility"]), "cursor", "app-server"),
            (process(1, None, "Code", &["--ms-enable-electron-run-as-node"]), "vscode", "app-server"),
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
        let ancestry = vec![process(1, None, "python", &["worker.py", "codex", "claude"] )];
        let harness = infer_harness("machine-default", &ancestry);

        assert_eq!(harness.kind, "unknown");
        assert_eq!(harness.confidence, MetadataConfidence::Unknown);
        assert_eq!(harness.evidence, "unavailable");
    }
}
