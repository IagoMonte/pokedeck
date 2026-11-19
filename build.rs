fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&["listar", "set_modo", "set_cfg", "set_hunt", "recalcular_hunt", "hunt_info", "set_layout", "set_foco", "set_sel", "add_conta", "remover", "abrir_trade", "fechar_trade", "ponte_msg"])),
    )
    .expect("tauri build");
}
