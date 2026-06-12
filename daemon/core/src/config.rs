use std::fs;
use std::io;
use std::path::{Path, PathBuf};
#[cfg(not(test))]
use std::process::Command;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::DEFAULT_LISTEN_ADDR;

const DOMAIN_EXAMPLE_COMMENT: &str = "# domain: example.your-tailnet.ts.net";

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("yaml error: {0}")]
    Yaml(#[from] serde_yaml::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SystemConfig {
    #[serde(default = "default_listen_addr")]
    pub listen_addr: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls: Option<TlsConfig>,
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            listen_addr: default_listen_addr(),
            domain: None,
            tls: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PairingState {
    #[serde(default)]
    pub clients: Vec<ClientCredentialRecord>,
    #[serde(default)]
    pub pairing: Option<PairingRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TlsConfig {
    pub cert_file: String,
    pub key_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClientCredentialRecord {
    pub client_id: String,
    pub label: String,
    pub secret_sha256_base64url: String,
    pub created_at_unix: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PairingRecord {
    pub code_sha256_base64url: String,
    pub expires_at_unix: i64,
}

#[derive(Debug, Deserialize)]
struct TailscaleStatus {
    #[serde(rename = "Self")]
    self_node: Option<TailscaleSelfNode>,
}

#[derive(Debug, Deserialize)]
struct TailscaleSelfNode {
    #[serde(rename = "DNSName")]
    dns_name: Option<String>,
}

fn default_listen_addr() -> String {
    DEFAULT_LISTEN_ADDR.to_string()
}

pub fn generated_default_system_config() -> SystemConfig {
    let mut config = SystemConfig::default();
    config.domain = detect_tailscale_self_dns_name();
    config
}

pub fn load_or_default(path: impl AsRef<Path>) -> Result<SystemConfig, ConfigError> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(SystemConfig::default());
    }
    let yaml = fs::read_to_string(path)?;
    Ok(serde_yaml::from_str(&yaml)?)
}

pub fn load_or_generated_default(path: impl AsRef<Path>) -> Result<SystemConfig, ConfigError> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(generated_default_system_config());
    }
    load_or_default(path)
}

pub fn save(path: impl AsRef<Path>, config: &SystemConfig) -> Result<(), ConfigError> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let yaml = serialize_system_config(config)?;
    fs::write(path, yaml)?;
    Ok(())
}

fn serialize_system_config(config: &SystemConfig) -> Result<String, ConfigError> {
    let yaml = serde_yaml::to_string(config)?;
    if config.domain.is_some() {
        return Ok(yaml);
    }
    Ok(insert_domain_example_comment(yaml))
}

fn insert_domain_example_comment(yaml: String) -> String {
    let mut output = String::with_capacity(yaml.len() + DOMAIN_EXAMPLE_COMMENT.len() + 1);
    let mut inserted = false;

    for line in yaml.lines() {
        output.push_str(line);
        output.push('\n');
        if !inserted && line.starts_with("listenAddr:") {
            output.push_str(DOMAIN_EXAMPLE_COMMENT);
            output.push('\n');
            inserted = true;
        }
    }

    if !inserted {
        output.push_str(DOMAIN_EXAMPLE_COMMENT);
        output.push('\n');
    }

    output
}

pub fn pairing_state_path(config_path: impl AsRef<Path>) -> PathBuf {
    config_path.as_ref().with_file_name("pairing.yaml")
}

pub fn daemon_status_path(config_path: impl AsRef<Path>) -> PathBuf {
    config_path.as_ref().with_file_name("daemon.status")
}

pub fn load_pairing_state_or_default(path: impl AsRef<Path>) -> Result<PairingState, ConfigError> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(PairingState::default());
    }
    let yaml = fs::read_to_string(path)?;
    Ok(serde_yaml::from_str(&yaml)?)
}

pub fn save_pairing_state(
    path: impl AsRef<Path>,
    pairing_state: &PairingState,
) -> Result<(), ConfigError> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let yaml = serde_yaml::to_string(pairing_state)?;
    fs::write(path, yaml)?;
    Ok(())
}

pub fn windows_program_data_config_path() -> PathBuf {
    let root = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    root.join("wgo").join("wgo.yaml")
}

pub fn macos_system_config_path() -> PathBuf {
    PathBuf::from("/Library")
        .join("Application Support")
        .join("wgo")
        .join("wgo.yaml")
}

pub fn macos_user_config_path() -> PathBuf {
    let root = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    root.join("Library")
        .join("Application Support")
        .join("wgo")
        .join("wgo-user.yaml")
}

#[cfg(not(test))]
fn detect_tailscale_self_dns_name() -> Option<String> {
    for exe in tailscale_exe_candidates() {
        let Ok(output) = Command::new(exe).args(["status", "--json"]).output() else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let Ok(json) = std::str::from_utf8(&output.stdout) else {
            continue;
        };
        if let Some(dns_name) = tailscale_self_dns_name_from_status_json(json) {
            return Some(dns_name);
        }
    }
    None
}

#[cfg(test)]
fn detect_tailscale_self_dns_name() -> Option<String> {
    None
}

fn tailscale_self_dns_name_from_status_json(json: &str) -> Option<String> {
    let status: TailscaleStatus = serde_json::from_str(json).ok()?;
    let dns_name = status.self_node?.dns_name?;
    let dns_name = dns_name.trim().trim_end_matches('.').to_ascii_lowercase();
    if dns_name.ends_with(".ts.net") {
        Some(dns_name)
    } else {
        None
    }
}

#[cfg(not(test))]
fn tailscale_exe_candidates() -> Vec<PathBuf> {
    let mut candidates = vec![PathBuf::from("tailscale")];
    if cfg!(windows) {
        if let Some(program_files) = std::env::var_os("ProgramFiles") {
            candidates.push(
                PathBuf::from(program_files)
                    .join("Tailscale")
                    .join("tailscale.exe"),
            );
        }
        if let Some(program_files_x86) = std::env::var_os("ProgramFiles(x86)") {
            candidates.push(
                PathBuf::from(program_files_x86)
                    .join("Tailscale")
                    .join("tailscale.exe"),
            );
        }
    }
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_roundtrip_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wgo.yaml");
        let config = SystemConfig {
            listen_addr: "0.0.0.0:9012".to_string(),
            domain: Some("pc.example.com".to_string()),
            tls: Some(TlsConfig {
                cert_file: r"C:\wgo\cert.pem".to_string(),
                key_file: r"C:\wgo\key.pem".to_string(),
            }),
        };
        save(&path, &config).unwrap();
        assert_eq!(load_or_default(&path).unwrap(), config);
    }

    #[test]
    fn omits_absent_optional_system_config_fields() {
        let config = SystemConfig::default();
        let yaml = serde_yaml::to_string(&config).unwrap();

        assert!(yaml.contains("listenAddr:"));
        assert!(!yaml.contains("domain:"));
        assert!(!yaml.contains("tls:"));
        assert_eq!(serde_yaml::from_str::<SystemConfig>(&yaml).unwrap(), config);
    }

    #[test]
    fn save_writes_domain_example_comment_when_domain_is_absent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wgo.yaml");

        save(&path, &SystemConfig::default()).unwrap();

        let yaml = fs::read_to_string(&path).unwrap();
        assert!(yaml.contains("listenAddr:"));
        assert!(yaml.contains(DOMAIN_EXAMPLE_COMMENT));
        assert!(!yaml.lines().any(|line| line.starts_with("domain:")));
        assert_eq!(
            load_or_default(&path).unwrap(),
            SystemConfig {
                listen_addr: default_listen_addr(),
                domain: None,
                tls: None,
            }
        );
    }

    #[test]
    fn pairing_state_roundtrip_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("wgo.yaml");
        let path = pairing_state_path(&config_path);
        let state = PairingState {
            clients: vec![ClientCredentialRecord {
                client_id: "client".to_string(),
                label: "browser".to_string(),
                secret_sha256_base64url: "hash".to_string(),
                created_at_unix: 100,
            }],
            pairing: Some(PairingRecord {
                code_sha256_base64url: "code".to_string(),
                expires_at_unix: 400,
            }),
        };

        save_pairing_state(&path, &state).unwrap();

        assert_eq!(load_pairing_state_or_default(&path).unwrap(), state);
    }

    #[test]
    fn parses_tailscale_self_dns_name() {
        let json = r#"{
          "Self": {
            "DNSName": "Example.tail123456.ts.net."
          }
        }"#;

        assert_eq!(
            tailscale_self_dns_name_from_status_json(json).as_deref(),
            Some("example.tail123456.ts.net")
        );
    }

    #[test]
    fn ignores_non_tailscale_self_dns_name() {
        let json = r#"{
          "Self": {
            "DNSName": "example.com."
          }
        }"#;

        assert_eq!(tailscale_self_dns_name_from_status_json(json), None);
    }
}
