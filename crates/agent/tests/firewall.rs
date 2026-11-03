use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use faultdeck_agent::firewall::Iptables;

fn mock_iptables(dir: &std::path::Path) -> PathBuf {
    let script = dir.join("iptables");
    let mut f = std::fs::File::create(&script).unwrap();
    write!(
        f,
        r#"#!/bin/sh
BASE="$(dirname "$0")/state"
mkdir -p "$BASE"
if [ "$1" = "-N" ]; then
  if [ -e "$BASE/N" ]; then echo "Chain already exists" >&2; exit 1; fi
  : > "$BASE/N"
elif [ "$1" = "-I" ]; then
  if [ -e "$BASE/I" ]; then echo "Bad rule (does a matching rule exist in that chain?)" >&2; exit 1; fi
  : > "$BASE/I"
elif [ "$1" = "-D" ]; then
  if [ -e "$BASE/D" ]; then echo "Bad rule (does a matching rule exist in that chain?)" >&2; exit 1; fi
  : > "$BASE/D"
elif [ "$1" = "-F" ]; then
  if [ ! -e "$BASE/F" ]; then echo "No chain/target/match by that name" >&2; exit 1; fi
  : > "$BASE/F"
elif [ "$1" = "-X" ]; then
  if [ -e "$BASE/X" ]; then echo "Too many links" >&2; exit 1; fi
  : > "$BASE/X"
fi
exit 0
"#
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

#[test]
fn ensure_chain_is_idempotent_over_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let iptables = mock_iptables(dir.path());
    let fw = Iptables {
        dry_run: false,
        path: Some(iptables),
    };
    fw.ensure_chain().unwrap();
    fw.ensure_chain().unwrap();
    assert!(dir.path().join("state/N").exists());
    assert!(dir.path().join("state/I").exists());
}

#[test]
fn rollback_is_idempotent_when_chain_absent() {
    let dir = tempfile::tempdir().unwrap();
    let iptables = mock_iptables(dir.path());
    let fw = Iptables {
        dry_run: false,
        path: Some(iptables),
    };
    // Chain absent: -D "Bad rule", -F "No chain", -X "Too many links" (if the
    // chain is still referenced) are all tolerated -> idempotent.
    fw.rollback().unwrap();
    assert!(dir.path().join("state/D").exists());
}
