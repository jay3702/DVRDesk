fn main() {
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=../src-tauri/icons/icon.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../src-tauri/icons/icon.ico");
        res.compile().expect("failed to embed the Windows icon resource");
    }
}
