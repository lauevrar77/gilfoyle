#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = gilfoyle_host::parse_os_release(text).is_debian_like();
        let _ = gilfoyle_host::parse_package_list(text);
    }
});
