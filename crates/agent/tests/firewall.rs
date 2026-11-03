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
        if args.first().map(|s| *s) == Some(prefix.as_str()) {
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
    let (fw, calls) = recorder();
    fw.ensure_chain().unwrap();
    let calls = calls.lock().unwrap().clone();
    assert!(calls.iter().any(|c| c.starts_with("-N FAULTDECK")));
    assert!(calls.iter().any(|c| c.starts_with("-I INPUT -j FAULTDECK")));
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
    assert!(calls.iter().any(|c| c.contains("--dport 443")));
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
    assert!(calls.iter().any(|c| c.contains("-A FAULTDECK")
        && c.contains("-d 192.0.2.0/24")
        && c.contains("-j DROP")));
    assert!(calls.iter().any(|c| c.starts_with("-D INPUT -j FAULTDECK")));
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
