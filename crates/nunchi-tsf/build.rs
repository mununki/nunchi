fn main() {
    println!("cargo:rerun-if-changed=res/nunchi.rc");
    println!("cargo:rerun-if-changed=res/nunchi.ico");
    println!("cargo:rerun-if-changed=res/mode-on.ico");
    println!("cargo:rerun-if-changed=res/mode-off.ico");
    embed_resource::compile("res/nunchi.rc", embed_resource::NONE).manifest_required().unwrap();
}
