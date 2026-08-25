use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=c/fastpath.c");
    println!("cargo:rerun-if-changed=c/fastpath.h");

    let target = env::var("TARGET").expect("Cargo did not provide TARGET");
    let arch = env::var("CARGO_CFG_TARGET_ARCH").expect("Cargo did not provide target arch");
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo did not provide OUT_DIR"));
    let object = out_dir.join("fastpath.o");
    let archive = out_dir.join("librustix_fastpath.a");

    let compiler = tool_from_env("CC", &target).unwrap_or_else(|| match arch.as_str() {
        "x86_64" => "x86_64-linux-gnu-gcc".into(),
        "aarch64" => "aarch64-linux-gnu-gcc".into(),
        other => panic!("unsupported C fastpath architecture: {other}"),
    });
    let archiver = tool_from_env("AR", &target).unwrap_or_else(|| "ar".into());

    let mut compile = Command::new(&compiler);
    compile.args([
        "-c",
        "c/fastpath.c",
        "-o",
        object.to_str().expect("non-UTF8 build output path"),
        "-std=c11",
        "-O3",
        "-ffreestanding",
        "-fno-builtin",
        "-fno-pic",
        "-fno-pie",
        "-fno-stack-protector",
        "-fno-asynchronous-unwind-tables",
        "-fno-unwind-tables",
        "-fno-tree-loop-distribute-patterns",
        "-Wall",
        "-Wextra",
        "-Werror",
    ]);
    match arch.as_str() {
        "x86_64" => {
            compile.args(["-m64", "-mcmodel=kernel", "-mno-red-zone", "-march=x86-64"]);
        }
        "aarch64" => {
            compile.arg("-march=armv8-a");
        }
        _ => unreachable!(),
    }
    run(&mut compile, "C fastpath compiler");

    let mut archive_command = Command::new(&archiver);
    archive_command.args([
        "crs",
        archive.to_str().expect("non-UTF8 archive path"),
        object.to_str().expect("non-UTF8 object path"),
    ]);
    run(&mut archive_command, "C fastpath archiver");

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=rustix_fastpath");
}

fn tool_from_env(prefix: &str, target: &str) -> Option<String> {
    let target_key = target.replace('-', "_");
    env::var(format!("{prefix}_{target_key}"))
        .ok()
        .or_else(|| env::var(prefix).ok())
}

fn run(command: &mut Command, description: &str) {
    let status = command
        .status()
        .unwrap_or_else(|error| panic!("failed to start {description}: {error}"));
    if !status.success() {
        panic!("{description} exited with {status}");
    }
}
