# Packaging

How Incipit is made into something a learner installs: a disk image for macOS, an installer for
Windows and an AppImage for Linux. Each is made by one command, from a checkout, and none is signed
yet. What signing, hosting and updating need from the maintainer is at the end.

## Making a package

Each command runs on the system it makes the package for, from the top of the checkout, and writes to
`target/package/` (not in git). Each needs Rust, `cargo install cargo-about` (it writes the notice of
the licences of every library inside) and what building Incipit needs on that system
([development.md](development.md)).

| System | Command | Needs besides | Makes |
|---|---|---|---|
| macOS | `tools/setup`, then `tools/package-mac` | nothing (`codesign`, `hdiutil` come with the Mac) | `Incipit.app` and `Incipit-VERSION-arm64.dmg` |
| Windows | `tools/package-windows`, in Git Bash | Visual Studio's build tools with the Windows SDK, [NSIS](https://nsis.sourceforge.io), pdfium's DLL put in `lib\lib` by hand ([development.md](development.md)) | `Incipit-VERSION-x64-setup.exe` (`arm64` on an ARM PC) |
| Linux | `tools/setup`, then `tools/package-linux` | the packages of the Linux job in `.github/workflows/ci.yml`, [appimagetool](https://github.com/AppImage/appimagetool/releases) on the `PATH` | `Incipit-VERSION-x86_64.AppImage` (`aarch64` on an ARM machine) |

VERSION is the workspace's version in `Cargo.toml`. Beside each file the script writes its SHA-256
(`.sha256`). The scripts share `tools/package-common`; the files they use are in `packaging/`: the
icon (`tools/make-icons` makes the `.icns`, `.ico` and `.png` from `incipit.svg`; the three are kept
in git), `identifier`, the macOS `Info.plist.in`, the Windows `incipit.nsi`, the Linux
`incipit.desktop.in`, and `about.toml` and `licenses.hbs` for the licence notice.

The program in a package is built with the `dist` profile (`Cargo.toml`): `release` with symbols
stripped and thin LTO, from `Cargo.lock` as it is (`--locked`). The scripts also have rustc rewrite
the build machine's paths (the checkout, Cargo's folder, the toolchain) out of the program, so a
download does not carry the maker's home folder. The Windows program is built with its C runtime
inside, so a PC that never installed Visual C++ runs it.

### What was made and run here

Everything below was made on a Mac with Apple silicon. Sizes are bytes as measured, and megabytes of
a million.

| Package | Size | State |
|---|---|---|
| macOS bundle `Incipit.app` | 83,934,982 (83.9 MB) | Made. Started from a copy outside the checkout with `open`; it drew a PDF page with the pdfium inside it |
| macOS `Incipit-0.0.0-arm64.dmg` | 27,118,055 (27.1 MB) | Made and mounted; the bundle in it verifies and starts |
| Windows `Incipit-0.0.0-x64-setup.exe` | 27,573,623 (27.6 MB) | The installer script ran, on a stand-in program (below). Not run on Windows |
| Linux `.AppImage` | not made | The script's AppDir step ran on a stand-in program; the desktop file validates; see below |

The program alone, built here: 93,815,152 with `release` and 75,283,248 with `dist`. In the macOS
bundle it is 74,845,696 (the signature is rewritten), beside pdfium's library (7,579,296), the icon and
the licences (1.1 MB of notice). The disk image's compression was chosen by trying three: 36,921,769
with zlib, 36,354,957 with LZFSE, 27,118,055 with LZMA. LZMA is read by macOS 10.15 and later and the
bundle needs 13.

**Windows could not be built here.** GPUI's release build compiles its shaders with `fxc.exe`
(`crates/gpui_windows/build.rs` at the pinned revision does so only when the build runs on Windows),
and without them the build stops in `gpui_windows`. The program built for Windows on this Mac is a
stand-in: cross-built with the MinGW toolchain for `x86_64-pc-windows-gnu`, with GPUI's shaders
compiled when it runs (`--config profile.dist.package.gpui_windows.debug-assertions=true`, a command
line setting that is not in the repository). It is 85,539,840 bytes, has the icon as a resource, and
imports only system DLLs. Its installer was only built, and its file list read with 7-Zip: the
program, `pdfium.dll` (the x64 archive of the pinned build) and the licences. The Windows
installer script was not run, on Windows or under any emulation: the registry entries, shortcuts and
uninstaller are unseen. The size of a real MSVC program, and of its installer, is not known.

**Linux could not be built here.** `cargo build --target x86_64-unknown-linux-gnu` stops at the first
library with C code (`psm`), because this Mac has no Linux C compiler, and beyond that the program
links against the window system's libraries (X11, Wayland, xkbcommon, fontconfig, ALSA, udev, D-Bus),
none of which is here. A virtual machine or a container would have done it and was not allowed. The
script's other steps ran with a stand-in for the program (a macOS executable, so nothing about it is
Linux's) and the real x64 `libpdfium.so`: the AppDir came out with the right layout, and
`desktop-file-validate` accepts its desktop file. The AppImage itself was not made (appimagetool
is a Linux program), and no AppImage was started. What the program needs at run time from a Linux
system is unchecked.

## What the app needs beside its program

Looked up in the code and in the pinned builds of pdfium; `otool`, `objdump` and the packages
themselves were used where a number is given.

| What | Where it comes from | In a package |
|---|---|---|
| pdfium, which draws PDFs | A library the program loads by path when a PDF is opened (`incipit-book`, `Pdf::open`), pinned by `tools/setup` to the `chromium/8152` build of [pdfium-binaries](https://github.com/bblanchon/pdfium-binaries). It was built for macOS 13 (the macOS bundle therefore says 13). The Windows DLL imports only ADVAPI32, GDI32, KERNEL32 and USER32; the Linux library needs only libc, libm, libgcc_s and libpthread (x64 archives, read here) | Yes: `Contents/Frameworks` (macOS), beside the program (Windows), `usr/lib` (Linux). `src/resources.rs` looks in those places, then in `lib/lib` of any folder above the program, which is where a checkout has it. Before this, the path was fixed when the program was built, so a program moved to another machine could not draw a PDF |
| History of the vault | Jujutsu used as a Rust library (`jj-lib`, with its Git backend in Rust). No `jj` or `git` program is run, and no libgit2 or OpenSSL is linked | Nothing to add |
| Math | Typst and its fonts, inside the program (`typst-assets`), and the `mitex` package, inside the program | Nothing to add |
| Icons | SVG files inside the program | Nothing to add |
| Fonts of the window | The system's: the system font and SF Mono or Menlo on macOS, Segoe UI and Cascadia Mono or Consolas on Windows, the first of Inter, Noto Sans, Cantarell, Ubuntu, DejaVu Sans and Liberation Sans that is installed on Linux (`incipit-ui`) | Nothing |
| The reader's book text | The family Charter (`reader/rows.rs`), which macOS has and Windows and Linux do not. What those two draw instead was not looked at | Nothing. See "Not done" |
| Sample books | Only the `INCIPIT_BOOK` switch reads them; the product does not | Not shipped. `INCIPIT_SAMPLES` names the folder for a packaged program |
| Command-line models | Searched for in `PATH` and in the places installers put them (`incipit-llm`, `process::search_dirs`), because a program started from the Finder or a menu has a short `PATH` | Nothing |
| macOS system libraries | The program links only Apple's frameworks and `libSystem` (`otool -L`) | Nothing |
| Windows system libraries | The stand-in imports only DLLs that belong to Windows (Direct3D 11, DirectComposition, DirectWrite, DXGI, ICU); that every Windows 10 build has them was not checked | Nothing. The installer refuses Windows older than 10 |
| Linux system libraries | GPUI draws through wgpu (Vulkan) and talks to X11 or Wayland, with xkbcommon, fontconfig and FreeType; the CI job installs the development packages of these. Which ones the finished program needs at run time is what `ldd` says, which the script prints | The AppImage holds none: a graphics driver and the window system's libraries cannot be carried |

## What each package holds

```
Incipit.app/Contents/            macOS
    Info.plist                   name, identifier, version, minimum system, PDF and EPUB as "Open With"
    MacOS/incipit
    Frameworks/libpdfium.dylib
    Resources/incipit.icns
    Resources/licenses/          LICENSE, THIRD-PARTY-NOTICES.txt, pdfium/
```

Windows installs to `%LOCALAPPDATA%\Programs\Incipit`: `incipit.exe`, `pdfium.dll`, `licenses\` and the
uninstaller, a Start menu shortcut, an entry in Settings > Apps, and the entries that list Incipit in
"Open with" for PDF and EPUB files. It asks for no administrator's password. Uninstalling removes
what it put there and leaves vaults and the app's own folder (`%APPDATA%\Incipit`) alone. The
program must not be running when the installer runs.

The Linux AppDir is `usr/bin/incipit`, `usr/lib/libpdfium.so`, `usr/share/applications/IDENTIFIER.desktop`
(named by the identifier, which is how Wayland matches a window to it), the icon and
`usr/share/licenses/incipit/`, with `AppRun` linking to the program.

Incipit is AGPL-3.0. Each package carries `LICENSE`, the notice of the Rust libraries (made by
`cargo about` from `packaging/about.toml`, which lists the licences the project takes in; a library
under another one stops the build until someone reads it) and pdfium's own licences, which cover the
libraries pdfium is made of. The fonts Typst carries are not in the notice and their licences have
not been read.

### Opening a book from outside

The macOS bundle, the Windows entries and the Linux desktop file all say Incipit opens PDF and EPUB
files, as the brief asked, with the lowest rank so it never takes a file type from the reader a person
already uses. **The program does not yet act on a file it is given**: it starts and shows Today. Handling
it needs a decision (add the book to the library, or open it loose, as the sample books are) and code
in the app: the file arrives in the arguments on Windows and Linux and as an event on macOS.

## How GPUI apps are packaged

Zed is the one that was looked at, through its scripts in the checkout of the GPUI revision Incipit
pins (`script/bundle-mac`, `script/bundle-linux`, `script/bundle-windows.ps1`). They are GPL, so they
were read for the approach and nothing was copied.

- **macOS**: a `cargo-bundle` fork builds the `.app` from metadata in `Cargo.toml`; the program, a
  helper and a bundled `git` are signed with a Developer ID certificate and the hardened runtime; the
  disk image is made with `hdiutil`, signed, sent to Apple's notary service (`notarytool`) and
  stapled. Incipit does the same steps by hand in a shell script, and has no helper programs.
- **Linux**: a `.tar.gz` of a folder with `bin`, `libexec`, `lib` (the shared libraries the program
  needs, copied except a few system ones, with an rpath of `$ORIGIN/../lib`), icons and a desktop
  file. It is not an AppImage. Incipit carries only pdfium and relies on the system for the rest.
- **Windows**: a PowerShell script builds the program and its helpers, signs them, and makes an Inno
  Setup installer.

Incipit uses a script per system, like Zed, but an AppImage on Linux (one file a learner can keep
anywhere, as the brief suggested) and NSIS on Windows (it also runs on a Mac and Linux, so the
installer script can be checked there; Inno Setup runs only on Windows).

## Unsigned: what a learner sees

Nothing here was seen on a Mac that had downloaded the package, because the copies made here carry no
download mark. This is what the systems document.

- **macOS**: the bundle is signed ad hoc: its parts cannot be changed unnoticed, but nobody vouches for
  it. A downloaded copy is stopped the first time ("cannot be checked for malicious software"). The
  learner opens System Settings > Privacy & Security and chooses Open Anyway after the first try (on
  older systems: right-click the app, Open). Once.
- **Windows**: SmartScreen says "Windows protected your PC"; More info, then Run anyway.
- **Linux**: the file is made executable (its Properties) and double-clicked. AppImages need FUSE, which
  newer Ubuntu releases do not install by default; an AppImage can also be run with
  `--appimage-extract-and-run`.

If a screenshot is wanted of the app started from the Finder, note `INCIPIT_SHOT`: its picture is taken
by `screencapture`, which needs the program that started the app to be allowed to record the screen.
A terminal is; an app started with `open` is not unless the person allows it, so the app's own
picture fails ("could not create image from window"). The picture of the bundle was taken from outside
(`screencapture -l` of the window the bundle opened, from a program that is allowed).

## Signing, when the accounts exist

None of this was run: there is no certificate here.

**macOS.** An Apple Developer Program membership gives a "Developer ID Application" certificate.
`CODESIGN_IDENTITY="Developer ID Application: NAME (TEAM)" tools/package-mac` signs the library and the
bundle with the hardened runtime (`--options runtime --timestamp`). Whether the hardened runtime needs
an entitlement for something Incipit does (the library is signed by the same identity, so library
validation should pass; the Metal shaders are compiled at run time with GPUI's `runtime_shaders`; it
starts command-line programs) is found out by notarising. Notarise and staple the disk image:

```sh
xcrun notarytool submit target/package/Incipit-VERSION-arm64.dmg --keychain-profile PROFILE --wait
xcrun stapler staple target/package/Incipit-VERSION-arm64.dmg
```

`PROFILE` is made once with `xcrun notarytool store-credentials`, from an App Store Connect API key or an
app-specific password. The bundle inside the image should be stapled too if people will copy it out
(`stapler staple Incipit.app`, before making the image, which means a script change to notarise the
bundle first).

**Windows.** A code signing certificate (from a certificate authority, or Microsoft's Trusted
Signing service) signs `incipit.exe` before the installer is made, and the installer after, with
`signtool sign /fd SHA256 /tr TIMESTAMP_URL /td SHA256 ...`. A new publisher is still warned about by
SmartScreen until its signature has built a reputation (an extended validation certificate does not wait).

**Linux.** Nothing is required. An AppImage can carry a detached signature (`appimagetool --sign`)
that few people check.

## Updates

No updater is built. The choices:

1. **The app checks a releases page and says so.** At start (or from Settings), the app asks for the
   newest version number (the GitHub Releases API, `releases/latest`, when releases are hosted
   there), compares it with its own, and shows "Version X is available" with a button that opens the
   releases page in the browser. The learner downloads and installs the new package. It is the same on
   all three systems, needs no key and no framework, and is small. It needs: the
   hosting decision; one new network request that the app does by itself (a switch to turn it off, and
   a line in [models.md](models.md)'s account of what leaves the machine); the maintainer to say what
   the page is. On Linux an AppImage can also carry update information
   (`appimagetool -u 'gh-releases-zsync|OWNER|REPO|latest|Incipit-*-x86_64.AppImage.zsync'`) so
   AppImageUpdate can update it in place; that needs the `.zsync` file uploaded with each release.
2. **A framework: Sparkle on macOS, WinSparkle on Windows.** The app downloads, checks and installs
   the update and starts again, from an "appcast" file the maintainer hosts. Sparkle is a framework
   put in `Contents/Frameworks` and called from Rust through Objective-C bindings; WinSparkle is a DLL
   with a C interface. Each needs: a pair of keys (Sparkle's `generate_keys`; the private key stays with
   the maintainer, the public one goes in the program: `Info.plist`'s `SUPublicEDKey` for Sparkle, a
   resource for WinSparkle),
   an HTTPS place for the appcast and the files, the Developer ID and notarisation above (an
   unsigned update is stopped again by the system), and a Windows certificate for the same reason. It is
   the most comfortable for the learner and the most work, with two native libraries and the keys to
   guard.
3. **Writing an updater in Rust**, as Zed does: download, check a signature, replace the files,
   restart. The most code and the most risk (a bug can break the installed app); nothing here favours
   it over choice 2.

Choice 1 can ship before any signing exists and does not lock in a later choice 2. That is a
suggestion; the decision is the maintainer's.

## A release workflow, described and not added

No release workflow is in `.github/workflows/`: where releases are hosted and how they are signed are
not decided. This is the one to add. The repository is public, so the standard runners cost no Actions
minutes.

- **Trigger**: pushing a tag `vVERSION` (and `workflow_dispatch` for a trial that makes a draft). A first
  step checks that the tag matches the version in `Cargo.toml`, and that CI passed for the commit.
- **Three build jobs**, one per system, each checking out with the actions pinned to commits as `ci.yml`
  does, installing Rust and `cargo-about`, and running the script above:
  macOS on a `macos-15` runner (`tools/setup`, then `tools/package-mac`); Windows on `windows-latest`
  (download the pinned pdfium DLL into `lib\lib`, install NSIS, `bash tools/package-windows`; a second job
  on a Windows ARM runner for `arm64`); Linux on the oldest Ubuntu runner there is (the Linux packages from
  `ci.yml`, `tools/setup`, appimagetool downloaded by a pinned URL and checked against its SHA-256,
  `tools/package-linux`), because a program built against a newer glibc does not start on an older one.
  Each uploads its package and `.sha256` as an artifact.
- **One release job**, after the three, with `contents: write` and nothing else: it downloads the
  artifacts and creates a draft GitHub release with `gh release create --draft` and the files attached.
  The maintainer reads it, then publishes. The build jobs get a read-only token.
- **Signing**, when it is decided: the macOS job imports the certificate from a secret into a temporary
  keychain, runs `tools/package-mac` with `CODESIGN_IDENTITY`, and notarises with a stored API key; the
  Windows job signs with the certificate or the signing service's login. The secrets are repository
  secrets the maintainer adds; none exists yet, and none is named in the repository.

## Decisions for the maintainer

1. **Apple Developer ID and notarisation.** Join the Apple Developer Program (a yearly fee), make a
   Developer ID Application certificate and notarisation credentials. Until then every macOS learner
   has to allow the app by hand.
2. **A Windows signing certificate**, or Microsoft's Trusted Signing. Until then SmartScreen warns.
3. **Where releases are hosted**: GitHub Releases (free for a public repository) or a site of your own.
   The update check, the download links in the README and the appcast all depend on it.
4. **The bundle identifier.** It is `dev.mihaicristian.incipit`, which the app already used as its window
   class; it is kept in `packaging/identifier` and read by the program and all three scripts. Changing
   it after a release changes where macOS keeps the app's settings and permissions, and the
   Windows registry keys. Pick one from a domain you control before the first release.
5. **The update choice**: 1, 2 or 3 above.
6. **Smaller ones**: the icon (drawn here as a first mark); the name shown as the publisher and the
   copyright line (none is set); whether to build for Intel Macs (the script builds for the Mac it runs
   on; an Intel package needs the x86_64 pdfium and a second build); a Linux format beside the AppImage
   (a Flatpak is the usual next one); whether the Windows installer should also offer all-users
   installation; and the list of licences the project takes into a package (`packaging/about.toml`,
   which includes MPL-2.0 and Unicode-3.0 libraries).

## Not done

- A program built and run for Windows and for Linux. Both are in CI, which builds and tests them.
- The program acting on a file given to it (see "Opening a book from outside").
- A serif for the reader on Windows and Linux (Charter is macOS's); bundling an open one is a choice
  about size and licence.
- The icon for macOS 26's own style (an Icon Composer file): the bundle's `.icns` is the older kind.
