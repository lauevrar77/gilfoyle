#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let entries: Vec<String> = text.split('\n').map(|line| line.to_string()).collect();
        let _ = gilfoyle_core::ContainerAllowlist::parse(entries);
    }
});
