use std::fs::{self, File};
use std::io::{self, BufReader};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::sign::CertifiedKey;
use tracing::info;
use wgo_daemon_core::config::{SystemConfig, TlsConfig};

const TAILSCALE_CERT_MIN_VALIDITY: &str = "168h";

pub struct PreparedCertificate {
    pub certified_key: Arc<CertifiedKey>,
}

pub fn prepare_server_certificate(
    config: &mut SystemConfig,
    _listen_addr: SocketAddr,
    config_path: &Path,
    provider: &CryptoProvider,
) -> Result<PreparedCertificate> {
    if let Some(tls) = &config.tls {
        let (chain, key) = load_configured_certificate(config_path, tls)?;
        return Ok(PreparedCertificate {
            certified_key: build_certified_key(chain, key, provider)?,
        });
    }

    if let Some(domain) = normalized_domain(config.domain.as_deref())? {
        if !is_tailscale_domain(&domain) {
            bail!(
                "domain {domain} requires tls.certFile and tls.keyFile; only .ts.net domains can auto-load Tailscale certificates"
            );
        }

        let (cert_file, key_file) = tailscale_cert_paths(config_path, &domain);
        ensure_tailscale_certificate(&domain, &cert_file, &key_file)?;
        let (chain, key) = load_pem_certificate(&cert_file, &key_file)?;
        return Ok(PreparedCertificate {
            certified_key: build_certified_key(chain, key, provider)?,
        });
    }

    bail!("configure tls.certFile/keyFile or set domain to a .ts.net hostname for Tailscale certificates")
}

pub fn configured_certificate_paths(
    config: &SystemConfig,
    config_path: &Path,
) -> Result<Vec<PathBuf>> {
    if let Some(tls) = &config.tls {
        return Ok(vec![
            resolve_config_relative(config_path, &tls.cert_file),
            resolve_config_relative(config_path, &tls.key_file),
        ]);
    }

    let Some(domain) = normalized_domain(config.domain.as_deref())? else {
        return Ok(Vec::new());
    };
    if !is_tailscale_domain(&domain) {
        return Ok(Vec::new());
    }

    let (cert_file, key_file) = tailscale_cert_paths(config_path, &domain);
    Ok(vec![cert_file, key_file])
}

pub fn uses_scheduled_certificate_refresh(config: &SystemConfig) -> bool {
    config.tls.is_none()
        && normalized_domain(config.domain.as_deref())
            .ok()
            .flatten()
            .is_some_and(|domain| is_tailscale_domain(&domain))
}

fn load_configured_certificate(
    config_path: &Path,
    tls: &TlsConfig,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    let cert_file = resolve_config_relative(config_path, &tls.cert_file);
    let key_file = resolve_config_relative(config_path, &tls.key_file);
    load_pem_certificate(&cert_file, &key_file)
}

fn load_pem_certificate(
    cert_file: &Path,
    key_file: &Path,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    let mut cert_reader = BufReader::new(
        File::open(cert_file)
            .with_context(|| format!("failed to open certificate file {}", cert_file.display()))?,
    );
    let chain = rustls_pemfile::certs(&mut cert_reader)
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format!("failed to parse certificate file {}", cert_file.display()))?;
    if chain.is_empty() {
        bail!(
            "certificate file {} contains no certificates",
            cert_file.display()
        );
    }

    let mut key_reader = BufReader::new(
        File::open(key_file)
            .with_context(|| format!("failed to open private key file {}", key_file.display()))?,
    );
    let key = rustls_pemfile::private_key(&mut key_reader)
        .with_context(|| format!("failed to parse private key file {}", key_file.display()))?
        .with_context(|| format!("private key file {} contains no key", key_file.display()))?;

    Ok((chain, key))
}

fn build_certified_key(
    chain: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
    provider: &CryptoProvider,
) -> Result<Arc<CertifiedKey>> {
    Ok(Arc::new(CertifiedKey::from_der(chain, key, provider)?))
}

fn resolve_config_relative(config_path: &Path, raw: &str) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        return path;
    }
    config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(path)
}

fn normalized_domain(domain: Option<&str>) -> Result<Option<String>> {
    let Some(domain) = domain else {
        return Ok(None);
    };
    let domain = domain.trim().trim_end_matches('.').to_ascii_lowercase();
    if domain.is_empty() {
        return Ok(None);
    }
    if domain.contains("://") || domain.contains('/') || domain.contains('\\') {
        bail!("domain must be a hostname, not a URL or path");
    }
    Ok(Some(domain))
}

fn is_tailscale_domain(domain: &str) -> bool {
    domain.ends_with(".ts.net")
}

fn tailscale_cert_paths(config_path: &Path, domain: &str) -> (PathBuf, PathBuf) {
    let base = config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("certs")
        .join("tailscale");
    let stem = sanitize_filename(domain);
    (
        base.join(format!("{stem}.crt.pem")),
        base.join(format!("{stem}.key.pem")),
    )
}

fn sanitize_filename(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn ensure_tailscale_certificate(domain: &str, cert_file: &Path, key_file: &Path) -> Result<()> {
    if let Some(parent) = cert_file.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(parent) = key_file.parent() {
        fs::create_dir_all(parent)?;
    }

    let args = [
        "cert".to_string(),
        format!("--cert-file={}", cert_file.display()),
        format!("--key-file={}", key_file.display()),
        format!("--min-validity={TAILSCALE_CERT_MIN_VALIDITY}"),
        domain.to_string(),
    ];

    for exe in tailscale_exe_candidates() {
        match Command::new(&exe).args(&args).output() {
            Ok(output) if output.status.success() => {
                info!(
                    domain,
                    cert_file = %cert_file.display(),
                    key_file = %key_file.display(),
                    "loaded Tailscale certificate"
                );
                return Ok(());
            }
            Ok(output) => {
                bail!(
                    "tailscale cert failed for {domain}: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(err).with_context(|| format!("failed to run {}", exe.display()));
            }
        }
    }

    bail!("tailscale executable was not found; install Tailscale or configure tls.certFile/keyFile")
}

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
    } else if cfg!(target_os = "macos") {
        candidates.push(PathBuf::from("/usr/local/bin/tailscale"));
        candidates.push(PathBuf::from("/opt/homebrew/bin/tailscale"));
        candidates.push(PathBuf::from(
            "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
        ));
    }
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcgen::{CertificateParams, KeyPair};
    use wgo_daemon_core::config::{SystemConfig, TlsConfig};

    #[test]
    fn loads_configured_pem_certificate() {
        let dir = tempfile::tempdir().unwrap();
        let cert_path = dir.path().join("cert.pem");
        let key_path = dir.path().join("key.pem");
        let config_path = dir.path().join("wgo.yaml");

        let key = KeyPair::generate().unwrap();
        let cert = CertificateParams::new(vec!["pc.example.com".to_string()])
            .unwrap()
            .self_signed(&key)
            .unwrap();
        fs::write(&cert_path, cert.pem()).unwrap();
        fs::write(&key_path, key.serialize_pem()).unwrap();

        let mut config = SystemConfig {
            domain: Some("pc.example.com".to_string()),
            tls: Some(TlsConfig {
                cert_file: "cert.pem".to_string(),
                key_file: "key.pem".to_string(),
            }),
            ..SystemConfig::default()
        };
        let provider = web_transport_quinn::crypto::default_provider();

        let prepared = prepare_server_certificate(
            &mut config,
            "127.0.0.1:9012".parse().unwrap(),
            &config_path,
            &provider,
        )
        .unwrap();
        assert_eq!(prepared.certified_key.cert.len(), 1);
    }

    #[test]
    fn rejects_non_tailscale_domain_without_tls_config() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = SystemConfig {
            domain: Some("pc.example.com".to_string()),
            ..SystemConfig::default()
        };
        let provider = web_transport_quinn::crypto::default_provider();

        let err = match prepare_server_certificate(
            &mut config,
            "127.0.0.1:9012".parse().unwrap(),
            dir.path(),
            &provider,
        ) {
            Ok(_) => panic!("non-Tailscale domain without TLS config should fail"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("requires tls.certFile"));
    }

    #[test]
    fn rejects_missing_certificate_source() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = SystemConfig::default();
        let provider = web_transport_quinn::crypto::default_provider();

        let err = match prepare_server_certificate(
            &mut config,
            "127.0.0.1:9012".parse().unwrap(),
            dir.path(),
            &provider,
        ) {
            Ok(_) => panic!("missing TLS source should fail"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("configure tls.certFile/keyFile"));
    }

    #[test]
    fn recognizes_tailscale_domains_case_insensitively() {
        assert_eq!(
            normalized_domain(Some("MiniPC.Tail1234.ts.net.")).unwrap(),
            Some("minipc.tail1234.ts.net".to_string())
        );
        assert!(is_tailscale_domain("minipc.tail1234.ts.net"));
    }

    #[test]
    fn schedules_refresh_only_for_managed_tailscale_config() {
        let tailscale = SystemConfig {
            domain: Some("minipc.tail1234.ts.net".to_string()),
            ..SystemConfig::default()
        };
        let explicit_tls = SystemConfig {
            tls: Some(TlsConfig {
                cert_file: "cert.pem".to_string(),
                key_file: "key.pem".to_string(),
            }),
            ..SystemConfig::default()
        };

        assert!(uses_scheduled_certificate_refresh(&tailscale));
        assert!(!uses_scheduled_certificate_refresh(&explicit_tls));
        assert!(!uses_scheduled_certificate_refresh(&SystemConfig::default()));
    }
}
