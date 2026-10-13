fn main() {
    println!("cargo:rerun-if-changed=../../assets/icons/toolboxcraft.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("../../assets/icons/toolboxcraft.ico")
            .set("ProductName", "ToolboxCraft")
            .set("FileDescription", "ToolboxCraft")
            .set("OriginalFilename", "toolboxcraft.exe")
            .compile()
            .expect("Cannot embed ToolboxCraft Windows icon and version resources");
    }
}
