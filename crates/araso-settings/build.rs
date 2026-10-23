fn main() {
    println!("cargo:rerun-if-changed=res/settings.rc");
    println!("cargo:rerun-if-changed=../araso-tsf/res/araso.ico");
    embed_resource::compile("res/settings.rc", embed_resource::NONE).manifest_optional().unwrap();
}
