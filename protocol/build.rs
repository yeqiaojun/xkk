use std::{
    fs,
    path::{Path, PathBuf},
};

use prost::Message;
use prost_types::{FileDescriptorSet, compiler::CodeGeneratorRequest};

const GENERATED_FILES: [&str; 5] = ["xkk.v1.rs", "xkk.xmongo.rs", "model.xmongo.rs", "xkk.registry.rs", "xkk.descriptor.bin"];

fn main() {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let proto_dir = manifest.join("../proto");
    let protocol_proto = proto_dir.join("xkk.proto");
    let model_proto = proto_dir.join("model.proto");
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("locate vendored protobuf compiler");
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());

    assert!(protoc.is_file(), "missing protocol compiler: {}", protoc.display());

    println!("cargo:rerun-if-changed={}", protocol_proto.display());
    println!("cargo:rerun-if-changed={}", model_proto.display());
    println!("cargo:rerun-if-env-changed=XKK_PROTO_UPDATE");

    let checked_in_dir = manifest.join("generated");
    for file in GENERATED_FILES {
        println!("cargo:rerun-if-changed={}", checked_in_dir.join(file).display());
    }

    unsafe { std::env::set_var("PROTOC", &protoc) };

    let descriptor_path = out_dir.join("xkk.descriptor.bin");
    prost_build::Config::new()
        .message_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)] #[serde(default)]")
        .enum_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]")
        .file_descriptor_set_path(&descriptor_path)
        .compile_protos(&[protocol_proto, model_proto], &[proto_dir])
        .expect("compile xkk protobuf");

    normalize_generated_comments(&out_dir.join("xkk.v1.rs"));
    generate_xmongo_traits(&descriptor_path, &out_dir);
    generate_registry(&descriptor_path, &out_dir.join("xkk.registry.rs"));
    sync_checked_in_generated(&out_dir, &checked_in_dir);
}

fn normalize_generated_comments(path: &Path) {
    let source = fs::read_to_string(path).expect("read generated protobuf source");
    let normalized = source.replace(r#"bson:"\_id""#, r#"bson:"_id""#);
    if normalized != source {
        fs::write(path, normalized).expect("normalize generated protobuf source");
    }
}

fn generate_registry(descriptor_path: &Path, output_path: &Path) {
    let descriptor = fs::read(descriptor_path).expect("read xkk descriptor set");
    let source = xproto::generate_registry(
        &descriptor,
        &xproto::RegistryOptions {
            enum_name: "MsgId",
            unspecified_name: "UNSPECIFIED",
            id_range: 100..=u16::MAX as i32,
            message_path: "pb",
            enum_path: "crate::pb::MsgId",
            wire_trait_path: "xproto::WireMessage",
            request_trait_path: "xproto::RequestMessage",
            response_trait_path: "xproto::ResponseMessage",
            notification_trait_path: "xproto::NotificationMessage",
            registry_type: "xproto::MessageRegistry",
            result_type: "Result<(), ProtocolError>",
            registration: xproto::RegistrationKind::Application,
            emit_constants: false,
        },
    )
    .expect("generate xkk registry");
    fs::write(output_path, source).expect("write generated message registry");
}

fn generate_xmongo_traits(descriptor_path: &Path, out_dir: &Path) {
    let descriptor = fs::read(descriptor_path).expect("read xkk descriptor set");
    let descriptor = FileDescriptorSet::decode(descriptor.as_slice()).expect("decode xkk descriptor set");
    let response = protoc_gen_xmongo_trait::generate(CodeGeneratorRequest {
        file_to_generate: vec!["xkk.proto".to_string(), "model.proto".to_string()],
        proto_file: descriptor.file,
        ..CodeGeneratorRequest::default()
    });
    if let Some(error) = response.error {
        panic!("generate xmongo traits: {error}");
    }
    for file in response.file {
        let name = file.name.expect("xmongo generated file has a name");
        let content = file.content.expect("xmongo generated file has content");
        let content = format!("{}\n", content.trim_end());
        fs::write(out_dir.join(name), content).expect("write generated xmongo traits");
    }
}

fn sync_checked_in_generated(out_dir: &Path, checked_in_dir: &Path) {
    let update = std::env::var_os("XKK_PROTO_UPDATE").is_some();
    if update {
        fs::create_dir_all(checked_in_dir).expect("create checked-in protocol directory");
    }

    for file in GENERATED_FILES {
        let generated = fs::read(out_dir.join(file)).expect("read generated protocol artifact");
        let checked_in_path = checked_in_dir.join(file);

        if update {
            if fs::read(&checked_in_path).ok().as_deref() != Some(generated.as_slice()) {
                fs::write(&checked_in_path, generated).expect("update checked-in protocol artifact");
            }
            continue;
        }

        let checked_in = fs::read(&checked_in_path).unwrap_or_else(|_| {
            panic!("missing checked-in protocol artifact {}; run bash scripts/gen-proto.sh", checked_in_path.display())
        });
        if file.ends_with(".bin") {
            assert_eq!(checked_in, generated, "checked-in protocol artifact {checked_in_path:?} is stale");
        } else {
            assert!(
                same_text(&checked_in, &generated),
                "checked-in protocol artifact {} is stale; run bash scripts/gen-proto.sh",
                checked_in_path.display()
            );
        }
    }
}

fn same_text(left: &[u8], right: &[u8]) -> bool {
    String::from_utf8_lossy(left).replace("\r\n", "\n") == String::from_utf8_lossy(right).replace("\r\n", "\n")
}
