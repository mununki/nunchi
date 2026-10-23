fn main() {
    println!("cargo:rerun-if-changed=res/araso.rc");
    println!("cargo:rerun-if-changed=res/araso.ico");
    println!("cargo:rerun-if-changed=res/mode-on.ico");
    println!("cargo:rerun-if-changed=res/mode-off.ico");
    embed_resource::compile("res/araso.rc", embed_resource::NONE).manifest_required().unwrap();
}
