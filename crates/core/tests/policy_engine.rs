//! Integration tests for the Policy Engine: rule evaluation
//! (app / protocol / src / dst / port -> action labels + order),
//! first-match-wins ordering, and the implicit match-all default.

use sdwanlite_core::{Policy, PolicyRule, RouteAction, RuleMatch, SelectionOrder};

fn action(labels: &[&str], order: SelectionOrder) -> RouteAction {
    RouteAction {
        labels: labels.iter().map(|s| s.to_string()).collect(),
        order,
    }
}

fn policy() -> Policy {
    Policy {
        name: "test-policy".into(),
        description: String::new(),
        rules: vec![
            PolicyRule {
                r#match: RuleMatch {
                    app: Some("voip".into()),
                    ..Default::default()
                },
                action: action(&["MPLS"], SelectionOrder::PriorityFailover),
            },
            PolicyRule {
                r#match: RuleMatch {
                    src_prefix: Some("10.0.0.0/8".into()),
                    dst_prefix: Some("192.168.1.0/24".into()),
                    ..Default::default()
                },
                action: action(&["LTE"], SelectionOrder::PriorityFailover),
            },
            PolicyRule {
                r#match: RuleMatch {
                    protocol: Some("udp".into()),
                    dst_port: Some(53),
                    ..Default::default()
                },
                action: action(&["ISP1"], SelectionOrder::LoadBalance),
            },
        ],
        default_action: action(&["ISP1", "LTE"], SelectionOrder::LoadBalance),
        installed: false,
    }
}

#[test]
fn match_by_app_selects_action_and_order() {
    let p = policy();
    let hit = p.evaluate(Some("voip"), None, None, None, None);
    assert_eq!(hit.labels, vec!["MPLS".to_string()]);
    assert_eq!(hit.order, SelectionOrder::PriorityFailover);
}

#[test]
fn match_by_cidr_prefixes() {
    let p = policy();
    let hit = p.evaluate(None, None, Some("10.1.2.3"), Some("192.168.1.50"), None);
    assert_eq!(hit.labels, vec!["LTE".to_string()]);
    assert_eq!(hit.order, SelectionOrder::PriorityFailover);
    // IP outside the dst prefix falls through to the default
    let miss = p.evaluate(None, None, Some("10.1.2.3"), Some("8.8.8.8"), None);
    assert_eq!(miss, &p.default_action);
}

#[test]
fn match_by_protocol_and_port() {
    let p = policy();
    let hit = p.evaluate(None, Some("udp"), None, None, Some(53));
    assert_eq!(hit.labels, vec!["ISP1".to_string()]);
    assert_eq!(hit.order, SelectionOrder::LoadBalance);
    // same protocol, wrong port -> default
    let miss = p.evaluate(None, Some("udp"), None, None, Some(123));
    assert_eq!(miss, &p.default_action);
}

#[test]
fn first_match_wins_in_rule_order() {
    let mut p = policy();
    p.rules.insert(
        0,
        PolicyRule {
            r#match: RuleMatch {
                app: Some("voip".into()),
                protocol: Some("udp".into()),
                ..Default::default()
            },
            action: action(&["LTE"], SelectionOrder::LoadBalance),
        },
    );
    let hit = p.evaluate(Some("voip"), Some("udp"), None, None, None);
    assert_eq!(hit.labels, vec!["LTE".to_string()]);
}

#[test]
fn implicit_default_is_match_all() {
    let p = policy();
    for traffic in [
        (None, None, None, None, None),
        (Some("unknown"), None, None, None, None),
        (None, Some("tcp"), None, None, Some(443)),
    ] {
        let hit = p.evaluate(traffic.0, traffic.1, traffic.2, traffic.3, traffic.4);
        assert_eq!(hit, &p.default_action, "traffic {traffic:?}");
    }
}

#[test]
fn validate_rejects_explicit_match_all_and_empty_actions() {
    let mut p = policy();
    assert!(p.validate().is_ok());

    let mut bad = policy();
    bad.rules.push(PolicyRule {
        r#match: RuleMatch::default(),
        action: action(&["X"], SelectionOrder::PriorityFailover),
    });
    assert!(bad.validate().is_err());

    let mut no_labels = policy();
    no_labels.rules[0].action.labels.clear();
    assert!(no_labels.validate().is_err());

    let mut no_default = policy();
    no_default.default_action.labels.clear();
    assert!(no_default.validate().is_err());
}

#[test]
fn cidr_contains_handles_families_and_boundaries() {
    let c = Policy::cidr_contains;
    assert!(c("10.0.0.0/8", "10.255.255.255"));
    assert!(!c("10.0.0.0/8", "11.0.0.0"));
    assert!(c("192.168.1.0/24", "192.168.1.0"));
    assert!(!c("192.168.1.0/24", "192.168.2.1"));
    assert!(c("0.0.0.0/0", "203.0.113.7"));
    assert!(c("::/0", "2001:db8::1"));
    assert!(!c("2001:db8::/32", "2001:db9::1"));
    // family mismatch and junk input
    assert!(!c("10.0.0.0/8", "2001:db8::1"));
    assert!(!c("not-a-cidr", "10.0.0.1"));
    assert!(!c("10.0.0.0/99", "10.0.0.1"));
}
