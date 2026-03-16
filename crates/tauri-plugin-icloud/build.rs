const COMMANDS: &[&str] = &[
    "check_icloud_available",
    "get_icloud_container_url",
    "trigger_download",
    "get_sync_status",
    "start_status_monitoring",
    "stop_status_monitoring",
    "migrate_to_icloud",
    "migrate_from_icloud",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).ios_path("ios").build();
}
