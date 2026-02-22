const COMMANDS: &[&str] = &[
    "get_products",
    "purchase",
    "restore_purchases",
    "get_subscription_status",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).ios_path("ios").build();
}
