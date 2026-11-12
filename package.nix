{
  lib,
  rustPlatform,
  pkg-config,
  installShellFiles,
  libgit2,
  zlib,
  # Cargo features to build with; `null` keeps the crate's default feature set.
  features ? null,
  # adds the terminal UI (`asciii tui`, cargo feature `tui`) to the features above
  withTui ? false,
}:

let
  cargoToml = lib.importTOML ./Cargo.toml;
in
rustPlatform.buildRustPackage {
  pname = cargoToml.package.name;
  inherit (cargoToml.package) version;

  # Only what the build needs, so `target/`, editor files etc. never reach the store.
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./Cargo.toml
      ./Cargo.lock
      ./build.rs
      ./src
      ./lang
      ./templates
      ./tests
      ./examples
      ./manual.md
      ./README.md
    ];
  };

  cargoLock.lockFile = ./Cargo.lock;

  buildNoDefaultFeatures = features != null;
  buildFeatures = lib.optionals (features != null) features ++ lib.optional withTui "tui";

  nativeBuildInputs = [
    pkg-config
    installShellFiles
  ];
  # nixpkgs sets LIBGIT2_NO_VENDOR=1, so libgit2-sys links against the system libgit2.
  buildInputs = [
    libgit2
    zlib
  ];

  # The config loader and some tests want a writable $HOME.
  preCheck = ''
    export HOME=$(mktemp -d)
  '';

  postInstall = ''
    export HOME=$(mktemp -d)
    for shell in bash zsh fish; do
      $out/bin/asciii complete $shell > asciii.$shell
    done
    installShellCompletion asciii.{bash,zsh,fish}
  '';

  meta = {
    description = "The advanced but simple commandline interface for invoice invocation";
    homepage = cargoToml.package.homepage;
    license = with lib.licenses; [
      mit
      asl20
    ];
    mainProgram = "asciii";
    platforms = lib.platforms.unix;
  };
}
