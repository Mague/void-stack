#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod state;

use state::AppState;

fn main() {
    // Fix PATH for macOS GUI apps launched from Finder/Launchpad/Dock.
    // Must be called before anything else so all child processes inherit the full PATH.
    let _ = fix_path_env::fix();

    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::projects::list_projects,
            commands::projects::add_project,
            commands::projects::remove_project_cmd,
            commands::projects::list_wsl_distros,
            commands::projects::browse_directory,
            commands::services::get_project_status,
            commands::services::start_all,
            commands::services::stop_all,
            commands::services::start_service,
            commands::services::stop_service,
            commands::logs::get_logs,
            commands::logs::filter_logs_cmd,
            commands::dependencies::check_dependencies,
            commands::diagrams::generate_diagram,
            commands::diagrams::save_diagram_file,
            commands::analysis::analyze_project_cmd,
            commands::docs::read_project_readme,
            commands::docs::list_project_docs,
            commands::docs::read_project_doc,
            commands::docs::read_project_file_cmd,
            commands::docs::list_project_files_cmd,
            commands::docs::generate_claudeignore_cmd,
            commands::space::scan_project_space,
            commands::space::scan_global_space,
            commands::space::delete_space_entry,
            commands::audit::run_security_audit,
            commands::debt::analyze_debt,
            commands::debt::save_debt_snapshot,
            commands::debt::list_debt_snapshots,
            commands::debt::compare_debt_snapshots,
            commands::docker::docker_analyze,
            commands::docker::docker_generate,
            commands::scan::scan_directory,
            commands::scan::add_service_cmd,
            commands::scan::remove_service_cmd,
            commands::scan::detect_docker_services,
            commands::scan::import_docker_services,
            commands::suggest::suggest_refactoring,
            commands::search::index_project_codebase_cmd,
            commands::search::semantic_search_cmd,
            commands::search::get_index_stats_cmd,
            commands::stats::get_token_stats_cmd,
        ])
        .run(tauri::generate_context!())
        .expect("error running void-stack desktop");
}
