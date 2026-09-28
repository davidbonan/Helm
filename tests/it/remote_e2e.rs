//! Business E2E for phone access (specs/remote.md): what only the real system can
//! answer.

use helm::remote::awake::{KeepAwake, REASON};

fn power_assertions() -> String {
    let out = std::process::Command::new("pmset")
        .args(["-g", "assertions"])
        .output()
        .expect("pmset ships with macOS");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn keep_awake_holds_a_sleep_assertion_until_dropped() {
    let awake = KeepAwake::begin();
    let held = power_assertions().contains(REASON);

    drop(awake);
    let released = !power_assertions().contains(REASON);

    assert!(
        held,
        "the activity must prevent idle system sleep (pmset lists it)"
    );
    assert!(released, "ending the activity must let the Mac sleep again");
}
