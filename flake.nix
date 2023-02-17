{
  description = "Simple reverse proxy";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    flake-utils.url = "github:numtide/flake-utils";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs = {
        nixpkgs.follows = "nixpkgs";
        flake-utils.follows = "flake-utils";
      };
    };
  };

  outputs = { nixpkgs, crane, flake-utils, rust-overlay, ... }:
    let
      toScreamingSnake = text: nixpkgs.lib.toUpper (builtins.replaceStrings [ "-" ] [ "_" ] text);
      regexFilter = regex: path: (builtins.match regex path) != null;
      # Produce toolchain necessary for rust cross compilation
      # Example: 
      # `makeCrossToolchain "x86_64-linux" "aarch64-linux"` 
      # to cross compile from x86 to aarch64
      makeCrossToolchain = localSystem: crossSystem: rec {
        inherit crossSystem;

        # Configure pkgs for cross compilation
        pkgs = import nixpkgs {
          inherit crossSystem localSystem;
          overlays = [ (import rust-overlay) ];
        };

        # Resolve rust target triple from the target platform; 
        # i.e. aarch64-linux -> aarch64-unknown-linux-gnu;
        rustTarget = pkgs.rust.toRustTarget pkgs.stdenv.targetPlatform;
        # Configure rust toolchain to include cross target 
        rustToolchain = pkgs.pkgsBuildHost.rust-bin.stable.latest.default.override {
          targets = [ rustTarget ];
        };
        # Use rust toolchain with crane 
        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;
      };

      # Applies crosss toolchain to makeCrate 
      applyCross = base: { pkgs, rustTarget, craneLib, ... }: pkgs.callPackage makeCrate (base // {
        inherit rustTarget craneLib;
      });

      makeCrate =
        { craneLib
        , rustTarget
        , darwin
        , lib
        , stdenv
        , pname
        , version
        }: craneLib.buildPackage {
          inherit pname version;

          src = lib.cleanSourceWith {
            src = ./.;
            filter = path: type:
              (regexFilter ".*\.md$" path)
              || (regexFilter ".*\.ya?ml$" path)
              || (craneLib.filterCargoSources path type);
          };

          # Builder machine dependencies
          buildInputs = [
          ] ++ lib.optionals stdenv.isDarwin [
            darwin.Security
          ];

          # Host machine dependencies
          nativeBuildInputs = [ ];

          CARGO_BUILD_TARGET = rustTarget;

          # Use target platform linker 
          "CARGO_TARGET_${toScreamingSnake rustTarget}_LINKER" = "${stdenv.cc.targetPrefix}cc";

          # In case some of the build dependencies need to be compiled on the build system
          HOST_CC = "${stdenv.cc.nativePrefix}cc";
        };
    in
    flake-utils.lib.eachDefaultSystem (localSystem:
      let
        ormosTOML = builtins.fromTOML (builtins.readFile ./ormos/Cargo.toml);
        pname = ormosTOML.package.name;
        version = ormosTOML.package.version;

        base = { inherit pname version; };

        crossFor = makeCrossToolchain localSystem;

        native = crossFor localSystem;
        aarch64-cross = crossFor "aarch64-linux";
        x86_64-cross = crossFor "x86_64-linux";

        pkgs = native.pkgs;

        default = applyCross base native;
        aarch64-crate = applyCross base aarch64-cross;
        x86_64-crate = applyCross base x86_64-cross;

        makeImage = cross: crate: cross.pkgs.dockerTools.buildLayeredImage {
          name = pname;
          tag = "${version}-${crate.stdenv.targetPlatform.uname.processor}";
          # Magic value for the actual ts;
          created = "now";
          contents = [ crate ];
          config = {
            Entrypoint = [ "${crate}/bin/${pname}" ];
          };
        };
      in
      {
        checks = { inherit default; };
        formatter = nixpkgs.legacyPackages.${localSystem}.nixpkgs-fmt;

        packages = rec {
          inherit default aarch64-crate x86_64-crate;

          image =
            if pkgs.stdenv.isAarch64
            then aarch64-image
            else x86_64-image;

          aarch64-image = makeImage aarch64-cross aarch64-crate;
          x86_64-image = makeImage x86_64-cross x86_64-crate;
        };

      });
}
