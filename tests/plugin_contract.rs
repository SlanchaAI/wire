use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_json(relative: &str) -> Value {
    let path = repo_root().join(relative);
    let body = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_json::from_str(&body).unwrap_or_else(|error| panic!("parse {}: {error}", path.display()))
}

fn assert_shared_components(manifest: &Value) {
    assert_eq!(manifest["name"], "wire");
    assert_eq!(manifest["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(manifest["skills"], "./skills/");
    assert_eq!(manifest["mcpServers"], "./.mcp.json");
    for field in ["skills", "mcpServers"] {
        let relative = manifest[field].as_str().expect("component path string");
        assert!(repo_root().join(relative).exists(), "missing {relative}");
    }
}

#[test]
fn plugin_manifests_share_components_and_track_release() {
    let claude = read_json(".claude-plugin/plugin.json");
    let codex = read_json(".codex-plugin/plugin.json");
    assert_shared_components(&claude);
    assert_shared_components(&codex);
    assert!(claude.get("hooks").is_some());
    assert!(codex.get("hooks").is_none());
    let mcp = read_json(".mcp.json");
    assert_eq!(mcp["mcpServers"]["wire"]["command"], "wire");
    assert_eq!(mcp["mcpServers"]["wire"]["args"], json!(["mcp"]));
}

#[test]
fn repository_marketplace_exposes_root_wire_plugin() {
    let marketplace = read_json(".agents/plugins/marketplace.json");
    assert_eq!(marketplace["name"], "wire");
    let plugins = marketplace["plugins"].as_array().expect("plugins array");
    assert_eq!(plugins.len(), 1);
    let wire = &plugins[0];
    assert_eq!(wire["name"], "wire");
    assert_eq!(wire["source"], json!({"source": "local", "path": "./"}));
    assert_eq!(wire["policy"]["installation"], "AVAILABLE");
    assert_eq!(wire["policy"]["authentication"], "ON_INSTALL");
    assert_eq!(wire["category"], "Developer Tools");
    assert!(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(".codex-plugin/plugin.json")
            .is_file()
    );
}

#[test]
fn bundled_skill_command_audit_rejects_removed_pairing_surface() {
    let removed = [
        "wire pair-list-pending",
        "wire pair-confirm",
        "wire init <handle>",
        "/wire:wire-",
    ];
    for entry in fs::read_dir(repo_root().join("skills")).expect("read skills") {
        let skill_path = entry.expect("skill entry").path().join("SKILL.md");
        if !skill_path.is_file() {
            continue;
        }
        let body = fs::read_to_string(&skill_path).expect("read skill");
        for signature in removed {
            assert!(
                !body.contains(signature),
                "{} advertises {signature}",
                skill_path.display()
            );
        }
    }
}

#[test]
fn codex_install_signatures_are_documented() {
    let signatures = [
        "codex plugin marketplace add SlanchaAi/wire",
        "codex plugin add wire@wire",
        "codex mcp add wire -- wire mcp",
    ];
    for relative in ["README.md", "docs/PLUGIN.md"] {
        let body = fs::read_to_string(repo_root().join(relative)).expect("read docs");
        for signature in signatures {
            assert!(body.contains(signature), "{relative} missing `{signature}`");
        }
    }
}
