use std::{path::PathBuf, process::Command};

fn main() {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let proto_dir = manifest.join("../proto");
    let protocol_proto = proto_dir.join("xkk.proto");
    let model_proto = proto_dir.join("model.proto");
    let tools = manifest.join("../../deps-rust/tools");
    let protoc = tools.join("protoc.exe");
    let xmongo_plugin = tools.join("protoc-gen-xmongo-trait.exe");
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());

    assert!(
        protoc.is_file(),
        "missing protocol compiler: {}",
        protoc.display()
    );
    assert!(
        xmongo_plugin.is_file(),
        "missing xmongo protocol plugin: {}",
        xmongo_plugin.display()
    );

    println!("cargo:rerun-if-changed={}", protocol_proto.display());
    println!("cargo:rerun-if-changed={}", model_proto.display());
    println!("cargo:rerun-if-changed={}", protoc.display());
    println!("cargo:rerun-if-changed={}", xmongo_plugin.display());

    let status = Command::new(&protoc)
        .arg(format!(
            "--plugin=protoc-gen-xmongo-trait={}",
            xmongo_plugin.display()
        ))
        .arg(format!("--xmongo-trait_out={}", out_dir.display()))
        .arg(format!("--proto_path={}", proto_dir.display()))
        .arg(&protocol_proto)
        .arg(&model_proto)
        .status()
        .expect("run protoc-gen-xmongo-trait");
    assert!(status.success(), "protoc-gen-xmongo-trait failed");

    // Build scripts run in their own process before compilation starts.
    unsafe { std::env::set_var("PROTOC", &protoc) };

    prost_build::Config::new()
        .message_attribute(
            ".",
            "#[derive(serde::Serialize, serde::Deserialize)] #[serde(default)]",
        )
        .enum_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]")
        .compile_protos(&[protocol_proto, model_proto], &[proto_dir])
        .unwrap();
}
