fn main() {
    // CARGO_CFG_TARGET_OS is the target; cfg!(windows) here would test the host.
    if std::env::var_os("CARGO_CFG_TARGET_OS").is_some_and(|os| os == "windows") {
        println!("cargo:rerun-if-changed=windows/mayi.rc");
        println!("cargo:rerun-if-changed=windows/mayi.exe.manifest");
        embed_resource::compile("windows/mayi.rc", embed_resource::NONE)
            .manifest_required()
            .expect("failed to embed the comctl32 v6 manifest");
    }
}
