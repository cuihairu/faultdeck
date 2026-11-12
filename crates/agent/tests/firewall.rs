use std::sync::{Arc, Mutex};

use faultdeck_agent::firewall::{FirewallError, Iptables};

/// Build an Iptables whose runner records every invocation as a string.
fn recorder() -> (Iptables, Arc<Mutex<Vec<String>>>) {
    let calls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let calls2 = calls.clone();
    let runner = Arc::new(move |args: &[&str]| -> Result<(), FirewallError> {
        let joined = args.join(" ");
        calls2.lock().unwrap().push(joined.clone());
        // -C is a check: on a fresh host no jump exists yet, so it reports
        // "Bad rule" and ensure_chain must follow with -I to install it.
        if args.first() == Some(&"-C") {
            Err(FirewallError::Command(
                "Bad rule (does a matching rule exist in that chain?)".into(),
            ))
        } else {
            Ok(())
        }
    });
    let fw = Iptables {
        dry_run: false,
        path: None,
        runner: Some(runner),
    };
    (fw, calls)
}

/// Build an Iptables whose runner fails for a specific arg prefix.
fn failing(prefix: &str) -> Iptables {
    let prefix = prefix.to_string();
    let runner = Arc::new(move |args: &[&str]| -> Result<(), FirewallError> {
        if args.first().copied() == Some(prefix.as_str()) {
            Err(FirewallError::Command("simulated failure".into()))
        } else {
            Ok(())
        }
    });
    Iptables {
        dry_run: false,
        path: None,
        runner: Some(runner),
    }
}

#[test]
fn ensure_chain_installs_chain_and_jump() {
    // Regression (audit 2026-11-11 P2): a disconnect used to be hooked from
    // INPUT only, so outbound traffic to the target was never dropped. The
    // chain must be jumped from every direction.
    let (fw, calls) = recorder();
    fw.ensure_chain().unwrap();
    let calls = calls.lock().unwrap().clone();
    assert!(calls.iter().any(|c| c.starts_with("-N FAULTDECK")));
    for chain in ["INPUT", "OUTPUT", "FORWARD"] {
        assert!(
            calls
                .iter()
                .any(|c| c.starts_with(&format!("-I {chain} -j FAULTDECK"))),
            "missing jump for {chain}: {calls:?}"
        );
    }
}

#[test]
fn ensure_chain_dry_run_emits_jump_install() {
    // Dry-run cannot execute -C, so it must treat the jumps as absent and
    // still emit the same -I command as a real first deployment.
    let (mut fw, calls) = recorder();
    fw.dry_run = true;
    fw.ensure_chain().unwrap();
    let calls = calls.lock().unwrap().clone();
    assert!(calls.iter().any(|c| c.starts_with("-N FAULTDECK")));
    for chain in ["INPUT", "OUTPUT", "FORWARD"] {
        assert!(
            calls
                .iter()
                .any(|c| c.starts_with(&format!("-I {chain} -j FAULTDECK"))),
            "missing jump for {chain}: {calls:?}"
        );
    }
}

#[test]
fn ensure_chain_pure_dry_run_succeeds() {
    // No runner and no binary path: dry-run must short-circuit every command
    // (including the -C probe that decides whether to emit -I) and succeed.
    let fw = Iptables {
        dry_run: true,
        path: None,
        runner: None,
    };
    fw.ensure_chain().unwrap();
}

#[test]
fn ensure_chain_tolerates_chain_exists() {
    // First -N fails with "Chain already exists"; ensure_chain must tolerate it.
    let runner = Arc::new(|args: &[&str]| -> Result<(), FirewallError> {
        if args.first() == Some(&"-N") {
            Err(FirewallError::Command("Chain already exists".into()))
        } else {
            Ok(())
        }
    });
    let fw = Iptables {
        dry_run: false,
        path: None,
        runner: Some(runner),
    };
    fw.ensure_chain().unwrap();
}

#[test]
fn ensure_chain_is_idempotent_over_jump_lifecycle() {
    // First call: chain absent -> -C fails with "Bad rule" -> -I installs.
    // Second call: -C succeeds -> no second -I, so the jump is not duplicated.
    use std::sync::atomic::{AtomicBool, Ordering};
    let installed = Arc::new(AtomicBool::new(false));
    let installed2 = installed.clone();
    let runner = Arc::new(move |args: &[&str]| -> Result<(), FirewallError> {
        match args.first() {
            Some(&"-N") => {
                if installed2.load(Ordering::SeqCst) {
                    Err(FirewallError::Command("Chain already exists".into()))
                } else {
                    Ok(())
                }
            }
            Some(&"-C") => {
                if installed2.load(Ordering::SeqCst) {
                    Ok(())
                } else {
                    Err(FirewallError::Command(
                        "Bad rule (does a matching rule exist in that chain?)".into(),
                    ))
                }
            }
            Some(&"-I") => {
                installed2.store(true, Ordering::SeqCst);
                Ok(())
            }
            _ => Ok(()),
        }
    });
    let fw = Iptables {
        dry_run: false,
        path: None,
        runner: Some(runner),
    };
    fw.ensure_chain().unwrap();
    fw.ensure_chain().unwrap();
}

#[test]
fn rollback_is_idempotent_when_chain_absent() {
    let runner = Arc::new(|args: &[&str]| -> Result<(), FirewallError> {
        match args.first() {
            Some(&"-D") => Err(FirewallError::Command(
                "Bad rule (does a matching rule exist in that chain?)".into(),
            )),
            Some(&"-F") => Err(FirewallError::Command(
                "No chain/target/match by that name".into(),
            )),
            Some(&"-X") => Err(FirewallError::Command("Too many links".into())),
            _ => Ok(()),
        }
    });
    let fw = Iptables {
        dry_run: false,
        path: None,
        runner: Some(runner),
    };
    fw.rollback().unwrap();
}

#[test]
fn apply_uses_source_destination_and_port_matches() {
    use faultdeck_core::{DisconnectSpec, FaultSpec, Injection, Target};
    use ipnet::IpNet;
    use std::str::FromStr;

    let (fw, calls) = recorder();
    let injection = Injection {
        fault: FaultSpec::Disconnect(DisconnectSpec { all_traffic: false }),
        target: Target {
            source: Some(IpNet::from_str("10.0.0.0/8").unwrap()),
            destination: Some(IpNet::from_str("192.0.2.0/24").unwrap()),
            destination_port: Some(443),
        },
    };
    fw.apply(&injection).unwrap();

    let calls = calls.lock().unwrap().clone();
    assert!(calls.iter().any(|c| c.contains("-s 10.0.0.0/8")));
    assert!(calls.iter().any(|c| c.contains("-d 192.0.2.0/24")));
    assert!(calls.iter().any(|c| c.contains("-p tcp")));
    assert!(calls.iter().any(|c| c.contains("-p udp")));
    assert!(calls.iter().any(|c| c.contains("--dport 443")));
    assert!(calls
        .iter()
        .any(|c| c.contains("-p tcp") && c.contains("--dport 443")));
    assert!(calls
        .iter()
        .any(|c| c.contains("-p udp") && c.contains("--dport 443")));
    assert!(calls.iter().any(|c| c.contains("-j DROP")));
}

#[test]
fn all_traffic_true_ignores_port() {
    use faultdeck_core::{DisconnectSpec, FaultSpec, Injection, Target};
    use ipnet::IpNet;
    use std::str::FromStr;

    let (fw, calls) = recorder();
    let injection = Injection {
        fault: FaultSpec::Disconnect(DisconnectSpec { all_traffic: true }),
        target: Target::destination_port(IpNet::from_str("192.0.2.0/24").unwrap(), 443),
    };
    fw.apply(&injection).unwrap();
    let calls = calls.lock().unwrap().clone();
    assert!(calls.iter().any(|c| c.contains("-d 192.0.2.0/24")));
    assert!(!calls.iter().any(|c| c.contains("--dport")));
}

#[test]
fn full_apply_rollback_lifecycle_is_recorded() {
    use faultdeck_core::{DisconnectSpec, FaultSpec, Injection, Target};
    use ipnet::IpNet;
    use std::str::FromStr;

    let (fw, calls) = recorder();
    let injection = Injection {
        fault: FaultSpec::Disconnect(DisconnectSpec { all_traffic: true }),
        target: Target::destination(IpNet::from_str("192.0.2.0/24").unwrap()),
    };
    fw.apply(&injection).unwrap();
    fw.rollback().unwrap();

    let calls = calls.lock().unwrap().clone();
    assert!(calls.iter().any(|c| c.starts_with("-N FAULTDECK")));
    assert!(calls.iter().any(|c| c.starts_with("-I INPUT -j FAULTDECK")));
    assert!(calls
        .iter()
        .any(|c| c.starts_with("-I OUTPUT -j FAULTDECK")));
    assert!(calls.iter().any(|c| c.contains("-A FAULTDECK")
        && c.contains("-d 192.0.2.0/24")
        && c.contains("-j DROP")));
    // Every direction's jump must be removed before the chain is flushed.
    for chain in ["INPUT", "OUTPUT", "FORWARD"] {
        assert!(
            calls
                .iter()
                .any(|c| c.starts_with(&format!("-D {chain} -j FAULTDECK"))),
            "missing jump removal for {chain}: {calls:?}"
        );
    }
    assert!(calls.iter().any(|c| c.starts_with("-F FAULTDECK")));
    assert!(calls.iter().any(|c| c.starts_with("-X FAULTDECK")));
}

#[test]
fn apply_propagates_non_idempotent_failure() {
    use faultdeck_core::{DisconnectSpec, FaultSpec, Injection, Target};
    use ipnet::IpNet;
    use std::str::FromStr;

    let fw = failing("-A");
    let injection = Injection {
        fault: FaultSpec::Disconnect(DisconnectSpec { all_traffic: true }),
        target: Target::destination(IpNet::from_str("192.0.2.0/24").unwrap()),
    };
    assert!(fw.apply(&injection).is_err());
}

#[test]
fn apply_rolls_back_partial_install_on_failure() {
    use faultdeck_core::{DisconnectSpec, FaultSpec, Injection, Target};
    use ipnet::IpNet;
    use std::str::FromStr;

    // The udp rule fails on the second -A of the same port after the tcp rule
    // already landed. apply() must tear the chain down so no orphaned DROP
    // rule is left behind for a client that is told the inject failed.
    let calls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let calls2 = calls.clone();
    let runner = Arc::new(move |args: &[&str]| -> Result<(), FirewallError> {
        let joined = args.join(" ");
        calls2.lock().unwrap().push(joined.clone());
        if args.first() == Some(&"-C") {
            return Err(FirewallError::Command(
                "Bad rule (does a matching rule exist in that chain?)".into(),
            ));
        }
        if args.first() == Some(&"-A") && joined.contains("-p udp") {
            return Err(FirewallError::Command("simulated udp failure".into()));
        }
        Ok(())
    });
    let fw = Iptables {
        dry_run: false,
        path: None,
        runner: Some(runner),
    };
    let injection = Injection {
        fault: FaultSpec::Disconnect(DisconnectSpec { all_traffic: false }),
        target: Target {
            source: Some(IpNet::from_str("10.0.0.0/8").unwrap()),
            destination: Some(IpNet::from_str("192.0.2.0/24").unwrap()),
            destination_port: Some(443),
        },
    };
    assert!(fw.apply(&injection).is_err());
    let calls = calls.lock().unwrap().clone();
    // The tcp rule was installed before the failure...
    assert!(calls
        .iter()
        .any(|c| c.contains("-p tcp") && c.contains("-j DROP")));
    // ...and the partial install was rolled back via the chain teardown.
    // The INPUT -> FAULTDECK jump must be removed first; otherwise -X fails
    // with "Too many links" because the chain is still referenced, leaving a
    // dangling jump in the real firewall.
    let jump_removal = calls
        .iter()
        .position(|c| c.starts_with("-D INPUT -j FAULTDECK"))
        .expect("jump removal");
    let chain_flush = calls
        .iter()
        .position(|c| c.starts_with("-F FAULTDECK"))
        .expect("chain flush");
    let chain_delete = calls
        .iter()
        .position(|c| c.starts_with("-X FAULTDECK"))
        .expect("chain delete");
    assert!(jump_removal < chain_flush && chain_flush < chain_delete);
}

#[test]
fn apply_rejects_mixed_family_before_installing_any_rule() {
    use faultdeck_core::{DisconnectSpec, FaultSpec, Injection, Target};
    use ipnet::IpNet;
    use std::str::FromStr;

    // A source V4 + destination V6 target must be rejected before any -A rule
    // is installed: the old path installed the V4 rule and then bailed on
    // TargetNotV4, leaving an orphaned DROP rule the caller never knew about.
    let (fw, calls) = recorder();
    let injection = Injection {
        fault: FaultSpec::Disconnect(DisconnectSpec { all_traffic: true }),
        target: Target {
            source: Some(IpNet::from_str("10.0.0.0/8").unwrap()),
            destination: Some(IpNet::from_str("2001:db8::/32").unwrap()),
            destination_port: None,
        },
    };
    assert!(matches!(
        fw.apply(&injection),
        Err(FirewallError::TargetNotV4)
    ));
    let calls = calls.lock().unwrap().clone();
    // The target must be rejected before any iptables call: no chain creation,
    // no jump install, and no DROP rule may be attempted at all.
    assert!(calls.is_empty());
}

#[test]
fn apply_emits_both_rules_when_source_equals_destination() {
    // Regression (audit 2026-11-11 P1.4): with source == destination the old
    // `source == network` guess matched on every networks() pass and emitted
    // the -s rule twice, so traffic *to* that CIDR was never dropped. Both
    // role rules must exist, each exactly once.
    use faultdeck_core::{DisconnectSpec, FaultSpec, Injection, Target};
    use ipnet::IpNet;
    use std::str::FromStr;

    let (fw, calls) = recorder();
    let cidr = IpNet::from_str("198.51.100.0/24").unwrap();
    let injection = Injection {
        fault: FaultSpec::Disconnect(DisconnectSpec { all_traffic: true }),
        target: Target {
            source: Some(cidr),
            destination: Some(cidr),
            destination_port: None,
        },
    };
    fw.apply(&injection).unwrap();

    let calls = calls.lock().unwrap().clone();
    let source_rules = calls
        .iter()
        .filter(|c| {
            c.contains("-A FAULTDECK") && c.contains("-s 198.51.100.0/24") && c.contains("-j DROP")
        })
        .count();
    let dest_rules = calls
        .iter()
        .filter(|c| {
            c.contains("-A FAULTDECK") && c.contains("-d 198.51.100.0/24") && c.contains("-j DROP")
        })
        .count();
    assert_eq!(source_rules, 1, "exactly one source rule, got {calls:?}");
    assert_eq!(dest_rules, 1, "exactly one destination rule, got {calls:?}");
}
