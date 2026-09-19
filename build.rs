fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");

    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "deskpulse");
        res.set("FileDescription", "Desktop statistics overlay");
        res.set("OriginalFilename", "deskpulse.exe");
        res.set("FileVersion", "0.1.0.0");
        res.set("ProductVersion", "0.1.0.0");
        // Never fail the build over an icon: warn and carry on.
        if let Err(error) = res.compile() {
            println!("cargo:warning=failed to embed Windows resources: {error}");
        }
    }
}
