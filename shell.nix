# Development shell: `nix-shell` (or `nix develop` via the flake).
{
  pkgs ? import ./nixpkgs.nix,
}:
let
  asciii = pkgs.callPackage ./package.nix { };
in
pkgs.mkShell {
  inputsFrom = [ asciii ];
  packages = with pkgs; [
    cargo
    rustc
    clippy
    rustfmt
    rust-analyzer
    git
  ];
  RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
}
