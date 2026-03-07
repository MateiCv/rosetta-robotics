#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = forge_formats::robodm::fuzz_classify_element(data);
});
