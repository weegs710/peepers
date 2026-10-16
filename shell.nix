{
  pkgs ? import <nixpkgs> { },
}:
let
  runtimeLibs = with pkgs; [
    libxkbcommon
    vulkan-loader
    wayland
  ];
in
pkgs.mkShell {
  packages = [ pkgs.pkg-config ];
  buildInputs = runtimeLibs;
  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibs;
  shellHook = ''export CARGO_TARGET_DIR="$HOME/.cache/cargo-target/peepers"'';
}
