# Arquitectura: void-stack

## Resumen

| | |
|---|---|
| **Patron** | Layered (confianza: 80%) |
| **Lenguaje** | Rust |
| **Modulos** | 201 archivos |
| **LOC** | 41506 lineas |
| **Deps externas** | 34 paquetes |

## Distribucion por Capas

| Capa | Archivos | LOC | % |
|------|----------|-----|---|
| Controller | 51 | 8442 | 20% |
| Service | 28 | 4203 | 10% |
| Repository | 7 | 2462 | 5% |
| Model | 17 | 4775 | 11% |
| Utility | 86 | 17502 | 42% |
| Config | 10 | 3200 | 7% |
| Test | 2 | 922 | 2% |

## Anti-patrones Detectados

### Alta Severidad

- **God Class**: 'crates/void-stack-mcp/src/server.rs' es demasiado grande (41 funciones)
  - *Sugerencia*: Dividir 'crates/void-stack-mcp/src/server.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'crates/void-stack-core/src/detector/mod.rs' es demasiado grande (41 funciones)
  - *Sugerencia*: Dividir 'crates/void-stack-core/src/detector/mod.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'crates/void-stack-core/src/ai/mod.rs' es demasiado grande (38 funciones)
  - *Sugerencia*: Dividir 'crates/void-stack-core/src/ai/mod.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'crates/void-stack-core/src/log_filter.rs' es demasiado grande (44 funciones)
  - *Sugerencia*: Dividir 'crates/void-stack-core/src/log_filter.rs' en modulos mas pequenos con responsabilidades claras
- **Fat Controller**: Controller 'crates/void-stack-cli/src/commands/project.rs' tiene 420 LOC — demasiada logica
  - *Sugerencia*: Mover la logica de negocio a una capa de servicio
- **Fat Controller**: Controller 'crates/void-stack-core/src/analyzer/imports/classifier/signals.rs' tiene 1009 LOC — demasiada logica
  - *Sugerencia*: Mover la logica de negocio a una capa de servicio
- **Excessive Coupling**: 'crates/void-stack-core/src/analyzer/history.rs' importa 31 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas
- **Excessive Coupling**: 'crates/void-stack-core/src/lib.rs' importa 23 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas

### Severidad Media

- **God Class**: 'crates/void-stack-tui/src/main.rs' es demasiado grande (611 LOC)
  - *Sugerencia*: Dividir 'crates/void-stack-tui/src/main.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'crates/void-stack-core/src/docker/parse.rs' es demasiado grande (779 LOC y 33 funciones)
  - *Sugerencia*: Dividir 'crates/void-stack-core/src/docker/parse.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'crates/void-stack-core/src/docker/generate_dockerfile/mod.rs' es demasiado grande (31 funciones)
  - *Sugerencia*: Dividir 'crates/void-stack-core/src/docker/generate_dockerfile/mod.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'crates/void-stack-core/src/file_reader.rs' es demasiado grande (22 funciones)
  - *Sugerencia*: Dividir 'crates/void-stack-core/src/file_reader.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'crates/void-stack-core/src/global_config/mod.rs' es demasiado grande (30 funciones)
  - *Sugerencia*: Dividir 'crates/void-stack-core/src/global_config/mod.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'crates/void-stack-core/src/diagram/api_routes/mod.rs' es demasiado grande (21 funciones)
  - *Sugerencia*: Dividir 'crates/void-stack-core/src/diagram/api_routes/mod.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'crates/void-stack-core/src/diagram/architecture/externals.rs' es demasiado grande (616 LOC y 27 funciones)
  - *Sugerencia*: Dividir 'crates/void-stack-core/src/diagram/architecture/externals.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'crates/void-stack-core/src/vector_index/mod.rs' es demasiado grande (28 funciones)
  - *Sugerencia*: Dividir 'crates/void-stack-core/src/vector_index/mod.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'crates/void-stack-core/src/runner/docker.rs' es demasiado grande (33 funciones)
  - *Sugerencia*: Dividir 'crates/void-stack-core/src/runner/docker.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'crates/void-stack-core/src/runner/local.rs' es demasiado grande (28 funciones)
  - *Sugerencia*: Dividir 'crates/void-stack-core/src/runner/local.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'crates/void-stack-core/src/hooks.rs' es demasiado grande (25 funciones)
  - *Sugerencia*: Dividir 'crates/void-stack-core/src/hooks.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'crates/void-stack-core/src/audit/vuln_patterns/mod.rs' es demasiado grande (22 funciones)
  - *Sugerencia*: Dividir 'crates/void-stack-core/src/audit/vuln_patterns/mod.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'crates/void-stack-core/src/audit/vuln_patterns/error_handling.rs' es demasiado grande (26 funciones)
  - *Sugerencia*: Dividir 'crates/void-stack-core/src/audit/vuln_patterns/error_handling.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'crates/void-stack-core/src/audit/config_check.rs' es demasiado grande (29 funciones)
  - *Sugerencia*: Dividir 'crates/void-stack-core/src/audit/config_check.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'crates/void-stack-core/src/audit/secrets.rs' es demasiado grande (25 funciones)
  - *Sugerencia*: Dividir 'crates/void-stack-core/src/audit/secrets.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'crates/void-stack-core/src/stats.rs' es demasiado grande (25 funciones)
  - *Sugerencia*: Dividir 'crates/void-stack-core/src/stats.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'crates/void-stack-core/src/claudeignore.rs' es demasiado grande (31 funciones)
  - *Sugerencia*: Dividir 'crates/void-stack-core/src/claudeignore.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'crates/void-stack-core/src/space/mod.rs' es demasiado grande (25 funciones)
  - *Sugerencia*: Dividir 'crates/void-stack-core/src/space/mod.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'crates/void-stack-core/src/analyzer/patterns/antipatterns.rs' es demasiado grande (28 funciones)
  - *Sugerencia*: Dividir 'crates/void-stack-core/src/analyzer/patterns/antipatterns.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'crates/void-stack-core/src/analyzer/complexity.rs' es demasiado grande (849 LOC y 35 funciones)
  - *Sugerencia*: Dividir 'crates/void-stack-core/src/analyzer/complexity.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'crates/void-stack-core/src/analyzer/imports/classifier/signals.rs' es demasiado grande (1009 LOC)
  - *Sugerencia*: Dividir 'crates/void-stack-core/src/analyzer/imports/classifier/signals.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'crates/void-stack-core/src/analyzer/imports/mod.rs' es demasiado grande (26 funciones)
  - *Sugerencia*: Dividir 'crates/void-stack-core/src/analyzer/imports/mod.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'crates/void-stack-core/src/analyzer/docs/markdown.rs' es demasiado grande (816 LOC y 33 funciones)
  - *Sugerencia*: Dividir 'crates/void-stack-core/src/analyzer/docs/markdown.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'crates/void-stack-core/src/analyzer/cross_project.rs' es demasiado grande (29 funciones)
  - *Sugerencia*: Dividir 'crates/void-stack-core/src/analyzer/cross_project.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'crates/void-stack-core/src/analyzer/history.rs' es demasiado grande (639 LOC y 30 funciones)
  - *Sugerencia*: Dividir 'crates/void-stack-core/src/analyzer/history.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'crates/void-stack-core/src/analyzer/best_practices/mod.rs' es demasiado grande (31 funciones)
  - *Sugerencia*: Dividir 'crates/void-stack-core/src/analyzer/best_practices/mod.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'crates/void-stack-core/src/ignore.rs' es demasiado grande (23 funciones)
  - *Sugerencia*: Dividir 'crates/void-stack-core/src/ignore.rs' en modulos mas pequenos con responsabilidades claras
- **Fat Controller**: Controller 'crates/void-stack-cli/src/commands/analysis/analyze.rs' tiene 400 LOC — demasiada logica
  - *Sugerencia*: Mover la logica de negocio a una capa de servicio
- **Fat Controller**: Controller 'crates/void-stack-cli/src/commands/service.rs' tiene 265 LOC — demasiada logica
  - *Sugerencia*: Mover la logica de negocio a una capa de servicio
- **Fat Controller**: Controller 'crates/void-stack-mcp/src/tools/analysis.rs' tiene 226 LOC — demasiada logica
  - *Sugerencia*: Mover la logica de negocio a una capa de servicio
- **Fat Controller**: Controller 'crates/void-stack-mcp/src/tools/projects.rs' tiene 255 LOC — demasiada logica
  - *Sugerencia*: Mover la logica de negocio a una capa de servicio
- **Fat Controller**: Controller 'crates/void-stack-mcp/src/tools/search.rs' tiene 273 LOC — demasiada logica
  - *Sugerencia*: Mover la logica de negocio a una capa de servicio
- **Fat Controller**: Controller 'crates/void-stack-desktop/src/commands/docker.rs' tiene 237 LOC — demasiada logica
  - *Sugerencia*: Mover la logica de negocio a una capa de servicio
- **Fat Controller**: Controller 'crates/void-stack-desktop/src/commands/analysis.rs' tiene 300 LOC — demasiada logica
  - *Sugerencia*: Mover la logica de negocio a una capa de servicio
- **Fat Controller**: Controller 'crates/void-stack-desktop/src/commands/debt.rs' tiene 279 LOC — demasiada logica
  - *Sugerencia*: Mover la logica de negocio a una capa de servicio
- **Fat Controller**: Controller 'crates/void-stack-desktop/src/commands/projects.rs' tiene 256 LOC — demasiada logica
  - *Sugerencia*: Mover la logica de negocio a una capa de servicio
- **Fat Controller**: Controller 'crates/void-stack-desktop/src/commands/scan.rs' tiene 328 LOC — demasiada logica
  - *Sugerencia*: Mover la logica de negocio a una capa de servicio
- **Fat Controller**: Controller 'crates/void-stack-core/src/global_config/mod.rs' tiene 387 LOC — demasiada logica
  - *Sugerencia*: Mover la logica de negocio a una capa de servicio
- **Fat Controller**: Controller 'crates/void-stack-core/src/audit/vuln_patterns/mod.rs' tiene 391 LOC — demasiada logica
  - *Sugerencia*: Mover la logica de negocio a una capa de servicio
- **Fat Controller**: Controller 'crates/void-stack-core/src/analyzer/imports/dart.rs' tiene 309 LOC — demasiada logica
  - *Sugerencia*: Mover la logica de negocio a una capa de servicio
- **Excessive Coupling**: 'crates/void-stack-core/src/runner/local.rs' importa 13 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas
- **Excessive Coupling**: 'crates/void-stack-core/src/analyzer/best_practices/mod.rs' importa 12 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas
- **Excessive Coupling**: 'crates/void-stack-core/src/analyzer/imports/rust_lang.rs' importa 11 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas
- **Excessive Coupling**: 'crates/void-stack-tui/src/ui/mod.rs' importa 15 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas
- **Excessive Coupling**: 'crates/void-stack-mcp/src/tools/mod.rs' importa 11 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas
- **Excessive Coupling**: 'crates/void-stack-core/src/vector_index/indexer.rs' importa 11 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas
- **Excessive Coupling**: 'crates/void-stack-desktop/src/commands/mod.rs' importa 15 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas
- **Excessive Coupling**: 'crates/void-stack-core/src/analyzer/docs/markdown.rs' importa 20 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas
- **Excessive Coupling**: 'crates/void-stack-core/src/vector_index/mod.rs' importa 19 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas
- **Excessive Coupling**: 'crates/void-stack-core/src/manager/process.rs' importa 14 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas
- **Excessive Coupling**: 'crates/void-stack-core/src/global_config/mod.rs' importa 11 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas
- **Excessive Coupling**: 'crates/void-stack-core/src/detector/mod.rs' importa 17 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas
- **Excessive Coupling**: 'crates/void-stack-core/src/runner/docker.rs' importa 15 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas
- **Excessive Coupling**: 'crates/void-stack-core/src/audit/vuln_patterns/mod.rs' importa 11 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas
- **Excessive Coupling**: 'crates/void-stack-core/src/diagram/mod.rs' importa 11 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas

## Mapa de Dependencias

```mermaid
graph LR
    subgraph controller ["Controller"]
        crates_void_stack_cli_src_commands_docker_rs["docker.rs"]
        crates_void_stack_cli_src_commands_analysis_audit_rs["audit.rs"]
        crates_void_stack_cli_src_commands_analysis_diagram_rs["diagram.rs"]
        crates_void_stack_cli_src_commands_analysis_suggest_rs["suggest.rs"]
        crates_void_stack_cli_src_commands_analysis_mod_rs["mod.rs"]
        crates_void_stack_cli_src_commands_analysis_analyze_rs["analyze.rs"]
        crates_void_stack_cli_src_commands_analysis_search_rs["search.rs"]
        crates_void_stack_cli_src_commands_deps_rs["deps.rs"]
        crates_void_stack_cli_src_commands_service_rs["service.rs"]
        crates_void_stack_cli_src_commands_mod_rs["mod.rs"]
        crates_void_stack_cli_src_commands_daemon_rs["daemon.rs"]
        crates_void_stack_cli_src_commands_project_rs["project.rs"]
        crates_void_stack_mcp_src_tools_docs_rs["docs.rs"]
        crates_void_stack_mcp_src_tools_docker_rs["docker.rs"]
        crates_void_stack_mcp_src_tools_analysis_rs["analysis.rs"]
        crates_void_stack_mcp_src_tools_suggest_rs["suggest.rs"]
        crates_void_stack_mcp_src_tools_services_rs["services.rs"]
        crates_void_stack_mcp_src_tools_mod_rs["mod.rs"]
        crates_void_stack_mcp_src_tools_space_rs["space.rs"]
        crates_void_stack_mcp_src_tools_debt_rs["debt.rs"]
        crates_void_stack_mcp_src_tools_projects_rs["projects.rs"]
        crates_void_stack_mcp_src_tools_stats_rs["stats.rs"]
        crates_void_stack_mcp_src_tools_diagrams_rs["diagrams.rs"]
        crates_void_stack_mcp_src_tools_search_rs["search.rs"]
        crates_void_stack_desktop_src_commands_docs_rs["docs.rs"]
        crates_void_stack_desktop_src_commands_docker_rs["docker.rs"]
        crates_void_stack_desktop_src_commands_analysis_rs["analysis.rs"]
        crates_void_stack_desktop_src_commands_audit_rs["audit.rs"]
        crates_void_stack_desktop_src_commands_suggest_rs["suggest.rs"]
        crates_void_stack_desktop_src_commands_services_rs["services.rs"]
        crates_void_stack_desktop_src_commands_mod_rs["mod.rs"]
        crates_void_stack_desktop_src_commands_dependencies_rs["dependencies.rs"]
        crates_void_stack_desktop_src_commands_space_rs["space.rs"]
        crates_void_stack_desktop_src_commands_debt_rs["debt.rs"]
        crates_void_stack_desktop_src_commands_projects_rs["projects.rs"]
        crates_void_stack_desktop_src_commands_stats_rs["stats.rs"]
        crates_void_stack_desktop_src_commands_diagrams_rs["diagrams.rs"]
        crates_void_stack_desktop_src_commands_search_rs["search.rs"]
        crates_void_stack_desktop_src_commands_scan_rs["scan.rs"]
        crates_void_stack_desktop_src_commands_logs_rs["logs.rs"]
        crates_void_stack_core_src_global_config_mod_rs["mod.rs"]
        crates_void_stack_core_src_diagram_api_routes_node_rs["node.rs"]
        crates_void_stack_core_src_diagram_api_routes_python_rs["python.rs"]
        crates_void_stack_core_src_diagram_drawio_mod_rs["mod.rs"]
        crates_void_stack_core_src_diagram_service_detection_rs["service_detection.rs"]
        crates_void_stack_core_src_diagram_db_models_drift_rs["drift.rs"]
        crates_void_stack_core_src_audit_vuln_patterns_mod_rs["mod.rs"]
        crates_void_stack_core_src_ai_ollama_rs["ollama.rs"]
        crates_void_stack_core_src_analyzer_imports_dart_rs["dart.rs"]
        crates_void_stack_core_src_analyzer_imports_classifier_signals_rs["signals.rs"]
        crates_void_stack_core_src_analyzer_docs_mod_rs["mod.rs"]
    end
    subgraph service ["Service"]
        crates_void_stack_tui_src_i18n_rs["i18n.rs"]
        crates_void_stack_proto_src_client_rs["client.rs"]
        crates_void_stack_core_src_docker_terraform_rs["terraform.rs"]
        crates_void_stack_core_src_diagram_architecture_infra_terraform_rs["terraform.rs"]
        crates_void_stack_core_src_backend_rs["backend.rs"]
        crates_void_stack_core_src_runner_docker_rs["docker.rs"]
        crates_void_stack_core_src_runner_local_rs["local.rs"]
        crates_void_stack_core_src_runner_rs["runner.rs"]
        crates_void_stack_core_src_manager_mod_rs["mod.rs"]
        crates_void_stack_core_src_manager_state_rs["state.rs"]
        crates_void_stack_core_src_detector_docker_rs["docker.rs"]
        crates_void_stack_core_src_detector_env_rs["env.rs"]
        crates_void_stack_core_src_detector_clippy_rs["clippy.rs"]
        crates_void_stack_core_src_detector_flutter_rs["flutter.rs"]
        crates_void_stack_core_src_detector_rust_lang_rs["rust_lang.rs"]
        crates_void_stack_core_src_detector_react_doctor_rs["react_doctor.rs"]
        crates_void_stack_core_src_detector_ollama_rs["ollama.rs"]
        crates_void_stack_core_src_detector_mod_rs["mod.rs"]
        crates_void_stack_core_src_detector_node_rs["node.rs"]
        crates_void_stack_core_src_detector_ruff_rs["ruff.rs"]
        crates_void_stack_core_src_detector_golang_rs["golang.rs"]
        crates_void_stack_core_src_detector_flutter_analyze_rs["flutter_analyze.rs"]
        crates_void_stack_core_src_detector_python_rs["python.rs"]
        crates_void_stack_core_src_detector_cuda_rs["cuda.rs"]
        crates_void_stack_core_src_detector_golangci_lint_rs["golangci_lint.rs"]
        crates_void_stack_core_src_analyzer_patterns_antipatterns_rs["antipatterns.rs"]
        crates_void_stack_core_src_analyzer_imports_javascript_rs["javascript.rs"]
        crates_void_stack_core_src_analyzer_imports_python_rs["python.rs"]
    end
    subgraph repository ["Repository"]
        crates_void_stack_core_src_docker_generate_compose_rs["generate_compose.rs"]
        crates_void_stack_core_src_diagram_db_models_sequelize_rs["sequelize.rs"]
        crates_void_stack_core_src_diagram_db_models_gorm_rs["gorm.rs"]
        crates_void_stack_core_src_vector_index_db_rs["db.rs"]
        crates_void_stack_core_src_vector_index_mod_rs["mod.rs"]
        crates_void_stack_core_src_vector_index_indexer_rs["indexer.rs"]
        crates_void_stack_core_src_stats_rs["stats.rs"]
    end
    subgraph model ["Model"]
        crates_void_stack_mcp_src_server_rs["server.rs"]
        crates_void_stack_tui_src_app_rs["app.rs"]
        crates_void_stack_desktop_src_state_rs["state.rs"]
        crates_void_stack_daemon_src_lifecycle_rs["lifecycle.rs"]
        crates_void_stack_core_src_docker_mod_rs["mod.rs"]
        crates_void_stack_core_src_diagram_mod_rs["mod.rs"]
        crates_void_stack_core_src_diagram_db_models_mod_rs["mod.rs"]
        crates_void_stack_core_src_diagram_db_models_python_rs["python.rs"]
        crates_void_stack_core_src_error_rs["error.rs"]
        crates_void_stack_core_src_audit_findings_rs["findings.rs"]
        crates_void_stack_core_src_space_mod_rs["mod.rs"]
        crates_void_stack_core_src_analyzer_patterns_mod_rs["mod.rs"]
        crates_void_stack_core_src_analyzer_graph_rs["graph.rs"]
        crates_void_stack_core_src_analyzer_imports_golang_rs["golang.rs"]
        crates_void_stack_core_src_analyzer_cross_project_rs["cross_project.rs"]
        crates_void_stack_core_src_analyzer_history_rs["history.rs"]
        crates_void_stack_core_src_model_rs["model.rs"]
    end
    subgraph utility ["Utility"]
        crates_void_stack_cli_src_main_rs["main.rs"]
        crates_void_stack_mcp_src_main_rs["main.rs"]
        crates_void_stack_tui_src_ui_security_rs["security.rs"]
        crates_void_stack_tui_src_ui_analysis_rs["analysis.rs"]
        crates_void_stack_tui_src_ui_tabs_rs["tabs.rs"]
        crates_void_stack_tui_src_ui_header_rs["header.rs"]
        crates_void_stack_tui_src_ui_services_rs["services.rs"]
        crates_void_stack_tui_src_ui_mod_rs["mod.rs"]
        crates_void_stack_tui_src_ui_space_rs["space.rs"]
        crates_void_stack_tui_src_ui_debt_rs["debt.rs"]
        crates_void_stack_tui_src_ui_help_rs["help.rs"]
        crates_void_stack_tui_src_ui_projects_rs["projects.rs"]
        crates_void_stack_tui_src_ui_footer_rs["footer.rs"]
        crates_void_stack_tui_src_ui_stats_rs["stats.rs"]
        crates_void_stack_tui_src_main_rs["main.rs"]
        crates_void_stack_proto_build_rs["build.rs"]
        crates_void_stack_proto_src_lib_rs["lib.rs"]
        crates_void_stack_desktop_build_rs["build.rs"]
        crates_void_stack_desktop_src_main_rs["main.rs"]
        crates_void_stack_daemon_src_server_rs["server.rs"]
        crates_void_stack_daemon_src_main_rs["main.rs"]
        crates_void_stack_core_src_docker_helm_rs["helm.rs"]
        crates_void_stack_core_src_docker_parse_rs["parse.rs"]
        crates_void_stack_core_src_docker_kubernetes_rs["kubernetes.rs"]
        crates_void_stack_core_src_docker_generate_dockerfile_flutter_rs["flutter.rs"]
        crates_void_stack_core_src_docker_generate_dockerfile_rust_lang_rs["rust_lang.rs"]
        crates_void_stack_core_src_docker_generate_dockerfile_go_rs["go.rs"]
        crates_void_stack_core_src_docker_generate_dockerfile_python_rs["python.rs"]
        crates_void_stack_core_src_file_reader_rs["file_reader.rs"]
        crates_void_stack_core_src_global_config_paths_rs["paths.rs"]
        crates_void_stack_core_src_global_config_scanner_rs["scanner.rs"]
        crates_void_stack_core_src_global_config_project_ops_rs["project_ops.rs"]
        crates_void_stack_core_src_security_rs["security.rs"]
        crates_void_stack_core_src_diagram_api_routes_swagger_rs["swagger.rs"]
        crates_void_stack_core_src_diagram_api_routes_grpc_rs["grpc.rs"]
        crates_void_stack_core_src_diagram_api_routes_mod_rs["mod.rs"]
        crates_void_stack_core_src_diagram_architecture_crates_rs["crates.rs"]
        crates_void_stack_core_src_diagram_architecture_infra_mod_rs["mod.rs"]
        crates_void_stack_core_src_diagram_architecture_infra_helm_rs["helm.rs"]
        crates_void_stack_core_src_diagram_architecture_infra_kubernetes_rs["kubernetes.rs"]
        crates_void_stack_core_src_diagram_architecture_mod_rs["mod.rs"]
        crates_void_stack_core_src_diagram_drawio_api_routes_rs["api_routes.rs"]
        crates_void_stack_core_src_diagram_drawio_db_models_rs["db_models.rs"]
        crates_void_stack_core_src_diagram_drawio_architecture_rs["architecture.rs"]
        crates_void_stack_core_src_diagram_drawio_common_rs["common.rs"]
        crates_void_stack_core_src_diagram_db_models_proto_rs["proto.rs"]
        crates_void_stack_core_src_diagram_db_models_prisma_rs["prisma.rs"]
        crates_void_stack_core_src_vector_index_chunker_rs["chunker.rs"]
        crates_void_stack_core_src_vector_index_stats_rs["stats.rs"]
        crates_void_stack_core_src_vector_index_search_rs["search.rs"]
        crates_void_stack_core_src_vector_index_voidignore_rs["voidignore.rs"]
        crates_void_stack_core_src_lib_rs["lib.rs"]
        crates_void_stack_core_src_process_util_rs["process_util.rs"]
        crates_void_stack_core_src_hooks_rs["hooks.rs"]
        crates_void_stack_core_src_manager_url_rs["url.rs"]
        crates_void_stack_core_src_manager_logs_rs["logs.rs"]
        crates_void_stack_core_src_audit_vuln_patterns_xss_rs["xss.rs"]
        crates_void_stack_core_src_audit_vuln_patterns_injection_rs["injection.rs"]
        crates_void_stack_core_src_audit_vuln_patterns_network_rs["network.rs"]
        crates_void_stack_core_src_audit_vuln_patterns_error_handling_rs["error_handling.rs"]
        crates_void_stack_core_src_audit_vuln_patterns_crypto_rs["crypto.rs"]
        crates_void_stack_core_src_audit_deps_rs["deps.rs"]
        crates_void_stack_core_src_audit_mod_rs["mod.rs"]
        crates_void_stack_core_src_ai_mod_rs["mod.rs"]
        crates_void_stack_core_src_ai_prompt_rs["prompt.rs"]
        crates_void_stack_core_src_log_filter_rs["log_filter.rs"]
        crates_void_stack_core_src_claudeignore_rs["claudeignore.rs"]
        crates_void_stack_core_src_analyzer_complexity_rs["complexity.rs"]
        crates_void_stack_core_src_analyzer_imports_rust_lang_rs["rust_lang.rs"]
        crates_void_stack_core_src_analyzer_imports_classifier_mod_rs["mod.rs"]
        crates_void_stack_core_src_analyzer_imports_mod_rs["mod.rs"]
        crates_void_stack_core_src_analyzer_docs_coverage_rs["coverage.rs"]
        crates_void_stack_core_src_analyzer_docs_sanitize_rs["sanitize.rs"]
        crates_void_stack_core_src_analyzer_docs_markdown_rs["markdown.rs"]
        crates_void_stack_core_src_analyzer_mod_rs["mod.rs"]
        crates_void_stack_core_src_analyzer_explicit_debt_rs["explicit_debt.rs"]
        crates_void_stack_core_src_analyzer_best_practices_flutter_rs["flutter.rs"]
        crates_void_stack_core_src_analyzer_best_practices_react_rs["react.rs"]
        crates_void_stack_core_src_analyzer_best_practices_report_rs["report.rs"]
        crates_void_stack_core_src_analyzer_best_practices_mod_rs["mod.rs"]
        crates_void_stack_core_src_analyzer_best_practices_vue_rs["vue.rs"]
        crates_void_stack_core_src_analyzer_best_practices_rust_bp_rs["rust_bp.rs"]
        crates_void_stack_core_src_analyzer_best_practices_oxlint_rs["oxlint.rs"]
        crates_void_stack_core_src_analyzer_best_practices_go_bp_rs["go_bp.rs"]
        crates_void_stack_core_src_analyzer_best_practices_python_rs["python.rs"]
        crates_void_stack_core_src_ignore_rs["ignore.rs"]
    end
    subgraph config ["Config"]
        crates_void_stack_core_src_docker_generate_dockerfile_mod_rs["mod.rs"]
        crates_void_stack_core_src_docker_generate_dockerfile_node_rs["node.rs"]
        crates_void_stack_core_src_diagram_architecture_externals_rs["externals.rs"]
        crates_void_stack_core_src_config_rs["config.rs"]
        crates_void_stack_core_src_manager_process_rs["process.rs"]
        crates_void_stack_core_src_audit_vuln_patterns_config_rs["config.rs"]
        crates_void_stack_core_src_audit_config_check_rs["config_check.rs"]
        crates_void_stack_core_src_audit_secrets_rs["secrets.rs"]
        crates_void_stack_core_src_analyzer_best_practices_angular_rs["angular.rs"]
        crates_void_stack_core_src_analyzer_best_practices_astro_rs["astro.rs"]
    end
    subgraph test ["Test"]
        crates_void_stack_core_tests_integration_analysis_rs["integration_analysis.rs"]
        crates_void_stack_core_src_analyzer_imports_classifier_tests_rs["tests.rs"]
    end
    crates_void_stack_cli_src_main_rs --> crates_void_stack_cli_src_commands_mod_rs
    crates_void_stack_cli_src_commands_mod_rs --> crates_void_stack_cli_src_commands_analysis_mod_rs
    crates_void_stack_mcp_src_main_rs --> crates_void_stack_mcp_src_tools_mod_rs
    crates_void_stack_tui_src_main_rs --> crates_void_stack_tui_src_ui_mod_rs
    crates_void_stack_desktop_src_main_rs --> crates_void_stack_desktop_src_commands_mod_rs
    crates_void_stack_core_src_docker_mod_rs --> crates_void_stack_core_src_docker_generate_dockerfile_mod_rs
    crates_void_stack_core_src_diagram_mod_rs --> crates_void_stack_core_src_diagram_api_routes_mod_rs
    crates_void_stack_core_src_diagram_mod_rs --> crates_void_stack_core_src_diagram_architecture_mod_rs
    crates_void_stack_core_src_diagram_mod_rs --> crates_void_stack_core_src_diagram_db_models_mod_rs
    crates_void_stack_core_src_diagram_mod_rs --> crates_void_stack_core_src_diagram_drawio_mod_rs
    crates_void_stack_core_src_diagram_architecture_mod_rs --> crates_void_stack_core_src_diagram_architecture_infra_mod_rs
    crates_void_stack_core_src_lib_rs --> crates_void_stack_core_src_ai_mod_rs
    crates_void_stack_core_src_lib_rs --> crates_void_stack_core_src_analyzer_mod_rs
    crates_void_stack_core_src_lib_rs --> crates_void_stack_core_src_audit_mod_rs
    crates_void_stack_core_src_lib_rs --> crates_void_stack_core_src_detector_mod_rs
    crates_void_stack_core_src_lib_rs --> crates_void_stack_core_src_diagram_mod_rs
    crates_void_stack_core_src_lib_rs --> crates_void_stack_core_src_docker_mod_rs
    crates_void_stack_core_src_lib_rs --> crates_void_stack_core_src_global_config_mod_rs
    crates_void_stack_core_src_lib_rs --> crates_void_stack_core_src_manager_mod_rs
    crates_void_stack_core_src_lib_rs --> crates_void_stack_core_src_space_mod_rs
    crates_void_stack_core_src_lib_rs --> crates_void_stack_core_src_vector_index_mod_rs
    crates_void_stack_core_src_runner_rs --> crates_void_stack_core_src_docker_mod_rs
    crates_void_stack_core_src_audit_mod_rs --> crates_void_stack_core_src_audit_vuln_patterns_mod_rs
    crates_void_stack_core_src_analyzer_imports_mod_rs --> crates_void_stack_core_src_analyzer_imports_classifier_mod_rs
    crates_void_stack_core_src_analyzer_mod_rs --> crates_void_stack_core_src_analyzer_best_practices_mod_rs
    crates_void_stack_core_src_analyzer_mod_rs --> crates_void_stack_core_src_analyzer_docs_mod_rs
    crates_void_stack_core_src_analyzer_mod_rs --> crates_void_stack_core_src_analyzer_imports_mod_rs
    crates_void_stack_core_src_analyzer_mod_rs --> crates_void_stack_core_src_analyzer_patterns_mod_rs
```

## Modulos

| Archivo | Capa | LOC | Clases | Funciones |
|---------|------|-----|--------|----------|
| `crates/void-stack-core/src/analyzer/imports/classifier/signals.rs` | Controller | 1009 | 0 | 0 |
| `crates/void-stack-core/src/analyzer/complexity.rs` | Utility | 849 | 2 | 35 |
| `crates/void-stack-core/src/analyzer/docs/markdown.rs` | Utility | 816 | 0 | 33 |
| `crates/void-stack-core/src/docker/parse.rs` | Utility | 779 | 0 | 33 |
| `crates/void-stack-core/src/analyzer/history.rs` | Model | 639 | 5 | 30 |
| `crates/void-stack-core/src/diagram/architecture/externals.rs` | Config | 616 | 0 | 27 |
| `crates/void-stack-tui/src/main.rs` | Utility | 611 | 1 | 16 |
| `crates/void-stack-mcp/src/server.rs` | Model | 596 | 1 | 41 |
| `crates/void-stack-core/src/analyzer/imports/classifier/tests.rs` | Test | 588 | 0 | 40 |
| `crates/void-stack-core/src/analyzer/best_practices/mod.rs` | Utility | 568 | 7 | 31 |
| `crates/void-stack-core/src/audit/config_check.rs` | Config | 547 | 0 | 29 |
| `crates/void-stack-core/src/audit/secrets.rs` | Config | 532 | 1 | 25 |
| `crates/void-stack-core/src/space/mod.rs` | Model | 506 | 2 | 25 |
| `crates/void-stack-core/src/ai/mod.rs` | Utility | 503 | 6 | 38 |
| `crates/void-stack-core/src/analyzer/patterns/antipatterns.rs` | Service | 498 | 3 | 28 |
| `crates/void-stack-core/src/runner/docker.rs` | Service | 484 | 2 | 33 |
| `crates/void-stack-tui/src/ui/analysis.rs` | Utility | 471 | 0 | 6 |
| `crates/void-stack-core/src/analyzer/imports/mod.rs` | Utility | 466 | 3 | 26 |
| `crates/void-stack-core/src/log_filter.rs` | Utility | 465 | 1 | 44 |
| `crates/void-stack-core/src/vector_index/mod.rs` | Repository | 457 | 0 | 28 |
| `crates/void-stack-core/src/analyzer/cross_project.rs` | Model | 457 | 2 | 29 |
| `crates/void-stack-core/src/detector/mod.rs` | Service | 442 | 4 | 41 |
| `crates/void-stack-core/src/audit/vuln_patterns/error_handling.rs` | Utility | 438 | 0 | 26 |
| `crates/void-stack-core/src/runner/local.rs` | Service | 433 | 1 | 28 |
| `crates/void-stack-core/src/claudeignore.rs` | Utility | 427 | 1 | 31 |
| `crates/void-stack-core/src/diagram/db_models/sequelize.rs` | Repository | 422 | 0 | 13 |
| `crates/void-stack-cli/src/commands/project.rs` | Controller | 420 | 0 | 12 |
| `crates/void-stack-cli/src/commands/analysis/analyze.rs` | Controller | 400 | 0 | 17 |
| `crates/void-stack-tui/src/app.rs` | Model | 397 | 4 | 20 |
| `crates/void-stack-core/src/audit/vuln_patterns/mod.rs` | Controller | 391 | 0 | 22 |

*... y 171 módulos más (ordenados por LOC, mostrando top 30)*

## Dependencias Externas

- `anyhow`
- `app`
- `async_trait`
- `chrono`
- `clap`
- `complexity`
- `coverage`
- `crossterm`
- `explicit_debt`
- `graph`
- `hnsw_rs`
- `patterns`
- `ratatui`
- `regex`
- `rmcp`
- `rusqlite`
- `schemars`
- `serde`
- `serde_yaml`
- `server`
- `signals`
- `state`
- `std`
- `super`
- `tauri`
- `tempfile`
- `thiserror`
- `tokio`
- `tokio_stream`
- `tonic`
- `tracing`
- `uuid`
- `void_stack_core`
- `void_stack_proto`

## Complejidad Ciclomatica

**Promedio**: 3.5 | **Funciones analizadas**: 1850 | **Funciones complejas (>=10)**: 176

| Funcion | Archivo | Linea | CC | LOC |
|---------|---------|-------|----|-----|
| `es` !! | `i18n.rs` | 33 | 152 | 173 |
| `en` !! | `i18n.rs` | 225 | 152 | 173 |
| `cmd_docker` !! | `docker.rs` | 7 | 34 | 152 |
| `generate` !! | `mod.rs` | 19 | 34 | 147 |
| `detect_from_env` !! | `externals.rs` | 51 | 33 | 93 |
| `detect_service_tech` !! | `projects.rs` | 72 | 32 | 58 |
| `render_db_models_page` !! | `db_models.rs` | 7 | 31 | 159 |
| `parse_k8s_yaml` !! | `kubernetes.rs` | 102 | 30 | 119 |
| `scan_weak_cryptography` !! | `crypto.rs` | 67 | 30 | 87 |
| `scan_subprojects` !! | `scanner.rs` | 8 | 29 | 74 |
| `collect_files_recursive` !! | `indexer.rs` | 401 | 29 | 90 |
| `main` !! | `main.rs` | 309 | 28 | 173 |
| `cmd_start` !! | `service.rs` | 15 | 28 | 136 |
| `parse_swagger_yaml_routes` !! | `swagger.rs` | 98 | 28 | 117 |
| `check` !! | `python.rs` | 21 | 28 | 121 |
| `detect_crate_relationships` !! | `crates.rs` | 6 | 26 | 70 |
| `index_project` !! | `indexer.rs` | 131 | 26 | 167 |
| `parse_file` !! | `javascript.rs` | 17 | 26 | 71 |
| `install_hint` !! | `process_util.rs` | 139 | 25 | 49 |
| `scan_debug_mode` !! | `config_check.rs` | 55 | 25 | 72 |

## Metricas de Acoplamiento

| Modulo | Fan-in | Fan-out |
|--------|--------|--------|
| `history.rs` | 0 | 31 |
| `lib.rs` | 0 | 23 |
| `markdown.rs` | 0 | 20 |
| `mod.rs` | 1 | 19 |
| `mod.rs` | 1 | 17 |
| `mod.rs` | 1 | 15 |
| `docker.rs` | 0 | 15 |
| `mod.rs` | 1 | 15 |
| `process.rs` | 0 | 14 |
| `local.rs` | 0 | 13 |
| `mod.rs` | 1 | 12 |
| `mod.rs` | 1 | 11 |
| `mod.rs` | 1 | 11 |
| `mod.rs` | 1 | 11 |
| `indexer.rs` | 0 | 11 |
| `mod.rs` | 1 | 11 |
| `rust_lang.rs` | 0 | 11 |
| `hooks.rs` | 0 | 10 |
| `mod.rs` | 1 | 10 |
| `mod.rs` | 1 | 10 |

## Test Coverage

⚠️ No se encontraron reportes de cobertura.

Para generar reportes de cobertura, ejecutar:
- **Rust**: `cargo install cargo-tarpaulin && cargo tarpaulin --out xml` (genera `cobertura.xml`)

## Deuda Tecnica Explicita

**Total**: 31 marcadores (FIXME: 1, HACK: 1, OPTIMIZE: 6, TEMP: 10, TODO: 11, XXX: 2)

| Archivo | Linea | Tipo | Texto |
|---------|-------|------|-------|
| `...-cli/src/commands/analysis/analyze.rs` | 228 | TODO | /FIXME/HACK). |
| `crates/void-stack-cli/src/main.rs` | 245 | OPTIMIZE | d .voidignore for semantic index quality |
| `crates/void-stack-cli/src/main.rs` | 266 | OPTIMIZE | d for the project's tech stack |
| `crates/void-stack-core/src/ai/prompt.rs` | 15 | OPTIMIZE | d prompt from analysis results. |
| `...ck-core/src/analyzer/explicit_debt.rs` | 3 | TODO | , FIXME, HACK, XXX, OPTIMIZE, BUG, TEMP, WORKAROUND. |
| `...ck-core/src/analyzer/explicit_debt.rs` | 228 | TODO | implement error handling", "rust"); |
| `...ck-core/src/analyzer/explicit_debt.rs` | 238 | FIXME | this is broken\n\ |
| `...ck-core/src/analyzer/explicit_debt.rs` | 240 | HACK | temporary workaround", |
| `...ck-core/src/analyzer/explicit_debt.rs` | 266 | TODO | add validation\n/* FIXME: memory leak */", |
| `...ck-core/src/analyzer/explicit_debt.rs` | 282 | TODO | a\n// FIXME: b\n// HACK: c\n// XXX: d\n// OPTIMIZE: e\n//... |
| `...ck-core/src/analyzer/explicit_debt.rs` | 304 | TODO | lowercase", "rust"); |
| `...ck-core/src/analyzer/explicit_debt.rs` | 314 | TODO | add logging\n}\n", |
| `...ck-core/src/analyzer/explicit_debt.rs` | 325 | TODO | should be skipped", |
| `...s/void-stack-core/src/analyzer/mod.rs` | 32 | TODO | , FIXME, HACK, etc.) found in source code. |
| `.../void-stack-core/src/audit/secrets.rs` | 179 | TEMP | late/placeholder syntax that |
| `.../void-stack-core/src/audit/secrets.rs` | 184 | TEMP | late variables, string interpolation |
| `.../void-stack-core/src/audit/secrets.rs` | 189 | TEMP | late generation) |
| `.../void-stack-core/src/audit/secrets.rs` | 347 | TEMP | late/format string generation |
| `...s/void-stack-core/src/claudeignore.rs` | 3 | OPTIMIZE | d `.claudeignore` patterns |
| `...re/src/diagram/db_models/sequelize.rs` | 174 | XXX | ' or "xxx") from a line. |
| `...re/src/diagram/db_models/sequelize.rs` | 190 | XXX | from a line and map to a simple type. |
| `...es/void-stack-core/src/diagram/mod.rs` | 97 | TEMP | dir alive by leaking it (test only) |
| `...src/docker/generate_dockerfile/mod.rs` | 3 | TEMP | lates follow official best practices: |
| `...es/void-stack-core/src/file_reader.rs` | 141 | TEMP | project directory for testing. |
| `crates/void-stack-core/src/stats.rs` | 135 | TEMP | file::tempdir) |
| `...tack-core/src/vector_index/indexer.rs` | 289 | TEMP | dir, then rename atomically to avoid |
| `...id-stack-core/src/vector_index/mod.rs` | 516 | TEMP | directory and index stats on disk |
| `...k-core/src/vector_index/voidignore.rs` | 5 | OPTIMIZE | d for semantic index quality. |
| `...k-core/src/vector_index/voidignore.rs` | 12 | OPTIMIZE | d for the vector index. |
| `...ck-core/tests/integration_analysis.rs` | 147 | TODO | add error handling\nfunction run() { /* FIXME: memory lea... |
| `crates/void-stack-tui/src/ui/debt.rs` | 10 | TODO | /FIXME/HACK markers found in source code. |

---
*Generado automaticamente por VoidStack*


---

# Arquitectura: crates/void-stack-cli

## Resumen

| | |
|---|---|
| **Patron** | Unknown (confianza: 30%) |
| **Lenguaje** | Rust |
| **Modulos** | 13 archivos |
| **LOC** | 2009 lineas |
| **Deps externas** | 5 paquetes |

## Distribucion por Capas

| Capa | Archivos | LOC | % |
|------|----------|-----|---|
| Controller | 12 | 1652 | 82% |
| Utility | 1 | 357 | 17% |

## Anti-patrones Detectados

### Alta Severidad

- **Fat Controller**: Controller 'src/commands/project.rs' tiene 420 LOC — demasiada logica
  - *Sugerencia*: Mover la logica de negocio a una capa de servicio

### Severidad Media

- **Fat Controller**: Controller 'src/commands/analysis/analyze.rs' tiene 400 LOC — demasiada logica
  - *Sugerencia*: Mover la logica de negocio a una capa de servicio
- **Fat Controller**: Controller 'src/commands/service.rs' tiene 265 LOC — demasiada logica
  - *Sugerencia*: Mover la logica de negocio a una capa de servicio
- **No Service Layer**: Proyecto tiene 12 controllers pero ninguna capa de servicio
  - *Sugerencia*: Crear una capa de servicios para separar la logica de negocio de los endpoints

## Mapa de Dependencias

```mermaid
graph LR
    subgraph controller ["Controller"]
        src_commands_docker_rs["docker.rs"]
        src_commands_analysis_audit_rs["audit.rs"]
        src_commands_analysis_diagram_rs["diagram.rs"]
        src_commands_analysis_suggest_rs["suggest.rs"]
        src_commands_analysis_mod_rs["mod.rs"]
        src_commands_analysis_analyze_rs["analyze.rs"]
        src_commands_analysis_search_rs["search.rs"]
        src_commands_deps_rs["deps.rs"]
        src_commands_service_rs["service.rs"]
        src_commands_mod_rs["mod.rs"]
        src_commands_daemon_rs["daemon.rs"]
        src_commands_project_rs["project.rs"]
    end
    subgraph utility ["Utility"]
        src_main_rs["main.rs"]
    end
    src_main_rs --> src_commands_mod_rs
    src_commands_mod_rs --> src_commands_analysis_mod_rs
```

## Modulos

| Archivo | Capa | LOC | Clases | Funciones |
|---------|------|-----|--------|----------|
| `src/commands/project.rs` | Controller | 420 | 0 | 12 |
| `src/commands/analysis/analyze.rs` | Controller | 400 | 0 | 17 |
| `src/main.rs` | Utility | 357 | 3 | 1 |
| `src/commands/service.rs` | Controller | 265 | 0 | 5 |
| `src/commands/docker.rs` | Controller | 149 | 0 | 1 |
| `src/commands/analysis/suggest.rs` | Controller | 80 | 0 | 1 |
| `src/commands/analysis/audit.rs` | Controller | 69 | 0 | 1 |
| `src/commands/analysis/diagram.rs` | Controller | 69 | 0 | 1 |
| `src/commands/analysis/search.rs` | Controller | 67 | 0 | 3 |
| `src/commands/deps.rs` | Controller | 61 | 0 | 1 |
| `src/commands/daemon.rs` | Controller | 54 | 0 | 3 |
| `src/commands/analysis/mod.rs` | Controller | 12 | 0 | 0 |
| `src/commands/mod.rs` | Controller | 6 | 0 | 0 |

## Dependencias Externas

- `anyhow`
- `clap`
- `std`
- `void_stack_core`
- `void_stack_proto`

## Complejidad Ciclomatica

**Promedio**: 7.6 | **Funciones analizadas**: 46 | **Funciones complejas (>=10)**: 10

| Funcion | Archivo | Linea | CC | LOC |
|---------|---------|-------|----|-----|
| `cmd_docker` !! | `docker.rs` | 7 | 34 | 152 |
| `main` !! | `main.rs` | 309 | 28 | 173 |
| `cmd_start` !! | `service.rs` | 15 | 28 | 136 |
| `cmd_audit` !! | `audit.rs` | 8 | 21 | 68 |
| `cmd_suggest` !! | `suggest.rs` | 6 | 20 | 79 |
| `cmd_check` !! | `deps.rs` | 7 | 19 | 60 |
| `cmd_diagram` !! | `diagram.rs` | 6 | 17 | 67 |
| `cmd_add_service` !! | `project.rs` | 97 | 15 | 79 |
| `cmd_list` ! | `project.rs` | 200 | 11 | 40 |
| `resolve_wsl_path` ! | `project.rs` | 438 | 10 | 43 |
| `print_complexity_summary`  | `analyze.rs` | 157 | 9 | 44 |
| `run_cross_project_analysis`  | `analyze.rs` | 316 | 9 | 44 |
| `cmd_analyze`  | `analyze.rs` | 12 | 8 | 54 |
| `cmd_status`  | `service.rs` | 198 | 8 | 34 |
| `collect_service_dirs`  | `analyze.rs` | 77 | 6 | 30 |
| `cmd_search`  | `search.rs` | 54 | 6 | 25 |
| `cmd_logs`  | `service.rs` | 237 | 6 | 48 |
| `status_icon`  | `service.rs` | 294 | 6 | 9 |
| `cmd_stats`  | `project.rs` | 335 | 6 | 42 |
| `cmd_claudeignore`  | `project.rs` | 395 | 6 | 30 |

## Metricas de Acoplamiento

| Modulo | Fan-in | Fan-out |
|--------|--------|--------|
| `mod.rs` | 1 | 6 |
| `mod.rs` | 1 | 5 |
| `daemon.rs` | 0 | 1 |
| `main.rs` | 0 | 1 |

## Test Coverage

⚠️ No se encontraron reportes de cobertura.

Para generar reportes de cobertura, ejecutar:
- **Rust**: `cargo install cargo-tarpaulin && cargo tarpaulin --out xml` (genera `cobertura.xml`)

## Deuda Tecnica Explicita

**Total**: 3 marcadores (OPTIMIZE: 2, TODO: 1)

| Archivo | Linea | Tipo | Texto |
|---------|-------|------|-------|
| `src/commands/analysis/analyze.rs` | 228 | TODO | /FIXME/HACK). |
| `src/main.rs` | 245 | OPTIMIZE | d .voidignore for semantic index quality |
| `src/main.rs` | 266 | OPTIMIZE | d for the project's tech stack |

---
*Generado automaticamente por VoidStack*


---

# Arquitectura: crates/void-stack-mcp

## Resumen

| | |
|---|---|
| **Patron** | MVC (confianza: 75%) |
| **Lenguaje** | Rust |
| **Modulos** | 14 archivos |
| **LOC** | 2219 lineas |
| **Deps externas** | 8 paquetes |

## Distribucion por Capas

| Capa | Archivos | LOC | % |
|------|----------|-----|---|
| Controller | 12 | 1602 | 72% |
| Model | 1 | 596 | 26% |
| Utility | 1 | 21 | 0% |

## Anti-patrones Detectados

### Alta Severidad

- **God Class**: 'src/server.rs' es demasiado grande (41 funciones)
  - *Sugerencia*: Dividir 'src/server.rs' en modulos mas pequenos con responsabilidades claras

### Severidad Media

- **Fat Controller**: Controller 'src/tools/analysis.rs' tiene 226 LOC — demasiada logica
  - *Sugerencia*: Mover la logica de negocio a una capa de servicio
- **Fat Controller**: Controller 'src/tools/projects.rs' tiene 255 LOC — demasiada logica
  - *Sugerencia*: Mover la logica de negocio a una capa de servicio
- **Fat Controller**: Controller 'src/tools/search.rs' tiene 273 LOC — demasiada logica
  - *Sugerencia*: Mover la logica de negocio a una capa de servicio
- **No Service Layer**: Proyecto tiene 12 controllers pero ninguna capa de servicio
  - *Sugerencia*: Crear una capa de servicios para separar la logica de negocio de los endpoints
- **Excessive Coupling**: 'src/tools/mod.rs' importa 11 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas

## Mapa de Dependencias

```mermaid
graph LR
    subgraph controller ["Controller"]
        src_tools_docs_rs["docs.rs"]
        src_tools_docker_rs["docker.rs"]
        src_tools_analysis_rs["analysis.rs"]
        src_tools_suggest_rs["suggest.rs"]
        src_tools_services_rs["services.rs"]
        src_tools_mod_rs["mod.rs"]
        src_tools_space_rs["space.rs"]
        src_tools_debt_rs["debt.rs"]
        src_tools_projects_rs["projects.rs"]
        src_tools_stats_rs["stats.rs"]
        src_tools_diagrams_rs["diagrams.rs"]
        src_tools_search_rs["search.rs"]
    end
    subgraph model ["Model"]
        src_server_rs["server.rs"]
    end
    subgraph utility ["Utility"]
        src_main_rs["main.rs"]
    end
    src_tools_services_rs --> src_server_rs
    src_tools_projects_rs --> src_server_rs
    src_server_rs --> src_tools_mod_rs
    src_main_rs --> src_tools_mod_rs
    src_main_rs --> src_server_rs
```

## Modulos

| Archivo | Capa | LOC | Clases | Funciones |
|---------|------|-----|--------|----------|
| `src/server.rs` | Model | 596 | 1 | 41 |
| `src/tools/search.rs` | Controller | 273 | 0 | 15 |
| `src/tools/projects.rs` | Controller | 255 | 0 | 5 |
| `src/tools/analysis.rs` | Controller | 226 | 0 | 4 |
| `src/tools/docs.rs` | Controller | 188 | 0 | 5 |
| `src/tools/docker.rs` | Controller | 161 | 0 | 2 |
| `src/tools/services.rs` | Controller | 143 | 0 | 6 |
| `src/tools/debt.rs` | Controller | 110 | 0 | 3 |
| `src/tools/suggest.rs` | Controller | 76 | 0 | 1 |
| `src/tools/space.rs` | Controller | 73 | 0 | 2 |
| `src/tools/mod.rs` | Controller | 45 | 0 | 3 |
| `src/tools/diagrams.rs` | Controller | 43 | 0 | 1 |
| `src/main.rs` | Utility | 21 | 0 | 1 |
| `src/tools/stats.rs` | Controller | 9 | 0 | 1 |

## Dependencias Externas

- `anyhow`
- `rmcp`
- `schemars`
- `serde`
- `std`
- `tokio`
- `tracing`
- `void_stack_core`

## Complejidad Ciclomatica

**Promedio**: 3.0 | **Funciones analizadas**: 90 | **Funciones complejas (>=10)**: 7

| Funcion | Archivo | Linea | CC | LOC |
|---------|---------|-------|----|-----|
| `docker_analyze` !! | `docker.rs` | 8 | 18 | 97 |
| `suggest_refactoring` !! | `suggest.rs` | 8 | 16 | 76 |
| `docker_generate` !! | `docker.rs` | 115 | 15 | 63 |
| `get_index_stats` ! | `search.rs` | 167 | 13 | 65 |
| `analyze_cross_project` ! | `analysis.rs` | 200 | 12 | 70 |
| `read_all_docs` ! | `docs.rs` | 72 | 11 | 80 |
| `analyze_project` ! | `analysis.rs` | 12 | 10 | 63 |
| `add_service`  | `projects.rs` | 225 | 9 | 69 |
| `generate_diagram`  | `diagrams.rs` | 8 | 9 | 39 |
| `audit_project`  | `analysis.rs` | 85 | 8 | 69 |
| `check_dependencies`  | `analysis.rs` | 167 | 8 | 27 |
| `list_doc_files`  | `mod.rs` | 29 | 7 | 19 |
| `add_project`  | `projects.rs` | 56 | 6 | 75 |
| `index_project_codebase`  | `search.rs` | 13 | 6 | 43 |
| `semantic_search`  | `search.rs` | 74 | 6 | 49 |
| `read_project_docs`  | `docs.rs` | 11 | 5 | 57 |
| `save_debt_snapshot`  | `debt.rs` | 8 | 5 | 40 |
| `scan_directory`  | `projects.rs` | 176 | 5 | 43 |

## Metricas de Acoplamiento

| Modulo | Fan-in | Fan-out |
|--------|--------|--------|
| `mod.rs` | 2 | 11 |
| `projects.rs` | 0 | 6 |
| `services.rs` | 0 | 5 |
| `main.rs` | 0 | 3 |
| `search.rs` | 0 | 2 |
| `analysis.rs` | 0 | 1 |
| `stats.rs` | 0 | 1 |
| `docs.rs` | 0 | 1 |
| `space.rs` | 0 | 1 |
| `server.rs` | 3 | 1 |

## Test Coverage

⚠️ No se encontraron reportes de cobertura.

Para generar reportes de cobertura, ejecutar:
- **Rust**: `cargo install cargo-tarpaulin && cargo tarpaulin --out xml` (genera `cobertura.xml`)

---
*Generado automaticamente por VoidStack*


---

# Arquitectura: crates/void-stack-tui

## Resumen

| | |
|---|---|
| **Patron** | Unknown (confianza: 30%) |
| **Lenguaje** | Rust |
| **Modulos** | 15 archivos |
| **LOC** | 3041 lineas |
| **Deps externas** | 7 paquetes |

## Distribucion por Capas

| Capa | Archivos | LOC | % |
|------|----------|-----|---|
| Service | 1 | 335 | 11% |
| Model | 1 | 397 | 13% |
| Utility | 13 | 2309 | 75% |

## Anti-patrones Detectados

### Severidad Media

- **God Class**: 'src/main.rs' es demasiado grande (611 LOC)
  - *Sugerencia*: Dividir 'src/main.rs' en modulos mas pequenos con responsabilidades claras
- **Excessive Coupling**: 'src/ui/mod.rs' importa 15 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas

## Mapa de Dependencias

```mermaid
graph LR
    subgraph service ["Service"]
        src_i18n_rs["i18n.rs"]
    end
    subgraph model ["Model"]
        src_app_rs["app.rs"]
    end
    subgraph utility ["Utility"]
        src_ui_security_rs["security.rs"]
        src_ui_analysis_rs["analysis.rs"]
        src_ui_tabs_rs["tabs.rs"]
        src_ui_header_rs["header.rs"]
        src_ui_services_rs["services.rs"]
        src_ui_mod_rs["mod.rs"]
        src_ui_space_rs["space.rs"]
        src_ui_debt_rs["debt.rs"]
        src_ui_help_rs["help.rs"]
        src_ui_projects_rs["projects.rs"]
        src_ui_footer_rs["footer.rs"]
        src_ui_stats_rs["stats.rs"]
        src_main_rs["main.rs"]
    end
    src_ui_tabs_rs --> src_app_rs
    src_ui_services_rs --> src_app_rs
    src_ui_mod_rs --> src_app_rs
    src_ui_footer_rs --> src_app_rs
    src_main_rs --> src_ui_mod_rs
    src_main_rs --> src_app_rs
    src_main_rs --> src_app_rs
    src_main_rs --> src_app_rs
    src_main_rs --> src_app_rs
    src_main_rs --> src_app_rs
```

## Modulos

| Archivo | Capa | LOC | Clases | Funciones |
|---------|------|-----|--------|----------|
| `src/main.rs` | Utility | 611 | 1 | 16 |
| `src/ui/analysis.rs` | Utility | 471 | 0 | 6 |
| `src/app.rs` | Model | 397 | 4 | 20 |
| `src/i18n.rs` | Service | 335 | 1 | 5 |
| `src/ui/services.rs` | Utility | 255 | 0 | 4 |
| `src/ui/security.rs` | Utility | 222 | 0 | 3 |
| `src/ui/space.rs` | Utility | 128 | 0 | 2 |
| `src/ui/debt.rs` | Utility | 124 | 0 | 1 |
| `src/ui/stats.rs` | Utility | 112 | 0 | 1 |
| `src/ui/help.rs` | Utility | 88 | 0 | 1 |
| `src/ui/header.rs` | Utility | 73 | 0 | 1 |
| `src/ui/footer.rs` | Utility | 73 | 0 | 1 |
| `src/ui/projects.rs` | Utility | 59 | 0 | 1 |
| `src/ui/mod.rs` | Utility | 54 | 0 | 2 |
| `src/ui/tabs.rs` | Utility | 39 | 0 | 1 |

## Dependencias Externas

- `anyhow`
- `chrono`
- `clap`
- `crossterm`
- `ratatui`
- `std`
- `void_stack_core`

## Complejidad Ciclomatica

**Promedio**: 10.0 | **Funciones analizadas**: 65 | **Funciones complejas (>=10)**: 13

| Funcion | Archivo | Linea | CC | LOC |
|---------|---------|-------|----|-----|
| `es` !! | `i18n.rs` | 33 | 152 | 173 |
| `en` !! | `i18n.rs` | 225 | 152 | 173 |
| `handle_key` !! | `main.rs` | 187 | 21 | 75 |
| `draw_services_table` ! | `services.rs` | 54 | 13 | 106 |
| `handle_services_key` ! | `main.rs` | 623 | 13 | 40 |
| `run_tab_action` ! | `main.rs` | 491 | 11 | 72 |
| `refresh_current` ! | `app.rs` | 262 | 11 | 30 |
| `start_selected` ! | `app.rs` | 351 | 11 | 25 |
| `draw_analysis_tab` ! | `analysis.rs` | 11 | 10 | 64 |
| `draw_complexity` ! | `analysis.rs` | 309 | 10 | 95 |
| `draw_debt_tab` ! | `debt.rs` | 11 | 10 | 118 |
| `handle_projects_key` ! | `main.rs` | 592 | 10 | 30 |
| `refresh_all` ! | `app.rs` | 294 | 10 | 28 |
| `run_loop`  | `main.rs` | 142 | 9 | 38 |
| `stop_selected`  | `app.rs` | 377 | 9 | 22 |
| `draw_findings`  | `security.rs` | 143 | 8 | 95 |
| `draw_with_project_sidebar`  | `mod.rs` | 51 | 8 | 18 |
| `move_down`  | `app.rs` | 221 | 8 | 22 |
| `start_all`  | `app.rs` | 324 | 8 | 26 |
| `check_deps`  | `app.rs` | 414 | 8 | 32 |

## Metricas de Acoplamiento

| Modulo | Fan-in | Fan-out |
|--------|--------|--------|
| `mod.rs` | 1 | 15 |
| `main.rs` | 0 | 8 |
| `footer.rs` | 0 | 5 |
| `services.rs` | 0 | 4 |
| `tabs.rs` | 0 | 4 |
| `header.rs` | 0 | 2 |
| `debt.rs` | 0 | 2 |
| `stats.rs` | 0 | 2 |
| `projects.rs` | 0 | 2 |
| `analysis.rs` | 0 | 2 |
| `space.rs` | 0 | 2 |
| `security.rs` | 0 | 2 |
| `help.rs` | 0 | 2 |
| `app.rs` | 9 | 1 |

## Test Coverage

⚠️ No se encontraron reportes de cobertura.

Para generar reportes de cobertura, ejecutar:
- **Rust**: `cargo install cargo-tarpaulin && cargo tarpaulin --out xml` (genera `cobertura.xml`)

## Deuda Tecnica Explicita

**Total**: 1 marcadores (TODO: 1)

| Archivo | Linea | Tipo | Texto |
|---------|-------|------|-------|
| `src/ui/debt.rs` | 10 | TODO | /FIXME/HACK markers found in source code. |

---
*Generado automaticamente por VoidStack*


---

# Arquitectura: crates/void-stack-proto

## Resumen

| | |
|---|---|
| **Patron** | Monolith (confianza: 50%) |
| **Lenguaje** | Rust |
| **Modulos** | 3 archivos |
| **LOC** | 233 lineas |
| **Deps externas** | 2 paquetes |

## Distribucion por Capas

| Capa | Archivos | LOC | % |
|------|----------|-----|---|
| Service | 1 | 147 | 63% |
| Utility | 2 | 86 | 36% |

## Anti-patrones

No se detectaron anti-patrones significativos.

## Mapa de Dependencias

```mermaid
graph LR
    subgraph service ["Service"]
        src_client_rs["client.rs"]
    end
    subgraph utility ["Utility"]
        build_rs["build.rs"]
        src_lib_rs["lib.rs"]
    end
```

## Modulos

| Archivo | Capa | LOC | Clases | Funciones |
|---------|------|-----|--------|----------|
| `src/client.rs` | Service | 147 | 1 | 12 |
| `src/lib.rs` | Utility | 82 | 0 | 4 |
| `build.rs` | Utility | 4 | 0 | 1 |

## Dependencias Externas

- `async_trait`
- `void_stack_core`

## Complejidad Ciclomatica

**Promedio**: 2.1 | **Funciones analizadas**: 17 | **Funciones complejas (>=10)**: 0

| Funcion | Archivo | Linea | CC | LOC |
|---------|---------|-------|----|-----|
| `from`  | `lib.rs` | 16 | 6 | 9 |
| `from`  | `lib.rs` | 28 | 6 | 9 |

## Metricas de Acoplamiento

| Modulo | Fan-in | Fan-out |
|--------|--------|--------|
| `client.rs` | 0 | 2 |
| `lib.rs` | 0 | 1 |

## Test Coverage

⚠️ No se encontraron reportes de cobertura.

Para generar reportes de cobertura, ejecutar:
- **Rust**: `cargo install cargo-tarpaulin && cargo tarpaulin --out xml` (genera `cobertura.xml`)

---
*Generado automaticamente por VoidStack*


---

# Arquitectura: crates/void-stack-desktop

## Resumen

| | |
|---|---|
| **Patron** | MVC (confianza: 75%) |
| **Lenguaje** | Rust |
| **Modulos** | 19 archivos |
| **LOC** | 2190 lineas |
| **Deps externas** | 5 paquetes |

## Distribucion por Capas

| Capa | Archivos | LOC | % |
|------|----------|-----|---|
| Controller | 16 | 2095 | 95% |
| Model | 1 | 33 | 1% |
| Utility | 2 | 62 | 2% |

## Anti-patrones Detectados

### Severidad Media

- **Fat Controller**: Controller 'src/commands/docker.rs' tiene 237 LOC — demasiada logica
  - *Sugerencia*: Mover la logica de negocio a una capa de servicio
- **Fat Controller**: Controller 'src/commands/analysis.rs' tiene 300 LOC — demasiada logica
  - *Sugerencia*: Mover la logica de negocio a una capa de servicio
- **Fat Controller**: Controller 'src/commands/debt.rs' tiene 279 LOC — demasiada logica
  - *Sugerencia*: Mover la logica de negocio a una capa de servicio
- **Fat Controller**: Controller 'src/commands/projects.rs' tiene 256 LOC — demasiada logica
  - *Sugerencia*: Mover la logica de negocio a una capa de servicio
- **Fat Controller**: Controller 'src/commands/scan.rs' tiene 328 LOC — demasiada logica
  - *Sugerencia*: Mover la logica de negocio a una capa de servicio
- **No Service Layer**: Proyecto tiene 16 controllers pero ninguna capa de servicio
  - *Sugerencia*: Crear una capa de servicios para separar la logica de negocio de los endpoints
- **Excessive Coupling**: 'src/commands/mod.rs' importa 15 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas

## Mapa de Dependencias

```mermaid
graph LR
    subgraph controller ["Controller"]
        src_commands_docs_rs["docs.rs"]
        src_commands_docker_rs["docker.rs"]
        src_commands_analysis_rs["analysis.rs"]
        src_commands_audit_rs["audit.rs"]
        src_commands_suggest_rs["suggest.rs"]
        src_commands_services_rs["services.rs"]
        src_commands_mod_rs["mod.rs"]
        src_commands_dependencies_rs["dependencies.rs"]
        src_commands_space_rs["space.rs"]
        src_commands_debt_rs["debt.rs"]
        src_commands_projects_rs["projects.rs"]
        src_commands_stats_rs["stats.rs"]
        src_commands_diagrams_rs["diagrams.rs"]
        src_commands_search_rs["search.rs"]
        src_commands_scan_rs["scan.rs"]
        src_commands_logs_rs["logs.rs"]
    end
    subgraph model ["Model"]
        src_state_rs["state.rs"]
    end
    subgraph utility ["Utility"]
        build_rs["build.rs"]
        src_main_rs["main.rs"]
    end
    src_main_rs --> src_commands_mod_rs
    src_main_rs --> src_state_rs
```

## Modulos

| Archivo | Capa | LOC | Clases | Funciones |
|---------|------|-----|--------|----------|
| `src/commands/scan.rs` | Controller | 328 | 3 | 5 |
| `src/commands/analysis.rs` | Controller | 300 | 9 | 2 |
| `src/commands/debt.rs` | Controller | 279 | 8 | 7 |
| `src/commands/projects.rs` | Controller | 256 | 3 | 7 |
| `src/commands/docker.rs` | Controller | 237 | 12 | 3 |
| `src/commands/docs.rs` | Controller | 121 | 0 | 6 |
| `src/commands/services.rs` | Controller | 96 | 1 | 6 |
| `src/commands/suggest.rs` | Controller | 84 | 2 | 1 |
| `src/commands/audit.rs` | Controller | 75 | 3 | 1 |
| `src/commands/search.rs` | Controller | 69 | 0 | 4 |
| `src/commands/diagrams.rs` | Controller | 68 | 1 | 2 |
| `src/commands/space.rs` | Controller | 62 | 1 | 4 |
| `src/main.rs` | Utility | 59 | 0 | 1 |
| `src/commands/dependencies.rs` | Controller | 51 | 1 | 1 |
| `src/commands/logs.rs` | Controller | 47 | 1 | 2 |
| `src/state.rs` | Model | 33 | 1 | 3 |
| `src/commands/mod.rs` | Controller | 16 | 0 | 0 |
| `src/commands/stats.rs` | Controller | 6 | 0 | 1 |
| `build.rs` | Utility | 3 | 0 | 1 |

## Dependencias Externas

- `serde`
- `std`
- `tauri`
- `tokio`
- `void_stack_core`

## Complejidad Ciclomatica

**Promedio**: 4.0 | **Funciones analizadas**: 57 | **Funciones complejas (>=10)**: 8

| Funcion | Archivo | Linea | CC | LOC |
|---------|---------|-------|----|-----|
| `detect_service_tech` !! | `projects.rs` | 72 | 32 | 58 |
| `analyze_project_sync` !! | `analysis.rs` | 109 | 18 | 209 |
| `import_docker_services` !! | `scan.rs` | 269 | 15 | 106 |
| `detect_docker_services` ! | `scan.rs` | 170 | 12 | 85 |
| `suggest_refactoring` ! | `suggest.rs` | 28 | 11 | 66 |
| `list_project_docs` ! | `docs.rs` | 44 | 10 | 27 |
| `docker_generate` ! | `docker.rs` | 207 | 10 | 52 |
| `add_service_cmd` ! | `scan.rs` | 74 | 10 | 58 |
| `check_dependencies`  | `dependencies.rs` | 20 | 8 | 37 |
| `enriched_dto`  | `debt.rs` | 121 | 7 | 91 |
| `read_project_doc`  | `docs.rs` | 77 | 6 | 17 |
| `states_to_dto`  | `services.rs` | 18 | 6 | 19 |
| `run_analysis`  | `debt.rs` | 218 | 5 | 17 |

## Metricas de Acoplamiento

| Modulo | Fan-in | Fan-out |
|--------|--------|--------|
| `mod.rs` | 1 | 15 |
| `main.rs` | 0 | 3 |
| `docs.rs` | 0 | 1 |
| `diagrams.rs` | 0 | 1 |
| `logs.rs` | 0 | 1 |
| `debt.rs` | 0 | 1 |
| `projects.rs` | 0 | 1 |
| `space.rs` | 0 | 1 |
| `docker.rs` | 0 | 1 |
| `services.rs` | 0 | 1 |
| `suggest.rs` | 0 | 1 |
| `dependencies.rs` | 0 | 1 |
| `analysis.rs` | 0 | 1 |
| `audit.rs` | 0 | 1 |
| `search.rs` | 0 | 1 |
| `state.rs` | 1 | 0 |

## Test Coverage

⚠️ No se encontraron reportes de cobertura.

Para generar reportes de cobertura, ejecutar:
- **Rust**: `cargo install cargo-tarpaulin && cargo tarpaulin --out xml` (genera `cobertura.xml`)

---
*Generado automaticamente por VoidStack*


---

# Arquitectura: crates/void-stack-daemon

## Resumen

| | |
|---|---|
| **Patron** | Monolith (confianza: 50%) |
| **Lenguaje** | Rust |
| **Modulos** | 3 archivos |
| **LOC** | 423 lineas |
| **Deps externas** | 9 paquetes |

## Distribucion por Capas

| Capa | Archivos | LOC | % |
|------|----------|-----|---|
| Model | 1 | 176 | 41% |
| Utility | 2 | 247 | 58% |

## Anti-patrones

No se detectaron anti-patrones significativos.

## Mapa de Dependencias

```mermaid
graph LR
    subgraph model ["Model"]
        src_server_rs["server.rs"]
    end
    subgraph utility ["Utility"]
        src_main_rs["main.rs"]
        src_lifecycle_rs["lifecycle.rs"]
    end
    src_main_rs --> src_lifecycle_rs
```

## Modulos

| Archivo | Capa | LOC | Clases | Funciones |
|---------|------|-----|--------|----------|
| `src/server.rs` | Model | 176 | 1 | 12 |
| `src/main.rs` | Utility | 172 | 2 | 4 |
| `src/lifecycle.rs` | Utility | 75 | 1 | 6 |

## Dependencias Externas

- `anyhow`
- `clap`
- `std`
- `tokio`
- `tokio_stream`
- `tonic`
- `tracing`
- `void_stack_core`
- `void_stack_proto`

## Complejidad Ciclomatica

**Promedio**: 2.8 | **Funciones analizadas**: 22 | **Funciones complejas (>=10)**: 1

| Funcion | Archivo | Linea | CC | LOC |
|---------|---------|-------|----|-----|
| `read_pid_file` ! | `lifecycle.rs` | 46 | 12 | 31 |
| `cmd_start`  | `main.rs` | 68 | 8 | 57 |
| `stream_logs`  | `server.rs` | 147 | 7 | 26 |
| `cmd_status`  | `main.rs` | 179 | 6 | 38 |

## Metricas de Acoplamiento

| Modulo | Fan-in | Fan-out |
|--------|--------|--------|
| `main.rs` | 0 | 4 |
| `lifecycle.rs` | 1 | 0 |

## Test Coverage

⚠️ No se encontraron reportes de cobertura.

Para generar reportes de cobertura, ejecutar:
- **Rust**: `cargo install cargo-tarpaulin && cargo tarpaulin --out xml` (genera `cobertura.xml`)

---
*Generado automaticamente por VoidStack*


---

# Arquitectura: crates/void-stack-core

## Resumen

| | |
|---|---|
| **Patron** | Layered (confianza: 80%) |
| **Lenguaje** | Rust |
| **Modulos** | 134 archivos |
| **LOC** | 31391 lineas |
| **Deps externas** | 21 paquetes |

## Distribucion por Capas

| Capa | Archivos | LOC | % |
|------|----------|-----|---|
| Controller | 10 | 2899 | 9% |
| Service | 25 | 3542 | 11% |
| Repository | 8 | 2594 | 8% |
| Model | 13 | 3523 | 11% |
| Utility | 65 | 14572 | 46% |
| Config | 11 | 3339 | 10% |
| Test | 2 | 922 | 2% |

## Anti-patrones Detectados

### Alta Severidad

- **God Class**: 'src/detector/mod.rs' es demasiado grande (41 funciones)
  - *Sugerencia*: Dividir 'src/detector/mod.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'src/ai/mod.rs' es demasiado grande (38 funciones)
  - *Sugerencia*: Dividir 'src/ai/mod.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'src/log_filter.rs' es demasiado grande (44 funciones)
  - *Sugerencia*: Dividir 'src/log_filter.rs' en modulos mas pequenos con responsabilidades claras
- **Fat Controller**: Controller 'src/analyzer/imports/classifier/signals.rs' tiene 1009 LOC — demasiada logica
  - *Sugerencia*: Mover la logica de negocio a una capa de servicio
- **Excessive Coupling**: 'src/lib.rs' importa 23 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas
- **Excessive Coupling**: 'src/analyzer/history.rs' importa 31 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas

### Severidad Media

- **God Class**: 'src/docker/parse.rs' es demasiado grande (779 LOC y 33 funciones)
  - *Sugerencia*: Dividir 'src/docker/parse.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'src/docker/generate_dockerfile/mod.rs' es demasiado grande (31 funciones)
  - *Sugerencia*: Dividir 'src/docker/generate_dockerfile/mod.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'src/file_reader.rs' es demasiado grande (22 funciones)
  - *Sugerencia*: Dividir 'src/file_reader.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'src/global_config/mod.rs' es demasiado grande (30 funciones)
  - *Sugerencia*: Dividir 'src/global_config/mod.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'src/diagram/api_routes/mod.rs' es demasiado grande (21 funciones)
  - *Sugerencia*: Dividir 'src/diagram/api_routes/mod.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'src/diagram/architecture/externals.rs' es demasiado grande (616 LOC y 27 funciones)
  - *Sugerencia*: Dividir 'src/diagram/architecture/externals.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'src/vector_index/mod.rs' es demasiado grande (28 funciones)
  - *Sugerencia*: Dividir 'src/vector_index/mod.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'src/runner/docker.rs' es demasiado grande (33 funciones)
  - *Sugerencia*: Dividir 'src/runner/docker.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'src/runner/local.rs' es demasiado grande (28 funciones)
  - *Sugerencia*: Dividir 'src/runner/local.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'src/hooks.rs' es demasiado grande (25 funciones)
  - *Sugerencia*: Dividir 'src/hooks.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'src/audit/vuln_patterns/mod.rs' es demasiado grande (22 funciones)
  - *Sugerencia*: Dividir 'src/audit/vuln_patterns/mod.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'src/audit/vuln_patterns/error_handling.rs' es demasiado grande (26 funciones)
  - *Sugerencia*: Dividir 'src/audit/vuln_patterns/error_handling.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'src/audit/config_check.rs' es demasiado grande (29 funciones)
  - *Sugerencia*: Dividir 'src/audit/config_check.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'src/audit/secrets.rs' es demasiado grande (25 funciones)
  - *Sugerencia*: Dividir 'src/audit/secrets.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'src/stats.rs' es demasiado grande (25 funciones)
  - *Sugerencia*: Dividir 'src/stats.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'src/claudeignore.rs' es demasiado grande (31 funciones)
  - *Sugerencia*: Dividir 'src/claudeignore.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'src/space/mod.rs' es demasiado grande (25 funciones)
  - *Sugerencia*: Dividir 'src/space/mod.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'src/analyzer/patterns/antipatterns.rs' es demasiado grande (28 funciones)
  - *Sugerencia*: Dividir 'src/analyzer/patterns/antipatterns.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'src/analyzer/complexity.rs' es demasiado grande (849 LOC y 35 funciones)
  - *Sugerencia*: Dividir 'src/analyzer/complexity.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'src/analyzer/imports/classifier/signals.rs' es demasiado grande (1009 LOC)
  - *Sugerencia*: Dividir 'src/analyzer/imports/classifier/signals.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'src/analyzer/imports/mod.rs' es demasiado grande (26 funciones)
  - *Sugerencia*: Dividir 'src/analyzer/imports/mod.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'src/analyzer/docs/markdown.rs' es demasiado grande (816 LOC y 33 funciones)
  - *Sugerencia*: Dividir 'src/analyzer/docs/markdown.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'src/analyzer/cross_project.rs' es demasiado grande (29 funciones)
  - *Sugerencia*: Dividir 'src/analyzer/cross_project.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'src/analyzer/history.rs' es demasiado grande (639 LOC y 30 funciones)
  - *Sugerencia*: Dividir 'src/analyzer/history.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'src/analyzer/best_practices/mod.rs' es demasiado grande (31 funciones)
  - *Sugerencia*: Dividir 'src/analyzer/best_practices/mod.rs' en modulos mas pequenos con responsabilidades claras
- **God Class**: 'src/ignore.rs' es demasiado grande (23 funciones)
  - *Sugerencia*: Dividir 'src/ignore.rs' en modulos mas pequenos con responsabilidades claras
- **Fat Controller**: Controller 'src/global_config/mod.rs' tiene 387 LOC — demasiada logica
  - *Sugerencia*: Mover la logica de negocio a una capa de servicio
- **Fat Controller**: Controller 'src/audit/vuln_patterns/mod.rs' tiene 391 LOC — demasiada logica
  - *Sugerencia*: Mover la logica de negocio a una capa de servicio
- **Fat Controller**: Controller 'src/analyzer/imports/dart.rs' tiene 309 LOC — demasiada logica
  - *Sugerencia*: Mover la logica de negocio a una capa de servicio
- **Excessive Coupling**: 'src/detector/mod.rs' importa 17 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas
- **Excessive Coupling**: 'src/manager/process.rs' importa 14 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas
- **Excessive Coupling**: 'src/vector_index/mod.rs' importa 19 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas
- **Excessive Coupling**: 'src/vector_index/indexer.rs' importa 11 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas
- **Excessive Coupling**: 'src/analyzer/imports/rust_lang.rs' importa 11 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas
- **Excessive Coupling**: 'src/runner/local.rs' importa 13 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas
- **Excessive Coupling**: 'src/runner/docker.rs' importa 15 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas
- **Excessive Coupling**: 'src/analyzer/best_practices/mod.rs' importa 12 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas
- **Excessive Coupling**: 'src/diagram/mod.rs' importa 11 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas
- **Excessive Coupling**: 'src/analyzer/docs/markdown.rs' importa 20 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas
- **Excessive Coupling**: 'src/audit/vuln_patterns/mod.rs' importa 11 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas
- **Excessive Coupling**: 'src/global_config/mod.rs' importa 11 modulos (fan-out alto)
  - *Sugerencia*: Reducir dependencias usando inyeccion de dependencias o fachadas

## Mapa de Dependencias

```mermaid
graph LR
    subgraph controller ["Controller"]
        src_global_config_mod_rs["mod.rs"]
        src_diagram_api_routes_node_rs["node.rs"]
        src_diagram_api_routes_python_rs["python.rs"]
        src_diagram_drawio_mod_rs["mod.rs"]
        src_diagram_db_models_drift_rs["drift.rs"]
        src_audit_vuln_patterns_mod_rs["mod.rs"]
        src_ai_ollama_rs["ollama.rs"]
        src_analyzer_imports_dart_rs["dart.rs"]
        src_analyzer_imports_classifier_signals_rs["signals.rs"]
        src_analyzer_docs_mod_rs["mod.rs"]
    end
    subgraph service ["Service"]
        src_diagram_architecture_infra_terraform_rs["terraform.rs"]
        src_diagram_architecture_infra_mod_rs["mod.rs"]
        src_diagram_drawio_common_rs["common.rs"]
        src_backend_rs["backend.rs"]
        src_runner_docker_rs["docker.rs"]
        src_runner_local_rs["local.rs"]
        src_runner_rs["runner.rs"]
        src_manager_mod_rs["mod.rs"]
        src_detector_docker_rs["docker.rs"]
        src_detector_clippy_rs["clippy.rs"]
        src_detector_flutter_rs["flutter.rs"]
        src_detector_rust_lang_rs["rust_lang.rs"]
        src_detector_react_doctor_rs["react_doctor.rs"]
        src_detector_ollama_rs["ollama.rs"]
        src_detector_mod_rs["mod.rs"]
        src_detector_node_rs["node.rs"]
        src_detector_ruff_rs["ruff.rs"]
        src_detector_golang_rs["golang.rs"]
        src_detector_flutter_analyze_rs["flutter_analyze.rs"]
        src_detector_python_rs["python.rs"]
        src_detector_cuda_rs["cuda.rs"]
        src_detector_golangci_lint_rs["golangci_lint.rs"]
        src_analyzer_imports_javascript_rs["javascript.rs"]
        src_analyzer_imports_python_rs["python.rs"]
        src_analyzer_history_rs["history.rs"]
    end
    subgraph repository ["Repository"]
        src_docker_generate_compose_rs["generate_compose.rs"]
        src_diagram_db_models_sequelize_rs["sequelize.rs"]
        src_diagram_db_models_gorm_rs["gorm.rs"]
        src_vector_index_db_rs["db.rs"]
        src_vector_index_mod_rs["mod.rs"]
        src_vector_index_indexer_rs["indexer.rs"]
        src_vector_index_stats_rs["stats.rs"]
        src_stats_rs["stats.rs"]
    end
    subgraph model ["Model"]
        src_docker_mod_rs["mod.rs"]
        src_diagram_mod_rs["mod.rs"]
        src_diagram_db_models_python_rs["python.rs"]
        src_vector_index_voidignore_rs["voidignore.rs"]
        src_error_rs["error.rs"]
        src_audit_findings_rs["findings.rs"]
        src_log_filter_rs["log_filter.rs"]
        src_claudeignore_rs["claudeignore.rs"]
        src_space_mod_rs["mod.rs"]
        src_analyzer_patterns_mod_rs["mod.rs"]
        src_analyzer_graph_rs["graph.rs"]
        src_analyzer_imports_golang_rs["golang.rs"]
        src_model_rs["model.rs"]
    end
    subgraph utility ["Utility"]
        src_docker_terraform_rs["terraform.rs"]
        src_docker_helm_rs["helm.rs"]
        src_docker_parse_rs["parse.rs"]
        src_docker_kubernetes_rs["kubernetes.rs"]
        src_docker_generate_dockerfile_flutter_rs["flutter.rs"]
        src_docker_generate_dockerfile_rust_lang_rs["rust_lang.rs"]
        src_docker_generate_dockerfile_go_rs["go.rs"]
        src_docker_generate_dockerfile_python_rs["python.rs"]
        src_file_reader_rs["file_reader.rs"]
        src_global_config_paths_rs["paths.rs"]
        src_global_config_scanner_rs["scanner.rs"]
        src_global_config_project_ops_rs["project_ops.rs"]
        src_security_rs["security.rs"]
        src_diagram_api_routes_swagger_rs["swagger.rs"]
        src_diagram_api_routes_grpc_rs["grpc.rs"]
        src_diagram_api_routes_mod_rs["mod.rs"]
        src_diagram_architecture_crates_rs["crates.rs"]
        src_diagram_architecture_infra_helm_rs["helm.rs"]
        src_diagram_architecture_infra_kubernetes_rs["kubernetes.rs"]
        src_diagram_architecture_mod_rs["mod.rs"]
        src_diagram_drawio_api_routes_rs["api_routes.rs"]
        src_diagram_drawio_db_models_rs["db_models.rs"]
        src_diagram_drawio_architecture_rs["architecture.rs"]
        src_diagram_service_detection_rs["service_detection.rs"]
        src_diagram_db_models_proto_rs["proto.rs"]
        src_diagram_db_models_mod_rs["mod.rs"]
        src_diagram_db_models_prisma_rs["prisma.rs"]
        src_vector_index_chunker_rs["chunker.rs"]
        src_vector_index_search_rs["search.rs"]
        src_lib_rs["lib.rs"]
        src_process_util_rs["process_util.rs"]
        src_hooks_rs["hooks.rs"]
        src_manager_state_rs["state.rs"]
        src_manager_url_rs["url.rs"]
        src_manager_logs_rs["logs.rs"]
        src_audit_vuln_patterns_xss_rs["xss.rs"]
        src_audit_vuln_patterns_injection_rs["injection.rs"]
        src_audit_vuln_patterns_network_rs["network.rs"]
        src_audit_vuln_patterns_error_handling_rs["error_handling.rs"]
        src_audit_vuln_patterns_crypto_rs["crypto.rs"]
        src_audit_deps_rs["deps.rs"]
        src_audit_mod_rs["mod.rs"]
        src_ai_mod_rs["mod.rs"]
        src_ai_prompt_rs["prompt.rs"]
        src_analyzer_patterns_antipatterns_rs["antipatterns.rs"]
        src_analyzer_complexity_rs["complexity.rs"]
        src_analyzer_imports_rust_lang_rs["rust_lang.rs"]
        src_analyzer_imports_classifier_mod_rs["mod.rs"]
        src_analyzer_imports_mod_rs["mod.rs"]
        src_analyzer_docs_coverage_rs["coverage.rs"]
        src_analyzer_docs_sanitize_rs["sanitize.rs"]
        src_analyzer_docs_markdown_rs["markdown.rs"]
        src_analyzer_cross_project_rs["cross_project.rs"]
        src_analyzer_mod_rs["mod.rs"]
        src_analyzer_explicit_debt_rs["explicit_debt.rs"]
        src_analyzer_best_practices_flutter_rs["flutter.rs"]
        src_analyzer_best_practices_react_rs["react.rs"]
        src_analyzer_best_practices_report_rs["report.rs"]
        src_analyzer_best_practices_mod_rs["mod.rs"]
        src_analyzer_best_practices_vue_rs["vue.rs"]
        src_analyzer_best_practices_rust_bp_rs["rust_bp.rs"]
        src_analyzer_best_practices_oxlint_rs["oxlint.rs"]
        src_analyzer_best_practices_go_bp_rs["go_bp.rs"]
        src_analyzer_best_practices_python_rs["python.rs"]
        src_ignore_rs["ignore.rs"]
    end
    subgraph config ["Config"]
        src_docker_generate_dockerfile_mod_rs["mod.rs"]
        src_docker_generate_dockerfile_node_rs["node.rs"]
        src_diagram_architecture_externals_rs["externals.rs"]
        src_config_rs["config.rs"]
        src_manager_process_rs["process.rs"]
        src_detector_env_rs["env.rs"]
        src_audit_vuln_patterns_config_rs["config.rs"]
        src_audit_config_check_rs["config_check.rs"]
        src_audit_secrets_rs["secrets.rs"]
        src_analyzer_best_practices_angular_rs["angular.rs"]
        src_analyzer_best_practices_astro_rs["astro.rs"]
    end
    subgraph test ["Test"]
        tests_integration_analysis_rs["integration_analysis.rs"]
        src_analyzer_imports_classifier_tests_rs["tests.rs"]
    end
    src_docker_mod_rs --> src_docker_generate_dockerfile_mod_rs
    src_docker_generate_compose_rs --> src_model_rs
    src_file_reader_rs --> src_error_rs
    src_global_config_paths_rs --> src_error_rs
    src_global_config_mod_rs --> src_error_rs
    src_diagram_mod_rs --> src_diagram_api_routes_mod_rs
    src_diagram_mod_rs --> src_diagram_architecture_mod_rs
    src_diagram_mod_rs --> src_diagram_db_models_mod_rs
    src_diagram_mod_rs --> src_diagram_drawio_mod_rs
    src_diagram_mod_rs --> src_model_rs
    src_diagram_architecture_infra_terraform_rs --> src_docker_mod_rs
    src_diagram_architecture_infra_mod_rs --> src_docker_mod_rs
    src_diagram_architecture_infra_helm_rs --> src_docker_mod_rs
    src_diagram_architecture_infra_kubernetes_rs --> src_docker_mod_rs
    src_diagram_architecture_externals_rs --> src_security_rs
    src_diagram_architecture_mod_rs --> src_diagram_architecture_infra_mod_rs
    src_diagram_architecture_mod_rs --> src_docker_mod_rs
    src_diagram_drawio_mod_rs --> src_model_rs
    src_diagram_drawio_architecture_rs --> src_security_rs
    src_config_rs --> src_error_rs
    src_runner_docker_rs --> src_error_rs
    src_runner_docker_rs --> src_model_rs
    src_runner_docker_rs --> src_process_util_rs
    src_runner_local_rs --> src_error_rs
    src_runner_local_rs --> src_model_rs
    src_lib_rs --> src_ai_mod_rs
    src_lib_rs --> src_analyzer_mod_rs
    src_lib_rs --> src_audit_mod_rs
    src_lib_rs --> src_detector_mod_rs
    src_lib_rs --> src_diagram_mod_rs
    src_lib_rs --> src_docker_mod_rs
    src_lib_rs --> src_global_config_mod_rs
    src_lib_rs --> src_manager_mod_rs
    src_lib_rs --> src_space_mod_rs
    src_lib_rs --> src_vector_index_mod_rs
    src_hooks_rs --> src_error_rs
    src_hooks_rs --> src_model_rs
    src_hooks_rs --> src_process_util_rs
    src_runner_rs --> src_docker_mod_rs
    src_runner_rs --> src_model_rs
    src_manager_mod_rs --> src_model_rs
    src_manager_state_rs --> src_model_rs
    src_manager_state_rs --> src_runner_rs
    src_manager_process_rs --> src_error_rs
    src_manager_process_rs --> src_hooks_rs
    src_manager_process_rs --> src_model_rs
    src_manager_process_rs --> src_runner_rs
    src_manager_process_rs --> src_model_rs
    src_detector_ollama_rs --> src_security_rs
    src_audit_mod_rs --> src_audit_vuln_patterns_mod_rs
    %% ... y 21 conexiones mas
```

## Modulos

| Archivo | Capa | LOC | Clases | Funciones |
|---------|------|-----|--------|----------|
| `src/analyzer/imports/classifier/signals.rs` | Controller | 1009 | 0 | 0 |
| `src/analyzer/complexity.rs` | Utility | 849 | 2 | 35 |
| `src/analyzer/docs/markdown.rs` | Utility | 816 | 0 | 33 |
| `src/docker/parse.rs` | Utility | 779 | 0 | 33 |
| `src/analyzer/history.rs` | Service | 639 | 5 | 30 |
| `src/diagram/architecture/externals.rs` | Config | 616 | 0 | 27 |
| `src/analyzer/imports/classifier/tests.rs` | Test | 588 | 0 | 40 |
| `src/analyzer/best_practices/mod.rs` | Utility | 568 | 7 | 31 |
| `src/audit/config_check.rs` | Config | 547 | 0 | 29 |
| `src/audit/secrets.rs` | Config | 532 | 1 | 25 |
| `src/space/mod.rs` | Model | 506 | 2 | 25 |
| `src/ai/mod.rs` | Utility | 503 | 6 | 38 |
| `src/analyzer/patterns/antipatterns.rs` | Utility | 498 | 3 | 28 |
| `src/runner/docker.rs` | Service | 484 | 2 | 33 |
| `src/analyzer/imports/mod.rs` | Utility | 466 | 3 | 26 |
| `src/log_filter.rs` | Model | 465 | 1 | 44 |
| `src/vector_index/mod.rs` | Repository | 457 | 0 | 28 |
| `src/analyzer/cross_project.rs` | Utility | 457 | 2 | 29 |
| `src/detector/mod.rs` | Service | 442 | 4 | 41 |
| `src/audit/vuln_patterns/error_handling.rs` | Utility | 438 | 0 | 26 |
| `src/runner/local.rs` | Service | 433 | 1 | 28 |
| `src/claudeignore.rs` | Model | 427 | 1 | 31 |
| `src/diagram/db_models/sequelize.rs` | Repository | 422 | 0 | 13 |
| `src/audit/vuln_patterns/mod.rs` | Controller | 391 | 0 | 22 |
| `src/global_config/mod.rs` | Controller | 387 | 1 | 30 |
| `src/stats.rs` | Repository | 387 | 5 | 25 |
| `src/vector_index/indexer.rs` | Repository | 386 | 1 | 8 |
| `src/docker/generate_compose.rs` | Repository | 384 | 1 | 14 |
| `src/docker/kubernetes.rs` | Utility | 375 | 0 | 9 |
| `src/docker/terraform.rs` | Utility | 365 | 0 | 12 |

*... y 104 módulos más (ordenados por LOC, mostrando top 30)*

## Dependencias Externas

- `async_trait`
- `chrono`
- `complexity`
- `coverage`
- `explicit_debt`
- `graph`
- `hnsw_rs`
- `patterns`
- `regex`
- `rusqlite`
- `serde`
- `serde_yaml`
- `signals`
- `std`
- `super`
- `tempfile`
- `thiserror`
- `tokio`
- `tracing`
- `uuid`
- `void_stack_core`

## Complejidad Ciclomatica

**Promedio**: 3.2 | **Funciones analizadas**: 1553 | **Funciones complejas (>=10)**: 137

| Funcion | Archivo | Linea | CC | LOC |
|---------|---------|-------|----|-----|
| `generate` !! | `mod.rs` | 19 | 34 | 147 |
| `detect_from_env` !! | `externals.rs` | 51 | 33 | 93 |
| `render_db_models_page` !! | `db_models.rs` | 7 | 31 | 159 |
| `parse_k8s_yaml` !! | `kubernetes.rs` | 102 | 30 | 119 |
| `scan_weak_cryptography` !! | `crypto.rs` | 67 | 30 | 87 |
| `scan_subprojects` !! | `scanner.rs` | 8 | 29 | 74 |
| `collect_files_recursive` !! | `indexer.rs` | 401 | 29 | 90 |
| `parse_swagger_yaml_routes` !! | `swagger.rs` | 98 | 28 | 117 |
| `check` !! | `python.rs` | 21 | 28 | 121 |
| `detect_crate_relationships` !! | `crates.rs` | 6 | 26 | 70 |
| `index_project` !! | `indexer.rs` | 131 | 26 | 167 |
| `parse_file` !! | `javascript.rs` | 17 | 26 | 71 |
| `install_hint` !! | `process_util.rs` | 139 | 25 | 49 |
| `scan_debug_mode` !! | `config_check.rs` | 55 | 25 | 72 |
| `scan_go_error_discard` !! | `error_handling.rs` | 203 | 23 | 78 |
| `count_js_branches` !! | `complexity.rs` | 323 | 23 | 44 |
| `generate_architecture_page` !! | `architecture.rs` | 12 | 22 | 139 |
| `extract_datatype_from_line` !! | `sequelize.rs` | 191 | 22 | 40 |
| `parse_django_field` !! | `python.rs` | 194 | 22 | 34 |
| `scan_cors_config` !! | `config_check.rs` | 131 | 22 | 62 |

## Metricas de Acoplamiento

| Modulo | Fan-in | Fan-out |
|--------|--------|--------|
| `history.rs` | 0 | 31 |
| `lib.rs` | 0 | 23 |
| `markdown.rs` | 0 | 20 |
| `mod.rs` | 1 | 19 |
| `mod.rs` | 1 | 17 |
| `docker.rs` | 0 | 15 |
| `process.rs` | 0 | 14 |
| `local.rs` | 0 | 13 |
| `mod.rs` | 1 | 12 |
| `mod.rs` | 1 | 11 |
| `mod.rs` | 1 | 11 |
| `indexer.rs` | 0 | 11 |
| `rust_lang.rs` | 0 | 11 |
| `mod.rs` | 1 | 11 |
| `mod.rs` | 1 | 10 |
| `mod.rs` | 1 | 10 |
| `hooks.rs` | 1 | 10 |
| `mod.rs` | 1 | 9 |
| `prompt.rs` | 0 | 9 |
| `mod.rs` | 1 | 9 |

## Test Coverage

⚠️ No se encontraron reportes de cobertura.

Para generar reportes de cobertura, ejecutar:
- **Rust**: `cargo install cargo-tarpaulin && cargo tarpaulin --out xml` (genera `cobertura.xml`)

## Deuda Tecnica Explicita

**Total**: 27 marcadores (FIXME: 1, HACK: 1, OPTIMIZE: 4, TEMP: 10, TODO: 9, XXX: 2)

| Archivo | Linea | Tipo | Texto |
|---------|-------|------|-------|
| `src/ai/prompt.rs` | 15 | OPTIMIZE | d prompt from analysis results. |
| `src/analyzer/explicit_debt.rs` | 3 | TODO | , FIXME, HACK, XXX, OPTIMIZE, BUG, TEMP, WORKAROUND. |
| `src/analyzer/explicit_debt.rs` | 228 | TODO | implement error handling", "rust"); |
| `src/analyzer/explicit_debt.rs` | 238 | FIXME | this is broken\n\ |
| `src/analyzer/explicit_debt.rs` | 240 | HACK | temporary workaround", |
| `src/analyzer/explicit_debt.rs` | 266 | TODO | add validation\n/* FIXME: memory leak */", |
| `src/analyzer/explicit_debt.rs` | 282 | TODO | a\n// FIXME: b\n// HACK: c\n// XXX: d\n// OPTIMIZE: e\n//... |
| `src/analyzer/explicit_debt.rs` | 304 | TODO | lowercase", "rust"); |
| `src/analyzer/explicit_debt.rs` | 314 | TODO | add logging\n}\n", |
| `src/analyzer/explicit_debt.rs` | 325 | TODO | should be skipped", |
| `src/analyzer/mod.rs` | 32 | TODO | , FIXME, HACK, etc.) found in source code. |
| `src/audit/secrets.rs` | 179 | TEMP | late/placeholder syntax that |
| `src/audit/secrets.rs` | 184 | TEMP | late variables, string interpolation |
| `src/audit/secrets.rs` | 189 | TEMP | late generation) |
| `src/audit/secrets.rs` | 347 | TEMP | late/format string generation |
| `src/claudeignore.rs` | 3 | OPTIMIZE | d `.claudeignore` patterns |
| `src/diagram/db_models/sequelize.rs` | 174 | XXX | ' or "xxx") from a line. |
| `src/diagram/db_models/sequelize.rs` | 190 | XXX | from a line and map to a simple type. |
| `src/diagram/mod.rs` | 97 | TEMP | dir alive by leaking it (test only) |
| `src/docker/generate_dockerfile/mod.rs` | 3 | TEMP | lates follow official best practices: |
| `src/file_reader.rs` | 141 | TEMP | project directory for testing. |
| `src/stats.rs` | 135 | TEMP | file::tempdir) |
| `src/vector_index/indexer.rs` | 289 | TEMP | dir, then rename atomically to avoid |
| `src/vector_index/mod.rs` | 516 | TEMP | directory and index stats on disk |
| `src/vector_index/voidignore.rs` | 5 | OPTIMIZE | d for semantic index quality. |
| `src/vector_index/voidignore.rs` | 12 | OPTIMIZE | d for the vector index. |
| `tests/integration_analysis.rs` | 147 | TODO | add error handling\nfunction run() { /* FIXME: memory lea... |

---
*Generado automaticamente por VoidStack*


---

