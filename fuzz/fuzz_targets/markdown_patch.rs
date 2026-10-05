#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    agent_doc_fuzz_harness::markdown_patch(data);
});
