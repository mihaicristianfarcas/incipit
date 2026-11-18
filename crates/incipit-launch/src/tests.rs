//! Tests of launches in one process, each on an app folder in a temporary folder: the program
//! running is a `Listener` heard on its thread, a later launch is a call to `launch`, and what is
//! heard comes over a channel that is waited on with a deadline, so a message that never arrives
//! fails the test instead of hanging it.

use std::sync::mpsc::{channel, Receiver};
use std::time::Instant;

use super::*;

/// Long enough for a message to cross on a loaded machine.
const DEADLINE: Duration = Duration::from_secs(20);

/// The program running for `home`, and what it hears.
fn first(home: &Path) -> (Hearing, Receiver<Vec<PathBuf>>) {
    let Launch::First(listener) = launch(home, &[]) else {
        panic!("no program was running, so this one is the first");
    };
    let (send, heard) = channel();
    let hearing = listener.hear(move |files| {
        let _ = send.send(files);
    });
    (hearing, heard)
}

fn next(heard: &Receiver<Vec<PathBuf>>) -> Vec<PathBuf> {
    heard
        .recv_timeout(DEADLINE)
        .expect("the program running heard the launch before the deadline")
}

fn book(home: &Path, name: &str) -> PathBuf {
    home.join(name)
}

#[test]
fn a_later_launch_hands_its_files_to_the_program_running() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("Incipit");
    let (_hearing, heard) = first(&home);
    let files = vec![book(dir.path(), "paper.pdf"), book(dir.path(), "book.epub")];
    assert!(matches!(launch(&home, &files), Launch::HandedOver));
    assert_eq!(next(&heard), files);
}

#[test]
fn a_launch_with_no_files_asks_the_program_running_to_come_forward() {
    let dir = tempfile::tempdir().unwrap();
    let (_hearing, heard) = first(dir.path());
    assert!(matches!(launch(dir.path(), &[]), Launch::HandedOver));
    assert_eq!(next(&heard), Vec::<PathBuf>::new());
}

#[test]
fn a_file_named_from_the_working_folder_is_handed_over_as_an_absolute_path() {
    let dir = tempfile::tempdir().unwrap();
    let (_hearing, heard) = first(dir.path());
    let named = PathBuf::from("samples").join("paper.pdf");
    assert!(matches!(
        launch(dir.path(), std::slice::from_ref(&named)),
        Launch::HandedOver
    ));
    assert_eq!(next(&heard), [std::env::current_dir().unwrap().join(named)]);
}

#[test]
fn many_files_arrive_in_several_messages_and_all_are_heard() {
    let dir = tempfile::tempdir().unwrap();
    let (_hearing, heard) = first(dir.path());
    let files: Vec<PathBuf> = (0..message::MAX_FILES + 3)
        .map(|n| book(dir.path(), &format!("book {n}.pdf")))
        .collect();
    assert!(matches!(launch(dir.path(), &files), Launch::HandedOver));
    let mut got = next(&heard);
    got.extend(next(&heard));
    got.sort();
    let mut want = files;
    want.sort();
    assert_eq!(got, want);
}

#[test]
fn what_is_not_a_message_is_dropped_and_the_program_goes_on_hearing() {
    let dir = tempfile::tempdir().unwrap();
    let (_hearing, heard) = first(dir.path());
    let mut garbage = connect(dir.path()).unwrap();
    garbage.write_all(b"GET / HTTP/1.1\r\n\r\n").unwrap();
    drop(garbage);
    let mut too_long = connect(dir.path()).unwrap();
    let mut bytes = message::FIRST_LINE.to_vec();
    bytes.resize(message::MAX_MESSAGE + 10, b'a');
    // The program stops reading at the longest message, so the end may not all be taken.
    let _ = too_long.write_all(&bytes);
    drop(too_long);
    // A connection that sends nothing.
    drop(connect(dir.path()).unwrap());
    let file = book(dir.path(), "after.pdf");
    assert!(matches!(
        launch(dir.path(), std::slice::from_ref(&file)),
        Launch::HandedOver
    ));
    assert_eq!(next(&heard), [file]);
    assert!(heard.try_recv().is_err(), "nothing else was heard");
}

#[test]
fn once_the_program_running_stops_the_next_launch_is_the_first() {
    let dir = tempfile::tempdir().unwrap();
    let (hearing, _heard) = first(dir.path());
    drop(hearing);
    let (_again, heard) = first(dir.path());
    assert!(matches!(launch(dir.path(), &[]), Launch::HandedOver));
    next(&heard);
}

#[test]
fn two_app_folders_are_two_programs() {
    let dir = tempfile::tempdir().unwrap();
    let (_one, heard_one) = first(&dir.path().join("one"));
    let (_two, heard_two) = first(&dir.path().join("two"));
    let file = book(dir.path(), "a.pdf");
    assert!(matches!(
        launch(&dir.path().join("two"), std::slice::from_ref(&file)),
        Launch::HandedOver
    ));
    assert_eq!(next(&heard_two), [file]);
    assert!(heard_one.try_recv().is_err());
}

#[test]
fn a_lock_file_left_by_a_program_that_ended_is_taken_again() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(LOCK), b"").unwrap();
    let (_hearing, heard) = first(dir.path());
    assert!(matches!(launch(dir.path(), &[]), Launch::HandedOver));
    next(&heard);
}

#[test]
fn a_run_that_must_stay_itself_listens_but_never_hands_over() {
    let dir = tempfile::tempdir().unwrap();
    let listener = listen(dir.path()).unwrap();
    assert!(listen(dir.path()).is_err(), "only one program listens");
    let (send, heard) = channel();
    let _hearing = listener.hear(move |files| {
        let _ = send.send(files);
    });
    assert!(matches!(launch(dir.path(), &[]), Launch::HandedOver));
    next(&heard);
}

#[test]
fn a_program_that_holds_the_folder_but_cannot_be_reached_leaves_a_launch_alone() {
    let dir = tempfile::tempdir().unwrap();
    let _held = take(dir.path()).unwrap().expect("nobody held it");
    let started = Instant::now();
    assert!(matches!(launch(dir.path(), &[]), Launch::Alone(_)));
    assert!(started.elapsed() < DEADLINE);
}

#[test]
fn the_named_pipe_is_called_after_the_app_folder_the_same_way_every_time() {
    let one = pipe_name(Path::new(r"C:\Users\Learner\AppData\Roaming\Incipit"));
    assert_eq!(
        one,
        pipe_name(Path::new(r"c:\users\learner\appdata\roaming\incipit"))
    );
    assert_ne!(one, pipe_name(Path::new(r"C:\Users\Learner\Incipit test")));
    assert!(one.starts_with("incipit-") && one.len() == "incipit-".len() + 16);
    // FNV-1a of nothing, so the hash is the same in every build.
    assert_eq!(pipe_name(Path::new("")), "incipit-cbf29ce484222325");
}

#[cfg(unix)]
#[test]
fn a_socket_left_by_a_program_that_ended_is_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let left = std::os::unix::net::UnixListener::bind(dir.path().join(SOCKET)).unwrap();
    drop(left);
    assert!(dir.path().join(SOCKET).exists(), "the socket file was left");
    let (_hearing, heard) = first(dir.path());
    let file = book(dir.path(), "a.pdf");
    assert!(matches!(
        launch(dir.path(), std::slice::from_ref(&file)),
        Launch::HandedOver
    ));
    assert_eq!(next(&heard), [file]);
}

#[cfg(unix)]
#[test]
fn only_the_owner_may_write_to_the_socket_and_it_goes_when_the_program_stops() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let (hearing, _heard) = first(dir.path());
    let socket = dir.path().join(SOCKET);
    let mode = std::fs::metadata(&socket).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
    drop(hearing);
    assert!(!socket.exists());
}

#[cfg(unix)]
#[test]
fn something_else_at_the_sockets_name_is_not_removed() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(SOCKET), b"a note").unwrap();
    assert!(matches!(launch(dir.path(), &[]), Launch::Alone(_)));
    assert_eq!(std::fs::read(dir.path().join(SOCKET)).unwrap(), b"a note");
}
