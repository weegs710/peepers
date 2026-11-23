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
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      peepers = pkgs.callPackage ./package.nix { };
    in
    {
      packages.${system} = {
        inherit peepers;
        default = peepers;
      };

      devShells.${system}.default = import ./shell.nix { inherit pkgs; };
    };
}
