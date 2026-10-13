#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
fn main() {
    if let Err(err) =
        toolboxcraft_platform::self_update::apply(&std::env::args().skip(1).collect::<Vec<_>>())
    {
        toolboxcraft_platform::notify("ToolboxCraft update failed", &err.to_string());
        eprintln!("{err:#}");
        std::process::exit(1);
    }
}
