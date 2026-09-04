use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Modifier;

mod common;

use tale::action::{
    self, ActionContext, ActionId, Binding, TaskOrigin, TaskPresentation, TaskTrigger,
};
use tale::app::{App, DiagnosticsSection, InteractionMode, Overlay, Route, TaskOverlayState};
use tale::cli::Cli;
use tale::config::{self, EnvironmentValues};
use tale::domain::account::{LocalAccount, LocalSection};
use tale::domain::policy_workflow::{
    PolicyDocument, PolicyPreview, PolicySelectorType, PolicyWorkflow,
};
use tale::domain::service::{ServiceActionRequest, ServiceSection};
use tale::domain::source::{ExecutableSource, LocalCapabilities, LocalExecutable};
use tale::effect::Effect;
use tale::event::{Event, InputEvent, PolicyEvent, SourceEvent};
use tale::mock;
use tale::paths::{PathEnvironment, Platform};
use tale::ui;
use tale::ui::theme::{ColorCapability, StyleRole, Theme, ThemeId};

fn mock_app() -> Option<App> {
    let root = std::path::PathBuf::from("/fictional/tale-actions");
    let cli = Cli {
        command: None,
        profile: None,
        config: Some(root.join("missing.toml")),
        view: None,
        read_only: false,
        no_local: false,
        tailscale_path: None,
        tailscale_socket: None,
        mock: true,
    };
    let environment = EnvironmentValues {
        config_file: None,
        tailscale_path: None,
        tailscale_socket: None,
        no_color: false,
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

#[test]
fn task_presentation_uses_action_origin_and_trigger() {
    let origin = |route, diagnostics_section, service_section| TaskOrigin {
        route,
        diagnostics_section,
        service_section,
    };
    let devices = origin(Route::Devices, None, None);
    let dns = origin(Route::Dns, None, None);
    let diagnostics_client = origin(Route::Diagnostics, Some(DiagnosticsSection::Client), None);
    let diagnostics_dns = origin(
        Route::Diagnostics,
        Some(DiagnosticsSection::DnsStatus),
        None,
    );
    let services = origin(Route::Services, None, Some(ServiceSection::Serve));
    let explicit = TaskTrigger::Explicit;

    for (action_id, action_origin, expected) in [
        (
            ActionId::LocalProbeConnection,
            devices,
            TaskPresentation::Overlay,
        ),
        (
            ActionId::LocalNetcheck,
            diagnostics_client,
            TaskPresentation::Overlay,
        ),
        (
            ActionId::LocalNetcheckLive,
            diagnostics_client,
            TaskPresentation::Overlay,
        ),
        (ActionId::LocalWhois, devices, TaskPresentation::Overlay),
        (ActionId::LocalDnsStatus, dns, TaskPresentation::Background),
        (
            ActionId::LocalDnsStatus,
            diagnostics_dns,
            TaskPresentation::Background,
        ),
        (ActionId::LocalDnsStatus, devices, TaskPresentation::Overlay),
        (ActionId::LocalDnsQuery, dns, TaskPresentation::Background),
        (ActionId::LocalDnsQuery, devices, TaskPresentation::Overlay),
        (
            ActionId::DevicesTaildropSend,
            devices,
            TaskPresentation::Overlay,
        ),
        (
            ActionId::DevicesTaildropReceive,
            devices,
            TaskPresentation::Overlay,
        ),
        (
            ActionId::ServicesServeCreate,
            services,
            TaskPresentation::Background,
        ),
        (
            ActionId::ServicesMetricsRefresh,
            diagnostics_client,
            TaskPresentation::Background,
        ),
        (
            ActionId::ServicesBugReportCreate,
            diagnostics_client,
            TaskPresentation::Background,
        ),
        (
            ActionId::LocalConnect,
            origin(Route::Local, None, None),
            TaskPresentation::Background,
        ),
        (
            ActionId::AdminDeviceRename,
            devices,
            TaskPresentation::Background,
        ),
        (
            ActionId::AdminRoutesReplaceApprovals,
            origin(Route::Routes, None, None),
            TaskPresentation::Overlay,
        ),
        (
            ActionId::LocalAccountLogin,
            origin(Route::Local, None, None),
            TaskPresentation::TerminalHandoff,
        ),
        (
            ActionId::LocalAccountLogout,
            origin(Route::Local, None, None),
            TaskPresentation::TerminalHandoff,
        ),
        (
            ActionId::LocalSshOpen,
            devices,
            TaskPresentation::TerminalHandoff,
        ),
        (
            ActionId::LocalNcOpen,
            devices,
            TaskPresentation::TerminalHandoff,
        ),
        (
            ActionId::MockSuccess,
            devices,
            TaskPresentation::OpenInspector,
        ),
        (
            ActionId::MockFailure,
            devices,
            TaskPresentation::OpenInspector,
        ),
        (
            ActionId::MockCancellable,
            devices,
            TaskPresentation::OpenInspector,
        ),
        (
            ActionId::MockNonCancellable,
            devices,
            TaskPresentation::OpenInspector,
        ),
    ] {
        assert_eq!(
            action_id.task_presentation(action_origin, explicit),
            expected,
            "action: {action_id:?}, origin: {action_origin:?}"
        );
    }

    for action_id in [
        ActionId::LocalConnect,
        ActionId::LocalDisconnect,
        ActionId::LocalPreferencesEdit,
        ActionId::LocalExitNodeSelect,
        ActionId::LocalRoutesEditAdvertisements,
        ActionId::LocalAccountSwitch,
        ActionId::LocalAccountRemove,
        ActionId::LocalSyspolicyReload,
        ActionId::ServicesServeCreate,
        ActionId::ServicesServeEdit,
        ActionId::ServicesServeRemove,
        ActionId::ServicesServeReset,
        ActionId::ServicesFunnelCreate,
        ActionId::ServicesFunnelEdit,
        ActionId::ServicesFunnelPublish,
        ActionId::ServicesFunnelUnpublish,
        ActionId::ServicesFunnelReset,
        ActionId::ServicesDriveShare,
        ActionId::ServicesDriveRename,
        ActionId::ServicesDriveUnshare,
        ActionId::ServicesCertificateObtain,
        ActionId::AdminDeviceRename,
        ActionId::AdminDeviceTagsReplace,
        ActionId::AdminDeviceApprove,
        ActionId::AdminDeviceRevokeApproval,
        ActionId::AdminDeviceKeyExpiryConfigure,
        ActionId::AdminDeviceKeyExpireNow,
        ActionId::AdminDeviceDelete,
        ActionId::AdminDnsPreferencesEdit,
        ActionId::AdminDnsNameserversReplace,
        ActionId::AdminDnsSearchPathsReplace,
        ActionId::AdminDnsSplitCreate,
        ActionId::AdminDnsSplitEdit,
        ActionId::AdminDnsSplitRemove,
        ActionId::AdminUserApprove,
        ActionId::AdminUserRoleChange,
        ActionId::AdminUserSuspend,
        ActionId::AdminUserRestore,
        ActionId::AdminUserDelete,
    ] {
        assert_eq!(
            action_id.task_presentation(services, explicit),
            TaskPresentation::Background,
            "background action: {action_id:?}"
        );
    }

    assert_eq!(
        ActionId::LocalProbeConnection.task_presentation(devices, TaskTrigger::Automatic),
        TaskPresentation::Background
    );
    assert_eq!(
        ActionId::AdminRoutesReplaceApprovals
            .task_presentation(origin(Route::Routes, None, None), TaskTrigger::BatchChild),
        TaskPresentation::Background
    );
}

/// The same app the screenshots come from: a local client, and an admin
/// profile alongside it.
fn local_app(with_admin_profile: bool) -> Option<App> {
    let root = std::env::temp_dir().join(format!(
        "tale-actions-{}-{with_admin_profile}",
        std::process::id()
    ));
    if std::fs::create_dir_all(&root).is_err() {
        return None;
    }
    let config_path = root.join("config.toml");
    if with_admin_profile
        && std::fs::write(
            &config_path,
            "[profiles.audit]\ntailnet = \"example.test\"\nread_only = false\ncredential = \"audit\"\ncredential_backend = \"file\"\ncredential_file = \"credentials.toml\"\n",
        )
        .is_err()
    {
        return None;
    }
    let cli = Cli {
        command: None,
        // A profile is active only when it is asked for; the fixture asks when
        // it is the admin-profile variant it is testing.
        profile: with_admin_profile.then(|| "audit".to_owned()),
        config: Some(if with_admin_profile {
            config_path
        } else {
            root.join("missing.toml")
        }),
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
    let mut app = config::resolve(&cli, &environment, &path_environment)
        .ok()
        .map(App::new)?;
    if with_admin_profile {
        // Both sources on one tailnet, which is what makes the two halves of the
        // devices menu belong to the same rows.
        common::install_aligned_sources(&mut app, "fixture.ts.net", &["node-01", "node-02"]);
        app.views.devices.selected_id = Some(tale::domain::device::DeviceId::new("node-01"));
    }
    Some(app)
}

#[test]
fn a_started_output_operation_opens_a_contextual_task_overlay() {
    let Some(mut app) = local_app(true) else {
        return;
    };
    let capabilities = LocalCapabilities::all_supported();
    app.local_executable = Some(LocalExecutable {
        path: "tailscale".into(),
        socket_path: None,
        source: ExecutableSource::Path,
        version: "1.98.9".to_owned(),
        daemon_version: Some("1.98.9".to_owned()),
        build: None,
        capabilities,
    });
    app.local_capabilities = capabilities;
    app.views.devices.scroll = 1;
    app.views.devices.filter_draft = "node".to_owned();
    let source_selection = app.views.devices.selected_id.clone();

    let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
        KeyCode::Char('a'),
        KeyModifiers::NONE,
    ))));
    let effects = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
        KeyCode::Char('p'),
        KeyModifiers::NONE,
    ))));

    assert!(
        matches!(
            effects.as_slice(),
            [
                Effect::StartLocalDiagnostic { task_id, .. },
                Effect::PersistTaskHistory(tasks),
            ] if app.tasks.selected == Some(*task_id) && tasks.len() == 1
        ),
        "effects: {effects:?}; selected: {:?}; error: {:?}",
        app.tasks.selected,
        app.runtime_error
    );
    assert_eq!(app.current_route(), Route::Devices);
    let task_id = app.tasks.selected;
    assert!(matches!(
        app.overlays.last(),
        Some(Overlay::Task(state)) if Some(state.task_id) == task_id && state.scroll == 0
    ));
    assert!(app.notifications.last().is_some_and(|notice| {
        notice.message == "ping target node-01.fixture.ts.net running · @ view task"
    }));
    assert!(
        tale::action::find_action(ActionId::LocalProbeConnection)
            .is_some_and(|action| action.label == "Ping")
    );

    let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
        KeyCode::Char('@'),
        KeyModifiers::NONE,
    ))));
    assert_eq!(app.current_route(), Route::Tasks);
    assert_eq!(app.focus, tale::app::Focus::Inspector);
    assert_eq!(app.tasks.selected, task_id);

    let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
        KeyCode::Esc,
        KeyModifiers::NONE,
    ))));
    assert_eq!(app.current_route(), Route::Devices);
    assert_eq!(app.focus, tale::app::Focus::Collection);

    let Some(task_id) = task_id else {
        return;
    };
    app.overlays
        .push(Overlay::Task(TaskOverlayState::new(task_id)));
    let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
        KeyCode::Esc,
        KeyModifiers::NONE,
    ))));
    assert!(app.overlays.is_empty());
    assert_eq!(app.current_route(), Route::Devices);
    assert_eq!(app.views.devices.selected_id, source_selection);
    assert_eq!(app.views.devices.scroll, 1);
    assert_eq!(app.views.devices.filter_draft, "node");
    assert!(
        app.tasks
            .get(task_id)
            .is_some_and(|task| !task.state.is_terminal())
    );
}

#[test]
fn mock_task_actions_open_the_full_task_inspector() {
    let Some(mut app) = mock_app() else {
        return;
    };
    let _ = app.update(Event::Source(SourceEvent::LoadSucceeded {
        generation: 1,
        devices: mock::devices(),
        observed_at: mock::MOCK_NOW,
    }));
    app.set_route(Route::Devices);

    for code in [KeyCode::Char('a'), KeyCode::Char('m'), KeyCode::Char('s')] {
        let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
            code,
            KeyModifiers::NONE,
        ))));
    }

    assert_eq!(app.current_route(), Route::Tasks);
    assert_eq!(app.focus, tale::app::Focus::Inspector);
    assert!(app.tasks.selected.is_some());
    assert!(app.overlays.is_empty());
    assert_eq!(
        app.focused_task().map(|task| task.action_id),
        Some(ActionId::MockSuccess)
    );
}

/// An action is offered where its subject is on screen. The local client's
/// verbs were one list handed to every route that had none of its own, so
/// `:credentials` offered `remove local account` — a key acting on something
/// the route does not show.
#[test]
fn local_actions_are_offered_only_where_their_subject_is() {
    let app = local_app(false);
    assert!(app.is_some());
    let Some(mut app) = app else {
        return;
    };

    // A route about neither the machine nor a device gets neither set.
    for route in [Route::Credentials, Route::Access, Route::Audit] {
        app.set_route(route);
        let actions = app.contextual_actions();
        for id in [
            ActionId::LocalConnect,
            ActionId::LocalAccountRemove,
            ActionId::LocalSshOpen,
            ActionId::LocalProbeConnection,
            ActionId::DiagnosticCopy,
        ] {
            assert!(
                !actions.contains(&id),
                "{route:?} still offers {id:?}, which acts on something it does not show"
            );
        }
    }

    // The client tab keeps the machine's verbs and does not mix in account rows.
    app.set_route(Route::Local);
    let local = app.contextual_actions();
    for id in [
        ActionId::LocalConnect,
        ActionId::LocalDisconnect,
        ActionId::LocalPreferencesEdit,
        ActionId::LocalSyspolicyReload,
    ] {
        assert!(local.contains(&id), "the local route lost {id:?}");
    }
    assert!(!local.contains(&ActionId::LocalAccountSwitch));
    assert!(!local.contains(&ActionId::LocalAccountRemove));
    assert!(
        !local.contains(&ActionId::LocalSshOpen),
        "a session to the selected device is not an action on this machine"
    );

    // The accounts tab owns account actions, targeting its selected row.
    app.local_accounts.push(LocalAccount {
        id: "profile-work".to_owned(),
        tailnet_name: Some("example.ts.net".to_owned()),
        account_name: Some("operator@example.com".to_owned()),
        display_name: None,
        profile_name: Some("work".to_owned()),
        active: true,
    });
    app.views.local.section = LocalSection::Accounts;
    let accounts = app.contextual_actions();
    for id in [
        ActionId::LocalAccountSwitch,
        ActionId::LocalAccountLogin,
        ActionId::LocalAccountLogout,
        ActionId::LocalAccountRemove,
    ] {
        assert!(accounts.contains(&id), "the accounts tab lost {id:?}");
    }
    assert!(!accounts.contains(&ActionId::LocalConnect));

    // Everything that acts on a selected row lives where the rows are.
    app.set_route(Route::Devices);
    let devices = app.contextual_actions();
    for id in [
        ActionId::LocalProbeConnection,
        ActionId::LocalWhois,
        ActionId::LocalSshOpen,
        ActionId::LocalNcOpen,
        ActionId::DevicesTaildropSend,
        ActionId::DevicesTaildropReceive,
    ] {
        assert!(devices.contains(&id), "the devices route lost {id:?}");
    }
    assert!(!devices.contains(&ActionId::LocalConnect));

    app.set_route(Route::Diagnostics);
    assert!(app.contextual_actions().contains(&ActionId::DiagnosticCopy));
    assert!(app.contextual_actions().contains(&ActionId::LocalDnsStatus));
    let _ = app.dispatch_action(ActionId::SectionNext);
    assert_eq!(app.views.diagnostics.section, DiagnosticsSection::DnsStatus);
}

#[test]
fn diagnostics_load_the_visible_section() {
    let Some(mut app) = local_app(false) else {
        return;
    };
    let capabilities = LocalCapabilities::all_supported();
    app.local_executable = Some(LocalExecutable {
        path: "tailscale".into(),
        socket_path: None,
        source: ExecutableSource::Path,
        version: "1.98.9".to_owned(),
        daemon_version: Some("1.98.9".to_owned()),
        build: None,
        capabilities,
    });
    app.local_capabilities = capabilities;

    let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
        KeyCode::Char(':'),
        KeyModifiers::NONE,
    ))));
    let _ = app.update(Event::Input(InputEvent::Paste("diagnostics".to_owned())));
    let effects = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    ))));
    assert!(matches!(
        effects.as_slice(),
        [
            Effect::StartServiceTask {
                request: ServiceActionRequest::Metrics,
                ..
            },
            Effect::PersistTaskHistory(tasks),
        ] if tasks.len() == 1
    ));
    assert_eq!(app.current_route(), Route::Diagnostics);
    assert!(app.overlays.is_empty());
    assert!(app.tasks.selected.is_none());
    assert!(app.notifications.is_empty());

    app.set_route(Route::Diagnostics);
    let effects = app.dispatch_action(ActionId::SectionNext);
    assert!(matches!(
        effects.as_slice(),
        [Effect::StartLocalDiagnostic {
            request: tale::local::diagnostics::DiagnosticRequest::DnsStatus,
            ..
        }]
    ));
}

#[test]
fn dns_route_loads_local_status_directly() {
    let Some(mut app) = local_app(false) else {
        return;
    };
    let capabilities = LocalCapabilities::all_supported();
    app.local_executable = Some(LocalExecutable {
        path: "tailscale".into(),
        socket_path: None,
        source: ExecutableSource::Path,
        version: "1.98.9".to_owned(),
        daemon_version: Some("1.98.9".to_owned()),
        build: None,
        capabilities,
    });
    app.local_capabilities = capabilities;

    let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
        KeyCode::Char(':'),
        KeyModifiers::NONE,
    ))));
    let _ = app.update(Event::Input(InputEvent::Paste("dns".to_owned())));
    let effects = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    ))));

    assert!(
        matches!(
            effects.as_slice(),
            [
                Effect::StartLocalDiagnostic {
                    request: tale::local::diagnostics::DiagnosticRequest::DnsStatus,
                    ..
                },
                Effect::PersistTaskHistory(tasks),
            ] if tasks.len() == 1
        ),
        "unexpected effects: {effects:?}"
    );
    assert_eq!(app.current_route(), Route::Dns);
    assert!(app.overlays.is_empty());
    assert!(app.tasks.selected.is_none());
    assert!(app.notifications.is_empty());
}

/// A profile for the tailnet this machine is on adds the tailnet's verbs to the
/// devices menu; it does not take away the ones the local client offers on the
/// same row. Both sets share one menu, so no sequence may shadow another.
///
/// Which tailnet the rows belong to is what decides whether the local half is
/// offered at all, so the fixture has to put both sources on one tailnet —
/// `tests/device_source.rs` covers the case where they diverge.
#[test]
fn the_devices_menu_carries_admin_and_local_actions_together() {
    let Some(mut app) = local_app(true) else {
        return;
    };
    // Without both halves the test proves nothing.
    assert!(app.admin.profile.is_some(), "the fixture has no profile");
    assert_eq!(
        app.device_view_source(),
        tale::app::DeviceViewSource::Composed,
        "the fixture must have both sources on one tailnet"
    );
    app.set_route(Route::Devices);
    let actions = app.contextual_actions();
    assert!(actions.contains(&ActionId::AdminDeviceRename));
    assert!(actions.contains(&ActionId::LocalSshOpen));
    assert!(actions.contains(&ActionId::DevicesTaildropSend));
    assert!(action::validate_transient_sequences(&actions).is_ok());
}

#[test]
fn access_edit_action_becomes_reopen_while_a_workflow_exists() {
    let Some(mut app) = local_app(true) else {
        return;
    };
    app.set_route(Route::Access);
    let actions = app.contextual_actions();
    assert!(actions.contains(&ActionId::AdminPolicyEdit));
    assert!(!actions.contains(&ActionId::AdminPolicyEditorReopen));

    app.policy_workflow = Some(PolicyWorkflow::opening(
        1,
        "audit".to_owned(),
        "example.test".to_owned(),
        1,
    ));
    let actions = app.contextual_actions();
    assert!(!actions.contains(&ActionId::AdminPolicyEdit));
    assert!(actions.contains(&ActionId::AdminPolicyEditorReopen));
    assert!(action::validate_transient_sequences(&actions).is_ok());
}

#[test]
fn changed_policy_workflow_actions_have_visible_mock_results() {
    let Some(mut app) = mock_app() else {
        return;
    };
    let candidate_bytes = b"{\n  // Fictional mock policy\n  \"groups\": { \"group:ops\": [\"alice@example.test\", \"bob@example.test\"] },\n}\n";
    let file = tale::temporary::TemporaryPolicyFile::create(candidate_bytes);
    assert!(file.is_ok());
    let Ok(file) = file else {
        return;
    };
    let Some(base_bytes) = app
        .admin
        .policy
        .snapshot
        .as_ref()
        .map(|snapshot| snapshot.source_bytes.as_slice())
    else {
        return;
    };
    let base = PolicyDocument::from_slice(base_bytes, tale::mock::MOCK_NOW);
    let candidate = PolicyDocument::from_slice(candidate_bytes, tale::mock::MOCK_NOW);
    assert!(base.is_ok() && candidate.is_ok());
    let (Ok(base), Ok(candidate)) = (base, candidate) else {
        return;
    };
    let mut workflow = PolicyWorkflow::opening(1, "mock".to_owned(), "example.test".to_owned(), 1);
    workflow.set_base(base);
    workflow.set_candidate(candidate, file.path().to_path_buf());
    app.policy_workflow = Some(workflow);
    app.set_route(Route::Access);

    let actions = app.contextual_actions();
    for action_id in [
        ActionId::AdminPolicyEditorReopen,
        ActionId::AdminPolicyRemoteRefresh,
        ActionId::AdminPolicyValidate,
        ActionId::AdminPolicyPreview,
        ActionId::AdminPolicyDiff,
        ActionId::AdminPolicyCandidateDiscard,
        ActionId::AdminPolicyWorkflowClose,
    ] {
        assert!(actions.contains(&action_id), "missing {action_id:?}");
        assert!(app.action_is_available(action_id), "disabled {action_id:?}");
    }

    assert!(
        app.dispatch_action(ActionId::AdminPolicyRemoteRefresh)
            .is_empty()
    );
    assert!(
        app.dispatch_action(ActionId::AdminPolicyValidate)
            .is_empty()
    );
    assert!(
        app.policy_workflow
            .as_ref()
            .and_then(PolicyWorkflow::validation)
            .is_some()
    );

    assert!(app.dispatch_action(ActionId::AdminPolicyDiff).is_empty());
    assert!(
        app.policy_workflow
            .as_ref()
            .and_then(PolicyWorkflow::diff)
            .is_some()
    );
    let backend = TestBackend::new(100, 40);
    let mut terminal = match Terminal::new(backend) {
        Ok(terminal) => terminal,
        Err(_) => return,
    };
    assert!(terminal.draw(|frame| ui::render(frame, &app)).is_ok());
    let rendered = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("Policy actions"));
    assert!(rendered.contains("Policy diff"));
    assert!(!rendered.contains("Policy source · read-only"));

    assert!(app.dispatch_action(ActionId::AdminPolicyPreview).is_empty());
    assert!(matches!(app.overlays.last(), Some(Overlay::Form(_))));
    app.overlays.pop();

    if let Some(workflow) = app.policy_workflow.as_mut()
        && let Some(candidate_hash) = workflow.candidate().map(|value| value.hash().to_owned())
    {
        let _ = workflow.set_preview(PolicyPreview {
            candidate_hash,
            selector_type: PolicySelectorType::User,
            selector: "alice@example.test".to_owned(),
            matches: Vec::new(),
            observed_at: tale::mock::MOCK_NOW,
        });
    }
    assert!(app.action_is_available(ActionId::AdminPolicyApply));
    assert!(app.dispatch_action(ActionId::AdminPolicyApply).is_empty());
    assert!(matches!(
        app.overlays.last(),
        Some(Overlay::Confirmation(_))
    ));
    app.overlays.pop();

    assert!(
        app.dispatch_action(ActionId::AdminPolicyCandidateDiscard)
            .is_empty()
    );
    assert!(matches!(
        app.overlays.last(),
        Some(Overlay::Confirmation(_))
    ));
    app.overlays.pop();
    assert!(
        app.dispatch_action(ActionId::AdminPolicyWorkflowClose)
            .is_empty()
    );
    assert!(matches!(
        app.overlays.last(),
        Some(Overlay::Confirmation(_))
    ));
}

#[test]
fn unchanged_editor_exit_returns_to_policy_source() {
    let Some(mut app) = mock_app() else {
        return;
    };
    let base = PolicyDocument::from_slice(b"{}\n", 1);
    assert!(base.is_ok());
    let Ok(base) = base else {
        return;
    };
    let mut workflow = PolicyWorkflow::opening(7, "mock".to_owned(), "example.test".to_owned(), 1);
    workflow.set_base(base.clone());
    workflow.set_candidate(
        base.clone(),
        std::path::PathBuf::from("/tmp/mock-policy.hujson"),
    );
    app.policy_workflow = Some(workflow);
    let effects = app.update(Event::Policy(Box::new(PolicyEvent::EditorFinished {
        workflow_id: 7,
        result: Ok(base),
        path: std::path::PathBuf::from("/tmp/mock-policy.hujson"),
        editor_success: true,
        editor_code: Some(0),
    })));

    assert!(app.policy_workflow.is_none());
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, tale::effect::Effect::ResumeTerminal))
    );
}

#[test]
fn device_rename_starts_with_the_short_machine_name() {
    let Some(mut app) = local_app(true) else {
        return;
    };
    app.set_route(Route::Devices);
    let effects = app.dispatch_action(ActionId::AdminDeviceRename);
    assert!(effects.is_empty());
    assert!(matches!(
        app.overlays.last(),
        Some(Overlay::Form(state)) if state.value("name") == "node-01"
    ));
}

#[test]
fn device_danger_actions_start_verified_preflight_without_a_form() {
    for action_id in [
        ActionId::AdminDeviceRevokeApproval,
        ActionId::AdminDeviceKeyExpireNow,
        ActionId::AdminDeviceDelete,
    ] {
        let Some(mut app) = local_app(true) else {
            return;
        };
        app.set_route(Route::Devices);

        let effects = app.dispatch_action(action_id);

        assert!(
            matches!(
                effects.as_slice(),
                [Effect::StartAdminPreflight { request, .. }]
                    if request.action_id == action_id
                        && request.change.action_id() == action_id
            ),
            "{action_id:?} did not start its verified admin preflight"
        );
        assert!(app.overlays.is_empty());
        assert_ne!(
            app.runtime_error.as_deref(),
            Some("this action has no admin form")
        );
    }
}

#[test]
fn danger_menu_limits_destructive_fill_to_the_heading() {
    let Some(mut app) = local_app(true) else {
        return;
    };
    app.set_route(Route::Devices);
    app.theme = Theme::new(ThemeId::TailscaleDark, ColorCapability::TrueColor);
    let _ = app.dispatch_action(ActionId::ResourceActions);

    let backend = TestBackend::new(200, 50);
    let terminal = Terminal::new(backend).ok();
    assert!(terminal.is_some());
    let Some(mut terminal) = terminal else {
        return;
    };
    assert!(terminal.draw(|frame| ui::render(frame, &app)).is_ok());
    let buffer = terminal.backend().buffer();
    let destructive = app.theme.style(StyleRole::RiskDestructive);
    let danger = app.theme.style(StyleRole::StateDanger);
    let muted = app.theme.style(StyleRole::TextMuted);
    let mut heading_checked = false;
    let mut entry_checked = false;

    for y in 0..50 {
        let row = (0..200)
            .filter_map(|x| buffer.cell((x, y)))
            .map(|cell| cell.symbol())
            .collect::<String>();
        if let Some(start) = row.find(" Danger ")
            && let Ok(x) = u16::try_from(start.saturating_add(1))
            && let Some(cell) = buffer.cell((x, y))
        {
            assert_eq!(Some(cell.fg), destructive.fg);
            assert!(cell.modifier.contains(Modifier::REVERSED));
            heading_checked = true;
        }
        if let Some(start) = row.find("revoke device approval")
            && let Ok(label_x) = u16::try_from(start)
            && let Some(key_x) = label_x.checked_sub(2)
            && let (Some(label), Some(key)) = (buffer.cell((label_x, y)), buffer.cell((key_x, y)))
        {
            assert_eq!(Some(label.fg), muted.fg);
            assert!(!label.modifier.contains(Modifier::REVERSED));
            assert_eq!(Some(key.fg), danger.fg);
            assert!(!key.modifier.contains(Modifier::REVERSED));
            entry_checked = true;
        }
    }
    assert!(heading_checked, "the Danger heading was not rendered");
    assert!(entry_checked, "the destructive action row was not rendered");
}

#[test]
fn every_required_action_is_registered() {
    let registered: Vec<_> = action::shell_actions()
        .into_iter()
        .map(|spec| spec.id)
        .collect();
    for id in [
        ActionId::AppQuit,
        ActionId::ViewCommandLine,
        ActionId::ViewFilter,
        ActionId::ViewRefresh,
        ActionId::ViewRefreshAll,
        ActionId::ViewHelp,
        ActionId::ViewTasks,
        ActionId::ViewHistoryBack,
        ActionId::ViewHistoryForward,
        ActionId::CollectionMoveUp,
        ActionId::CollectionMoveDown,
        ActionId::CollectionFirst,
        ActionId::CollectionLast,
        ActionId::CollectionPageUp,
        ActionId::CollectionPageDown,
        ActionId::CollectionOpen,
        ActionId::CollectionSort,
        ActionId::CollectionWideColumns,
        ActionId::ResourceActions,
        ActionId::ResourceCopy,
        ActionId::TaskStop,
    ] {
        assert!(registered.contains(&id), "missing {}", id.as_str());
    }
}

#[test]
fn every_action_risk_maps_to_an_explicit_semantic_role() {
    for spec in action::all_actions() {
        let role = spec.risk.style_role();
        assert!(matches!(
            role,
            tale::ui::theme::StyleRole::RiskObserve
                | tale::ui::theme::StyleRole::RiskReversible
                | tale::ui::theme::StyleRole::RiskDisruptive
                | tale::ui::theme::StyleRole::RiskDestructive
        ));
        assert!(!role.signal().label.is_empty());
    }
}

#[test]
fn every_local_service_action_is_registered_with_required_risk_metadata() {
    let registered: Vec<_> = action::local_service_actions()
        .into_iter()
        .map(|spec| (spec.id, spec.risk))
        .collect();
    for id in [
        ActionId::ViewServices,
        ActionId::ViewDiagnostics,
        ActionId::SectionNext,
        ActionId::SectionPrevious,
        ActionId::ServicesServeRefresh,
        ActionId::ServicesServeCreate,
        ActionId::ServicesServeEdit,
        ActionId::ServicesServeRemove,
        ActionId::ServicesServeReset,
        ActionId::ServicesFunnelCreate,
        ActionId::ServicesFunnelEdit,
        ActionId::ServicesFunnelUnpublish,
        ActionId::ServicesFunnelReset,
        ActionId::DevicesTaildropSend,
        ActionId::DevicesTaildropReceive,
        ActionId::ServicesDriveRefresh,
        ActionId::ServicesDriveShare,
        ActionId::ServicesDriveRename,
        ActionId::ServicesDriveUnshare,
        ActionId::ServicesCertificateObtain,
        ActionId::ServicesMetricsRefresh,
        ActionId::ServicesBugReportCreate,
    ] {
        assert!(
            registered
                .iter()
                .any(|(registered_id, _)| *registered_id == id),
            "missing {}",
            id.as_str()
        );
    }
    assert_eq!(
        registered
            .iter()
            .find(|(id, _)| *id == ActionId::ServicesFunnelCreate)
            .map(|(_, risk)| *risk),
        Some(tale::action::Risk::Disruptive)
    );
    assert_eq!(
        registered
            .iter()
            .find(|(id, _)| *id == ActionId::ServicesServeReset)
            .map(|(_, risk)| *risk),
        Some(tale::action::Risk::Disruptive)
    );
    // Taking one mapping down is narrower than a reset but no less disruptive
    // to whoever is using it.
    for id in [
        ActionId::ServicesServeRemove,
        ActionId::ServicesFunnelUnpublish,
    ] {
        assert_eq!(
            registered
                .iter()
                .find(|(registered_id, _)| *registered_id == id)
                .map(|(_, risk)| *risk),
            Some(tale::action::Risk::Disruptive),
            "{} is not disruptive",
            id.as_str()
        );
    }
    assert_eq!(
        registered
            .iter()
            .find(|(id, _)| *id == ActionId::ServicesDriveUnshare)
            .map(|(_, risk)| *risk),
        Some(tale::action::Risk::Disruptive)
    );
}

#[test]
fn no_duplicate_active_binding_exists_in_one_context() {
    for context in [
        ActionContext::Root,
        ActionContext::Collection,
        ActionContext::Detail,
        ActionContext::Audit,
    ] {
        let actions = action::shell_actions();
        for (index, left) in actions.iter().enumerate() {
            if !left.contexts.contains(&context) {
                continue;
            }
            for right in actions.iter().skip(index + 1) {
                if !right.contexts.contains(&context) {
                    continue;
                }
                for left_binding in left.default_bindings {
                    for right_binding in right.default_bindings {
                        assert_ne!(
                            left_binding, right_binding,
                            "duplicate binding in {context:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn dispatch_uses_registered_bindings_and_footer_reports_more() {
    let key = KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE);
    assert_eq!(
        action::action_for_key(key, ActionContext::Collection),
        Some(ActionId::ViewRefresh)
    );
    assert_eq!(
        action::action_for_key(
            KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE),
            ActionContext::Root
        ),
        Some(ActionId::ViewCommandLine)
    );
    let footer = action::footer_hints(ActionContext::Collection, Route::Devices, 20);
    assert!(footer.iter().any(|hint| hint == "? more"));
    assert!(
        action::footer_rows(
            &action::footer_actions(ActionContext::Collection, Route::Devices, 20),
            20,
        )
        .len()
            <= action::FOOTER_MAX_ROWS
    );
    let footer = action::footer_hints(ActionContext::Collection, Route::Devices, 120);
    assert_eq!(
        footer
            .iter()
            .take(6)
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec![
            ": command",
            "/ filter",
            "? help",
            "a actions",
            "y copy",
            "@ tasks"
        ]
    );
    assert_eq!(
        footer.iter().filter(|hint| hint.starts_with("? ")).count(),
        1
    );

    let footer = action::footer_hints(ActionContext::Detail, Route::Devices, 240);
    assert_eq!(
        footer
            .iter()
            .take(6)
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec![
            ": command",
            "/ search",
            "? help",
            "a actions",
            "y copy",
            "@ tasks"
        ]
    );
    let next = footer.iter().position(|hint| hint == "n next");
    let previous = footer.iter().position(|hint| hint == "N previous");
    assert!(next.is_some_and(|next| previous == Some(next.saturating_add(1))));
    let sort = footer.iter().position(|hint| hint == "s sort");
    assert!(previous.is_some_and(|previous| sort == Some(previous.saturating_add(1))));
    let refresh = footer.iter().position(|hint| hint == "r refresh");
    let refresh_all = footer.iter().position(|hint| hint == "R refresh-all");
    assert!(refresh.is_some_and(|refresh| refresh_all == Some(refresh.saturating_add(1))));

    let app = mock_app();
    assert!(app.is_some());
    if let Some(mut app) = app {
        app.set_route(Route::Devices);
        let _ = app.dispatch_action(ActionId::ResourceActions);
        assert!(app.tasks.all().is_empty());
        assert!(matches!(app.interaction, InteractionMode::Transient(_)));
        assert!(app.runtime_error.is_none());
        let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
            KeyCode::Esc,
            KeyModifiers::NONE,
        ))));
        let _ = app.update(Event::Source(SourceEvent::LoadSucceeded {
            generation: 1,
            devices: mock::devices(),
            observed_at: mock::MOCK_NOW,
        }));
        let _ = app.dispatch_action(ActionId::ResourceActions);
        assert!(matches!(app.interaction, InteractionMode::Transient(_)));
        assert!(app.runtime_error.is_none());
        let _ = app.dispatch_action(ActionId::MockSuccess);
        assert_eq!(app.tasks.all().len(), 1);
        let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
            KeyCode::Char('a'),
            KeyModifiers::NONE,
        ))));
    }
}

#[test]
fn binding_type_labels_are_stable() {
    assert_eq!(Binding::Char(':').label(), ":");
    assert_eq!(Binding::Ctrl('d').label(), "C-d");
    assert_eq!(Binding::Enter.label(), "Enter");
}

#[test]
fn transient_sequences_and_reserved_history_bindings_are_stable() {
    let actions = [
        ActionId::LocalConnect,
        ActionId::LocalDisconnect,
        ActionId::LocalAccountSwitch,
        ActionId::LocalProbeConnection,
        ActionId::DiagnosticCopy,
        ActionId::SavedViewCreate,
        ActionId::CollectionExport,
    ];
    assert!(action::validate_transient_sequences(&actions).is_ok());
    assert!(
        action::validate_transient_sequences(&[ActionId::LocalConnect, ActionId::MockCancellable])
            .is_ok()
    );
    assert_eq!(
        action::transient_sequence(ActionId::MockCancellable),
        Some("mc")
    );
    assert_eq!(
        action::transient_sequence(ActionId::LocalAccountSwitch),
        Some("as")
    );
    assert_eq!(
        action::transient_sequence(ActionId::LocalProbeConnection),
        Some("p")
    );
    assert_eq!(
        action::transient_sequence(ActionId::LocalSshOpen),
        Some("ss")
    );
    assert_eq!(
        action::transient_sequence(ActionId::LocalNcOpen),
        Some("nc")
    );
    assert_eq!(
        action::action_for_key(
            KeyEvent::new(KeyCode::Char('['), KeyModifiers::NONE),
            ActionContext::Collection,
        ),
        Some(ActionId::ViewHistoryBack)
    );
    assert_eq!(
        action::action_for_key(
            KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE),
            ActionContext::Collection,
        ),
        Some(ActionId::ViewHistoryForward)
    );
}

#[test]
fn saved_view_form_names_the_current_view_without_exposing_storage_syntax() {
    let Some(mut app) = mock_app() else {
        return;
    };
    app.resolved_config.experimental_features.saved_views = true;
    app.saved_views =
        tale::saved_views::SavedViewsState::load(&app.resolved_config.paths.state_dir).ok();
    app.set_route(Route::Devices);
    let _ = app.dispatch_action(ActionId::SavedViewCreate);
    assert!(matches!(app.overlays.last(), Some(Overlay::Form(_))));
    if let Some(Overlay::Form(form)) = app.overlays.last() {
        assert_eq!(form.fields.len(), 1);
        assert_eq!(form.fields[0].key, "name");
        assert!(
            form.fields
                .iter()
                .all(|field| !matches!(field.key, "columns" | "filter" | "sort" | "wide"))
        );
    }
}

#[test]
fn saved_views_are_hidden_when_the_experimental_feature_is_disabled() {
    let Some(mut app) = mock_app() else {
        return;
    };
    app.set_route(Route::Devices);
    assert!(app.saved_views.is_none());
    assert!(
        !app.contextual_actions()
            .contains(&ActionId::SavedViewCreate)
    );
    let _ = app.dispatch_action(ActionId::SavedViewCreate);
    assert!(!matches!(app.overlays.last(), Some(Overlay::Form(_))));
}

#[test]
fn empty_collections_do_not_advertise_row_actions() {
    let Some(mut app) = mock_app() else {
        return;
    };
    app.set_route(Route::Users);
    app.admin.users.snapshot = Some(Vec::new());
    let actions = app
        .footer_actions(160)
        .into_iter()
        .map(|hint| hint.action_id)
        .collect::<Vec<_>>();
    for unavailable in [
        ActionId::CollectionMoveUp,
        ActionId::CollectionMoveDown,
        ActionId::CollectionOpen,
        ActionId::CollectionInspect,
    ] {
        assert!(!actions.contains(&unavailable));
    }
}

#[test]
fn task_stop_is_advertised_only_while_the_selected_task_can_stop() {
    let Some(mut app) = mock_app() else {
        return;
    };
    app.set_route(Route::Tasks);
    assert!(
        !app.footer_actions(160)
            .iter()
            .any(|hint| hint.action_id == ActionId::TaskStop)
    );
    let task_id = app.tasks.create(
        ActionId::MockCancellable,
        "simulation",
        mock::MOCK_NOW,
        true,
    );
    assert!(
        app.footer_actions(160)
            .iter()
            .any(|hint| hint.action_id == ActionId::TaskStop)
    );
    assert!(app.tasks.start(task_id));
    assert!(app.tasks.succeed(
        task_id,
        mock::MOCK_NOW.saturating_add(1),
        "done",
        "complete"
    ));
    assert!(
        !app.footer_actions(160)
            .iter()
            .any(|hint| hint.action_id == ActionId::TaskStop)
    );
}

#[test]
fn tasks_offer_stop_for_active_work_and_retry_for_failed_replayable_work() {
    let Some(mut app) = mock_app() else {
        return;
    };
    app.set_route(Route::Tasks);

    let running = app.tasks.create(
        ActionId::MockCancellable,
        "running simulation",
        mock::MOCK_NOW,
        true,
    );
    assert!(app.tasks.start(running));
    app.tasks.selected = Some(running);
    assert!(app.contextual_actions().contains(&ActionId::TaskStop));
    assert!(app.contextual_actions().contains(&ActionId::TaskRetry));
    assert!(
        !app.contextual_actions()
            .contains(&ActionId::BatchReviewOutcomes)
    );
    assert!(app.action_unavailable_reason(ActionId::TaskStop).is_none());
    assert!(app.action_unavailable_reason(ActionId::TaskRetry).is_some());
    assert!(matches!(
        app.dispatch_action(ActionId::TaskStop).as_slice(),
        [Effect::CancelTask { task_id }] if *task_id == running
    ));

    let failed = app.tasks.create(
        ActionId::MockFailure,
        "failed simulation",
        mock::MOCK_NOW,
        true,
    );
    assert!(app.tasks.start(failed));
    assert!(app.tasks.fail(
        failed,
        mock::MOCK_NOW.saturating_add(1),
        "failed",
        "simulated failure"
    ));
    app.tasks.selected = Some(failed);
    assert!(app.action_unavailable_reason(ActionId::TaskStop).is_some());
    assert!(app.action_unavailable_reason(ActionId::TaskRetry).is_none());
    let effects = app.dispatch_action(ActionId::TaskRetry);
    assert!(matches!(
        effects.as_slice(),
        [Effect::StartMockTask {
            behavior: mock::MockTaskBehavior::DelayedFailure,
            ..
        }]
    ));
    assert_eq!(app.tasks.all().len(), 3);
}

#[test]
fn retry_reuses_the_retained_typed_diagnostic_request() {
    let Some(mut app) = local_app(true) else {
        return;
    };
    let capabilities = LocalCapabilities::all_supported();
    app.local_executable = Some(LocalExecutable {
        path: "tailscale".into(),
        socket_path: None,
        source: ExecutableSource::Path,
        version: "1.98.9".to_owned(),
        daemon_version: Some("1.98.9".to_owned()),
        build: None,
        capabilities,
    });
    app.local_capabilities = capabilities;

    let effects = app.dispatch_action(ActionId::LocalProbeConnection);
    let Some(task_id) = effects.iter().find_map(|effect| match effect {
        Effect::StartLocalDiagnostic { task_id, .. } => Some(*task_id),
        _ => None,
    }) else {
        return;
    };
    assert!(app.tasks.start(task_id));
    assert!(app.tasks.fail(
        task_id,
        mock::MOCK_NOW.saturating_add(1),
        "failed",
        "network unavailable"
    ));
    app.tasks.selected = Some(task_id);
    app.set_route(Route::Tasks);

    assert!(app.action_unavailable_reason(ActionId::TaskRetry).is_none());
    assert!(matches!(
        app.dispatch_action(ActionId::TaskRetry).as_slice(),
        [Effect::StartLocalDiagnostic {
            request: tale::local::diagnostics::DiagnosticRequest::Ping { target },
            ..
        }] if target == "node-01.fixture.ts.net"
    ));
}

#[test]
fn persisted_task_without_a_replay_request_explains_why_retry_is_disabled() {
    let Some(mut app) = mock_app() else {
        return;
    };
    app.set_route(Route::Tasks);
    let task_id = app.tasks.create(
        ActionId::LocalSshOpen,
        "Tailscale SSH",
        mock::MOCK_NOW,
        false,
    );
    assert!(app.tasks.start(task_id));
    assert!(app.tasks.fail(
        task_id,
        mock::MOCK_NOW.saturating_add(1),
        "interrupted",
        "Tale stopped before this task completed"
    ));
    app.tasks.selected = Some(task_id);

    assert!(
        app.action_unavailable_reason(ActionId::TaskRetry)
            .is_some_and(|reason| reason.contains("not retained in task history"))
    );

    let _ = app.dispatch_action(ActionId::ResourceActions);
    let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
        KeyCode::Char('r'),
        KeyModifiers::NONE,
    ))));
    let backend = TestBackend::new(160, 40);
    let Some(mut terminal) = Terminal::new(backend).ok() else {
        return;
    };
    assert!(terminal.draw(|frame| ui::render(frame, &app)).is_ok());
    let rendered = (0..40)
        .map(|y| {
            (0..160)
                .filter_map(|x| terminal.backend().buffer().cell((x, y)))
                .map(|cell| cell.symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("retry task"), "{rendered}");
    assert!(rendered.contains("stop task"), "{rendered}");
    assert!(
        rendered.contains("not retained in task history"),
        "{rendered}"
    );
}

#[test]
fn a_recoverable_interaction_error_does_not_make_user_quit_fail() {
    let Some(mut app) = mock_app() else {
        return;
    };
    app.runtime_error = Some("select a resource before running this action".to_owned());
    let _ = app.update(Event::Input(InputEvent::Key(KeyEvent::new(
        KeyCode::Char('q'),
        KeyModifiers::NONE,
    ))));
    assert!(app.runtime_error.is_none());
    assert!(matches!(
        app.shutdown_state,
        tale::app::ShutdownState::Requested(tale::event::ShutdownReason::UserQuit)
    ));
}

#[test]
fn every_transient_action_has_an_explicit_menu_group() {
    for id in ActionId::all() {
        if action::transient_sequence(*id).is_some() {
            assert!(
                action::transient_group(*id).is_some(),
                "missing group for {}",
                id.as_str()
            );
        }
    }
}

/// Binding labels used to come from a hand-listed match that fell through to
/// the placeholder "key". A newly bound character has to name itself.
#[test]
fn every_registered_binding_names_its_own_key() {
    for spec in action::all_actions() {
        for binding in spec.default_bindings {
            let label = binding.label();
            assert_ne!(
                label, "key",
                "{:?} renders its binding as a placeholder",
                spec.id
            );
            assert_ne!(
                label, "C-key",
                "{:?} renders its binding as a placeholder",
                spec.id
            );
        }
    }
}
