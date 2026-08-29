#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    mosh_client::fuzzing::exercise_state_transitions(data);
});
