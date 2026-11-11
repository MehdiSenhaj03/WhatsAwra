//! Compiles translations and embeds the icon and version information in
//! Windows executables.

fn main() {
    fastframe_i18n::build::compile_catalogs("assets/i18n");

    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=packaging/windows/whatsawra.ico");
        let mut resource = winresource::WindowsResource::new();
        resource
            .set_icon("packaging/windows/whatsawra.ico")
            .set("ProductName", "WhatsAwra")
            .set("FileDescription", "WhatsAwra");
        if let Err(error) = resource.compile() {
            println!("cargo:warning=Windows resources not embedded: {error}");
        }
    }
}
