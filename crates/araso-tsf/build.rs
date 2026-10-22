fn main() {
    println!("cargo:rerun-if-changed=res/araso.rc");
    println!("cargo:rerun-if-changed=res/araso.ico");
    embed_resource::compile("res/araso.rc", embed_resource::NONE).manifest_required().unwrap();
}
