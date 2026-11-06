fn main() {
    if std::env::var_os("CARGO_FEATURE_DESKTOP").is_some() {
        println!("cargo:rerun-if-changed=native/ui.cpp");
        let mut ui = cc::Build::new();
        ui.cpp(true).file("native/ui.cpp").flag("-std=c++17");
        let mut packages = Vec::new();
        for name in ["Qt6Widgets", "Qt6Network"] {
            let package = pkg_config::Config::new()
                .cargo_metadata(false)
                .probe(name)
                .unwrap_or_else(|e| panic!("desktop needs {name}: {e}"));
            for include in &package.include_paths {
                ui.include(include);
            }
            packages.push(package);
        }
        ui.compile("hyprrecorder_ui");
        for package in packages {
            link(package);
        }
    }
    if std::env::var_os("CARGO_FEATURE_RECORDER").is_some() {
        println!("cargo:rerun-if-changed=native/audio.c");
        let package = pkg_config::Config::new()
            .cargo_metadata(false)
            .probe("libpulse")
            .unwrap_or_else(|e| panic!("recorder needs libpulse development files: {e}"));
        let mut audio = cc::Build::new();
        audio.file("native/audio.c").flag("-std=gnu11");
        for include in &package.include_paths {
            audio.include(include);
        }
        audio.compile("hyprrecorder_audio");
        link(package);
    }
    if std::env::var_os("CARGO_FEATURE_PORTAL_SPIKE").is_some() {
        println!("cargo:rerun-if-changed=native/portal.c");
        let mut build = cc::Build::new();
        build
            .file("native/portal.c")
            .flag("-std=gnu11")
            .warnings(true);
        let packages: Vec<_> = ["gio-unix-2.0", "libpipewire-0.3"]
            .into_iter()
            .map(|name| {
                pkg_config::Config::new()
                    .cargo_metadata(false)
                    .probe(name)
                    .unwrap_or_else(|e| panic!("portal-spike needs {name} development files: {e}"))
            })
            .collect();
        for package in &packages {
            for include in &package.include_paths {
                build.include(include);
            }
        }
        build.compile("hyprrecorder_portal");
        for package in packages {
            for path in package.link_paths {
                println!("cargo:rustc-link-search=native={}", path.display());
            }
            for library in package.libs {
                println!("cargo:rustc-link-lib={library}");
            }
        }
    }
    println!("cargo:rerun-if-changed=native/video.c");
    if std::env::var_os("CARGO_FEATURE_CAPTURE_SPIKE").is_none() {
        return;
    }
    let mut build = cc::Build::new();
    build.file("native/video.c").flag("-std=c11").warnings(true);
    let mut packages = Vec::new();
    if std::env::var_os("CARGO_FEATURE_GPU_ENCODE_SPIKE").is_some() {
        build.define("HR_GPU_ENCODE", None);
        let package = pkg_config::Config::new()
            .cargo_metadata(false)
            .probe("libavfilter")
            .unwrap_or_else(|e| {
                panic!("gpu-encode-spike needs libavfilter development files: {e}")
            });
        for include in &package.include_paths {
            build.include(include);
        }
        packages.push(package);
    }
    for library in ["libavcodec", "libavformat", "libavutil", "libswscale"] {
        let package = pkg_config::Config::new()
            .cargo_metadata(false)
            .probe(library)
            .unwrap_or_else(|e| panic!("capture-spike needs {library} development files: {e}"));
        for include in &package.include_paths {
            build.include(include);
        }
        packages.push(package);
    }
    build.compile("hyprrecorder_video");
    if std::env::var_os("CARGO_FEATURE_DMABUF_SPIKE").is_some() {
        println!("cargo:rerun-if-changed=native/dmabuf.c");
        let mut gpu = cc::Build::new();
        gpu.file("native/dmabuf.c").flag("-std=c11").warnings(true);
        for library in ["gbm", "libdrm"] {
            let package = pkg_config::Config::new()
                .cargo_metadata(false)
                .probe(library)
                .unwrap_or_else(|e| panic!("dmabuf-spike needs {library} development files: {e}"));
            for include in &package.include_paths {
                gpu.include(include);
            }
            packages.push(package);
        }
        gpu.compile("hyprrecorder_dmabuf");
    }
    // FFmpeg must follow the bridge archive for --as-needed linkers.
    for package in packages {
        for path in package.link_paths {
            println!("cargo:rustc-link-search=native={}", path.display());
        }
        for library in package.libs {
            println!("cargo:rustc-link-lib={library}");
        }
    }
    for name in [
        "PKG_CONFIG_PATH",
        "PKG_CONFIG_LIBDIR",
        "PKG_CONFIG_SYSROOT_DIR",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
}

fn link(package: pkg_config::Library) {
    for path in package.link_paths {
        println!("cargo:rustc-link-search=native={}", path.display());
    }
    for library in package.libs {
        println!("cargo:rustc-link-lib={library}");
    }
}
