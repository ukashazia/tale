use std::fs;

mod common;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use tale::action::{self, ActionId};
use tale::app::{App, CopyField, InteractionMode, ProfileRow, Route};
use tale::cli::Cli;
use tale::config::{self, EnvironmentValues};
use tale::domain::profile::{CredentialPresence, ProbeState};
use tale::effect::Effect;
use tale::event::{CredentialEvent, Event, InputEvent};
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

fn press(app: &mut App, code: KeyCode) {
    let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
        code,
        KeyModifiers::NONE,
    ))));
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

/// Everything `:devices` offers on a row, `:profiles` offers on a row. The page
/// shipped with a route and an action but no menus, because an action with no
/// transient sequence makes the whole menu refuse to open rather than showing a
/// gap — the failure is silent from the keyboard.
#[test]
fn the_actions_menu_opens_and_carries_the_page_verb() {
    let Some(mut app) = profiles_app("actions") else {
        return;
    };
    inspected(&mut app, vec![("ops".to_owned(), stored())]);
    select(&mut app, "ops");

    let actions = app.contextual_actions();
    assert!(actions.contains(&ActionId::ProfileActivate));
    assert!(
        action::validate_transient_sequences(&actions).is_ok(),
        "every visible action needs a sequence or the menu will not open"
    );

    let _ = app.dispatch_action(ActionId::ResourceActions);
    assert!(matches!(app.interaction, InteractionMode::Transient(_)));
    assert!(app.runtime_error.is_none());
}

/// Saved views and exports describe collections Tale fetched. This page lists
/// the user's own configuration, so it offers neither rather than offering an
/// export that would write somebody else's rows.
#[test]
fn the_actions_menu_offers_nothing_that_has_no_subject_here() {
    let Some(mut app) = profiles_app("no-export") else {
        return;
    };
    select(&mut app, "local");
    let actions = app.contextual_actions();
    for absent in [
        ActionId::CollectionExport,
        ActionId::SavedViewCreate,
        ActionId::SavedViewApply,
    ] {
        assert!(!actions.contains(&absent), "{absent:?} has no subject here");
    }
}

/// The copy menu offers what the row actually has: a profile's credential
/// reference and store path, and for the local client its account instead.
#[test]
fn the_copy_menu_offers_the_selected_rows_own_facts() {
    let Some(mut app) = profiles_app("copy") else {
        return;
    };
    inspected(&mut app, vec![("ops".to_owned(), stored())]);

    select(&mut app, "ops");
    let fields = app.contextual_copy_fields();
    assert!(fields.contains(&CopyField::ProfileName));
    assert!(fields.contains(&CopyField::ProfileTailnet));
    assert!(fields.contains(&CopyField::ProfileCredential));
    assert!(fields.contains(&CopyField::ProfileBackend));
    // The local client has no credential to name.
    assert!(!fields.contains(&CopyField::ProfileAccount));

    let _ = app.dispatch_action(ActionId::ResourceCopy);
    assert!(matches!(app.interaction, InteractionMode::Transient(_)));

    app.interaction = InteractionMode::Normal;
    select(&mut app, "local");
    let fields = app.contextual_copy_fields();
    assert!(fields.contains(&CopyField::ProfileName));
    assert!(!fields.contains(&CopyField::ProfileCredential));
    assert!(!fields.contains(&CopyField::ProfileBackend));
}

/// Copying takes the value from the selected row, not from a remembered one.
#[test]
fn copying_a_field_takes_it_from_the_selected_row() {
    let Some(mut app) = profiles_app("copy-value") else {
        return;
    };
    inspected(&mut app, vec![("audit".to_owned(), stored())]);
    select(&mut app, "audit");
    let _ = app.dispatch_action(ActionId::ResourceCopy);
    // `t` is the tailnet, which for this profile is a name rather than `-`.
    let effects = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
        KeyCode::Char('t'),
        KeyModifiers::NONE,
    ))));
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::CopyText { text } if text == "example.test")),
        "the copy did not carry the selected row's tailnet: {effects:?}"
    );
}

/// `/` used to open with the device grammar and then filter nothing, so a query
/// that looked accepted changed no rows. The page has no fields; the whole row
/// is the haystack, the way `:tasks` works.
#[test]
fn the_filter_narrows_the_rows_it_offered_to_narrow() {
    let Some(mut app) = profiles_app("filter") else {
        return;
    };
    inspected(
        &mut app,
        vec![
            ("ops".to_owned(), stored()),
            ("audit".to_owned(), CredentialPresence::Missing),
        ],
    );
    app.set_route(Route::Profiles);
    assert!(!app.filter_schema().free_text.is_empty());

    let _ = app.dispatch_action(ActionId::ViewFilter);
    assert!(
        matches!(app.interaction, InteractionMode::FilterLine(_)),
        "the filter refused to open: {:?}",
        app.runtime_error
    );
    let _ = app.update(Event::Input(InputEvent::Paste("audit".to_owned())));
    press(&mut app, KeyCode::Enter);

    let labels = app
        .profile_rows()
        .iter()
        .map(|row| row.label().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(labels, vec!["audit".to_owned()]);
    // The total behind the filter is still reported, and so is the active row
    // even though the filter hid it.
    assert_eq!(app.all_profile_rows().len(), 3);

    // A term matching a column other than the name still finds its row.
    app.views.profiles.filter = "missing".to_owned();
    assert_eq!(
        app.profile_rows()
            .iter()
            .map(|row| row.label().to_owned())
            .collect::<Vec<_>>(),
        vec!["audit".to_owned()]
    );
}

/// Esc puts back the rows the filter was narrowing, and the cursor with them.
#[test]
fn escaping_the_filter_restores_the_rows() {
    let Some(mut app) = profiles_app("filter-esc") else {
        return;
    };
    select(&mut app, "ops");
    let before = app.views.profiles.selected;

    let _ = app.dispatch_action(ActionId::ViewFilter);
    let _ = app.update(Event::Input(InputEvent::Paste("audit".to_owned())));
    assert_eq!(app.profile_rows().len(), 1);

    press(&mut app, KeyCode::Esc);
    assert!(app.views.profiles.filter.is_empty());
    assert_eq!(app.profile_rows().len(), 3);
    assert_eq!(app.views.profiles.selected, before);
}

/// The activation acts on the row the filter left selected, not on the one that
/// was selected before it narrowed.
#[test]
fn activation_follows_the_filtered_selection() {
    let Some(mut app) = profiles_app("filter-activate") else {
        return;
    };
    inspected(&mut app, vec![("ops".to_owned(), stored())]);
    app.set_route(Route::Profiles);
    app.views.profiles.filter = "ops".to_owned();
    app.views.profiles.selected = 0;

    assert_eq!(
        app.selected_profile_row().map(|row| row.label().to_owned()),
        Some("ops".to_owned())
    );
    let effects = activate(&mut app);
    assert!(matches!(
        effects.first(),
        Some(Effect::StartProfileProbe { profile, .. }) if profile == "ops"
    ));
}

fn local_capable_app(name: &str) -> Option<App> {
    let root = std::env::temp_dir().join(format!("tale-hdr-{}-{name}", std::process::id()));
    let _ = fs::create_dir_all(&root);
    let config_path = root.join("config.toml");
    fs::write(
        &config_path,
        "[profiles.ops]\ntailnet = \"-\"\ncredential = \"ops\"\ncredential_backend = \"file\"\ncredential_file = \"credentials.toml\"\n",
    )
    .ok()?;
    let cli = Cli {
        command: None,
        profile: None,
        config: Some(config_path),
        view: None,
        read_only: false,
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

fn header_lines(app: &App) -> Vec<String> {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let (w, h) = (120u16, 30u16);
    let backend = TestBackend::new(w, h);
    let Ok(mut terminal) = Terminal::new(backend) else {
        return Vec::new();
    };
    if terminal.draw(|frame| tale::ui::render(frame, app)).is_err() {
        return Vec::new();
    }
    let buffer = terminal.backend().buffer();
    (0..6)
        .map(|y| {
            (0..w)
                .filter_map(|x| buffer.cell((x, y)).map(|cell| cell.symbol().to_owned()))
                .collect::<String>()
        })
        .collect()
}

fn header_row<'a>(lines: &'a [String], label: &str) -> Option<&'a str> {
    lines
        .iter()
        .find_map(|line| line.split_once(label))
        .map(|(_, rest)| rest.trim_end())
}

/// The header showed the local client's tailnet and called it the session's,
/// so a profile administering a different one was invisible. Both are named
/// now, and each says which it is.
#[test]
fn the_header_names_both_tailnets_when_they_differ() {
    let Some(mut app) = local_capable_app("header-divergent") else {
        return;
    };
    common::install_local(&mut app, "home.ts.net", &["self"]);
    inspected(&mut app, vec![("ops".to_owned(), stored())]);
    select(&mut app, "ops");
    let _ = activate(&mut app);
    let _ = app.update(Event::Credential(Box::new(
        CredentialEvent::ProfileProbed {
            profile: "ops".to_owned(),
            result: Ok(CredentialKind::AccessToken),
        },
    )));
    common::install_admin(&mut app, vec![common::admin_device("a", "work.ts.net")]);

    let lines = header_lines(&app);
    let local = header_row(&lines, "Local:");
    let profile = header_row(&lines, "Profile:");
    assert!(
        local.is_some_and(|row| row.contains("home.ts.net")),
        "the local row does not name the local tailnet: {local:?}"
    );
    assert!(
        profile.is_some_and(|row| row.contains("ops") && row.contains("work.ts.net")),
        "the profile row does not name the profile's tailnet: {profile:?}"
    );
    // Neither row may carry the other's tailnet, which is what made the old
    // single line unreadable.
    assert!(local.is_some_and(|row| !row.contains("work.ts.net")));
    assert!(profile.is_some_and(|row| !row.contains("home.ts.net")));
}

/// With no profile the row says so rather than going blank, because an empty
/// admin view and an unchosen profile look identical otherwise.
#[test]
fn the_header_says_when_no_profile_is_active() {
    let Some(mut app) = local_capable_app("header-none") else {
        return;
    };
    common::install_local(&mut app, "home.ts.net", &["self"]);
    let lines = header_lines(&app);
    assert!(header_row(&lines, "Local:").is_some_and(|row| row.contains("home.ts.net")));
    assert!(header_row(&lines, "Profile:").is_some_and(|row| row.contains("none")));
}

/// A tailnet named after its own suffix is one fact, not two.
#[test]
fn the_header_does_not_print_one_tailnet_twice() {
    let Some(mut app) = local_capable_app("header-once") else {
        return;
    };
    common::install_local(&mut app, "solo.ts.net", &["self"]);
    let lines = header_lines(&app);
    let local = header_row(&lines, "Local:").unwrap_or_default();
    assert_eq!(local.matches("solo.ts.net").count(), 1, "{local:?}");
}
