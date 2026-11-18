//! One program for each app folder: a launch while the program runs hands its files to it and
//! quits, on every system. macOS gives a second open to the program running by itself, but a
//! program started from a terminal, and every second launch on Windows and Linux ("Open With",
//! a double-clicked book, the program started again), is a second process; without this it would
//! be a second window, on the last vault.
//!
//! The program running holds the lock on a file in the app folder (`running.lock`) and listens on
//! a local socket only the same user can reach: a Unix socket in the app folder, `running.sock`,
//! that only its owner may write to (macOS, Linux), or a named pipe whose name is made from the
//! app folder's path, that refuses other computers and is open only to the user who made it
//! (Windows). At start, before a window opens, a launch (`launch`) takes the lock or finds it
//! held. Held, the launch connects, sends the absolute paths of the files it was given (or none,
//! which asks the program to come forward; `message`) and quits. Taken, it is the program
//! running: a socket file found there was left by one that ended, and is replaced. The system lets
//! go of the lock when a program ends, a crash too, and a program holds it only while it listens,
//! so a lock held always means a program to hand over to, and two launches at once cannot both
//! listen. The app folder decides, so two app folders (`INCIPIT_HOME`) are two programs, and the
//! vault's own lock keeps them off one vault.
//!
//! A Unix socket's path is limited (104 bytes on macOS, 108 on Linux), so a program whose app
//! folder's path is longer cannot listen: it runs on its own, and so does each launch after it.
//!
//! What arrives is untrusted: each connection is read on a thread of its own, up to the longest
//! message, and what is not a message of the form is dropped. The paths are given to the caller
//! (`Listener::hear`), whose rules decide which are files it takes.
//!
//! The standard library has Unix sockets but no named pipes, so the local socket is
//! `interprocess`'s, the usual crate for both behind one interface, which also sets a named pipe's
//! security and refuses other computers by itself; the rest is the standard library.

pub mod message;

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use interprocess::local_socket::{prelude::*, ListenerOptions, Name};

/// The file whose lock says a program runs for the app folder.
const LOCK: &str = "running.lock";

/// The Unix socket in the app folder.
#[cfg(unix)]
const SOCKET: &str = "running.sock";

/// How many times a launch that finds the lock held tries to reach the program holding it, and
/// how long it waits between tries. The program listens a moment after it takes the lock and no
/// event says when, so the launch tries again: for a second in all, after which the program
/// holding the lock is taken to be one that cannot listen, and the launch goes on alone.
const TRIES: u32 = 20;
const AGAIN: Duration = Duration::from_millis(50);

/// The most connections read at once: a launch makes one or a few, so more are not a launch.
const READERS: usize = 4;

/// How long a connection may take to send its message where the system can set a limit (macOS
/// and Linux; a named pipe has none, so a reader there waits until the other end closes).
const READ_FOR: Duration = Duration::from_secs(5);

/// Failed accepts in a row after which the program stops listening, rather than spin on a socket
/// that keeps failing.
const FAILURES: u32 = 16;

/// What a launch is.
#[derive(Debug)]
pub enum Launch {
    /// The program running for the app folder: later launches are heard once it calls `hear`.
    First(Listener),
    /// A program was running, and has the files: this one has nothing left to do.
    HandedOver,
    /// Neither, and why: the program goes on alone, without hearing later launches.
    Alone(String),
}

/// Decides what a program started for the app folder `home` is: it hands `files` (paths relative
/// to the working folder are made absolute) to a program already running and quits, or it is the
/// first. Blocks for at most about a second (`TRIES`).
pub fn launch(home: &Path, files: &[PathBuf]) -> Launch {
    let files: Vec<PathBuf> = files
        .iter()
        .filter_map(|file| std::path::absolute(file).ok())
        .collect();
    let mut why = String::new();
    for _ in 0..TRIES {
        match take(home) {
            Ok(Some(lock)) => {
                return match bind(home, lock) {
                    Ok(listener) => Launch::First(listener),
                    Err(e) => Launch::Alone(format!("it cannot listen for later launches: {e}")),
                }
            }
            Ok(None) => {}
            Err(e) => return Launch::Alone(format!("the app folder cannot be locked: {e}")),
        }
        match hand_over(home, &files) {
            Ok(()) => return Launch::HandedOver,
            Err(e) => why = e.to_string(),
        }
        std::thread::sleep(AGAIN);
    }
    Launch::Alone(format!("the program running could not be reached: {why}"))
}

/// Becomes the program that later launches hand their files to, without handing over to one
/// already running: for a run that must stay itself (one that only takes a picture).
pub fn listen(home: &Path) -> Result<Listener, String> {
    match take(home) {
        Ok(Some(lock)) => bind(home, lock).map_err(|e| e.to_string()),
        Ok(None) => Err("another program runs for this app folder".to_owned()),
        Err(e) => Err(format!("the app folder cannot be locked: {e}")),
    }
}

/// The lock on the app folder, or `None` when a program holds it.
fn take(home: &Path) -> io::Result<Option<File>> {
    std::fs::create_dir_all(home)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(home.join(LOCK))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(e),
    }
}

/// Sends the files to the program listening for `home`, a message at a time.
fn hand_over(home: &Path, files: &[PathBuf]) -> io::Result<()> {
    for message in message::encode(files) {
        let mut stream = connect(home)?;
        stream.write_all(&message)?;
        // A named pipe's flush waits until the program has read it all; the end of the
        // connection is the end of the message.
        stream.flush()?;
    }
    Ok(())
}

fn connect(home: &Path) -> io::Result<LocalSocketStream> {
    LocalSocketStream::connect(name(home)?)
}

/// The socket's name for the app folder `home`: a file in it.
#[cfg(unix)]
fn name(home: &Path) -> io::Result<Name<'static>> {
    use interprocess::local_socket::GenericFilePath;
    home.join(SOCKET).to_fs_name::<GenericFilePath>()
}

/// The socket's name for the app folder `home`: a named pipe called after the folder.
#[cfg(windows)]
fn name(home: &Path) -> io::Result<Name<'static>> {
    use interprocess::local_socket::GenericNamespaced;
    pipe_name(&dunce::canonicalize(home)?).to_ns_name::<GenericNamespaced>()
}

/// The named pipe's name for an app folder (canonical): the same for every program and every
/// build that has that folder, as Windows sees it (without regard to case), and another for each
/// folder. The hash is written out here, not taken from the standard library, whose hash may
/// change between Rust's versions and so between two builds of the app.
#[cfg(any(windows, test))]
fn pipe_name(home: &Path) -> String {
    // FNV-1a, 64 bits.
    let folder = home.to_string_lossy().to_lowercase();
    let hash = folder
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        });
    format!("incipit-{hash:016x}")
}

/// Listens on the socket for `home`, holding `lock`.
fn bind(home: &Path, lock: File) -> io::Result<Listener> {
    let options = ListenerOptions::new()
        .name(name(home)?)
        // The socket file is removed by the listener's owner once it has stopped (`Hearing`), so
        // a listener let go of late never removes the next program's.
        .reclaim_name(false);
    let socket = only_this_user(home, options)?;
    Ok(Listener {
        socket,
        home: home.to_path_buf(),
        lock,
    })
}

/// A Unix socket only its owner may write to, which connecting needs. The mode cannot be chosen
/// before the socket is made on macOS, so it is set just after: until then it has the umask's
/// usual mode, which lets no one else write either.
#[cfg(unix)]
fn only_this_user(home: &Path, options: ListenerOptions<'_>) -> io::Result<LocalSocketListener> {
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};
    let path = home.join(SOCKET);
    // Only the program holding the lock listens here, so a socket at the name was left by one
    // that ended (a crash, or a quit that left it); anything else there is not touched, and the
    // socket cannot be made.
    if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_socket()) {
        std::fs::remove_file(&path)?;
    }
    let socket = options.create_sync()?;
    if let Err(e) = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)) {
        drop(socket);
        let _ = std::fs::remove_file(&path);
        return Err(e);
    }
    Ok(socket)
}

/// A named pipe open only to its owner, the user who made it (the security descriptor's one entry
/// gives the owner everything and leaves out everyone else), and closed to other computers, which
/// `interprocess` refuses unless asked.
#[cfg(windows)]
fn only_this_user(_home: &Path, options: ListenerOptions<'_>) -> io::Result<LocalSocketListener> {
    use interprocess::os::windows::local_socket::ListenerOptionsExt;
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;
    let owner_only = widestring::U16CString::from_str("D:P(A;;GA;;;OW)")
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let descriptor = SecurityDescriptor::deserialize(&owner_only)?;
    options.security_descriptor(descriptor).create_sync()
}

/// The program running for an app folder, before it hears anything.
#[derive(Debug)]
pub struct Listener {
    socket: LocalSocketListener,
    home: PathBuf,
    lock: File,
}

impl Listener {
    /// Hears later launches from now on, on a thread of its own: `heard` is given the paths of
    /// each message, on a thread that read it, and is never given anything else. Listening goes
    /// on as long as the `Hearing` is kept.
    pub fn hear(self, heard: impl Fn(Vec<PathBuf>) + Send + Sync + 'static) -> Hearing {
        let stop = Arc::new(AtomicBool::new(false));
        let heard: Arc<dyn Fn(Vec<PathBuf>) + Send + Sync> = Arc::new(heard);
        let Listener { socket, home, lock } = self;
        let stopped = stop.clone();
        let thread = std::thread::Builder::new()
            .name("incipit-launch".into())
            .spawn(move || accept(socket, &stopped, &heard))
            .ok();
        Hearing {
            stop,
            thread,
            home,
            _lock: lock,
        }
    }
}

/// Takes each connection until told to stop, and reads it on a thread of its own.
fn accept(
    socket: LocalSocketListener,
    stop: &AtomicBool,
    heard: &Arc<dyn Fn(Vec<PathBuf>) + Send + Sync>,
) {
    let reading = Arc::new(AtomicUsize::new(0));
    let mut failures = 0;
    for connection in socket.incoming() {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let stream = match connection {
            Ok(stream) => stream,
            Err(_) => {
                failures += 1;
                if failures >= FAILURES {
                    break;
                }
                continue;
            }
        };
        failures = 0;
        if reading.fetch_add(1, Ordering::SeqCst) >= READERS {
            reading.fetch_sub(1, Ordering::SeqCst);
            continue;
        }
        let read = Reading(reading.clone());
        let heard = heard.clone();
        let _ = std::thread::Builder::new()
            .name("incipit-launch-read".into())
            .spawn(move || {
                let _read = read;
                if let Some(files) = read_message(stream) {
                    heard(files);
                }
            });
    }
}

/// One connection being read; counted while it is.
struct Reading(Arc<AtomicUsize>);

impl Drop for Reading {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The message a connection sends, up to the longest there is, or `None` when it sends none.
fn read_message(stream: LocalSocketStream) -> Option<Vec<PathBuf>> {
    // Not every system can limit a read (see READ_FOR).
    let _ = stream.set_recv_timeout(Some(READ_FOR));
    let mut bytes = Vec::new();
    let limit = u64::try_from(message::MAX_MESSAGE + 1).unwrap_or(u64::MAX);
    stream.take(limit).read_to_end(&mut bytes).ok()?;
    message::decode(&bytes)
}

/// Later launches being heard; dropping it stops listening and lets go of the app folder.
#[derive(Debug)]
pub struct Hearing {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    home: PathBuf,
    /// Last, so it is let go once listening has stopped.
    _lock: File,
}

impl Drop for Hearing {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // The thread waits on the next connection: one made here wakes it to see it is to stop.
        // When none can be made, the socket is gone and the thread with it, or it cannot be
        // woken; either way it is not waited for.
        let woken = connect(&self.home).is_ok();
        if let (true, Some(thread)) = (woken, self.thread.take()) {
            let _ = thread.join();
            #[cfg(unix)]
            let _ = std::fs::remove_file(self.home.join(SOCKET));
        }
    }
}

#[cfg(test)]
mod tests;
