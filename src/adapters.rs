use crate::rules::{ActionType, Risk};
use crate::util;
use anyhow::Result;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct AdapterCandidate {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub rule_id: String,
    pub rule_label: String,
    pub category: String,
    pub risk: Risk,
    pub action_type: ActionType,
    pub reversible: bool,
    pub reason: String,
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AdapterStatus {
    pub name: &'static str,
    pub detected: bool,
    pub preview_command: Vec<&'static str>,
    pub cleanup_command: Vec<&'static str>,
    pub summary: String,
}

#[derive(Debug, Clone, Copy)]
struct AdapterSpec {
    name: &'static str,
    category: &'static str,
    rule_id: &'static str,
    rule_label: &'static str,
    risk: Risk,
    preview_command: &'static [&'static str],
    cleanup_command: &'static [&'static str],
    reason: &'static str,
}

const ADAPTERS: &[AdapterSpec] = &[
    AdapterSpec {
        name: "systemd-journal",
        category: "systemd_journal",
        rule_id: "adapter-systemd-journal-vacuum",
        rule_label: "systemd journal vacuum",
        risk: Risk::Medium,
        preview_command: &["journalctl", "--disk-usage"],
        cleanup_command: &["journalctl", "--vacuum-time=30d"],
        reason: "systemd journal cleanup should use journalctl vacuum commands, not raw file deletion.",
    },
    AdapterSpec {
        name: "docker",
        category: "docker_data",
        rule_id: "adapter-docker-system-prune",
        rule_label: "Docker system prune",
        risk: Risk::Medium,
        preview_command: &["docker", "system", "df"],
        cleanup_command: &["docker", "system", "prune"],
        reason: "Docker cleanup should use docker system prune or docker builder prune. Volumes are intentionally excluded.",
    },
    AdapterSpec {
        name: "podman",
        category: "podman_data",
        rule_id: "adapter-podman-system-prune",
        rule_label: "Podman system prune",
        risk: Risk::Medium,
        preview_command: &["podman", "system", "df"],
        cleanup_command: &["podman", "system", "prune"],
        reason: "Podman/container storage should be cleaned through podman system prune, not raw file deletion.",
    },
    AdapterSpec {
        name: "nix",
        category: "nix_store",
        rule_id: "adapter-nix-gc-30d",
        rule_label: "Nix garbage collection older than 30 days",
        risk: Risk::Medium,
        preview_command: &["nix-store", "--gc", "--print-dead"],
        cleanup_command: &["nix-collect-garbage", "--delete-older-than", "30d"],
        reason: "Nix store paths must not be manually deleted. Use Nix garbage collection so live roots are respected.",
    },
    AdapterSpec {
        name: "pnpm",
        category: "node_cache",
        rule_id: "adapter-pnpm-store-prune",
        rule_label: "pnpm store prune",
        risk: Risk::Low,
        preview_command: &["pnpm", "store", "status"],
        cleanup_command: &["pnpm", "store", "prune"],
        reason: "pnpm store cleanup should use pnpm store prune.",
    },
    AdapterSpec {
        name: "pip",
        category: "python_cache",
        rule_id: "adapter-pip-cache-purge",
        rule_label: "pip cache purge",
        risk: Risk::Low,
        preview_command: &["pip", "cache", "info"],
        cleanup_command: &["pip", "cache", "purge"],
        reason: "pip cache is normally regenerable, but future installs may be slower or require network access.",
    },
    AdapterSpec {
        name: "uv",
        category: "python_cache",
        rule_id: "adapter-uv-cache-clean",
        rule_label: "uv cache clean",
        risk: Risk::Low,
        preview_command: &["uv", "cache", "dir"],
        cleanup_command: &["uv", "cache", "clean"],
        reason: "uv cache cleanup should use uv cache clean.",
    },
    AdapterSpec {
        name: "go",
        category: "go_cache",
        rule_id: "adapter-go-clean-cache",
        rule_label: "Go build/test cache clean",
        risk: Risk::Low,
        preview_command: &["go", "env", "GOCACHE"],
        cleanup_command: &["go", "clean", "-cache", "-testcache"],
        reason: "Go build and test caches are normally regenerable. Module cache cleanup is intentionally not included.",
    },
];

pub fn print_adapters() {
    println!("Adapters:");
    for status in inspect_adapters() {
        println!(
            "  {:<16} {}",
            status.name,
            if status.detected { "detected" } else { "missing" }
        );
        println!("       preview: {}", status.preview_command.join(" "));
        println!("       cleanup: {}", status.cleanup_command.join(" "));
        if !status.summary.is_empty() {
            println!("       summary: {}", status.summary);
        }
    }
}

pub fn build_adapter_candidates(max_risk: Risk) -> Vec<AdapterCandidate> {
    let mut out = Vec::new();

    for spec in ADAPTERS {
        if !command_exists(spec.preview_command[0]) {
            continue;
        }

        let summary = run_preview_summary(spec.preview_command);
        let blocked_reason = if spec.risk > max_risk {
            Some(format!(
                "adapter risk {} exceeds selected threshold {}",
                spec.risk, max_risk
            ))
        } else {
            None
        };

        out.push(AdapterCandidate {
            path: PathBuf::from(format!("adapter://{}", spec.name)),
            size_bytes: 0,
            rule_id: spec.rule_id.to_string(),
            rule_label: spec.rule_label.to_string(),
            category: spec.category.to_string(),
            risk: spec.risk,
            action_type: ActionType::ToolNative,
            reversible: false,
            reason: format!(
                "{} Suggested command: `{}`. Preview: {}",
                spec.reason,
                spec.cleanup_command.join(" "),
                summary
            ),
            blocked_reason,
        });
    }

    out
}

fn inspect_adapters() -> Vec<AdapterStatus> {
    ADAPTERS
        .iter()
        .map(|spec| {
            let detected = command_exists(spec.preview_command[0]);
            let summary = if detected {
                run_preview_summary(spec.preview_command)
            } else {
                String::new()
            };

            AdapterStatus {
                name: spec.name,
                detected,
                preview_command: spec.preview_command.to_vec(),
                cleanup_command: spec.cleanup_command.to_vec(),
                summary,
            }
        })
        .collect()
}

fn command_exists(command: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| {
                let candidate = dir.join(command);
                candidate.exists()
            })
        })
        .unwrap_or(false)
}

fn run_preview_summary(argv: &[&str]) -> String {
    if argv.is_empty() {
        return "no preview command".to_string();
    }

    let output = Command::new(argv[0]).args(&argv[1..]).output();

    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            summarize_output(&stdout)
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let text = if !stderr.trim().is_empty() { stderr } else { stdout };
            format!("preview exited with {}; {}", output.status, summarize_output(&text))
        }
        Err(err) => format!("preview unavailable: {err}"),
    }
}

fn summarize_output(output: &str) -> String {
    let normalized = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(3)
        .collect::<Vec<_>>()
        .join(" | ");

    if normalized.is_empty() {
        "no output".to_string()
    } else {
        truncate(&normalized, 240)
    }
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        value.to_string()
    } else {
        let mut out = value.chars().take(max_chars).collect::<String>();
        out.push_str("...");
        out
    }
}
