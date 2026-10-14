//! Windows gives a process's main thread a 1 MiB stack, against 8 MiB on Linux and macOS. The
//! binary drives its whole async entrypoint with `block_on` on that thread, and in debug builds the
//! resulting futures overflow 1 MiB (the Windows CI client died with "thread 'main' has overflowed
//! its stack"). Reserve a larger stack for the binary on Windows; the reservation is address space
//! only, committed lazily.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    const MAIN_THREAD_STACK_BYTES: u32 = 16 * 1024 * 1024;
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins=/STACK:{MAIN_THREAD_STACK_BYTES}");
    } else {
        println!("cargo:rustc-link-arg-bins=-Wl,--stack,{MAIN_THREAD_STACK_BYTES}");
    }
}
