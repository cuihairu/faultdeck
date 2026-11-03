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
        let bin = self.binary()?;
        if self.dry_run {
            return Ok(());
        }
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
        // Install the INPUT -> FAULTDECK jump. -I tolerates a pre-existing
        // jump ("Bad rule (does a matching rule exist...)"). -C only checks
        // and never installs, which is why DROP rules never took effect.
        self.run_ignoring(&["-I", "INPUT", "-j", CHAIN, "-t", "mangle"], &["Bad rule"])?;
        Ok(())
    }

    pub fn apply(&self, injection: &Injection) -> Result<()> {
        self.ensure_chain()?;
        let target = &injection.target;
        let all_traffic = match &injection.fault {
            FaultSpec::Disconnect(spec) => spec.all_traffic,
            _ => true,
        };
        for network in target.networks() {
            let IpNet::V4(net) = network else {
                return Err(FirewallError::TargetNotV4);
            };
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
            // Only restrict to the destination port when the spec says so.
            if !all_traffic {
                if let Some(port) = target.destination_port {
                    // iptables requires a protocol match for --dport; without
                    // -p tcp the rule fails with "unknown option --dport" and
                    // the port-level disconnect rule would never install.
                    args.push("-p".into());
                    args.push("tcp".into());
                    args.push("--dport".into());
                    args.push(port.to_string());
                }
            }
            args.push("-j".into());
            args.push("DROP".into());
            let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            self.run(&refs)?;
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
