use crate::persist;
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;

/// The games' frontend hardcodes `https://` (rgsFetcher.ts) → the local
/// RGS MUST be served over TLS, signed by a trusted local CA.
///
/// "SlotLab Local CA" (10 years) + `localhost`/127.0.0.1/::1 leaf
/// (397 days, silently regenerated as expiration approaches —
/// macOS caps leaf trust at 825 days).
#[allow(dead_code)] // ca_pem kept for export/diagnostics
pub struct TlsMaterial {
    pub ca_pem: String,
    pub leaf_cert_pem: String,
    pub leaf_key_pem: String,
}

fn certs_dir() -> PathBuf {
    persist::app_dir().join("certs")
}

pub fn ca_pem_path() -> PathBuf {
    certs_dir().join("slotlab-ca.pem")
}

/// Loads (or generates) CA + leaf.
pub fn ensure_material() -> Result<TlsMaterial> {
    let dir = certs_dir();
    std::fs::create_dir_all(&dir)?;
    let ca_cert_path = ca_pem_path();
    let ca_key_path = dir.join("slotlab-ca.key.pem");
    let leaf_cert_path = dir.join("localhost.pem");
    let leaf_key_path = dir.join("localhost.key.pem");

    let (ca_pem, ca_key_pem) = if ca_cert_path.exists() && ca_key_path.exists() {
        (
            std::fs::read_to_string(&ca_cert_path)?,
            std::fs::read_to_string(&ca_key_path)?,
        )
    } else {
        let (cert, key) = generate_ca()?;
        std::fs::write(&ca_cert_path, &cert)?;
        std::fs::write(&ca_key_path, &key)?;
        tracing::info!("\"SlotLab Local CA\" generated: {}", ca_cert_path.display());
        (cert, key)
    };

    let leaf_ok =
        leaf_cert_path.exists() && leaf_key_path.exists() && leaf_not_expiring(&leaf_cert_path);
    let (leaf_cert_pem, leaf_key_pem) = if leaf_ok {
        (
            std::fs::read_to_string(&leaf_cert_path)?,
            std::fs::read_to_string(&leaf_key_path)?,
        )
    } else {
        let (cert, key) = generate_leaf(&ca_pem, &ca_key_pem)?;
        std::fs::write(&leaf_cert_path, &cert)?;
        std::fs::write(&leaf_key_path, &key)?;
        tracing::info!("localhost cert (re)generated");
        (cert, key)
    };

    Ok(TlsMaterial {
        ca_pem,
        leaf_cert_pem,
        leaf_key_pem,
    })
}

fn generate_ca() -> Result<(String, String)> {
    use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, KeyPair, KeyUsagePurpose};
    let mut params = CertificateParams::default();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params
        .distinguished_name
        .push(DnType::CommonName, "SlotLab Local CA");
    params
        .distinguished_name
        .push(DnType::OrganizationName, "SlotLab");
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let now = time::OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::days(1);
    params.not_after = now + time::Duration::days(365 * 10);
    let key = KeyPair::generate()?;
    let cert = params.self_signed(&key)?;
    Ok((cert.pem(), key.serialize_pem()))
}

fn generate_leaf(ca_pem: &str, ca_key_pem: &str) -> Result<(String, String)> {
    use rcgen::{CertificateParams, DnType, ExtendedKeyUsagePurpose, KeyPair, SanType};
    let ca_key = KeyPair::from_pem(ca_key_pem).context("CA key")?;
    let ca_params = CertificateParams::from_ca_cert_pem(ca_pem).context("CA params from PEM")?;
    let ca = ca_params.self_signed(&ca_key).context("reload CA")?;

    let mut params = CertificateParams::default();
    params
        .distinguished_name
        .push(DnType::CommonName, "localhost");
    params.subject_alt_names = vec![
        SanType::DnsName("localhost".try_into()?),
        SanType::IpAddress(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)),
        SanType::IpAddress(std::net::IpAddr::V6(std::net::Ipv6Addr::LOCALHOST)),
    ];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let now = time::OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::days(1);
    // macOS only trusts leafs ≤ 825 days; 397 = safe margin.
    params.not_after = now + time::Duration::days(397);
    let key = KeyPair::generate()?;
    let cert = params.signed_by(&key, &ca, &ca_key)?;
    Ok((cert.pem(), key.serialize_pem()))
}

/// true if the leaf expires in more than 30 days (otherwise regenerate).
fn leaf_not_expiring(path: &std::path::Path) -> bool {
    // Simple heuristic: file age < 300 days (the leaf lives 397 days).
    match std::fs::metadata(path).and_then(|m| m.modified()) {
        Ok(modified) => match modified.elapsed() {
            Ok(age) => age.as_secs() < 300 * 24 * 3600,
            Err(_) => true,
        },
        Err(_) => false,
    }
}

/// Is the CA in the trust store? (`security verify-cert`)
///
/// Only macOS can answer: elsewhere (headless server reached through an SSH
/// tunnel) the browser runs on another machine, whose trust store the server
/// cannot see, so the banner would always be a false alarm.
pub fn is_trusted() -> bool {
    if !cfg!(target_os = "macos") {
        return certs_dir().join("localhost.pem").exists();
    }
    let leaf = certs_dir().join("localhost.pem");
    if !leaf.exists() {
        return false;
    }
    Command::new("/usr/bin/security")
        .args(["verify-cert", "-p", "ssl", "-c"])
        .arg(&leaf)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Installs the CA into the LOGIN keychain (prompts for the user's
/// password, no sudo).
pub fn install_ca() -> Result<()> {
    let home = std::env::var("HOME").context("HOME")?;
    let keychain = format!("{home}/Library/Keychains/login.keychain-db");
    let output = Command::new("/usr/bin/security")
        .args(["add-trusted-cert", "-r", "trustRoot", "-k", &keychain])
        .arg(ca_pem_path())
        .output()
        .context("running `security add-trusted-cert`")?;
    anyhow::ensure!(
        output.status.success(),
        "add-trusted-cert failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
