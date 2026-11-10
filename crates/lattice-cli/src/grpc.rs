use std::{collections::BTreeMap, fs, io, path::Path};

use lattice_config::GrpcConnectorConfig;
use lattice_connectors::grpc::{self, GrpcError};
use prost::Message;
use prost_types::FileDescriptorSet;
use serde::Serialize;
use thiserror::Error;

/// Errors raised while generating a subgraph from a gRPC descriptor set.
#[derive(Debug, Error)]
pub enum GrpcCliError {
    #[error("failed to read descriptor set: {0}")]
    Read(#[source] io::Error),
    #[error("failed to decode descriptor set (expected a protobuf FileDescriptorSet): {0}")]
    Decode(#[from] prost::DecodeError),
    #[error("failed to generate subgraph: {0}")]
    Generate(#[from] GrpcError),
    #[error("failed to write schema: {0}")]
    Write(#[source] io::Error),
}

/// A `lattice.yaml` subgraph block ready to paste under `subgraphs:`.
#[derive(Serialize)]
struct SubgraphSnippet {
    schema: String,
    grpc: GrpcConnectorConfig,
}

/// Generate a subgraph (SDL + gRPC connector config) from a descriptor set.
///
/// Reads a compiled protobuf `FileDescriptorSet` (from `protoc
/// --descriptor_set_out`), writes the GraphQL SDL to `schema_out`, and prints a
/// ready-to-paste `lattice.yaml` subgraph block to stdout.
///
/// # Errors
///
/// Returns [`GrpcCliError`] on read, decode, generation, or write failures.
pub fn run(descriptor_path: &Path, schema_out: &Path, name: &str, endpoint: &str) -> Result<(), GrpcCliError> {
    let bytes = fs::read(descriptor_path).map_err(GrpcCliError::Read)?;
    let fds = FileDescriptorSet::decode(bytes.as_slice())?;

    let generated = grpc::generate(&fds, endpoint, &descriptor_path.display().to_string())?;
    let field_count = generated.connector.fields.len();

    fs::write(schema_out, format!("{}\n", generated.sdl)).map_err(GrpcCliError::Write)?;

    let snippet = BTreeMap::from([(
        "subgraphs".to_string(),
        BTreeMap::from([(
            name.to_string(),
            SubgraphSnippet {
                schema: schema_out.display().to_string(),
                grpc: generated.connector,
            },
        )]),
    )]);

    // serde_yaml::to_string on an infallible, owned value does not fail here
    let yaml = serde_yaml::to_string(&snippet).unwrap_or_default();

    tracing::info!(
        schema = %schema_out.display(),
        fields = field_count,
        "generated subgraph from gRPC descriptor set"
    );
    println!("# Add this subgraph to your lattice.yaml:\n{yaml}");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost_types::{
        DescriptorProto, FieldDescriptorProto, FileDescriptorProto, MethodDescriptorProto, ServiceDescriptorProto,
        field_descriptor_proto::{Label, Type},
    };

    fn descriptor_bytes() -> Vec<u8> {
        let request = DescriptorProto {
            name: Some("GetPingRequest".to_string()),
            ..Default::default()
        };
        let response = DescriptorProto {
            name: Some("Pong".to_string()),
            field: vec![FieldDescriptorProto {
                name: Some("message".to_string()),
                number: Some(1),
                label: Some(Label::Optional as i32),
                r#type: Some(Type::String as i32),
                ..Default::default()
            }],
            ..Default::default()
        };
        let fds = FileDescriptorSet {
            file: vec![FileDescriptorProto {
                name: Some("ping.proto".to_string()),
                package: Some("ping.v1".to_string()),
                message_type: vec![request, response],
                service: vec![ServiceDescriptorProto {
                    name: Some("PingService".to_string()),
                    method: vec![MethodDescriptorProto {
                        name: Some("GetPing".to_string()),
                        input_type: Some(".ping.v1.GetPingRequest".to_string()),
                        output_type: Some(".ping.v1.Pong".to_string()),
                        client_streaming: Some(false),
                        server_streaming: Some(false),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        };
        fds.encode_to_vec()
    }

    #[test]
    fn run_writes_sdl_file_from_descriptor_set() {
        let dir = std::env::temp_dir().join(format!("lattice-grpc-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let desc = dir.join("ping.pb");
        let out = dir.join("out.graphql");
        fs::write(&desc, descriptor_bytes()).unwrap();

        run(&desc, &out, "ping", "http://ping.internal:50051").unwrap();

        let sdl = fs::read_to_string(&out).unwrap();
        assert!(sdl.contains("type Pong {"), "sdl:\n{sdl}");
        assert!(sdl.contains("input GetPingRequestInput {"), "sdl:\n{sdl}");
        assert!(
            sdl.contains("getPing(input: GetPingRequestInput!): Pong"),
            "sdl:\n{sdl}"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn run_rejects_non_descriptor_bytes() {
        let dir = std::env::temp_dir().join(format!("lattice-grpc-bad-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let desc = dir.join("bad.pb");
        let out = dir.join("out.graphql");
        // Not a valid protobuf FileDescriptorSet.
        fs::write(&desc, b"this is not protobuf, just random text bytes \xff\xfe").unwrap();

        let err = run(&desc, &out, "svc", "http://x:1");
        assert!(matches!(err, Err(GrpcCliError::Decode(_))), "got: {err:?}");

        fs::remove_dir_all(&dir).ok();
    }
}
