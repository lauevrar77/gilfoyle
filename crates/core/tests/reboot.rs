//! Domain tests for the reboot verdict.

use gilfoyle_core::{RebootOrigin, RebootStatus};

#[test]
fn required_carries_the_packages_and_a_debian_origin() {
    let status = RebootStatus::required(vec!["libc6".to_string(), "linux-image".to_string()]);
    assert_eq!(status.required, Some(true));
    assert_eq!(
        status.packages,
        vec!["libc6".to_string(), "linux-image".to_string()]
    );
    assert_eq!(status.origin, RebootOrigin::Debian);
}

#[test]
fn not_required_is_a_debian_false() {
    let status = RebootStatus::not_required();
    assert_eq!(status.required, Some(false));
    assert!(status.packages.is_empty());
    assert_eq!(status.origin, RebootOrigin::Debian);
}

#[test]
fn unsupported_is_unknown_never_false() {
    let status = RebootStatus::unsupported();
    assert_eq!(status.required, None);
    assert!(status.packages.is_empty());
    assert_eq!(status.origin, RebootOrigin::Unsupported);
}

#[test]
fn serialises_unknown_as_null_and_the_origin_in_snake_case() {
    let value = serde_json::to_value(RebootStatus::unsupported()).unwrap();
    assert_eq!(value["required"], serde_json::Value::Null);
    assert_eq!(value["origin"], "unsupported");
    assert_eq!(value["packages"], serde_json::json!([]));

    let value = serde_json::to_value(RebootStatus::not_required()).unwrap();
    assert_eq!(value["required"], false);
    assert_eq!(value["origin"], "debian");
}
