use std::cmp::Ordering;

use tale::domain::device::{DeviceId, SortDirection, SortField, SortSpec, compare_devices};
use tale::domain::filter::{self, Comparison, FilterField, FilterTerm};
use tale::mock;

#[test]
fn parser_covers_quoted_values_negation_or_and_comparisons_and_whitespace() {
    let parsed =
        filter::parse("owner:\"alice example\" !tag:guest online:true,unknown lastSeen:<7d");
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
        "lastSeen:<7x",
        "owner:",
        "tag:a,",
        "owner:\"unfinished",
        "!free-text",
    ] {
        let parsed = filter::parse(input);
        assert!(parsed.is_err(), "input should be invalid: {input}");
    }
}

#[test]
fn and_or_matching_uses_only_the_current_snapshot() {
    let devices = mock::devices();
    let parsed = filter::parse("online:true os:linux,android");
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
        compare_devices(&devices[0], &devices[1], asc),
        Ordering::Greater
    );
    assert_eq!(
        compare_devices(&devices[1], &devices[0], asc),
        Ordering::Less
    );

    let desc = SortSpec {
        field: SortField::Owner,
        direction: SortDirection::Descending,
    };
    assert_eq!(
        compare_devices(&devices[0], &devices[1], desc),
        Ordering::Greater
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
    let parsed = filter::parse("tag:server online:true");
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
fn route_filter_schemas_expose_only_valid_fields() {
    let devices = tale::domain::filter::device_schema();
    assert!(
        devices
            .fields
            .iter()
            .any(|field| field.canonical_name == "owner")
    );
    assert!(
        devices
            .fields
            .iter()
            .any(|field| field.canonical_name == "online")
    );
    assert!(tale::domain::filter::activity_schema().fields.is_empty());
}
