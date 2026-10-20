mod accounts;
mod binary_cookies;
mod commands;
mod crypto;
mod games;
mod launcher;
mod roblox;
mod updater;
mod windows;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            accounts::init(app.handle())?;

            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                updater::check_for_updates(&handle).await;
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_accounts,
            commands::get_accounts,
            commands::get_account_details,
            commands::delete_accounts,
            commands::export_accounts,
            commands::import_accounts,
            commands::launch_account,
            commands::kill_all_roblox,
            commands::show_alert,
            commands::confirm,
            games::discover_games,
            games::search_games,
            games::game_details,
            windows::open_website,
            windows::open_login_window,
        ])
        .run(tauri::generate_context!())
        .expect("error while running AltDeck");
}
