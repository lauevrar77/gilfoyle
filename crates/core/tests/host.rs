use gilfoyle_core::{LoadAverage, Memory};

#[test]
fn load_average_accepts_finite_non_negative_values() {
    let average = LoadAverage::new(1.5, 0.5, 0.0, 3, 300).unwrap();
    assert_eq!(average.one, 1.5);
    assert_eq!(average.five, 0.5);
    assert_eq!(average.fifteen, 0.0);
    assert_eq!(average.runnable, 3);
    assert_eq!(average.total, 300);
}

#[test]
fn load_average_rejects_negative_nan_and_infinity() {
    assert!(LoadAverage::new(-0.1, 0.0, 0.0, 0, 0).is_err());
    assert!(LoadAverage::new(0.0, -1.0, 0.0, 0, 0).is_err());
    assert!(LoadAverage::new(0.0, 0.0, -1.0, 0, 0).is_err());
    assert!(LoadAverage::new(f64::NAN, 0.0, 0.0, 0, 0).is_err());
    assert!(LoadAverage::new(0.0, f64::INFINITY, 0.0, 0, 0).is_err());
    assert!(LoadAverage::new(0.0, 0.0, f64::NEG_INFINITY, 0, 0).is_err());
}

#[test]
fn memory_derives_used_and_percent() {
    let memory = Memory::new(1000, 100, 250, 200, 50);
    assert_eq!(memory.total_kib, 1000);
    assert_eq!(memory.free_kib, 100);
    assert_eq!(memory.available_kib, 250);
    assert_eq!(memory.used_kib, 750);
    assert_eq!(memory.swap_total_kib, 200);
    assert_eq!(memory.swap_free_kib, 50);
    assert!((memory.used_percent - 75.0).abs() < 1e-9);
}

#[test]
fn memory_percent_is_zero_when_total_is_zero() {
    let memory = Memory::new(0, 0, 0, 0, 0);
    assert_eq!(memory.used_kib, 0);
    assert_eq!(memory.used_percent, 0.0);
}

#[test]
fn memory_saturates_when_available_exceeds_total() {
    let memory = Memory::new(100, 100, 500, 0, 0);
    assert_eq!(memory.used_kib, 0);
    assert_eq!(memory.used_percent, 0.0);
}
