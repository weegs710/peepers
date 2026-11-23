{
  description = "peepers, a system monitor and task manager for Linux";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

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
