{
  description = "peepers, a system monitor and task manager for Linux";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { nixpkgs, ... }:
    let
      system = "x86_64-linux";
      peepers = nixpkgs.legacyPackages.${system}.callPackage ./package.nix { };
    in
    {
      packages.${system} = {
        inherit peepers;
        default = peepers;
      };
    };
}
