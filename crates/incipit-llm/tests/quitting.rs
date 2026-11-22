//! Stopping every child when the app quits (`process::stop_all`). It stops all the children of
//! the process it runs in, so it has a test program of its own: beside the crate's other tests,
//! which run at the same time, it would stop their children too.

#![cfg(unix)]

use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use incipit_llm::call::Cancel;
use incipit_llm::process::{run, stop_all};

#[test]
fn a_child_still_running_when_the_app_quits_is_stopped_with_what_it_started() {
    let (started, said) = mpsc::channel();
    let begun = Instant::now();
    let call = thread::spawn(move || {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 60 & echo started; sleep 60"]);
        run(
            command,
            None,
            Duration::from_secs(120),
            &Cancel::new(),
            &mut |line| {
                let _ = started.send(line.to_owned());
            },
        )
    });
    // The child is listed before its first line can arrive.
    assert_eq!(said.recv().unwrap(), "started");
    stop_all();
    let exit = call.join().unwrap().unwrap();
    assert_eq!(exit.code, None, "ended by a signal");
    // What it started holds its output too: left running, it would keep the call open until the
    // drain after the child's exit (2 seconds) ran out.
    assert!(begun.elapsed() < Duration::from_millis(1500));
}
