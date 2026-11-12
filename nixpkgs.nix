# nixpkgs pinned to the revision recorded in flake.lock, for the non-flake entry points.
let
  lock = (builtins.fromJSON (builtins.readFile ./flake.lock)).nodes.nixpkgs.locked;
in
import (fetchTarball {
  url = "https://github.com/NixOS/nixpkgs/archive/${lock.rev}.tar.gz";
  sha256 = lock.narHash;
}) { }
