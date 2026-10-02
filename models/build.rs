fn main() -> Result<(), Box<dyn std::error::Error>> {
    // **No gRPC for the wasm reducer (issue #98).** `tonic`→`hyper`→`mio` do not build for
    // `wasm32-unknown-unknown`, and the reducer uses only the `prost` *messages*. Emitting no
    // clients/servers keeps the generated code free of any `tonic` reference, which is what lets
    // `models` drop tonic from its wasm dependency set (`Cargo.toml`). The host target is unchanged.
    let wasm = std::env::var("TARGET")
        .map(|t| t.starts_with("wasm32"))
        .unwrap_or(false);

    tonic_prost_build::configure()
        .build_server(!wasm)
        .build_client(!wasm)
        // `New.injections` is a `map<string, Par>` that participates in the content-addressed
        // state hash. Generate it as a `BTreeMap` so protobuf encoding iterates keys in sorted
        // order (a `HashMap` would make the post-state hash depend on process hash-seed order).
        .btree_map(".rholang.New")
        .compile_protos(
            &[
                "proto/casper.proto",
                "proto/routing.proto",
                "proto/kademlia.proto",
                "proto/RhoTypes.proto",
                "proto/service_error.proto",
                "proto/propose_service_common.proto",
                "proto/propose_service_v1.proto",
                "proto/deploy_service_common.proto",
                "proto/deploy_service_v1.proto",
                "proto/repl.proto",
            ],
            &["proto"],
        )?;
    Ok(())
}
