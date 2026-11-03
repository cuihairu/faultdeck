use std::path::PathBuf;
use std::process::Command;

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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Iptables {
    pub dry_run: bool,
    pub path: Option<PathBuf>,
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

    pub fn ensure_chain(&self) -> Result<()> {
        self.run(&["-N", CHAIN, "-t", "mangle"])?;
        self.run(&["-C", "INPUT", "-j", CHAIN, "-t", "mangle"])?;
        Ok(())
    }

    pub fn apply(&self, injection: &Injection) -> Result<()> {
        self.ensure_chain()?;
        let target = &injection.target;
        for network in target.networks() {
            let IpNet::V4(net) = network else {
                return Err(FirewallError::TargetNotV4);
            };
            let addr = net.addr();
            let prefix = net.prefix_len();
            self.run(&[
                "-A",
                CHAIN,
                "-t",
                "mangle",
                "-d",
                &format!("{addr}/{prefix}"),
                "-j",
                "DROP",
            ])?;
        }
        Ok(())
    }

    pub fn rollback(&self) -> Result<()> {
        self.run(&["-F", CHAIN, "-t", "mangle"])?;
        self.run(&["-X", CHAIN, "-t", "mangle"])?;
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
