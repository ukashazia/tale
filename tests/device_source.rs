//! Which tailnet `:devices` is showing, and who decides.

use std::fs;

mod common;

use tale::action::ActionId;
use tale::app::{App, DeviceViewSource, Route, SourceAlignment};
use tale::cli::Cli;
use tale::config::{self, EnvironmentValues};
use tale::domain::device::{magic_dns_suffix, same_tailnet};
use tale::paths::{PathEnvironment, Platform};

fn app_with_profile(name: &str) -> Option<App> {
    let root =
        std::env::temp_dir().join(format!("tale-device-source-{}-{name}", std::process::id()));
    let _ = fs::create_dir_all(&root);
    let config_path = root.join("config.toml");
    fs::write(
        &config_path,
        "[profiles.ops]\ntailnet = \"-\"\nread_only = true\ncredential = \"ops\"\ncredential_backend = \"file\"\ncredential_file = \"credentials.toml\"\n",
    )
    .ok()?;
    let cli = Cli {
        command: None,
        profile: Some("ops".to_owned()),
        config: Some(config_path),
        view: None,
        read_only: true,
        no_local: false,
        tailscale_path: Some(std::path::PathBuf::from("tailscale")),
        tailscale_socket: None,
        mock: false,
    };
    let environment = EnvironmentValues {
        config_file: None,
        tailscale_path: None,
        tailscale_socket: None,
        no_color: true,
    };
    let path_environment = PathEnvironment {
        platform: Platform::Unix,
        current_dir: root.clone(),
        xdg_config_home: Some(root.join("config")),
        home: Some(root.join("home")),
        xdg_state_home: Some(root.join("state")),
        xdg_cache_home: Some(root.join("cache")),
        appdata: None,
        localappdata: None,
    };
    config::resolve(&cli, &environment, &path_environment)
        .ok()
        .map(App::new)
}

/// A client connected to `suffix`, carrying one peer besides itself.
fn install_local(app: &mut App, suffix: &str) {
    common::install_local(app, suffix, &["local-self", "local-peer"]);
}

use common::{admin_device, install_admin};

fn row_ids(app: &App) -> Vec<String> {
    app.devices_resource
        .snapshot
        .iter()
        .map(|device| device.id.0.clone())
        .collect()
}

/// The suffix is what identifies a tailnet, and an unqualified name identifies
/// nothing rather than matching everything.
#[test]
fn a_tailnet_is_identified_by_its_magic_dns_suffix() {
    assert_eq!(
        magic_dns_suffix("host.tailc0de.ts.net"),
        Some("tailc0de.ts.net")
    );
    assert_eq!(
        magic_dns_suffix("host.tailc0de.ts.net."),
        Some("tailc0de.ts.net")
    );
    assert_eq!(magic_dns_suffix("host"), None);
    assert!(same_tailnet("Tailc0de.ts.net", "tailc0de.ts.net."));
    assert!(!same_tailnet("tailc0de.ts.net", "other.ts.net"));
    assert!(!same_tailnet("", ""));
}

/// The bug: a client on one tailnet and a profile on another produced a union
/// of both fleets under one heading, and which half showed depended on whichever
/// poll answered last.
#[test]
fn two_tailnets_are_never_merged() {
    let Some(mut app) = app_with_profile("divergent") else {
        return;
    };
    install_local(&mut app, "home.ts.net");
    install_admin(&mut app, vec![admin_device("admin-1", "work.ts.net")]);

    assert!(matches!(
        app.source_alignment(),
        SourceAlignment::Divergent { .. }
    ));
    assert_eq!(app.device_view_source(), DeviceViewSource::Admin);
    // The activated profile owns the list, and the local tailnet is absent from
    // it rather than mixed into it.
    assert_eq!(row_ids(&app), vec!["admin-1".to_owned()]);
}

/// The same two sources on one tailnet still compose: that is what the ID match
/// is for, and it is the only case where it means anything.
#[test]
fn one_tailnet_still_composes() {
    let Some(mut app) = app_with_profile("same") else {
        return;
    };
    install_local(&mut app, "home.ts.net");
    install_admin(
        &mut app,
        vec![
            admin_device("local-self", "home.ts.net"),
            admin_device("admin-only", "home.ts.net"),
        ],
    );

    assert_eq!(app.source_alignment(), SourceAlignment::SameTailnet);
    assert_eq!(app.device_view_source(), DeviceViewSource::Composed);
    let ids = row_ids(&app);
    // The local rows first, enriched where the admin list agrees, then whatever
    // only the admin list knows about.
    assert_eq!(ids.len(), 3);
    assert!(ids.contains(&"local-self".to_owned()));
    assert!(ids.contains(&"local-peer".to_owned()));
    assert!(ids.contains(&"admin-only".to_owned()));
}

/// An unproven match is not a match. Before the admin list has named its
/// tailnet there is nothing to compare, so nothing is composed on a guess.
#[test]
fn an_unidentified_tailnet_is_not_assumed_to_match() {
    let Some(mut app) = app_with_profile("unknown") else {
        return;
    };
    install_local(&mut app, "home.ts.net");
    assert_eq!(app.source_alignment(), SourceAlignment::Undetermined);
    assert_eq!(app.device_view_source(), DeviceViewSource::Admin);
}

/// A node shared in from another tailnet carries that tailnet's suffix. Letting
/// it speak for the tailnet being read would make every shared node a mismatch.
#[test]
fn a_shared_in_node_does_not_identify_the_tailnet() {
    let Some(mut app) = app_with_profile("shared") else {
        return;
    };
    install_local(&mut app, "home.ts.net");
    let mut external = admin_device("external-1", "other.ts.net");
    external.is_external = Some(true);
    install_admin(
        &mut app,
        vec![external, admin_device("local-self", "home.ts.net")],
    );
    assert_eq!(app.source_alignment(), SourceAlignment::SameTailnet);
}

/// Ping, whois, SSH and Taildrop all go through the local daemon. Offering them
/// against a tailnet it is not on offers something that cannot work.
#[test]
fn local_row_actions_are_withheld_for_an_unreachable_tailnet() {
    let Some(mut app) = app_with_profile("actions") else {
        return;
    };
    install_local(&mut app, "home.ts.net");
    app.set_route(Route::Devices);

    install_admin(&mut app, vec![admin_device("a", "work.ts.net")]);
    let actions = app.contextual_actions();
    for withheld in [
        ActionId::LocalProbeConnection,
        ActionId::LocalWhois,
        ActionId::LocalSshOpen,
        ActionId::DevicesTaildropSend,
    ] {
        assert!(
            !actions.contains(&withheld),
            "{withheld:?} was still offered"
        );
    }
    // The tailnet's own verbs are unaffected: those go over the API.
    assert!(actions.contains(&ActionId::AdminDeviceRename));

    install_admin(&mut app, vec![admin_device("a", "home.ts.net")]);
    let actions = app.contextual_actions();
    assert!(actions.contains(&ActionId::LocalSshOpen));
    assert!(actions.contains(&ActionId::DevicesTaildropSend));
}

/// The regression itself: repeated arrivals from either source must not change
/// what is on screen. It used to alternate with whichever answered last.
#[test]
fn repeated_refreshes_do_not_change_the_list() {
    let Some(mut app) = app_with_profile("stable") else {
        return;
    };
    install_local(&mut app, "home.ts.net");
    install_admin(&mut app, vec![admin_device("admin-1", "work.ts.net")]);
    let settled = row_ids(&app);

    for _ in 0..4 {
        install_local(&mut app, "home.ts.net");
        assert_eq!(row_ids(&app), settled, "a local poll changed the list");
        install_admin(&mut app, vec![admin_device("admin-1", "work.ts.net")]);
        assert_eq!(row_ids(&app), settled, "an admin refresh changed the list");
    }
}

/// With no profile there is one source and no question to answer.
#[test]
fn without_a_profile_the_local_client_owns_the_list() {
    let Some(mut app) = app_with_profile("local-only") else {
        return;
    };
    install_local(&mut app, "home.ts.net");
    install_admin(&mut app, vec![admin_device("admin-1", "work.ts.net")]);
    let _ = app.switch_profile(None);

    assert_eq!(app.source_alignment(), SourceAlignment::Single);
    assert_eq!(app.device_view_source(), DeviceViewSource::Local);
    let ids = row_ids(&app);
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&"local-self".to_owned()));
}
