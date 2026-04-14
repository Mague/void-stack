**void-stack** | Layered | 214 módulos | 32427 LOC | anti-patrones: 25

**Anti-patrones críticos:**
- [High] Fat Controller — crates/void-stack-cli/src/commands/project.rs
- [High] Fat Controller — crates/void-stack-mcp/src/server.rs
- [High] Fat Controller — crates/void-stack-core/src/analyzer/imports/classifier/signals.rs
- [High] Excessive Coupling — crates/void-stack-core/src/lib.rs

**Funciones complejas (CC≥10):**
- `crates/void-stack-tui/src/i18n.rs::es` CC=152
- `crates/void-stack-tui/src/i18n.rs::en` CC=152
- `crates/void-stack-core/src/vector_index/indexer.rs::index_project` CC=40
- `crates/void-stack-cli/src/commands/docker.rs::cmd_docker` CC=34
- `crates/void-stack-core/src/diagram/architecture/mod.rs::generate` CC=34



---

**crates/void-stack-cli** | Unknown | 13 módulos | 2009 LOC | anti-patrones: 4

**Anti-patrones críticos:**
- [High] Fat Controller — src/commands/project.rs

**Funciones complejas (CC≥10):**
- `src/commands/docker.rs::cmd_docker` CC=34
- `src/main.rs::main` CC=28
- `src/commands/service.rs::cmd_start` CC=28
- `src/commands/analysis/audit.rs::cmd_audit` CC=21
- `src/commands/analysis/suggest.rs::cmd_suggest` CC=20



---

**crates/void-stack-mcp** | MVC | 16 módulos | 2422 LOC | anti-patrones: 6

**Anti-patrones críticos:**
- [High] Fat Controller — src/server.rs

**Funciones complejas (CC≥10):**
- `src/tools/docker.rs::docker_analyze` CC=18
- `src/tools/suggest.rs::suggest_refactoring` CC=16
- `src/tools/docker.rs::docker_generate` CC=15
- `src/tools/search.rs::get_index_stats` CC=13
- `src/tools/analysis.rs::analyze_cross_project` CC=12



---

**crates/void-stack-tui** | Unknown | 15 módulos | 3041 LOC | anti-patrones: 2

**Funciones complejas (CC≥10):**
- `src/i18n.rs::es` CC=152
- `src/i18n.rs::en` CC=152
- `src/main.rs::handle_key` CC=21
- `src/ui/services.rs::draw_services_table` CC=13
- `src/main.rs::handle_services_key` CC=13



---

**crates/void-stack-proto** | Monolith | 3 módulos | 233 LOC | anti-patrones: 0



---

**crates/void-stack-desktop** | MVC | 19 módulos | 2190 LOC | anti-patrones: 7

**Funciones complejas (CC≥10):**
- `src/commands/projects.rs::detect_service_tech` CC=32
- `src/commands/analysis.rs::analyze_project_sync` CC=18
- `src/commands/scan.rs::import_docker_services` CC=15
- `src/commands/scan.rs::detect_docker_services` CC=12
- `src/commands/suggest.rs::suggest_refactoring` CC=11



---

**crates/void-stack-daemon** | Monolith | 3 módulos | 423 LOC | anti-patrones: 0

**Funciones complejas (CC≥10):**
- `src/lifecycle.rs::read_pid_file` CC=12



---

**crates/void-stack-core** | Layered | 145 módulos | 22109 LOC | anti-patrones: 9

**Anti-patrones críticos:**
- [High] Fat Controller — src/analyzer/imports/classifier/signals.rs
- [High] Excessive Coupling — src/lib.rs

**Funciones complejas (CC≥10):**
- `src/vector_index/indexer.rs::index_project` CC=40
- `src/diagram/architecture/mod.rs::generate` CC=34
- `src/diagram/architecture/externals.rs::detect_from_env` CC=33
- `src/diagram/drawio/db_models.rs::render_db_models_page` CC=31
- `src/docker/kubernetes.rs::parse_k8s_yaml` CC=30

