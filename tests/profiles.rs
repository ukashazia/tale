use std::fs;

use tale::action::ActionId;
use tale::app::{App, ProfileRow, Route};
use tale::cli::Cli;
use tale::config::{self, EnvironmentValues};
use tale::domain::profile::{CredentialPresence, ProbeState};
use tale::effect::Effect;
use tale::event::{CredentialEvent, Event};
use tale::paths::{PathEnvironment, Platform};
use tale::secrets::CredentialKind;

/// Two profiles and no request to activate either: what a session looks like
/// before anybody has chosen a source.
fn profiles_app(name: &str) -> Option<App> {
    let root = std::env::temp_dir().join(format!("tale-profiles-{}-{name}", std::process::id()));
    let _ = fs::create_dir_all(&root);
    let config_path = root.join("config.toml");
    fs::write(
        &config_path,
        "[profiles.ops]\ntailnet = \"-\"\ncredential = \"ops\"\ncredential_backend = \"file\"\ncredential_file = \"credentials.toml\"\n\
         [profiles.audit]\ntailnet = \"example.test\"\nread_only = true\ncredential = \"audit\"\ncredential_backend = \"file\"\ncredential_file = \"credentials.toml\"\n",
    )
    .ok()?;
    let cli = Cli {
        command: None,
        profile: None,
        config: Some(config_path),
        view: None,
        read_only: false,
        no_local: true,
        tailscale_path: None,
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

fn inspected(app: &mut App, presences: Vec<(String, CredentialPresence)>) {
    let _ = app.update(Event::Credential(Box::new(
        CredentialEvent::ProfilesInspected { presences },
    )));
}

fn stored() -> CredentialPresence {
    CredentialPresence::Stored {
        kind: CredentialKind::AccessToken,
        scopes: vec!["devices:core:read".to_owned()],
    }
}

fn select(app: &mut App, label: &str) {
    app.set_route(Route::Profiles);
    let index = app
        .profile_rows()
        .iter()
        .position(|row| row.label() == label);
    assert!(index.is_some(), "no row labelled {label}");
    if let Some(index) = index {
        app.views.profiles.selected = index;
    }
}

fn activate(app: &mut App) -> Vec<Effect> {
    app.dispatch_action(ActionId::ProfileActivate)
}

/// A configured credential does not make a session administer anything. Tale
/// starts on the client in front of it and waits to be pointed elsewhere.
#[test]
fn a_session_starts_on_the_local_client() {
    let app = profiles_app("start");
    assert!(app.is_some());
    let Some(app) = app else { return };
    assert!(app.admin.profile.is_none());
    let rows = app.profile_rows();
    assert_eq!(rows.len(), 3);
    assert!(matches!(rows.first(), Some(ProfileRow::Local { .. })));
    assert_eq!(rows.first().map(ProfileRow::label), Some("local"));
    assert!(rows.first().is_some_and(ProfileRow::active));
    assert!(!rows[1..].iter().any(ProfileRow::active));
}

/// The store's answer is what the page reports until somebody asks for more,
/// and it costs no request to know it.
#[test]
fn the_page_reports_the_store_before_it_reports_the_control_plane() {
    let app = profiles_app("store");
    assert!(app.is_some());
    let Some(mut app) = app else { return };
    assert!(
        app.profile_rows()
            .iter()
            .filter(|row| row.name().is_some())
            .all(|row| row.state_label() == "reading")
    );

    inspected(
        &mut app,
        vec![
            ("ops".to_owned(), stored()),
            ("audit".to_owned(), CredentialPresence::Missing),
        ],
    );
    let rows = app.profile_rows();
    let state = |label: &str| {
        rows.iter()
            .find(|row| row.label() == label)
            .map(ProfileRow::state_label)
    };
    // Stored but never verified, because nobody has asked for it yet.
    assert_eq!(state("ops"), Some("unverified"));
    assert_eq!(state("audit"), Some("missing"));
}

/// Nothing is sent for a profile that has no secret to send. The page says what
/// to do about it instead of spending a request to be told the same thing.
#[test]
fn a_profile_without_a_credential_is_not_probed() {
    let app = profiles_app("missing");
    assert!(app.is_some());
    let Some(mut app) = app else { return };
    inspected(
        &mut app,
        vec![("audit".to_owned(), CredentialPresence::Missing)],
    );
    select(&mut app, "audit");
    let effects = activate(&mut app);
    assert!(effects.is_empty());
    assert!(app.admin.profile.is_none());
    assert!(
        app.runtime_error
            .as_deref()
            .is_some_and(|error| error.contains("tale auth add audit"))
    );
}

/// Activation asks the control plane first and switches only on an answer.
#[test]
fn activation_probes_before_it_switches() {
    let app = profiles_app("probe");
    assert!(app.is_some());
    let Some(mut app) = app else { return };
    inspected(&mut app, vec![("ops".to_owned(), stored())]);
    select(&mut app, "ops");

    let effects = activate(&mut app);
    assert_eq!(effects.len(), 1);
    assert!(matches!(
        effects.first(),
        Some(Effect::StartProfileProbe { profile, credential, .. })
            if profile == "ops" && credential == "ops"
    ));
    // Nothing is active yet: the probe has not answered.
    assert!(app.admin.profile.is_none());
    assert!(
        app.profile_statuses
            .get("ops")
            .is_some_and(|status| status.probe == ProbeState::InFlight)
    );

    let _ = app.update(Event::Credential(Box::new(
        CredentialEvent::ProfileProbed {
            profile: "ops".to_owned(),
            result: Ok(CredentialKind::AccessToken),
        },
    )));
    assert_eq!(app.admin.profile.as_deref(), Some("ops"));
    assert_eq!(
        app.profile_rows()
            .iter()
            .find(|row| row.label() == "ops")
            .map(ProfileRow::state_label),
        Some("reachable")
    );
}

/// A credential the control plane rejects leaves the session where it was, with
/// the reason on the row rather than in a notification that scrolls away.
#[test]
fn a_rejected_probe_does_not_activate_anything() {
    let app = profiles_app("rejected");
    assert!(app.is_some());
    let Some(mut app) = app else { return };
    inspected(&mut app, vec![("ops".to_owned(), stored())]);
    select(&mut app, "ops");
    let _ = activate(&mut app);

    let _ = app.update(Event::Credential(Box::new(
        CredentialEvent::ProfileProbed {
            profile: "ops".to_owned(),
            result: Err("the credential was rejected by the Tailscale API".to_owned()),
        },
    )));
    assert!(app.admin.profile.is_none());
    let rows = app.profile_rows();
    let row = rows.iter().find(|row| row.label() == "ops");
    assert_eq!(row.map(ProfileRow::state_label), Some("rejected"));
    assert!(
        row.and_then(ProfileRow::detail)
            .is_some_and(|detail| detail.contains("rejected by the Tailscale API"))
    );
    assert!(rows.first().is_some_and(ProfileRow::active));
}

/// A verdict for an attempt the user has moved on from cannot activate anything.
#[test]
fn a_superseded_probe_is_ignored() {
    let app = profiles_app("superseded");
    assert!(app.is_some());
    let Some(mut app) = app else { return };
    inspected(
        &mut app,
        vec![("ops".to_owned(), stored()), ("audit".to_owned(), stored())],
    );
    select(&mut app, "ops");
    let _ = activate(&mut app);
    select(&mut app, "audit");
    let _ = activate(&mut app);

    let _ = app.update(Event::Credential(Box::new(
        CredentialEvent::ProfileProbed {
            profile: "ops".to_owned(),
            result: Ok(CredentialKind::AccessToken),
        },
    )));
    assert!(app.admin.profile.is_none());
}

/// Selecting the local row is how a session stops administering a tailnet. It
/// needs no credential and asks nothing of anybody.
#[test]
fn activating_the_local_row_deactivates_the_profile() {
    let app = profiles_app("clear");
    assert!(app.is_some());
    let Some(mut app) = app else { return };
    inspected(&mut app, vec![("ops".to_owned(), stored())]);
    select(&mut app, "ops");
    let _ = activate(&mut app);
    let _ = app.update(Event::Credential(Box::new(
        CredentialEvent::ProfileProbed {
            profile: "ops".to_owned(),
            result: Ok(CredentialKind::AccessToken),
        },
    )));
    assert_eq!(app.admin.profile.as_deref(), Some("ops"));

    select(&mut app, "local");
    let effects = activate(&mut app);
    assert!(app.admin.profile.is_none());
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::StartProfileProbe { .. }))
    );
}

/// The local client is where Tale starts, so it stays at the top whatever the
/// sort says; only the profiles are ordered.
#[test]
fn the_local_row_is_pinned_above_the_sort() {
    let app = profiles_app("sort");
    assert!(app.is_some());
    let Some(mut app) = app else { return };
    let labels = |app: &App| {
        app.profile_rows()
            .iter()
            .map(|row| row.label().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(labels(&app), vec!["local", "audit", "ops"]);

    app.views.profiles.sort.direction = tale::domain::device::SortDirection::Descending;
    assert_eq!(labels(&app), vec!["local", "ops", "audit"]);
}

/// Removing a stored credential is a fact about the store, so the page records
/// it whether or not the profile happened to be the active one.
#[test]
fn removing_a_credential_updates_a_profile_that_is_not_active() {
    let app = profiles_app("removed");
    assert!(app.is_some());
    let Some(mut app) = app else { return };
    inspected(&mut app, vec![("audit".to_owned(), stored())]);
    let _ = app.update(Event::Credential(Box::new(CredentialEvent::LocalRemoved {
        profile: "audit".to_owned(),
        reference: "audit".to_owned(),
        result: Ok(true),
    })));
    assert_eq!(
        app.profile_rows()
            .iter()
            .find(|row| row.label() == "audit")
            .map(ProfileRow::state_label),
        Some("missing")
    );
}

/// The route exists under its own name and carries the page.
#[test]
fn the_route_is_addressable() {
    assert_eq!(Route::parse("profiles"), Some(Route::Profiles));
    assert_eq!(Route::Profiles.label(), "profiles");
}
