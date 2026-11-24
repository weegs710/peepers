{
  description = "peepers, a system monitor and task manager for Linux";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  nixConfig = {
    extra-substituters = [ "https://peepers.cachix.org" ];
    extra-trusted-public-keys = [ "peepers.cachix.org-1:rP90IXXdICg4J7eVWuddscdW8QCobU/a945JKWcxWF8=" ];
  };

  outputs =
    { nixpkgs, ... }:
    let
      inherit (nixpkgs) lib;
      # every system flakes conventionally expose that peepers can build on (meta.platforms is Linux)
      forAllSystems =
        f:
        lib.genAttrs (lib.intersectLists lib.systems.flakeExposed lib.platforms.linux) (
          system: f nixpkgs.legacyPackages.${system}
        );
    in
    {
      packages = forAllSystems (
        pkgs:
        let
          peepers = pkgs.callPackage ./package.nix { };
        in
        {
          inherit peepers;
          default = peepers;
        }
      );

      devShells = forAllSystems (pkgs: {
        default = import ./shell.nix { inherit pkgs; };
      });
    };
}
