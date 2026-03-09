#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod state;

use state::AppState;

fn main() {
    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::projects::list_projects,
            commands::projects::add_project,
            commands::projects::remove_project_cmd,
            commands::services::get_project_status,
            commands::services::start_all,
            commands::services::stop_all,
            commands::services::start_service,
            commands::services::stop_service,
            commands::logs::get_logs,
            commands::dependencies::check_dependencies,
            commands::diagrams::generate_diagram,
            commands::analysis::analyze_project_cmd,
            commands::docs::read_project_readme,
            commands::docs::list_project_docs,
            commands::docs::read_project_doc,
        ])
        .run(tauri::generate_context!())
        .expect("error running devlaunch desktop");
}
