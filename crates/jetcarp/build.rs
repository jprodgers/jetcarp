//! Windows (MSVC): embeds `windows.manifest` (layered child windows need the Windows 8+
//! compatibility entry), and gives `jetcarp.exe` and `jetcarpw.exe` a version resource, so
//! Windows shows the program's name, version and licence in its Properties, Details, and Windows
//! Error Reporting names its version. The resource is written by `version_res.rs`, in plain Rust;
//! the linker compiles it into the program. macOS: embeds an `Info.plist` naming the program
//! JETCARP ([`mac_info_plist`]).

#[path = "version_res.rs"]
mod version_res;

/// macOS: an `Info.plist` inside the program (the `__TEXT,__info_plist` section), which is where
/// macOS reads a program's name from when it is not an app bundle: JETCARP, where it would show
/// the file's name, `jetcarp`. No identifier, so the program is still not taken for a bundle.
fn mac_info_plist() {
    let version = env!("CARGO_PKG_VERSION");
    let plist = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\">\n<dict>\n\
\t<key>CFBundleName</key>\n\t<string>JETCARP</string>\n\
\t<key>CFBundleDisplayName</key>\n\t<string>JETCARP</string>\n\
\t<key>CFBundleShortVersionString</key>\n\t<string>{version}</string>\n\
</dict>\n</plist>\n"
    );
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("Info.plist");
    std::fs::write(&out, plist).expect("write Info.plist");
    println!(
        "cargo:rustc-link-arg-bins=-Wl,-sectcreate,__TEXT,__info_plist,{}",
        out.display()
    );
}

fn main() {
    println!("cargo:rerun-if-changed=windows.manifest");
    println!("cargo:rerun-if-changed=version_res.rs");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if os == "macos" {
        mac_info_plist();
        return;
    }
    if os != "windows" || env != "msvc" {
        return;
    }
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let path = std::path::Path::new(&dir).join("windows.manifest");
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!(
        "cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}",
        path.display()
    );
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let number = |part: &str| -> u16 {
        std::env::var(format!("CARGO_PKG_VERSION_{part}"))
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    };
    let version = env!("CARGO_PKG_VERSION");
    for (bin, description) in [
        (
            "jetcarp",
            "JETCARP: cellular automata on tilings of the plane",
        ),
        ("jetcarpw", "JETCARP, started without a console window"),
    ] {
        let file_name = format!("{bin}.exe");
        let info = version_res::VersionInfo {
            numbers: [number("MAJOR"), number("MINOR"), number("PATCH"), 0],
            version,
            prerelease: !env!("CARGO_PKG_VERSION_PRE").is_empty(),
            product: "JETCARP",
            description,
            company: "Jimmie Rodgers",
            copyright: "Jimmie Rodgers. Free to use under the Unlicense or the MIT licence",
            file_name: &file_name,
        };
        let res = out.join(format!("{bin}-version.res"));
        std::fs::write(&res, version_res::res_file(&info)).unwrap();
        println!("cargo:rustc-link-arg-bin={bin}={}", res.display());
    }
}
