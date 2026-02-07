{
  pkgs,
  crane,
  lib,
  ...
}: let
  rustToolchain = pkgs.rust-bin.stable.latest.default;
  craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;
  workspaceRoot = builtins.path {
    name = "lattice-src";
    path = ../.;
  };

  # Aggressively prune the source tree for better caching.
  extraIgnores = ''
    *.nix
    *.md
    *.sh

    package.json
    assets.tar.gz

    /packages

    /flake.lock
    /renovate.json
    /scripts
    /packages/**/src

    node_modules/
    yarn.lock

    !/core/validation/README.md
    !/core/composition/README.md
    !/core/graphql-schema-diff/README.md
    !/packages/lattice-sdk/package.json
  '';

  src = pkgs.nix-gitignore.gitignoreSource [extraIgnores] (lib.cleanSourceWith {
    filter = lib.cleanSourceFilter;
    src = workspaceRoot;
  });

  version = pkgs.runCommand "getVersion" {} ''
    ${pkgs.dasel}/bin/dasel \
      --file ${../core/gateway-binary/Cargo.toml} \
      --selector package.version\
      --write - | tr -d "\n" > $out
  '';
in {
  packages.lattice-gateway = craneLib.buildPackage {
    inherit src;
    pname = "lattice-gateway";
    version = builtins.readFile version;
    stdenv = pkgs.clangStdenv;

    cargoBuildFlags = "-p lattice-gateway";
    cargoExtraArgs = "-p lattice-gateway";

    RUSTFLAGS = builtins.concatStringsSep " " [
      "-Arust-2018-idioms -Aunused-crate-dependencies"
      "-C linker=clang -C link-arg=-fuse-ld=lld"
    ];

    doCheck = false;

    nativeBuildInputs = [
      pkgs.pkg-config
      pkgs.openssl.dev
      pkgs.llvmPackages.bintools # lld
    ];
  };
}
