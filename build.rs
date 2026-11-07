//! Embeds the application icon and version details into the Windows executable.

fn main() -> std::io::Result<()> {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    winresource::WindowsResource::new()
        .set_icon("assets/icon.ico")
        .set("FileDescription", "ctermin")
        .set("ProductName", "ctermin")
        .compile()
}
