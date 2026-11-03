mod common;

use std::{process::Command, time::Duration};

use common::{Bus, GcrPrompt, use_bus_for_gcr};

/// Manual visual check of the actual GTK surface. The regular suite uses a
/// scripted UI and remains headless.
#[test]
#[ignore = "opens a real layer-shell prompt and needs a person at the display"]
fn gtk_prompt_round_trip() {
    let bus = Bus::start();
    use_bus_for_gcr(&bus);
    let mut daemon = Command::new(env!("CARGO_BIN_EXE_krytis-prompter"))
        .args(["--idle-timeout", "1"])
        .env("DBUS_SESSION_BUS_ADDRESS", &bus.address)
        .spawn()
        .expect("start krytis-prompter");

    let prompt = (0..100)
        .find_map(|_| match GcrPrompt::open(1) {
            Ok(prompt) => Some(prompt),
            Err(e) if e.contains("ServiceUnknown") => {
                std::thread::sleep(Duration::from_millis(20));
                None
            }
            Err(e) => panic!("open the live GTK prompt: {e}"),
        })
        .expect("daemon did not claim the prompter name");
    prompt.title("krytis-prompter live check");
    prompt.message("Type “krytis live check” in both fields, then check the box");
    prompt.password_new(true);
    prompt.choice_label("The dialog is visible, centered, and themed");
    let secret = prompt.password().expect("password call").expect("the live prompt was cancelled");
    assert_eq!(secret, b"krytis live check");
    assert!(prompt.password_strength() > 0, "new-password strength was not returned");
    assert!(prompt.choice_chosen(), "the live-check box was not selected");
    prompt.close().expect("close the live prompt");

    for _ in 0..30 {
        if daemon.try_wait().unwrap().is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = daemon.kill();
    let _ = daemon.wait();
    panic!("daemon did not idle-exit after the live prompt closed");
}
