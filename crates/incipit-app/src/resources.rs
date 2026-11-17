//! Where the files that come with the program are. A package (docs/packaging.md) keeps pdfium's
//! library beside the program, in a place each system has a name for: `Contents/Frameworks` in a
//! macOS bundle, the program's own folder on Windows, `lib` beside `bin` in a Linux AppDir. A
//! program run from a checkout (`cargo run`) finds the library in the checkout's `lib/lib`, where
//! `tools/setup` puts it, by looking up the folders above the program for one that has it.
//!
//! The search starts from where the running program is, never from a path fixed when it was built:
//! that path would be the builder's, and would not exist on the learner's machine.
//!
//! The sample books are for the `INCIPIT_BOOK` switch, which checks the reader without a vault.
//! They are not part of a package; `INCIPIT_SAMPLES` names the folder that holds them when the
//! program is not run from a checkout.

use std::path::{Path, PathBuf};

/// The folders a packaged program keeps pdfium's library in, as they lead from the folder the
/// program is in: beside it (Windows), in the bundle's `Frameworks` (macOS), in `lib` beside `bin`
/// (Linux).
const PACKAGED: [&str; 3] = [".", "../Frameworks", "../lib"];

/// Where `tools/setup` puts pdfium in a checkout, from the checkout's top.
const CHECKOUT_LIBRARY: &str = "lib/lib";

/// The folder pdfium's library is in, for the program running now. When it is nowhere, the
/// program's own folder: opening a PDF then says pdfium was not loaded from there.
pub(crate) fn pdfium() -> PathBuf {
    let program = std::env::current_exe().ok();
    match program
        .as_deref()
        .and_then(|p| library_near(p, &has_library))
    {
        Some(found) => found,
        None => program
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_default(),
    }
}

/// The sample book `name` (`paper.pdf`): in `INCIPIT_SAMPLES` when that is set, otherwise in the
/// `samples` folder of the checkout the program is run from.
pub(crate) fn sample(name: &str) -> PathBuf {
    if let Some(folder) = std::env::var_os("INCIPIT_SAMPLES") {
        return PathBuf::from(folder).join(name);
    }
    let program = std::env::current_exe().ok();
    program
        .as_deref()
        .and_then(Path::parent)
        .into_iter()
        .flat_map(Path::ancestors)
        .map(|top| top.join("samples"))
        .find(|folder| folder.is_dir())
        .unwrap_or_else(|| PathBuf::from("samples"))
        .join(name)
}

/// Whether `folder` holds pdfium's library.
fn has_library(folder: &Path) -> bool {
    folder.join(incipit_book::library_file()).is_file()
}

/// The folder with pdfium's library for a program at `program`: first the places a package keeps
/// it, then `lib/lib` of every folder above the program. `has` says whether a folder holds the
/// library, so the order can be tested without the library.
fn library_near(program: &Path, has: &dyn Fn(&Path) -> bool) -> Option<PathBuf> {
    let here = program.parent()?;
    let packaged = PACKAGED.iter().map(|relative| here.join(relative));
    let checkout = here.ancestors().map(|top| top.join(CHECKOUT_LIBRARY));
    packaged.chain(checkout).find(|folder| has(folder))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tree of empty files and the folders above them.
    fn tree(files: &[&str]) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        for file in files {
            let path = root.path().join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        }
        root
    }

    /// The folder (from `root`) library file `library` is found in for the program at `program`.
    fn found(root: &Path, program: &str, library: &str) -> Option<PathBuf> {
        library_near(&root.join(program), &|folder| {
            folder.join(library).is_file()
        })
        .map(|folder| folder.strip_prefix(root).unwrap().to_path_buf())
    }

    #[test]
    fn a_macos_bundle_keeps_the_library_in_its_frameworks_folder() {
        let root = tree(&[
            "Incipit.app/Contents/MacOS/incipit",
            "Incipit.app/Contents/Frameworks/libpdfium.dylib",
        ]);
        let folder = found(
            root.path(),
            "Incipit.app/Contents/MacOS/incipit",
            "libpdfium.dylib",
        );
        assert_eq!(
            folder.as_deref(),
            Some(Path::new("Incipit.app/Contents/MacOS/../Frameworks"))
        );
    }

    #[test]
    fn a_windows_install_keeps_the_library_beside_the_program() {
        let root = tree(&["Incipit/incipit.exe", "Incipit/pdfium.dll"]);
        let folder = found(root.path(), "Incipit/incipit.exe", "pdfium.dll");
        assert_eq!(folder.as_deref(), Some(Path::new("Incipit/.")));
    }

    #[test]
    fn a_linux_app_dir_keeps_the_library_in_lib_beside_bin() {
        let root = tree(&["usr/bin/incipit", "usr/lib/libpdfium.so"]);
        let folder = found(root.path(), "usr/bin/incipit", "libpdfium.so");
        assert_eq!(folder.as_deref(), Some(Path::new("usr/bin/../lib")));
    }

    #[test]
    fn a_checkouts_library_is_found_from_a_build_folder_below_it() {
        let root = tree(&[
            "checkout/target/debug/incipit",
            "checkout/lib/lib/libpdfium.so",
        ]);
        let folder = found(root.path(), "checkout/target/debug/incipit", "libpdfium.so");
        assert_eq!(folder.as_deref(), Some(Path::new("checkout/lib/lib")));
        // Also under a build for another target, one folder deeper.
        let folder = found(
            root.path(),
            "checkout/target/x86_64-unknown-linux-gnu/dist/incipit",
            "libpdfium.so",
        );
        assert_eq!(folder.as_deref(), Some(Path::new("checkout/lib/lib")));
    }

    #[test]
    fn the_packaged_library_wins_over_a_checkout_above_it() {
        // A bundle built inside a checkout (target/package) must use its own library.
        let root = tree(&[
            "checkout/lib/lib/libpdfium.dylib",
            "checkout/target/package/Incipit.app/Contents/MacOS/incipit",
            "checkout/target/package/Incipit.app/Contents/Frameworks/libpdfium.dylib",
        ]);
        let folder = found(
            root.path(),
            "checkout/target/package/Incipit.app/Contents/MacOS/incipit",
            "libpdfium.dylib",
        );
        assert_eq!(
            folder.as_deref(),
            Some(Path::new(
                "checkout/target/package/Incipit.app/Contents/MacOS/../Frameworks"
            ))
        );
    }

    #[test]
    fn no_library_anywhere_is_none() {
        let root = tree(&["Incipit/incipit.exe"]);
        assert_eq!(
            found(root.path(), "Incipit/incipit.exe", "pdfium.dll"),
            None
        );
    }
}
