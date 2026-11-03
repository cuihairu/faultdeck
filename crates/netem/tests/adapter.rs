use faultdeck_netem::{
    AdapterError, FaultSpec, Injection, LatencySpec, LossSpec, NetemAdapter, Target, TcCommand,
};
use ipnet::IpNet;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct Recorder {
    commands: Arc<Mutex<Vec<String>>>,
}

impl Recorder {
    fn run(&self, command: &TcCommand) -> faultdeck_netem::Result<()> {
        self.commands.lock().unwrap().push(command.to_string());
        Ok(())
    }
}

fn latency() -> Injection {
    Injection {
        fault: FaultSpec::Latency(LatencySpec { milliseconds: 120 }),
        target: Target::destination_port(IpNet::from_str("192.0.2.0/24").unwrap(), 7000),
    }
}

fn empty_executor() -> impl Fn(&TcCommand) -> faultdeck_netem::Result<()> {
    |_: &TcCommand| -> faultdeck_netem::Result<()> { Ok(()) }
}

#[test]
fn plans_targeted_latency() {
    let adapter = NetemAdapter::with_executor("eth1", empty_executor());
    let commands = adapter.plan(&latency()).unwrap();

    assert_eq!(
        commands[0].to_string(),
        "tc qdisc add dev eth1 root handle 100: prio bands 3"
    );
    assert!(commands.contains(&TcCommand {
        program: "tc".into(),
        arguments: vec![
            "qdisc".into(),
            "add".into(),
            "dev".into(),
            "eth1".into(),
            "parent".into(),
            "100:2".into(),
            "handle".into(),
            "50:".into(),
            "netem".into(),
            "delay".into(),
            "120ms".into()
        ]
    }));
    assert!(commands
        .iter()
        .all(|c| !c.to_string().contains("qdisc del dev eth1 root")));
}

#[test]
fn inject_applies_and_persists() {
    let recorder = Recorder::default();
    let recorder2 = recorder.clone();
    let adapter =
        NetemAdapter::with_executor("lab0", move |command: &TcCommand| recorder2.run(command));
    adapter.inject(&latency()).unwrap();

    let commands = recorder.commands.lock().unwrap().clone();
    assert_eq!(commands.len(), 5);
    assert_eq!(
        commands[0],
        "tc qdisc add dev lab0 root handle 100: prio bands 3"
    );
    assert!(!commands
        .iter()
        .any(|c| c.contains("qdisc del dev lab0 root")));

    adapter.rollback().unwrap();
    let commands = recorder.commands.lock().unwrap().clone();
    assert_eq!(commands.last().unwrap(), "tc qdisc del dev lab0 root");
}

#[test]
fn inject_partial_failure_rolls_back_created_qdisc() {
    let recorder = Recorder::default();
    let recorder2 = recorder.clone();
    let call = Arc::new(Mutex::new(0usize));
    let call2 = call.clone();
    let adapter = NetemAdapter::with_executor("lab1", move |command: &TcCommand| {
        let _ = recorder2.run(command);
        let mut n = call2.lock().unwrap();
        *n += 1;
        if *n == 4 {
            Err(AdapterError::Command {
                program: "tc".into(),
                arguments: command.arguments.join(" "),
                message: "device busy".into(),
            })
        } else {
            Ok(())
        }
    });
    let result = adapter.inject(&latency());
    assert!(result.is_err());
    let commands = recorder.commands.lock().unwrap().clone();
    // Failure after root qdisc created -> cleanup rollback emitted.
    assert!(commands.iter().any(|c| c == "tc qdisc del dev lab1 root"));
}

#[test]
fn validates_loss_target_and_ipv6() {
    let adapter = NetemAdapter::with_executor("eth0", empty_executor());
    let loss = Injection {
        fault: FaultSpec::Loss(LossSpec { percent: 2.5 }),
        target: Target::destination(IpNet::from_str("198.51.100.0/24").unwrap()),
    };
    assert!(adapter.plan(&loss).is_ok());

    let empty = Injection {
        fault: FaultSpec::Loss(LossSpec { percent: 2.5 }),
        target: Target::any(),
    };
    assert!(matches!(
        adapter.plan(&empty).unwrap_err(),
        AdapterError::EmptyTarget
    ));

    let zero = Injection {
        fault: FaultSpec::Loss(LossSpec { percent: 0.0 }),
        target: loss.target.clone(),
    };
    assert!(matches!(
        adapter.plan(&zero).unwrap_err(),
        AdapterError::InvalidLossPercent { percent: 0.0 }
    ));

    let ipv6 = Injection {
        fault: loss.fault.clone(),
        target: Target::destination(IpNet::from_str("2001:db8::/32").unwrap()),
    };
    assert!(matches!(
        adapter.plan(&ipv6).unwrap_err(),
        AdapterError::UnsupportedIpv6
    ));
}

#[test]
fn rollback_refuses_foreign_root_qdisc() {
    struct Foreign;
    impl faultdeck_netem::CommandExecutor for Foreign {
        fn run(&self, _: &faultdeck_netem::TcCommand) -> faultdeck_netem::Result<()> {
            Ok(())
        }
        fn run_capture(
            &self,
            command: &faultdeck_netem::TcCommand,
        ) -> faultdeck_netem::Result<String> {
            if command.arguments.iter().any(|a| a == "show") {
                Ok("qdisc fq_codel 1: root refcnt 2\n".to_string())
            } else {
                Ok(String::new())
            }
        }
    }
    let adapter = faultdeck_netem::NetemAdapter::with_executor("eth0", Foreign);
    assert!(matches!(
        adapter.rollback().unwrap_err(),
        AdapterError::ExistingRootQdisc { .. }
    ));
}

#[test]
fn inject_does_not_delete_foreign_qdisc_on_toctou_failure() {
    struct ToctouForeign;
    impl faultdeck_netem::CommandExecutor for ToctouForeign {
        fn run(&self, command: &faultdeck_netem::TcCommand) -> faultdeck_netem::Result<()> {
            // The preflight shows no root qdisc, but when the first command
            // (root qdisc add) runs a foreign qdisc has appeared -> it fails.
            // We must not emit a subsequent "qdisc del ... root".
            assert!(
                !command.arguments.iter().any(|a| a == "del"),
                "must not delete foreign qdisc"
            );
            if command.arguments.iter().any(|a| a == "add") {
                return Err(AdapterError::Command {
                    program: "tc".into(),
                    arguments: command.arguments.join(" "),
                    message: "file exists".into(),
                });
            }
            Ok(())
        }
        fn run_capture(
            &self,
            command: &faultdeck_netem::TcCommand,
        ) -> faultdeck_netem::Result<String> {
            if command.arguments.iter().any(|a| a == "show") {
                // No qdisc at preflight time.
                Ok(String::new())
            } else {
                Ok(String::new())
            }
        }
    }
    let adapter = faultdeck_netem::NetemAdapter::with_executor("eth0", ToctouForeign);
    let result = adapter.inject(&latency());
    assert!(result.is_err());
}

#[test]
fn inject_leftover_does_not_delete_foreign_qdisc_on_toctou_failure() {
    struct ToctouForeignAfterCleanup {
        commands: Arc<Mutex<Vec<String>>>,
    }
    impl faultdeck_netem::CommandExecutor for ToctouForeignAfterCleanup {
        fn run(&self, command: &faultdeck_netem::TcCommand) -> faultdeck_netem::Result<()> {
            self.commands.lock().unwrap().push(command.to_string());
            // A leftover 100: qdisc exists, so inject first issues a cleanup
            // `del`. That cleanup is legitimate. The subsequent root `add`
            // fails because a foreign qdisc appeared in the window; the
            // buggy code would then emit a SECOND `del` (rollback_cleanup)
            // and delete that foreign qdisc. Assert we never issue a second
            // delete after the cleanup.
            if command.arguments.iter().any(|a| a == "add") {
                return Err(faultdeck_netem::AdapterError::Command {
                    program: "tc".into(),
                    arguments: command.arguments.join(" "),
                    message: "file exists".into(),
                });
            }
            Ok(())
        }
        fn run_capture(
            &self,
            command: &faultdeck_netem::TcCommand,
        ) -> faultdeck_netem::Result<String> {
            if command.arguments.iter().any(|a| a == "show") {
                // Preflight reports a leftover of our own 100: qdisc.
                Ok("qdisc prio 100: root refcnt 2\n".to_string())
            } else {
                Ok(String::new())
            }
        }
    }
    let recorder = Recorder::default();
    let adapter = faultdeck_netem::NetemAdapter::with_executor(
        "eth0",
        ToctouForeignAfterCleanup {
            commands: recorder.commands.clone(),
        },
    );
    let result = adapter.inject(&latency());
    assert!(result.is_err());
    let commands = recorder.commands.lock().unwrap().clone();
    // The cleanup del must be issued before the add that fails, and the
    // rollback_cleanup must NOT issue a second del (the failed add never
    // created our qdisc, so a second del would delete the foreign qdisc).
    assert_eq!(commands[0], "tc qdisc del dev eth0 root");
    assert_eq!(
        commands
            .iter()
            .filter(|c| *c == "tc qdisc del dev eth0 root")
            .count(),
        1
    );
}

#[test]
fn inject_leftover_cleans_own_qdisc_before_reapply() {
    // A leftover 100: qdisc from a previous crashed run must be removed
    // before the root qdisc `add`, otherwise `add` fails with "File exists"
    // and inject can never recover from a crash/restart.
    // A crash can leave a 100: qdisc behind. inject must issue a cleanup
    // `del` before the first `add`, otherwise the add fails with "File exists".
    let recorder = Recorder::default();
    let recorder2 = recorder.clone();
    struct Executor {
        commands: Arc<Mutex<Vec<String>>>,
    }
    impl faultdeck_netem::CommandExecutor for Executor {
        fn run(&self, command: &faultdeck_netem::TcCommand) -> faultdeck_netem::Result<()> {
            self.commands.lock().unwrap().push(command.to_string());
            Ok(())
        }
        fn run_capture(
            &self,
            _command: &faultdeck_netem::TcCommand,
        ) -> faultdeck_netem::Result<String> {
            Ok("qdisc prio 100: root refcnt 2\n".to_string())
        }
    }
    let adapter = faultdeck_netem::NetemAdapter::with_executor(
        "eth0",
        Executor {
            commands: recorder2.commands.clone(),
        },
    );
    let commands = adapter.inject(&latency()).unwrap();
    assert_eq!(
        recorder.commands.lock().unwrap()[0],
        "tc qdisc del dev eth0 root"
    );
    assert_eq!(
        recorder.commands.lock().unwrap()[1],
        "tc qdisc add dev eth0 root handle 100: prio bands 3"
    );
    assert_eq!(commands.len(), 5);
}

#[test]
fn rollback_is_idempotent_when_device_missing() {
    struct MissingDevice;
    impl faultdeck_netem::CommandExecutor for MissingDevice {
        fn run(&self, command: &faultdeck_netem::TcCommand) -> faultdeck_netem::Result<()> {
            if command.arguments.iter().any(|a| a == "del") {
                return Err(AdapterError::Command {
                    program: "tc".into(),
                    arguments: command.arguments.join(" "),
                    message: "Cannot find device \"eth0\"".into(),
                });
            }
            Ok(())
        }
        fn run_capture(
            &self,
            command: &faultdeck_netem::TcCommand,
        ) -> faultdeck_netem::Result<String> {
            if command.arguments.iter().any(|a| a == "show") {
                Err(AdapterError::Command {
                    program: "tc".into(),
                    arguments: command.arguments.join(" "),
                    message: "Cannot find device \"eth0\"".into(),
                })
            } else {
                Ok(String::new())
            }
        }
    }
    let adapter = faultdeck_netem::NetemAdapter::with_executor("eth0", MissingDevice);
    // Both show and del report "Cannot find device": rollback must succeed.
    assert!(adapter.rollback().is_ok());
}
