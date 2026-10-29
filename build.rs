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
    println!("cargo:rerun-if-changed=src/overlay.wgsl");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-link-arg-bins=/DEPENDENTLOADFLAG:0x800");
    for dll in ["mfplat.dll", "mfreadwrite.dll"] {
        println!("cargo:rustc-link-arg-bins=/DELAYLOAD:{dll}");
    }
    println!("cargo:rustc-link-arg-bins=delayimp.lib");
    println!("cargo:rustc-link-arg-bins=/CETCOMPAT");

    build_shell_assets(&out);
    build_shaders(&out);
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
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
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

fn build_shaders(out: &Path) {
    use naga::back::hlsl;
    let source = fs::read_to_string("src/overlay.wgsl").unwrap();
    let module = naga::front::wgsl::parse_str(&source).unwrap_or_else(|e| panic!("overlay.wgsl: {e}"));
    let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all()).validate(&module).unwrap_or_else(|e| panic!("overlay.wgsl: {e:?}"));
    let target = |register: u32| hlsl::BindTarget { space: 0, register, binding_array_size: None, dynamic_storage_buffer_offsets_index: None, restrict_indexing: false };
    let mut binding_map = hlsl::BindingMap::default();
    binding_map.insert(naga::ResourceBinding { group: 0, binding: 0 }, target(0));
    binding_map.insert(naga::ResourceBinding { group: 0, binding: 2 }, hlsl::BindTarget { space: 255, ..target(0) });
    for (register, binding) in [1, 3, 4, 5, 6, 7].into_iter().enumerate() {
        binding_map.insert(naga::ResourceBinding { group: 0, binding }, target(register as u32));
    }
    let mut sampler_buffer_binding_map = hlsl::SamplerIndexBufferBindingMap::default();
    sampler_buffer_binding_map.insert(hlsl::SamplerIndexBufferKey { group: 0 }, target(6));
    let sampler_heap_target = hlsl::SamplerHeapBindTargets { standard_samplers: target(0), comparison_samplers: target(2048) };
    let options = hlsl::Options { binding_map, fake_missing_bindings: false, special_constants_binding: Some(target(1)), sampler_heap_target, sampler_buffer_binding_map, ..Default::default() };
    for (stage, entry, profile) in [(naga::ShaderStage::Vertex, "vs", "vs_5_1"), (naga::ShaderStage::Fragment, "fs", "ps_5_1")] {
        let (stage_module, stage_info) = naga::back::pipeline_constants::process_overrides(&module, &info, Some((stage, entry)), &Default::default()).unwrap_or_else(|e| panic!("overlay.wgsl {entry}: {e}"));
        let fragment = (stage == naga::ShaderStage::Vertex).then(|| hlsl::FragmentEntryPoint::new(&module, "fs").expect("overlay.wgsl has no fs entry point"));
        let pipeline_options = hlsl::PipelineOptions { entry_point: Some((stage, entry.to_string())) };
        let mut hlsl_source = String::new();
        let mut reflection = hlsl::Writer::new(&mut hlsl_source, &options, &pipeline_options).write(&stage_module, &stage_info, fragment.as_ref()).unwrap_or_else(|e| panic!("overlay.wgsl {entry} to HLSL: {e}"));
        let name = reflection.entry_point_names.pop().expect("one entry point").unwrap_or_else(|e| panic!("overlay.wgsl {entry}: {e}"));
        fs::write(out.join(format!("overlay_{entry}.hlsl")), &hlsl_source).unwrap();
        fs::write(out.join(format!("overlay_{entry}.dxbc")), compile_hlsl(&hlsl_source, &name, profile)).unwrap();
    }
}

fn compile_hlsl(source: &str, entry: &str, profile: &str) -> Vec<u8> {
    use windows::Win32::Graphics::Direct3D::{Fxc::{D3DCOMPILE_ENABLE_STRICTNESS, D3DCompile}, ID3DBlob, ID3DInclude};
    use windows::core::PCSTR;
    let entry_name = std::ffi::CString::new(entry).unwrap();
    let profile_name = std::ffi::CString::new(profile).unwrap();
    let mut code: Option<ID3DBlob> = None;
    let mut errors: Option<ID3DBlob> = None;
    let bytes = |blob: &ID3DBlob| unsafe { std::slice::from_raw_parts(blob.GetBufferPointer().cast::<u8>(), blob.GetBufferSize()) }.to_vec();
    let result = unsafe {
        D3DCompile(
            source.as_ptr().cast(),
            source.len(),
            PCSTR::null(),
            None,
            None::<&ID3DInclude>,
            PCSTR(entry_name.as_ptr().cast()),
            PCSTR(profile_name.as_ptr().cast()),
            D3DCOMPILE_ENABLE_STRICTNESS,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    match result {
        Ok(()) => bytes(&code.expect("FXC returned no bytecode")),
        Err(e) => panic!("FXC failed on {entry} ({profile}): {e}: {}", errors.map(|b| String::from_utf8_lossy(&bytes(&b)).into_owned()).unwrap_or_default()),
    }
}
