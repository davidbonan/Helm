//! Whether the macOS application firewall keeps the phone out (specs/remote.md §2):
//! read from `socketfilterfw`, which answers without admin rights.

use std::path::Path;

const SOCKETFILTERFW: &str = "/usr/libexec/ApplicationFirewall/socketfilterfw";
pub const SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.Network-Settings.extension?Firewall";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirewallBlock {
    /// *Block all incoming connections*: no per-app rule lets the phone in.
    AllIncoming,
    /// helm's own rule says *Block incoming connections*.
    Helm,
}

/// What blocks the running executable; `None` when nothing does or the firewall
/// can't be read.
pub fn helm_blocked() -> Option<FirewallBlock> {
    let executable = std::env::current_exe().ok()?;
    let executable = executable.canonicalize().unwrap_or(executable);
    let global = super::network::run(SOCKETFILTERFW, &["--getglobalstate"])?;
    let apps = super::network::run(SOCKETFILTERFW, &["--listapps"]).unwrap_or_default();
    block_from(&global, &apps, &executable)
}

pub fn open_settings() {
    let _ = std::process::Command::new("open").arg(SETTINGS_URL).spawn();
}

/// `--getglobalstate` ends in `(State = 0|1|2)`, 2 = block all; `--listapps`
/// prints `N : <path>` then the rule on the next line.
pub fn block_from(global: &str, apps: &str, executable: &Path) -> Option<FirewallBlock> {
    if global.contains("(State = 2)") {
        return Some(FirewallBlock::AllIncoming);
    }
    if !global.contains("(State = 1)") {
        return None;
    }
    let mut lines = apps.lines();
    while let Some(line) = lines.next() {
        let Some((_, path)) = line.split_once(" : ") else {
            continue;
        };
        if Path::new(path.trim()) == executable {
            return lines
                .next()
                .is_some_and(|rule| rule.contains("Block incoming"))
                .then_some(FirewallBlock::Helm);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const ON: &str = "Firewall is enabled. (State = 1)\n";
    const HELM: &str = "/Applications/helm.app/Contents/MacOS/helm";

    fn apps(rule: &str) -> String {
        format!(
            "Total number of apps = 2 \n1 : /usr/sbin/cupsd \n             (Block incoming connections)\n2 : {HELM} \n             ({rule} incoming connections)\n"
        )
    }

    #[test]
    fn block_all_blocks_whatever_helm_s_rule() {
        let global = "Firewall is blocking all non-essential incoming connections. (State = 2)\n";

        assert_eq!(
            block_from(global, &apps("Allow"), Path::new(HELM)),
            Some(FirewallBlock::AllIncoming)
        );
    }

    #[test]
    fn helm_s_block_rule_blocks_and_its_allow_rule_lets_in() {
        assert_eq!(
            block_from(ON, &apps("Block"), Path::new(HELM)),
            Some(FirewallBlock::Helm)
        );
        assert_eq!(block_from(ON, &apps("Allow"), Path::new(HELM)), None);
    }

    #[test]
    fn only_the_executable_s_own_rule_counts() {
        assert_eq!(
            block_from(ON, &apps("Allow"), Path::new("/usr/sbin/cupsd")),
            Some(FirewallBlock::Helm)
        );
        assert_eq!(
            block_from(ON, &apps("Block"), Path::new("/target/debug/helm")),
            None
        );
    }

    #[test]
    fn a_disabled_firewall_blocks_nothing() {
        let global = "Firewall is disabled. (State = 0)\n";

        assert_eq!(block_from(global, &apps("Block"), Path::new(HELM)), None);
    }
}
