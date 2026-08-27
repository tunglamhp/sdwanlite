//! Path Labels CRUD roundtrip: the store persists as a JSON document
//! (atomic file write in the daemon); these tests cover the serialization
//! contract and a file roundtrip with an on-disk temp file.

use std::io::Write;

use sdwanlite_core::{
    PathLabel, PathPolicyStore, Policy, PolicyRule, RouteAction, RuleMatch, SelectionOrder,
};

fn label(name: &str, ifaces: &[&str], tunnels: &[&str], desc: &str) -> PathLabel {
    PathLabel {
        name: name.into(),
        interfaces: ifaces.iter().map(|s| s.to_string()).collect(),
        tunnels: tunnels.iter().map(|s| s.to_string()).collect(),
        description: desc.into(),
    }
}

fn policy(name: &str, app: &str, labels: &[&str]) -> Policy {
    Policy {
        name: name.into(),
        description: String::new(),
        rules: vec![PolicyRule {
            r#match: RuleMatch {
                app: Some(app.into()),
                ..Default::default()
            },
            action: RouteAction {
                labels: labels.iter().map(|s| s.to_string()).collect(),
                order: SelectionOrder::PriorityFailover,
            },
        }],
        default_action: RouteAction {
            labels: vec!["ISP1".into()],
            order: SelectionOrder::LoadBalance,
        },
        installed: false,
    }
}

fn temp_store_path(tag: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("sdwanlite-test-{tag}-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

#[test]
fn labels_serialize_roundtrip_through_json() {
    let mut st = PathPolicyStore::default();
    st.labels
        .push(label("ISP1", &["wan0"], &[], "primary fiber"));
    st.labels
        .push(label("LTE", &["wan1"], &["tun-lte"], "backup cellular"));

    let json = serde_json::to_string(&st).unwrap();
    let back: PathPolicyStore = serde_json::from_str(&json).unwrap();

    assert_eq!(back, st);
    assert_eq!(back.labels[0].name, "ISP1");
    assert_eq!(back.labels[1].tunnels, vec!["tun-lte".to_string()]);
}

#[test]
fn label_crud_roundtrip_on_disk() {
    let path = temp_store_path("labels");
    let mut st = PathPolicyStore::default();
    st.labels
        .push(label("ISP1", &["wan0"], &[], "primary fiber"));

    // PUT-equivalent: overwrite the whole document
    let json = serde_json::to_string(&st).unwrap();
    {
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(json.as_bytes()).unwrap();
        f.sync_all().unwrap();
    }
    // GET-equivalent: read it back
    let loaded: PathPolicyStore =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(loaded.labels, st.labels);

    // UPDATE: modify a field and rewrite
    st.labels[0].description = "primary fiber (updated)".into();
    let json = serde_json::to_string(&st).unwrap();
    std::fs::write(&path, json).unwrap();
    let reloaded: PathPolicyStore =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(reloaded.labels[0].description, "primary fiber (updated)");

    // DELETE: drop the label and rewrite
    st.labels.clear();
    let json = serde_json::to_string(&st).unwrap();
    std::fs::write(&path, json).unwrap();
    let emptied: PathPolicyStore =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(emptied.labels.is_empty());

    let _ = std::fs::remove_file(&path);
}

#[test]
fn labels_and_policies_roundtrip_together() {
    let path = temp_store_path("both");
    let mut st = PathPolicyStore::default();
    st.labels
        .push(label("MPLS", &["wan2"], &["tun-mpls"], "mpls path"));
    st.labels
        .push(label("ISP1", &["wan0"], &[], "primary fiber"));
    st.policies.push(policy("voip-priority", "voip", &["MPLS"]));

    let json = serde_json::to_string(&st).unwrap();
    std::fs::write(&path, json).unwrap();

    let back: PathPolicyStore =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(back.labels.len(), 2);
    assert_eq!(back.policies.len(), 1);
    assert_eq!(back.policies[0].name, "voip-priority");
    assert_eq!(
        back.policies[0].rules[0].action.labels,
        vec!["MPLS".to_string()]
    );
    // label referenced by the policy still resolves by name
    let names: Vec<&str> = back.labels.iter().map(|l| l.name.as_str()).collect();
    assert!(names.contains(&"MPLS"));
    assert!(names.contains(&"ISP1"));

    let _ = std::fs::remove_file(&path);
}
