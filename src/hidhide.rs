//! Runtime probe for HidHide config.
//!
//! HidHide is set up at install time by the NSIS installer (allow-list:
//! our helper.exe; block-list: FortniteClient-Win64-Shipping.exe). This
//! module shells out to HidHideCLI.exe at startup to confirm those
//! entries are still present so the health response can warn the
//! streamer if e.g. they wiped HidHide config with another app.
//!
//! Errors here are never fatal: a "hidhide unknown" health flag is
//! still better than the helper refusing to start.

#![allow(dead_code)]

use serde::Serialize;
use std::path::PathBuf;
use std::process::Command;

/// Default install path for the HidHide CLI shipped with the .msi.
/// Same path on all Windows versions per HidHide's MSI defaults.
const HIDHIDE_CLI: &str = r"C:\Program Files\Nefarius Software Solutions\HidHide\x64\HidHideCLI.exe";

/// Fortnite's main executable name — this is what we add to HidHide's
/// blocked-apps list at install time. Bound by EAC, so don't rename
/// without confirming the binary name in a current Fortnite build.
pub const FORTNITE_EXE: &str = "FortniteClient-Win64-Shipping.exe";

#[derive(Debug, Clone, Serialize)]
pub struct HidHideStatus {
    pub cli_present: bool,
    pub helper_on_allowlist: Option<bool>,
    pub fortnite_on_blocklist: Option<bool>,
    pub raw: Option<String>,
}

impl HidHideStatus {
    pub fn summary(&self) -> &'static str {
        if !self.cli_present {
            return "cli_missing";
        }
        match (self.helper_on_allowlist, self.fortnite_on_blocklist) {
            (Some(true), Some(true)) => "configured",
            (Some(false), _) => "helper_not_whitelisted",
            (_, Some(false)) => "fortnite_not_blocked",
            _ => "unknown",
        }
    }
}

pub fn probe() -> HidHideStatus {
    if !cli_path().exists() {
        return HidHideStatus {
            cli_present: false,
            helper_on_allowlist: None,
            fortnite_on_blocklist: None,
            raw: None,
        };
    }

    let helper_exe = std::env::current_exe()
        .ok()
        .map(|p| p.to_string_lossy().to_lowercase());

    let app_list = run_cli(&["--app-list"]).unwrap_or_default();
    let cloak_list = run_cli(&["--dev-list"]).unwrap_or_default();

    let helper_on_allowlist = helper_exe.map(|h| app_list.to_lowercase().contains(&h));

    // We don't track Fortnite by device id — we only added it to the
    // app list as a "block when running" entry. HidHide's CLI exposes
    // that via --app-list as well; the convention is the same path.
    let fortnite_on_blocklist = Some(app_list.to_lowercase().contains(&FORTNITE_EXE.to_lowercase()));

    HidHideStatus {
        cli_present: true,
        helper_on_allowlist,
        fortnite_on_blocklist,
        raw: Some(format!("apps:{}\ncloak:{}", app_list, cloak_list)),
    }
}

fn cli_path() -> PathBuf {
    PathBuf::from(HIDHIDE_CLI)
}

fn run_cli(args: &[&str]) -> Option<String> {
    let out = Command::new(cli_path()).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}
