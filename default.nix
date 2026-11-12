# Non-flake entry point: `nix-build`, `nix-env -f . -i`.
{
  pkgs ? import ./nixpkgs.nix,
}:
pkgs.callPackage ./package.nix { }
