//! Windows toast delivery, and whether this build can perform it at all.
//!
//! # Why this is not `tauri_plugin_notification`
//!
//! The plugin's `NotificationBuilder::show` ends in
//! `tauri::async_runtime::spawn(async move { let _ = notification.show(); })`.
//! The result is discarded, so every caller learns the same thing whether the
//! toast reached the shell or failed outright: nothing. The desktop recorded
//! `OsDeliveryStatus::Delivered` for 38 notifications on that basis, which is a
//! claim it had no evidence for. For a tool whose whole argument is measurement
//! over inference, a delivery status that cannot come back false is the defect,
//! not a cosmetic one.
//!
//! So this module calls the same underlying toast API the plugin does --
//! `tauri_winrt_notification`, already in the dependency graph beneath it --
//! synchronously, and reports what Windows actually said. The plugin is still
//! what the app initialises and what answers `permission_state`; only the send
//! moved here.
//!
//! # Why an `Ok` from Windows is still not enough
//!
//! `ToastNotifier::Show` returns success for an unregistered
//! `System.AppUserModel.ID` and then silently drops the toast -- no error, no
//! notification, nothing in the Action Center. Treating that `Ok` as delivery
//! would rebuild the same unfalsifiable claim one layer down. [`deliverability`]
//! is therefore checked *first*, and a build Windows cannot attribute a toast to
//! fails before the API is called, naming the reason.

use serde::Serialize;
use std::path::{Path, PathBuf};

/// Whether a toast sent from this process can reach the Windows shell.
///
/// Carries the app id in every arm that has one, because "which identity did
/// you send under" is the first question worth asking when a toast does not
/// appear.
///
/// # What this does *not* depend on
///
/// Not where the executable lives. `tauri_plugin_notification` skips setting
/// `System.AppUserModel.ID` whenever the exe sits under `target/debug` or
/// `target/release`, which is why toasts from a `cargo run` build go nowhere
/// through the plugin -- but this module sets the id itself, unconditionally.
/// Measured on 2026-08-17: a toast sent with `dev.contexttrace.desktop` from
/// `target/debug` on a machine where the app is installed was accepted by
/// Windows and advanced that app id's `LastNotificationAddedTime`. So a
/// development build delivers exactly as well as an installed one, and
/// registration is the whole question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
// The per-variant `rename_all` is the load-bearing one, and is why every other
// tagged enum in this crate carries it too: the container attribute renames an
// enum's *variants*, never the fields inside them. Without it `app_id` and
// `exe_dir` went out in snake_case while every other DTO spoke camelCase, and
// the frontend -- which validates a payload before rendering it -- rejected the
// whole notification status, which nulled the settings loaded beside it.
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Deliverability {
    /// A Start Menu shortcut carries this app id, so Windows knows who a toast
    /// is from.
    #[serde(rename_all = "camelCase")]
    Ready { app_id: String },
    /// Nothing on this machine associates the app id with an application, so
    /// Windows will accept a toast and silently drop it. `exe_dir` is carried
    /// because the usual shape of this state is a build run out of a Cargo
    /// target directory on a machine where the app was never installed, and
    /// naming the directory is what makes the message actionable.
    #[serde(rename_all = "camelCase")]
    Unregistered {
        app_id: String,
        exe_dir: Option<String>,
    },
    /// Not Windows. The desktop ships for Windows only; this arm exists so the
    /// crate still compiles and reports honestly elsewhere. Absent on Windows
    /// rather than merely unreachable there: a state that cannot occur should
    /// not be representable, and leaving it in would also be the one variant
    /// nothing ever constructs on the platform that ships.
    #[cfg(not(windows))]
    Unsupported,
}

impl Deliverability {
    /// The one-line explanation shown when a toast cannot be delivered.
    ///
    /// `None` for [`Deliverability::Ready`]: there is nothing to explain.
    pub fn obstacle(&self) -> Option<String> {
        match self {
            Deliverability::Ready { .. } => None,
            Deliverability::Unregistered { app_id, exe_dir } => {
                let running = exe_dir
                    .as_deref()
                    .filter(|dir| is_cargo_target_dir(Path::new(dir)))
                    .map(|dir| format!(" This build is running from {dir}."))
                    .unwrap_or_default();
                Some(format!(
                    "No installed shortcut on this machine carries the app id {app_id}, so \
                     Windows accepts a toast and then discards it. Install ContextTrace once; \
                     after that any build can deliver.{running}"
                ))
            }
            #[cfg(not(windows))]
            Deliverability::Unsupported => {
                Some("OS notifications are supported on Windows only.".into())
            }
        }
    }
}

/// Send one toast, and report what actually happened.
pub fn deliver(
    deliverability: &Deliverability,
    title: &str,
    body: &str,
) -> ct_domain::OsDeliveryStatus {
    let Deliverability::Ready { app_id } = deliverability else {
        return ct_domain::OsDeliveryStatus::Failed {
            reason: deliverability
                .obstacle()
                .unwrap_or_else(|| "undeliverable".into()),
        };
    };
    show(app_id, title, body)
}

#[cfg(windows)]
fn show(app_id: &str, title: &str, body: &str) -> ct_domain::OsDeliveryStatus {
    match tauri_winrt_notification::Toast::new(app_id)
        .title(title)
        .text1(body)
        .show()
    {
        Ok(()) => ct_domain::OsDeliveryStatus::Delivered,
        Err(error) => ct_domain::OsDeliveryStatus::Failed {
            reason: error.to_string(),
        },
    }
}

#[cfg(not(windows))]
fn show(_app_id: &str, _title: &str, _body: &str) -> ct_domain::OsDeliveryStatus {
    ct_domain::OsDeliveryStatus::Failed {
        reason: "OS notifications are supported on Windows only.".into(),
    }
}

/// Work out, from this machine, whether toasts can be delivered.
///
/// Read-only throughout. Registering an app id would mean writing to
/// `HKCU\Software\Classes\AppUserModelId`, and the startup panel discloses
/// exactly one directory this tool writes to; a registry key created behind
/// that sentence would falsify it. So this observes and reports, and the
/// installer stays the only thing that registers anything.
#[cfg(windows)]
pub fn deliverability(app_id: &str, product_name: &str) -> Deliverability {
    if shortcut_declares(app_id, product_name) {
        return Deliverability::Ready {
            app_id: app_id.into(),
        };
    }
    Deliverability::Unregistered {
        app_id: app_id.into(),
        exe_dir: std::env::current_exe()
            .ok()
            .as_deref()
            .and_then(Path::parent)
            .map(|dir| dir.display().to_string()),
    }
}

#[cfg(not(windows))]
pub fn deliverability(_app_id: &str, _product_name: &str) -> Deliverability {
    Deliverability::Unsupported
}

/// `…/target/debug` and `…/target/release`.
///
/// Used only to phrase the explanation -- a build directory is a likely reason
/// an app id was never registered, not a reason delivery is refused. Matched on
/// the parent directory's name rather than on the string `target` anywhere in
/// the path, so a user who installs into `C:\target\release of tools` is not
/// told they are running a development build.
fn is_cargo_target_dir(dir: &Path) -> bool {
    let named = |path: Option<&Path>, name: &str| {
        path.and_then(Path::file_name)
            .is_some_and(|found| found.eq_ignore_ascii_case(name))
    };
    (named(Some(dir), "debug") || named(Some(dir), "release")) && named(dir.parent(), "target")
}

/// Whether a Start Menu shortcut on this machine carries `app_id`.
///
/// The NSIS installer writes `<Start Menu>\Programs\<ProductName>.lnk` with
/// `System.AppUserModel.ID` set, and that shortcut is the whole mechanism by
/// which Windows learns the app id belongs to an application. Reading the
/// property properly means an `IPropertyStore` round trip through COM; the
/// shortcut is under 2 KiB and stores the id as UTF-16, so this searches its
/// bytes instead.
///
/// That is a substring test, and it is stated as one: it answers "this
/// shortcut mentions this app id", not "this shortcut's AppUserModel.ID
/// property is this app id". The two can only diverge if some other property
/// of the same shortcut happens to contain the identifier, which would mean
/// the shortcut names the app anyway.
#[cfg_attr(not(windows), allow(dead_code))]
fn shortcut_declares(app_id: &str, product_name: &str) -> bool {
    let needle: Vec<u8> = app_id
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    start_menu_roots()
        .iter()
        .flat_map(|root| shortcut_candidates(root, product_name))
        .filter_map(|path| std::fs::read(path).ok())
        .any(|bytes| contains(&bytes, &needle))
}

#[cfg_attr(not(windows), allow(dead_code))]
fn start_menu_roots() -> Vec<PathBuf> {
    ["APPDATA", "ProgramData"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|base| {
            Path::new(&base)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs")
        })
        .collect()
}

/// The shortcut the installer writes, plus the one-folder-deep variant a user
/// gets if they move it into a group. Deliberately not a full recursive walk of
/// the Start Menu: this runs on a UI thread's behalf, and the answer does not
/// improve by reading every shortcut on the machine.
#[cfg_attr(not(windows), allow(dead_code))]
fn shortcut_candidates(root: &Path, product_name: &str) -> Vec<PathBuf> {
    let leaf = format!("{product_name}.lnk");
    let mut found = vec![root.join(&leaf)];
    if let Ok(entries) = std::fs::read_dir(root) {
        found.extend(
            entries
                .flatten()
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                .map(|entry| entry.path().join(&leaf)),
        );
    }
    found.retain(|path| path.is_file());
    found
}

#[cfg_attr(not(windows), allow(dead_code))]
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack.len() >= needle.len()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ready_build_has_nothing_to_explain() {
        let ready = Deliverability::Ready {
            app_id: "dev.contexttrace.desktop".into(),
        };
        assert_eq!(ready.obstacle(), None);
    }

    #[test]
    fn an_unregistered_app_id_is_named_and_a_build_directory_is_added_when_there_is_one() {
        let from_a_build = Deliverability::Unregistered {
            app_id: "dev.contexttrace.desktop".into(),
            exe_dir: Some("C:\\work\\target\\debug".into()),
        }
        .obstacle()
        .expect("an obstacle to explain");
        assert!(
            from_a_build.contains("dev.contexttrace.desktop"),
            "{from_a_build:?} does not say which identity failed"
        );
        assert!(
            from_a_build.contains("C:\\work\\target\\debug"),
            "{from_a_build:?} does not say where it is running from"
        );

        // An install location is not a fact worth reciting: it explains
        // nothing about why the id is unregistered, so it is left out.
        let from_an_install = Deliverability::Unregistered {
            app_id: "dev.contexttrace.desktop".into(),
            exe_dir: Some("C:\\Users\\me\\AppData\\Local\\ContextTrace".into()),
        }
        .obstacle()
        .expect("an obstacle to explain");
        assert!(!from_an_install.contains("AppData"), "{from_an_install:?}");
    }

    #[test]
    fn an_undeliverable_build_fails_before_windows_is_asked() {
        let status = deliver(
            &Deliverability::Unregistered {
                app_id: "dev.contexttrace.desktop".into(),
                exe_dir: None,
            },
            "Context compacted",
            "Compaction reclaimed prompt space.",
        );
        let ct_domain::OsDeliveryStatus::Failed { reason } = status else {
            panic!("an unregistered app id cannot deliver");
        };
        assert!(reason.contains("Install ContextTrace once"), "{reason:?}");
    }

    /// The frontend validates this payload before rendering it, and rejects
    /// the whole notification status when a key is missing -- which took the
    /// settings panel down with it, because one failed load nulled all three.
    ///
    /// `rename_all` on an *enum* renames its variants, not the fields inside
    /// them, so the container attribute alone left `app_id` and `exe_dir` in
    /// snake_case on the wire while every other DTO spoke camelCase. Asserted
    /// on the serialised keys rather than on a round trip, because Rust can
    /// deserialise its own snake_case happily; only the TypeScript reader
    /// could tell the difference.
    #[test]
    fn deliverability_states_reach_the_frontend_in_camel_case() {
        let ready = serde_json::to_value(Deliverability::Ready {
            app_id: "dev.contexttrace.desktop".into(),
        })
        .expect("serialisable");
        assert_eq!(ready["state"], "ready");
        assert_eq!(ready["appId"], "dev.contexttrace.desktop");

        let unregistered = serde_json::to_value(Deliverability::Unregistered {
            app_id: "dev.contexttrace.desktop".into(),
            exe_dir: Some("C:\\work\\target\\debug".into()),
        })
        .expect("serialisable");
        assert_eq!(unregistered["state"], "unregistered");
        assert_eq!(unregistered["appId"], "dev.contexttrace.desktop");
        assert_eq!(unregistered["exeDir"], "C:\\work\\target\\debug");

        // The absent directory stays an explicit null: the reader accepts
        // `string | null` and would reject the key being missing entirely.
        let no_dir = serde_json::to_value(Deliverability::Unregistered {
            app_id: "dev.contexttrace.desktop".into(),
            exe_dir: None,
        })
        .expect("serialisable");
        assert!(no_dir.get("exeDir").is_some_and(serde_json::Value::is_null));
    }

    #[test]
    fn cargo_target_layouts_are_recognised_and_lookalikes_are_not() {
        assert!(is_cargo_target_dir(Path::new("C:\\work\\target\\debug")));
        assert!(is_cargo_target_dir(Path::new("C:\\work\\target\\release")));
        assert!(
            !is_cargo_target_dir(Path::new("C:\\Users\\me\\AppData\\Local\\ContextTrace")),
            "the installed location is not a build directory"
        );
        assert!(
            !is_cargo_target_dir(Path::new("C:\\target\\release of tools")),
            "'target' earlier in the path is not a Cargo layout"
        );
        assert!(
            !is_cargo_target_dir(Path::new("C:\\work\\debug")),
            "a debug directory with no target parent is not a Cargo layout"
        );
    }

    #[test]
    fn utf16_needles_are_found_in_shortcut_bytes() {
        let app_id = "dev.contexttrace.desktop";
        let encoded: Vec<u8> = app_id.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let mut shortcut = vec![0x4c, 0x00, 0x00, 0x00];
        shortcut.extend_from_slice(&encoded);
        shortcut.extend_from_slice(&[0, 0, 0]);
        assert!(contains(&shortcut, &encoded));
        assert!(
            !contains(&shortcut, app_id.as_bytes()),
            "the id is stored as UTF-16, so an ASCII search would miss it"
        );
        assert!(!contains(&[], &encoded));
    }
}
