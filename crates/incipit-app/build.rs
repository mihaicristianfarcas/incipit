//! Puts the app's icon and its name into the Windows program, as a resource: GPUI takes the window's
//! and the taskbar's icon from the program's own resource 1, and Explorer shows the same icon on the
//! file. On any other system there is nothing to do.
//!
//! The icon is optional: if the resource compiler (`rc.exe` with Visual Studio's build tools, or
//! `windres` when cross-building with MinGW) cannot be found, the build goes on and says so.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../packaging/icon/incipit.ico");
    // The target, not the machine the build runs on: a Windows program can be built from a Mac.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon("../../packaging/icon/incipit.ico")
        .set("ProductName", "Incipit")
        .set("FileDescription", "Incipit");
    if let Err(error) = resource.compile() {
        println!("cargo:warning=the icon was not put into the program: {error}");
    }
}
