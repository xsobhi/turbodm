//! On Windows: give turbodm.exe its icon and version info.

fn main() {
    println!("cargo:rerun-if-changed=data/turbodm.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("data/turbodm.ico");
        res.set("FileDescription", "TurboDM download manager");
        res.set("ProductName", "TurboDM");
        res.compile().expect("Windows resources");
    }
}
