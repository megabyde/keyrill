use super::{support::client::Client, worker_support::*};
use keyrill_windows_proof::{
    diagnostics::{format_runtime, read_runtime},
    proof_inspect::parse_runtime,
};
use std::ffi::c_void;
use windows::Win32::Foundation::S_OK;
use windows_core::HRESULT;

#[test]
fn runtime_status_reader_preserves_native_source_states_and_errors() {
    let files = Files::new();
    files.write("en.add", b"");
    files.write("ru.add", b"");
    let _policy = Environment::set("KEYRILL_WINDOWS_POLICY", "model");
    let _en = Environment::set("KEYRILL_WINDOWS_EN_DICTIONARY", files.path("en.add"));
    let _ru = Environment::set("KEYRILL_WINDOWS_RU_DICTIONARY", files.path("ru.add"));
    let c = Client::new();
    // SAFETY: The test DLL exports this exact versioned maintenance signature.
    let maintain: unsafe extern "system" fn(*mut c_void) -> HRESULT =
        unsafe { c.api.export(b"KeyrillTestMaintain\0") };

    for (changed, states, errors, expected_sources) in [
        (
            None,
            [2, 2],
            [0, 0],
            "en_state=2; en_error=0; ru_state=2; ru_error=0",
        ),
        (
            Some("ru.add"),
            [2, 3],
            [0, 6],
            "en_state=2; en_error=0; ru_state=3; ru_error=6",
        ),
        (
            Some("en.add"),
            [3, 3],
            [6, 6],
            "en_state=3; en_error=6; ru_state=3; ru_error=6",
        ),
    ] {
        if let Some(name) = changed {
            files.write(name, b"bad\0data");
        }
        wait(|| {
            // SAFETY: The client retains its live controlled service handle.
            assert_eq!(unsafe { maintain(c.handle) }, S_OK);
            let native = runtime(&c.service);
            native.model_ready == 1
                && native.effective == 2
                && native.sources == states
                && native.source_errors == errors
        });
        let native = runtime(&c.service);
        let observed = read_runtime(&c.thread).unwrap();
        let text = format_runtime(&observed);
        let expected = format!(
            "Keyrill runtime v1; activation={}; requested=2; effective=2; model_ready=1; revision={}; error=0; configuration_error=0; {expected_sources}",
            native.activation, native.revision,
        );
        println!("native sources={states:?} errors={errors:?}; observed {text}");
        assert_eq!(text, expected);
        assert_eq!(parse_runtime(&text), Some(observed));
        assert_eq!(
            &observed[8..],
            &[states[0], errors[0], states[1], errors[1]]
        );
    }
    c.host.drained();
}
