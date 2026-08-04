use std::sync::OnceLock;

use clap::Parser;
use criterion::{Criterion, criterion_group, criterion_main};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::hint::black_box;
use tale::app::{App, Route};
use tale::cli::Cli;
use tale::config::{self, EnvironmentValues};
use tale::domain::Timestamp;
use tale::domain::device::{Device, DeviceId};
use tale::domain::filter;
use tale::domain::flow::{
    AggregateDimension, FlowConnection, FlowFilter, FlowMessage, aggregate_checked,
};
use tale::domain::health::{ApprovalState, HealthDevice, HealthSnapshot};
use tale::event::{Event, InputEvent};
use tale::mock;
use tale::paths::PathEnvironment;
use tale::ui;

const NOW: Timestamp = 1_775_000_000;

fn devices(count: usize) -> Vec<Device> {
    let templates = mock::devices();
    let mut result = Vec::with_capacity(count);
    for index in 0..count {
        let mut device = templates[index % templates.len()].clone();
        device.id = DeviceId::new(format!("bench-device-{index:05}"));
        device.display_name = format!("bench-device-{index:05}");
        device.hostname = format!("bench-{index:05}.example.test");
        device.last_seen = Some(NOW.saturating_sub((index % 3_600) as u64));
        result.push(device);
    }
    result
}

fn flow_messages(count: usize) -> Vec<FlowMessage> {
    let mut result = Vec::with_capacity(count);
    for index in 0..count {
        result.push(FlowMessage {
            node_id: format!("reporter-{index:05}"),
            reporting_node_name: Some("reporter.example.test".to_owned()),
            logged: "2026-08-05T00:00:00Z".to_owned(),
            start: "2026-08-05T00:00:00Z".to_owned(),
            end: "2026-08-05T00:00:01Z".to_owned(),
            source_node: None,
            destination_nodes: Vec::new(),
            virtual_traffic: vec![FlowConnection {
                proto: if index % 2 == 0 { "tcp" } else { "udp" }.to_owned(),
                src: "100.64.0.1".to_owned(),
                dst: "100.64.0.2".to_owned(),
                src_port: Some(443),
                dst_port: Some(443),
                tx_packets: 1,
                tx_bytes: 128,
                rx_packets: 2,
                rx_bytes: 256,
            }],
            subnet_traffic: Vec::new(),
            exit_traffic: Vec::new(),
            physical_traffic: Vec::new(),
        });
    }
    result
}

fn health_snapshot(count: usize) -> HealthSnapshot {
    HealthSnapshot {
        now: NOW,
        devices: (0..count)
            .map(|index| HealthDevice {
                stable_id: format!("health-device-{index:05}"),
                source_id: "admin".to_owned(),
                key_expires_at: None,
                approval: ApprovalState::Approved,
                client_version: Some("1.98.9".to_owned()),
                posture_read_succeeded: true,
                posture_attributes_present: Some(true),
            })
            .collect(),
        users: Vec::new(),
        resources: Vec::new(),
        routes: Vec::new(),
        posture_integration_enabled: false,
        relay_samples: Vec::new(),
    }
}

fn mock_app() -> Option<App> {
    let cli = Cli::try_parse_from(["tale", "--mock"]).ok()?;
    let environment = EnvironmentValues {
        config_file: None,
        profile: None,
        access_token_present: false,
        tailscale_path: None,
        no_color: false,
    };
    let root = std::path::PathBuf::from("/fictional/tale-bench");
    let paths = PathEnvironment {
        platform: tale::paths::Platform::Unix,
        current_dir: root.clone(),
        xdg_config_home: Some(root.join("config")),
        home: Some(root.join("home")),
        xdg_state_home: Some(root.join("state")),
        xdg_cache_home: Some(root.join("cache")),
        appdata: None,
        localappdata: None,
    };
    let config = config::resolve(&cli, &environment, &paths).ok()?;
    let mut app = App::new(config);
    app.route_stack = vec![Route::Devices];
    app.devices_resource.snapshot = devices(5_000);
    app.devices_resource.generation = 1;
    app.devices_resource.observed_at = Some(NOW);
    app.devices_resource.health = tale::domain::SourceHealth::Healthy;
    Some(app)
}

fn bench_filter(c: &mut Criterion) {
    let data = devices(5_000);
    let expression =
        filter::parse("online:true os:linux").unwrap_or_else(|_| filter::FilterExpression::empty());
    c.bench_function("filter_5000_devices", |bench| {
        bench.iter(|| {
            let count = data
                .iter()
                .filter(|device| expression.matches(device, NOW))
                .count();
            black_box(count);
        });
    });
}

fn bench_sort(c: &mut Criterion) {
    let data = devices(5_000);
    c.bench_function("stable_sort_5000_devices", |bench| {
        bench.iter(|| {
            let mut values = data.clone();
            values.sort_by(|left, right| {
                left.display_name
                    .cmp(&right.display_name)
                    .then_with(|| left.id.cmp(&right.id))
            });
            black_box(values.len());
        });
    });
}

fn bench_flow_aggregate(c: &mut Criterion) {
    static DATA: OnceLock<Vec<FlowMessage>> = OnceLock::new();
    let data = DATA.get_or_init(|| flow_messages(250_000));
    let filter = FlowFilter::default();
    c.bench_function("aggregate_250000_flow_messages", |bench| {
        bench.iter(|| {
            let result = aggregate_checked(data, &filter, &[AggregateDimension::Protocol]);
            black_box(result.map_or(0, |rows| rows.len()));
        });
    });
}

fn bench_health(c: &mut Criterion) {
    let snapshot = health_snapshot(5_000);
    c.bench_function("derive_5000_health_inputs", |bench| {
        bench.iter(|| black_box(snapshot.findings().len()));
    });
}

fn bench_render(c: &mut Criterion) {
    let Some(app) = mock_app() else {
        return;
    };
    let backend = TestBackend::new(160, 45);
    let mut terminal = match Terminal::new(backend) {
        Ok(terminal) => terminal,
        Err(_) => return,
    };
    c.bench_function("render_prepared_160x45_frame", |bench| {
        bench.iter(|| {
            let result = terminal.draw(|frame| ui::render(frame, &app));
            black_box(result.is_ok());
        });
    });
}

fn bench_render_compact(c: &mut Criterion) {
    let Some(app) = mock_app() else {
        return;
    };
    let backend = TestBackend::new(80, 24);
    let mut terminal = match Terminal::new(backend) {
        Ok(terminal) => terminal,
        Err(_) => return,
    };
    c.bench_function("render_prepared_80x24_frame", |bench| {
        bench.iter(|| {
            let result = terminal.draw(|frame| ui::render(frame, &app));
            black_box(result.is_ok());
        });
    });
}

fn bench_input_dispatch(c: &mut Criterion) {
    let Some(mut app) = mock_app() else {
        return;
    };
    let event = Event::Input(InputEvent::Key(KeyEvent::new(
        KeyCode::Char('j'),
        KeyModifiers::NONE,
    )));
    c.bench_function("input_dispatch_to_render_request", |bench| {
        bench.iter(|| {
            black_box(app.update(event.clone()).len());
        });
    });
}

fn bench_mock_startup(c: &mut Criterion) {
    c.bench_function("mock_startup_to_first_frame", |bench| {
        bench.iter(|| {
            let Some(app) = mock_app() else {
                return;
            };
            let backend = TestBackend::new(80, 24);
            let mut terminal = match Terminal::new(backend) {
                Ok(terminal) => terminal,
                Err(_) => return,
            };
            let result = terminal.draw(|frame| ui::render(frame, &app));
            black_box(result.is_ok());
        });
    });
}

criterion_group!(
    phase9,
    bench_filter,
    bench_sort,
    bench_flow_aggregate,
    bench_health,
    bench_render,
    bench_render_compact,
    bench_input_dispatch,
    bench_mock_startup
);
criterion_main!(phase9);
