//! Runtime probe for HidHide config.
//!
//! HidHide is set up at install time by the NSIS installer
//! (whitelists our helper.exe + flips the machine-wide cloak on). At
//! startup we shell out to HidHideCLI.exe to confirm the helper is
//! still on the whitelist; everything else (which device is actually
//! hidden, which apps the streamer has added themselves) is the
//! streamer's domain via the HidHide tray UI.
//!
//! HidHideCLI's read commands require admin -- the driver returns
//! ERROR_ACCESS_DENIED (0x5) for non-elevated callers. The helper
//! runs as the streamer's user (no admin), so we treat a denied probe
//! as "unknown_needs_admin" rather than reporting false negatives.
//!
//! Errors here are never fatal: a hint in the health response is
//! still better than the helper refusing to start.

#![allow(dead_code)]

use serde::Serialize;
use std::path::PathBuf;
use std::process::Command;

/// Default install path for the HidHide CLI shipped with the signed setup.
/// Same path on all Windows versions per HidHide's installer defaults
/// (.exe bundle since v1.5.230; .msi before that -- layout unchanged).
const HIDHIDE_CLI: &str =
    r"C:\Program Files\Nefarius Software Solutions\HidHide\x64\HidHideCLI.exe";

#[derive(Debug, Clone, Serialize)]
pub struct HidHideStatus {
    pub cli_present: bool,
    pub helper_on_allowlist: Option<bool>,
    pub fortnite_on_allowlist: Option<bool>,
    pub cloak_enabled: Option<bool>,
    pub access_denied: bool,
    pub raw: Option<String>,
}

impl HidHideStatus {
    pub fn summary(&self) -> &'static str {
        if !self.cli_present {
            return "cli_missing";
        }
        if self.access_denied {
            return "unknown_needs_admin";
        }
        match (
            self.helper_on_allowlist,
            self.fortnite_on_allowlist,
            self.cloak_enabled,
        ) {
            (Some(false), _, _) => "helper_not_whitelisted",
            (_, Some(true), _) => "fortnite_still_whitelisted",
            (Some(true), Some(false), Some(true)) => "configured",
            (_, _, Some(false)) => "cloak_off",
            _ => "unknown",
        }
    }
}

pub fn probe() -> HidHideStatus {
    if !cli_path().exists() {
        return HidHideStatus {
            cli_present: false,
            helper_on_allowlist: None,
            fortnite_on_allowlist: None,
            cloak_enabled: None,
            access_denied: false,
            raw: None,
        };
    }

    let helper_exe = std::env::current_exe()
        .ok()
        .map(|p| p.to_string_lossy().to_lowercase());

    let (app_list, denied) = match run_cli(&["--app-list"]) {
        CliOutput::Ok(s) => (s, false),
        CliOutput::AccessDenied => (String::new(), true),
        CliOutput::OtherErr => (String::new(), false),
    };

    let cloak_enabled = if denied {
        None
    } else {
        match run_cli(&["--cloak-state"]) {
            CliOutput::Ok(s) => {
                let l = s.to_lowercase();
                Some(
                    l.contains("on")
                        || l.contains("enabled")
                        || l.contains("true")
                        || l.contains("1"),
                )
            }
            _ => None,
        }
    };

    // If --app-list returned empty (CLI may silently no-op without admin on
    // some HidHide builds), we cannot honestly say the helper *isn't*
    // whitelisted -- only that we cannot prove it is. Treat empty as
    // unknown so the health line doesn't slander the install.
    let list_empty = app_list.trim().is_empty();

    let helper_on_allowlist = if denied || list_empty {
        None
    } else {
        helper_exe.map(|h| app_list.to_lowercase().contains(&h))
    };

    let fortnite_on_allowlist = if denied || list_empty {
        None
    } else {
        Some(
            app_list
                .to_lowercase()
                .contains("fortniteclient-win64-shipping.exe"),
        )
    };

    HidHideStatus {
        cli_present: true,
        helper_on_allowlist,
        fortnite_on_allowlist,
        cloak_enabled,
        access_denied: denied,
        raw: if denied {
            None
        } else {
            Some(format!("apps:{}", app_list))
        },
    }
}

fn cli_path() -> PathBuf {
    PathBuf::from(HIDHIDE_CLI)
}

enum CliOutput {
    Ok(String),
    AccessDenied,
    OtherErr,
}

fn run_cli(args: &[&str]) -> CliOutput {
    let Ok(out) = Command::new(cli_path()).args(args).output() else {
        return CliOutput::OtherErr;
    };
    if out.status.success() {
        return match String::from_utf8(out.stdout) {
            Ok(s) => CliOutput::Ok(s),
            Err(_) => CliOutput::OtherErr,
        };
    }
    // HidHideCLI prints "Error code 0x0005 ... Access is denied." on
    // stdout (not stderr) when invoked without admin. Detect that so
    // the health response can hint at the cause instead of falsely
    // claiming the helper isn't whitelisted.
    let combined =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    if combined.contains("0x0005") || combined.to_lowercase().contains("access is denied") {
        CliOutput::AccessDenied
    } else {
        CliOutput::OtherErr
    }
}
