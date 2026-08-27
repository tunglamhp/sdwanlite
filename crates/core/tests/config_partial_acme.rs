//! A partial [acme] section (only some fields set) must not break config
//! loading — every Acme field defaults.

#[test]
fn partial_acme_section_loads() {
    let toml = r#"
[general]
name = "t"
api_addr = "127.0.0.1"

[acme]
enabled = true
email = "admin@example.com"

[lb]
"#;
    let cfg: sdwanlite_core::Config = toml::from_str(toml).unwrap();
    assert!(cfg.acme.enabled);
    assert_eq!(cfg.acme.email, "admin@example.com");
    assert!(cfg.acme.domains.is_empty());
    assert_eq!(cfg.acme.renew_days, 30); // default
    assert_eq!(cfg.acme.http01_port, 80); // default
}

#[test]
fn empty_acme_section_loads() {
    let toml = r#"
[general]
name = "t"

[acme]
"#;
    let cfg: sdwanlite_core::Config = toml::from_str(toml).unwrap();
    assert!(!cfg.acme.enabled);
    assert!(cfg.acme.domains.is_empty());
    assert!(cfg.acme.cert_file.is_empty());
}
