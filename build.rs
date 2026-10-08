//! On Windows: give turbodm.exe its icon, version info, the toolbar and category icons
//! (data/windows/*.ico, with ids in $OUT_DIR/icons.rs), and a manifest asking for Windows'
//! current controls (common controls 6: drawn in the style of the Windows version).

const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="TurboDM" version="1.0.0.0"/>
  <dependency><dependentAssembly>
    <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0"
      processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
  </dependentAssembly></dependency>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1"><application>
    <supportedOS Id="{35138b9a-5d96-4fbd-8e2d-a2440225f93a}"/>
    <supportedOS Id="{4a2f28e3-53b9-4441-ba9c-d69d4a4a6e38}"/>
    <supportedOS Id="{1f676c76-80e1-4239-95bb-83d0f6d0da78}"/>
    <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
  </application></compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3"><windowsSettings>
    <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true</dpiAware>
    <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
  </windowsSettings></application>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3"><security><requestedPrivileges>
    <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
  </requestedPrivileges></security></trustInfo>
</assembly>
"#;

fn main() {
    println!("cargo:rerun-if-changed=data/turbodm.ico");
    println!("cargo:rerun-if-changed=data/windows");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let root = std::env::var("CARGO_MANIFEST_DIR").unwrap().replace('\\', "/");
    let mut names: Vec<String> = std::fs::read_dir("data/windows").expect("data/windows").flatten()
        .filter_map(|e| e.file_name().to_str()?.strip_suffix(".ico").map(String::from)).collect();
    names.sort();
    let (mut rc, mut ids) = (String::new(), String::new());
    for (i, name) in names.iter().enumerate() {
        let id = 101 + i;
        rc += &format!("{id} ICON \"{root}/data/windows/{name}.ico\"\n");
        ids += &format!("pub const {}: u16 = {id};\n", name.to_uppercase().replace('-', "_"));
    }
    std::fs::write(std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("icons.rs"), ids).unwrap();
    let mut res = winresource::WindowsResource::new();
    res.set_icon("data/turbodm.ico");
    res.set("FileDescription", "TurboDM download manager");
    res.set("ProductName", "TurboDM");
    res.set_manifest(MANIFEST);
    res.append_rc_content(&rc);
    res.compile().expect("Windows resources");
}
