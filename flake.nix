{
  description = "asciii - the advanced but simple commandline interface for invoice invocation";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      forAllSystems = nixpkgs.lib.genAttrs [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      pkgsFor = system: nixpkgs.legacyPackages.${system};
    in
    {
      packages = forAllSystems (system: rec {
        asciii = (pkgsFor system).callPackage ./package.nix { };
        # with the terminal UI: `nix run .#asciii-tui -- tui`
        asciii-tui = (pkgsFor system).callPackage ./package.nix { withTui = true; };
        default = asciii;
      });

      apps = forAllSystems (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.asciii}/bin/asciii";
        };
      });

      devShells = forAllSystems (system: {
        default = import ./shell.nix { pkgs = pkgsFor system; };
      });

      overlays.default = final: _prev: {
        asciii = final.callPackage ./package.nix { };
      };

      formatter = forAllSystems (system: (pkgsFor system).nixfmt-rfc-style);
    };
}
