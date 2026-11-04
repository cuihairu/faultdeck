use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use faultdeck_core::{FaultSpec, Injection, Target};
use ipnet::IpNet;
use thiserror::Error;

pub const CHAIN: &str = "FAULTDECK";
pub const JUMP_CHAIN: &str = "INPUT";

#[derive(Debug, Error)]
pub enum FirewallError {
    #[error("iptables missing")]
    IptablesMissing,
    #[error("command failed: {0}")]
    Command(String),
    #[error("unsupported fault for firewall: {0}")]
    Unsupported(String),
    #[error("target network must be IPv4")]
    TargetNotV4,
}

pub type Result<T> = std::result::Result<T, FirewallError>;

/// Injected command runner used by tests to observe iptables invocations
/// without shelling out (avoids /tmp script "Text file busy" races).
pub type CommandRunner = dyn Fn(&[&str]) -> Result<()> + Send + Sync;

#[derive(Clone, Default)]
pub struct Iptables {
    pub dry_run: bool,
    pub path: Option<PathBuf>,
    pub runner: Option<Arc<CommandRunner>>,
}

impl Iptables {
    fn binary(&self) -> Result<&PathBuf> {
        if let Some(path) = &self.path {
            Ok(path)
        } else {
            Err(FirewallError::IptablesMissing)
        }
    }

    fn run(&self, args: &[&str]) -> Result<()> {
        if let Some(runner) = &self.runner {
            return runner(args);
        }
        if self.dry_run {
            return Ok(());
        }
        let bin = self.binary()?;
        let output = Command::new(bin)
            .args(args)
            .output()
            .map_err(|e| FirewallError::Command(e.to_string()))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(FirewallError::Command(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ))
        }
    }

    /// Run a command, treating a specific failure message as success so the
    /// chain/rule lifecycle can be idempotent (e.g. "chain already exists").
    fn run_ignoring(&self, args: &[&str], messages: &[&str]) -> Result<()> {
        match self.run(args) {
            Ok(()) => Ok(()),
            Err(FirewallError::Command(message))
                if messages.iter().any(|m| message.contains(m)) =>
            {
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    pub fn ensure_chain(&self) -> Result<()> {
        // -N fails with "Chain already exists" when the chain is present
        // after a previous run; tolerate it for idempotence.
        self.run_ignoring(&["-N", CHAIN, "-t", "mangle"], &["Chain already exists"])?;
        // Install the INPUT -> FAULTDECK jump only if it is not already there.
        // iptables -I inserts unconditionally and never reports "Bad rule",
        // so relying on that message to tolerate a pre-existing jump was dead
        // code: a leftover jump (after agent restart, applied is in-memory)
        // would accumulate another copy on every apply. -C checks first; only
        // a missing jump triggers the insert.
        let jump = ["-C", "INPUT", "-j", CHAIN, "-t", "mangle"];
        // In dry-run we cannot query iptables, so treat the jump as absent:
        // this keeps the emitted -I command in step with a real first deploy.
        let already_present = if self.dry_run {
            false
        } else {
            match self.run(&jump) {
                Ok(()) => true,
                Err(FirewallError::Command(message)) => {
                    // "Bad rule" means no matching rule exists -> not installed.
                    // Any other failure (e.g. iptables missing) propagates below.
                    if message.contains("Bad rule") {
                        false
                    } else {
                        return Err(FirewallError::Command(message));
                    }
                }
                Err(e) => return Err(e),
            }
        };
        if !already_present {
            self.run_ignoring(&["-I", "INPUT", "-j", CHAIN, "-t", "mangle"], &["Bad rule"])?;
        }
        Ok(())
    }

    pub fn apply(&self, injection: &Injection) -> Result<()> {
        let target = &injection.target;
        // Pre-validate every network is IPv4 before touching iptables. The
        // server path rejects IPv6 up front, but apply()/process_request are
        // public and a mixed-family target (source V4 + destination V6) would
        // otherwise install the V4 rule and then bail on TargetNotV4, leaving
        // an orphaned DROP rule behind for a caller told nothing was applied.
        if target.networks().any(|network| !matches!(network, IpNet::V4(_))) {
            return Err(FirewallError::TargetNotV4);
        }
        self.ensure_chain()?;
        let all_traffic = match &injection.fault {
            FaultSpec::Disconnect(spec) => spec.all_traffic,
            _ => true,
        };
        for network in target.networks() {
            let IpNet::V4(net) = network else { unreachable!() };
            let addr = net.addr();
            let prefix = net.prefix_len();
            let mut args: Vec<String> =
                vec!["-A".into(), CHAIN.into(), "-t".into(), "mangle".into()];
            // source is a source match (-s), destination a dest match (-d).
            if target.source == Some(*network) {
                args.push("-s".into());
            } else {
                args.push("-d".into());
            }
            args.push(format!("{addr}/{prefix}"));
            // Port-level disconnect drops both TCP and UDP on the port; a
            // single protocol match would leave the other transport untouched.
            let protocols: &[&str] = if !all_traffic && target.destination_port.is_some() {
                &["tcp", "udp"]
            } else {
                &["any"]
            };
            for protocol in protocols {
                let mut rule = args.clone();
                if *protocol != "any" {
                    // iptables requires a protocol match for --dport; without
                    // -p the rule fails with "unknown option --dport" and the
                    // port-level disconnect rule would never install.
                    rule.push("-p".into());
                    rule.push((*protocol).into());
                    rule.push("--dport".into());
                    rule.push(target.destination_port.unwrap().to_string());
                }
                rule.push("-j".into());
                rule.push("DROP".into());
                let refs: Vec<&str> = rule.iter().map(|s| s.as_str()).collect();
                if let Err(e) = self.run(&refs) {
                    // A partial install (e.g. the tcp rule landed but the udp
                    // rule failed) would leave an orphaned DROP rule while the
                    // caller believes nothing was applied. Tear the whole chain
                    // down so iptables state matches the error the client sees;
                    // rollback errors are best-effort, but if the teardown
                    // itself fails the iptables state is still dirty and the
                    // operator needs a clue beyond the original failure.
                    if let Err(rb) = self.rollback() {
                        eprintln!("firewall rollback after partial install failed: {rb}");
                    }
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    pub fn rollback(&self) -> Result<()> {
        // Remove the INPUT -> FAULTDECK jump first; otherwise -X fails with
        // "Too many links" because the chain is still referenced. Tolerate a
        // missing jump ("Bad rule") for idempotence.
        self.run_ignoring(&["-D", "INPUT", "-j", CHAIN, "-t", "mangle"], &["Bad rule"])?;
        // Flush then delete the chain, tolerating an absent chain.
        self.run_ignoring(&["-F", CHAIN, "-t", "mangle"], &["No chain", "No rule"])?;
        self.run_ignoring(
            &["-X", CHAIN, "-t", "mangle"],
            &["No chain", "No rule", "Too many links"],
        )?;
        Ok(())
    }
}

/// Decide whether an injection is a disconnect (iptables) fault.
pub fn is_disconnect(injection: &Injection) -> bool {
    matches!(injection.fault, FaultSpec::Disconnect(_))
}

pub fn validate_disconnect_target(target: &Target) -> Result<()> {
    let _ = target;
    Ok(())
}
