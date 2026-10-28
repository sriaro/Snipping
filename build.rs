#[path = "src/icon.rs"]
mod icon;
#[path = "src/shell_assets.rs"]
mod shell_assets;

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

const SIZES: [u32; 9] = [16, 20, 24, 32, 40, 48, 64, 128, 256];

fn main() {
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    unsafe {
        env::set_var("TEMP", &out);
        env::set_var("TMP", &out);
    }
    println!("cargo:rerun-if-changed=src/icon.rs");
    println!("cargo:rerun-if-changed=src/shell_assets.rs");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-link-arg-bins=/DEPENDENTLOADFLAG:0x800");
    for dll in ["mfplat.dll", "mfreadwrite.dll"] {
        println!("cargo:rustc-link-arg-bins=/DELAYLOAD:{dll}");
    }
    println!("cargo:rustc-link-arg-bins=delayimp.lib");
    println!("cargo:rustc-link-arg-bins=/CETCOMPAT");

    build_shell_assets(&out);
    let ico_path = out.join("snipping.ico");
    fs::write(&ico_path, build_ico()).unwrap();
    let manifest_path = out.join("snipping.manifest");
    fs::write(
        &manifest_path,
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>
"#,
    )
    .unwrap();

    let version = env::var("CARGO_PKG_VERSION").unwrap();
    let parts: Vec<&str> = version.split('.').collect();
    let (maj, min, pat) = (parts[0], parts[1], parts[2]);
    let rc = format!(
        r#"1 ICON "{ico}"
1 24 "{manifest}"

1 VERSIONINFO
FILEVERSION {maj},{min},{pat},0
PRODUCTVERSION {maj},{min},{pat},0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "Snipping"
      VALUE "FileDescription", "Snipping"
      VALUE "ProductName", "Snipping"
      VALUE "InternalName", "Snipping"
      VALUE "LegalCopyright", "Copyright (C) Snipping"
      VALUE "FileVersion", "{version}"
      VALUE "ProductVersion", "{version}"
      VALUE "OriginalFilename", "Snipping.exe"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x0409, 0x04B0
  END
END
"#,
        ico = ico_path.display().to_string().replace('\\', "\\\\"),
        manifest = manifest_path.display().to_string().replace('\\', "\\\\"),
    );
    let rc_path = out.join("snipping.rc");
    fs::write(&rc_path, rc).unwrap();
    embed_resource::compile(&rc_path, embed_resource::NONE).manifest_optional().unwrap();
}

fn build_ico() -> Vec<u8> {
    let images: Vec<(u32, Vec<u8>)> = SIZES.iter().map(|&s| (s, icon::render(s).encode_png().expect("png encode"))).collect();
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (s, png) in &images {
        out.push(if *s >= 256 { 0 } else { *s as u8 });
        out.push(if *s >= 256 { 0 } else { *s as u8 });
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in &images {
        out.extend_from_slice(png);
    }
    out
}

fn makepri() -> PathBuf {
    println!("cargo:rerun-if-env-changed=MAKEPRI");
    if let Some(tool) = env::var_os("MAKEPRI").map(PathBuf::from) {
        assert!(tool.is_file(), "MAKEPRI must name an existing Windows SDK makepri.exe");
        return tool;
    }
    let target = env::var("TARGET").unwrap();
    for key in [format!("RC_{target}"), format!("RC_{}", target.replace('-', "_")), "RC".to_string()] {
        println!("cargo:rerun-if-env-changed={key}");
        if let Some(rc) = env::var_os(&key).map(PathBuf::from) {
            if let Some(tool) = rc.parent().map(|folder| folder.join("makepri.exe")).filter(|tool| tool.is_file()) {
                return tool;
            }
        }
    }
    if let Some(tool) = embed_resource::find_windows_sdk_tool("rc.exe").and_then(|rc| rc.parent().map(|folder| folder.join("makepri.exe"))).filter(|tool| tool.is_file()) {
        return tool;
    }
    embed_resource::find_windows_sdk_tool("makepri.exe").expect("Windows 10 SDK MakePri is required to build the taskbar icon resource index")
}

fn build_shell_assets(out: &Path) {
    use std::collections::{BTreeMap, btree_map::Entry};
    let root = out.join("shell_assets");
    let assets = root.join("Assets");
    fs::create_dir_all(&assets).unwrap();
    let specs: Vec<_> = shell_assets::specs().collect();
    let mut generated = format!("pub const PNGS: [&[u8]; {}] = [\n", specs.len());
    let mut encoded = BTreeMap::<u32, Vec<u8>>::new();
    for (name, size) in specs {
        let bytes = match encoded.entry(size) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let mut bytes = Vec::new();
                {
                    let mut encoder = png::Encoder::new(&mut bytes, size, size);
                    encoder.set_color(png::ColorType::Rgba);
                    encoder.set_depth(png::BitDepth::Eight);
                    let mut writer = encoder.write_header().expect("shell icon PNG header");
                    writer.write_image_data(&icon::render_rgba(size)).expect("shell icon PNG data");
                }
                entry.insert(bytes)
            }
        };
        let path = assets.join(name);
        fs::write(&path, bytes).unwrap();
        generated.push_str(&format!("include_bytes!({:?}),\n", path));
    }
    generated.push_str("];\n");
    fs::write(out.join("shell_asset_data.rs"), generated).unwrap();
    let config = out.join("shell_priconfig.xml");
    fs::write(
        &config,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<resources targetOsVersion="10.0.0" majorVersion="1">
  <index root="\" startIndexAt="\">
    <default>
      <qualifier name="Language" value="en-US"/>
      <qualifier name="Contrast" value="standard"/>
      <qualifier name="Scale" value="100"/>
      <qualifier name="TargetSize" value="256"/>
      <qualifier name="Theme" value="dark"/>
      <qualifier name="AlternateForm" value=""/>
    </default>
    <indexer-config type="folder" foldernameAsQualifier="true" filenameAsQualifier="true" qualifierDelimiter="."/>
  </index>
</resources>
"#,
    )
    .unwrap();
    let tool = makepri();
    let mut command = Command::new(&tool);
    command.arg("new").arg("/pr").arg(&root).arg("/cf").arg(&config).arg("/in").arg(shell_assets::PACKAGE).arg("/of").arg(out.join("shell_resources.pri")).arg("/o");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let output = command.output().expect("run Windows SDK MakePri");
    assert!(output.status.success(), "MakePri failed at {}: {} {}", tool.display(), String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
}
