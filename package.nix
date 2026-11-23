{
  lib,
  rustPlatform,
  pkg-config,
  wayland,
  libxkbcommon,
  vulkan-loader,
  libx11,
  libxcursor,
  libxi,
}:
let
  runtimeLibs = [
    wayland
    libxkbcommon
    vulkan-loader
  ];
  # winit opens these only in an X11 session, so they are needed at run time and not to build
  x11Libs = [
    libx11
    libxcursor
    libxi
  ];
in
rustPlatform.buildRustPackage {
  pname = "peepers";
  inherit ((lib.importTOML ./Cargo.toml).workspace.package) version;

  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./Cargo.toml
      ./Cargo.lock
      ./crates
      ./packaging/peepers.desktop
      ./packaging/peepers.png
    ];
  };

  cargoLock.lockFile = ./Cargo.lock;
  cargoBuildFlags = [
    "-p"
    "peepers"
  ];

  # these read the live machine, and the build sandbox has no /sys, system bus, udisks, desktop entries or real mounts
  checkFlags = map (test: "--skip=${test}") [
    "apps::tests::this_session_lists_apps_once_each_in_name_order"
    "cpu::tests::this_machine_reports_a_topology"
    "desktop::tests::this_session_has_applications_installed"
    "drivers::tests::the_running_kernel_lists_loaded_and_built_in_modules"
    "health::tests::a_drive_on_this_machine_reports_its_health"
    "health::tests::a_drive_without_sata_smart_says_so_plainly"
    "network::tests::loopback_is_recognised_by_its_hardware_type"
    "report::tests::this_machine_reports_its_memory_and_a_disk"
    "sensors::tests::this_machine_has_a_chip_with_a_temperature_sensor"
    "sessions::tests::this_machine_has_a_session_for_this_user"
    "sysfs::tests::numbered_dirs_ignore_other_names"
    "systemd::tests::this_machine_lists_services_with_an_active_one"
    "volumes::tests::this_machine_lists_its_root_with_a_size"
    "tests::the_rail_shows_a_light_for_each_disk"
  ];

  nativeBuildInputs = [ pkg-config ];
  buildInputs = runtimeLibs;

  # an RPATH and not an LD_LIBRARY_PATH wrapper, which would leak into the commands peepers runs
  postFixup = ''
    patchelf --add-rpath ${lib.makeLibraryPath (runtimeLibs ++ x11Libs)} $out/bin/peepers
  '';

  postInstall = ''
    install -Dm644 packaging/peepers.desktop $out/share/applications/peepers.desktop
    install -Dm644 packaging/peepers.png $out/share/icons/hicolor/512x512/apps/peepers.png
  '';

  meta = {
    description = "System monitor and task manager for Linux";
    homepage = "https://github.com/weegs710/peepers";
    license = lib.licenses.mit;
    mainProgram = "peepers";
    platforms = lib.platforms.linux;
  };
}
