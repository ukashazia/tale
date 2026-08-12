use std::cmp::Ordering;

use tale::domain::device::{DeviceId, SortDirection, SortField, SortSpec, compare_devices};
use tale::domain::filter::{self, Comparison, FilterField, FilterTerm};
use tale::mock;

fn parse(input: &str) -> Result<filter::FilterExpression, filter::FilterError> {
    filter::parse(input, &filter::device_schema())
}

#[test]
fn parser_covers_quoted_values_negation_or_and_comparisons_and_whitespace() {
    let parsed = parse("owner:\"alice example\" !tag:guest online:true,unknown last-seen:<7d");
    assert!(parsed.is_ok());
    if let Ok(expression) = parsed {
        assert_eq!(expression.terms.len(), 4);
        assert!(matches!(
            &expression.terms[0],
            FilterTerm::Field { field: FilterField::Owner, values, .. } if values == &["alice example"]
        ));
        assert!(matches!(
            &expression.terms[1],
            FilterTerm::Field {
                field: FilterField::Tag,
                negated: true,
                ..
            }
        ));
        assert!(matches!(
            &expression.terms[2],
            FilterTerm::Field { field: FilterField::Online, values, .. } if values == &["true", "unknown"]
        ));
        assert!(matches!(
            &expression.terms[3],
            FilterTerm::Field { field: FilterField::LastSeen, comparison: Some(Comparison::Less(duration)), .. } if duration.as_secs() == 7 * 86_400
        ));
    }
}

#[test]
fn invalid_structured_terms_remain_errors() {
    for input in [
        "unknown:value",
        "last-seen:<7x",
        "last-seen:7d",
        "owner:",
        "tag:a,",
        "owner:\"unfinished",
        "!free-text",
        "online:yes",
        "approval:true",
        "online:contains=tru",
        "name:contains=build",
    ] {
        let parsed = parse(input);
        assert!(parsed.is_err(), "input should be invalid: {input}");
    }
}

#[test]
fn errors_name_the_expected_syntax_for_the_offending_term() {
    let unknown = parse("ownr:alice");
    assert!(unknown.is_err());
    if let Err(error) = unknown {
        assert!(error.message.contains("unknown field ownr"));
        assert!(error.expected.contains("owner"));
        assert!(error.expected.contains("last-seen"));
    }

    let bad_value = parse("online:yes");
    assert!(bad_value.is_err());
    if let Err(error) = bad_value {
        assert_eq!(error.expected, "online:true|false|unknown");
        assert!(error.to_string().contains("column"));
    }

    let missing_comparison = parse("last-seen:7d");
    assert!(missing_comparison.is_err());
    if let Err(error) = missing_comparison {
        assert_eq!(error.expected, "last-seen:<7d");
    }

    let unclosed = parse("owner:\"alice");
    assert!(unclosed.is_err());
    if let Err(error) = unclosed {
        assert!(error.expected.contains('"'));
    }
}

#[test]
fn only_canonical_field_spellings_parse() {
    for alias in [
        "lastSeen:<7d",
        "last_seen:<7d",
        "state:true",
        "authorized:approved",
        "keyExpiry:soon",
        "clientVersion:1.0",
        "role:exit-node",
        "shared:external",
    ] {
        assert!(parse(alias).is_err(), "alias should not parse: {alias}");
    }
    assert!(parse("last-seen:<7d").is_ok());
    assert!(parse("key-expiry:soon").is_ok());
    assert!(parse("route-role:exit-node").is_ok());
    assert!(parse("version:1.0").is_ok());
}

#[test]
fn and_or_matching_uses_only_the_current_snapshot() {
    let devices = mock::devices();
    let parsed = parse("online:true os:linux,android");
    assert!(parsed.is_ok());
    if let Ok(expression) = parsed {
        let matched: Vec<_> = devices
            .iter()
            .filter(|device| expression.matches(device, mock::MOCK_NOW))
            .map(|device| device.id.to_string())
            .collect();
        assert_eq!(matched, vec!["dev-a01", "dev-e05", "dev-k11"]);
    }
}

#[test]
fn stable_sort_has_id_tie_breaking_and_missing_values_follow_direction() {
    let mut devices = mock::devices();
    devices[0].owner = None;
    devices[1].owner = None;
    devices[0].id = DeviceId::new("tie-b");
    devices[1].id = DeviceId::new("tie-a");
    let asc = SortSpec {
        field: SortField::Owner,
        direction: SortDirection::Ascending,
    };
    assert_eq!(
        compare_devices(&devices[0], &devices[1], asc, mock::MOCK_NOW),
        Ordering::Greater
    );
    assert_eq!(
        compare_devices(&devices[1], &devices[0], asc, mock::MOCK_NOW),
        Ordering::Less
    );

    let desc = SortSpec {
        field: SortField::Owner,
        direction: SortDirection::Descending,
    };
    assert_eq!(
        compare_devices(&devices[0], &devices[1], desc, mock::MOCK_NOW),
        Ordering::Greater
    );
}

#[test]
fn last_seen_sort_orders_elapsed_age_in_the_requested_direction() {
    let now = mock::MOCK_NOW;
    let mut devices = mock::devices().into_iter().take(4).collect::<Vec<_>>();
    for (device, age) in devices
        .iter_mut()
        .zip([Some(1), Some(60), Some(3_600), None])
    {
        device.last_seen = age.map(|age| now.saturating_sub(age));
    }

    let ascending = SortSpec {
        field: SortField::LastSeen,
        direction: SortDirection::Ascending,
    };
    devices.sort_by(|left, right| compare_devices(left, right, ascending, now));
    assert_eq!(
        devices
            .iter()
            .filter_map(|device| device.age_at(now))
            .collect::<Vec<_>>(),
        vec![1, 60, 3_600]
    );
    assert!(
        devices
            .last()
            .is_some_and(|device| device.last_seen.is_none())
    );

    let descending = SortSpec {
        field: SortField::LastSeen,
        direction: SortDirection::Descending,
    };
    devices.sort_by(|left, right| compare_devices(left, right, descending, now));
    assert_eq!(
        devices
            .iter()
            .filter_map(|device| device.age_at(now))
            .collect::<Vec<_>>(),
        vec![3_600, 60, 1]
    );
    assert!(
        devices
            .last()
            .is_some_and(|device| device.last_seen.is_none())
    );
}

#[test]
fn five_thousand_fictional_devices_filter_without_identity_loss() {
    let seed = mock::devices();
    let mut devices = Vec::with_capacity(5_000);
    for index in 0..5_000 {
        let mut device = seed[index % seed.len()].clone();
        device.id = DeviceId::new(format!("fictional-{index:04}"));
        device.display_name = format!("fictional-{index:04}");
        devices.push(device);
    }
    let parsed = parse("tag:server online:true");
    assert!(parsed.is_ok());
    if let Ok(expression) = parsed {
        let ids: Vec<_> = devices
            .iter()
            .filter(|device| expression.matches(device, mock::MOCK_NOW))
            .map(|device| device.id.to_string())
            .collect();
        assert!(!ids.is_empty());
        assert!(ids.iter().all(|id| id.starts_with("fictional-")));
        assert_eq!(ids.len(), 358);
    }
}

#[test]
fn route_schemas_declare_every_parseable_field_with_guidance() {
    let devices = filter::device_schema();
    let names = devices.fields().map(|spec| spec.name).collect::<Vec<_>>();
    for expected in [
        "id",
        "name",
        "owner",
        "tag",
        "os",
        "online",
        "path",
        "last-seen",
        "property",
        "approval",
        "key-expiry",
        "version",
        "sharing",
        "posture",
        "route-role",
    ] {
        assert!(names.contains(&expected), "schema is missing {expected}");
    }
    // Every offered field explains itself and accepts at least one operator.
    for spec in devices.fields() {
        assert!(!spec.description.is_empty());
        assert!(!spec.operators.is_empty());
        assert!(!spec.expected_syntax().is_empty());
    }

    // Suggestions are route-scoped: Activity has no device vocabulary at all.
    let activity = filter::tasks_schema();
    assert!(activity.is_empty());
    assert!(activity.field("owner").is_none());
    assert!(!activity.free_text.is_empty());
    assert!(
        filter::parse("owner:alice", &activity).is_err(),
        "device fields must not parse on activity"
    );
}

#[test]
fn token_spans_track_quoted_sections() {
    assert_eq!(filter::token_spans(""), Vec::new());
    assert_eq!(filter::token_spans("os:linux"), vec![(0, 8)]);
    assert_eq!(filter::token_spans("os:linux tag:a"), vec![(0, 8), (9, 14)]);
    assert_eq!(
        filter::token_spans("owner:\"alice example\" tag:a"),
        vec![(0, 21), (22, 27)]
    );
}

#[test]
fn named_and_bare_text_filters_take_predictable_substrings() {
    let devices = mock::devices();
    let matched = |query: &str| {
        parse(query).map_or_else(
            |_| Vec::new(),
            |expression| {
                devices
                    .iter()
                    .filter(|device| expression.matches(device, mock::MOCK_NOW))
                    .map(|device| device.display_name.clone())
                    .collect::<Vec<_>>()
            },
        )
    };

    // A named field no longer needs the value spelled out in full.
    assert_eq!(matched("name:build"), vec!["build-01".to_owned()]);
    assert_eq!(matched("id:a01"), vec!["build-01".to_owned()]);
    assert_eq!(matched("tag:serv"), vec!["build-01".to_owned()]);
    assert!(matched("owner:alice").contains(&"build-01".to_owned()));
    assert!(matched("os:lin").contains(&"build-01".to_owned()));

    // It does need the value as written, so a named term cannot drift.
    assert!(matched("name:bld").is_empty());
    assert_eq!(matched("os:ios").len(), 2);
    assert!(!matched("os:ios").contains(&"win-lab".to_owned()));

    // Bare words use the same predictable substring rule. They cannot drift
    // across unrelated characters in a value.
    assert!(matched("bld").is_empty());
    assert_eq!(matched("uild"), vec!["build-01".to_owned()]);
    assert_eq!(
        matched("alice"),
        vec!["build-01".to_owned(), "studio-mac".to_owned()]
    );
}

#[test]
fn a_loose_match_never_spans_two_unrelated_fields() {
    let devices = mock::devices();
    let spanning = parse("serverprod");
    assert!(spanning.is_ok());
    if let Ok(expression) = spanning {
        // `server` and `prod` are two separate tags; joining the fields into one
        // blob would match this, searching them separately must not.
        assert!(
            !devices
                .iter()
                .any(|device| expression.matches(device, mock::MOCK_NOW))
        );
    }
}

#[test]
fn closed_vocabularies_stay_exact_and_starts_with_narrows_a_substring() {
    let devices = mock::devices();
    let count = |query: &str| {
        parse(query).map_or(usize::MAX, |expression| {
            devices
                .iter()
                .filter(|device| expression.matches(device, mock::MOCK_NOW))
                .count()
        })
    };

    // Enumerated fields are pinned by the parser, so they never widen.
    assert!(parse("online:tru").is_err());
    assert!(parse("path:dir").is_err());
    assert!(count("path:direct") > 0);

    // `starts_with=` narrows a substring to a prefix; `contains=` is gone
    // because a bare term already means exactly that.
    assert_eq!(count("name:uild"), 1);
    assert_eq!(count("name:starts_with=bui"), 1);
    assert_eq!(count("name:starts_with=uild"), 0);
    assert!(parse("name:contains=uild").is_err());
}
