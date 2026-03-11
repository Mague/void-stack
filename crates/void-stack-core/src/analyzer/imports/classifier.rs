//! Scoring-based architectural layer classifier.
//!
//! Classifies source code files into architectural layers (Controller, Service,
//! Repository, Model, Utility, Config, Test) using a weighted scoring system:
//!
//! 1. **Deterministic overrides** — test files, config files, entry points
//! 2. **Content signals** — patterns in the code (+1 to +5 weight each)
//! 3. **Directory bonus** — universal naming conventions (+2 bonus)
//! 4. **Fan-in/fan-out** — dependency graph position (post-scoring refinement)

use super::super::graph::*;

/// Classify a module into an architectural layer based on path and content.
pub(crate) fn classify_layer(path: &str, content: &str) -> ArchLayer {
    // 1. Deterministic path-level overrides (test, config, entry points)
    if let Some(layer) = classify_by_path_keywords(path) {
        return layer;
    }

    // 2. Score-based classification: content + path bonuses
    let scores = compute_layer_scores(path, content);

    // Find the layer with the highest score (ignore Unknown)
    let best = scores
        .iter()
        .filter(|(layer, _)| *layer != ArchLayer::Unknown)
        .max_by_key(|(_, score)| *score);

    match best {
        Some((layer, score)) if *score > 0 => *layer,
        _ => ArchLayer::Unknown,
    }
}

/// Refine Unknown modules using fan-in/fan-out from the dependency graph.
/// This is the dynamic, language-agnostic classification — no hardcoded names.
pub(crate) fn refine_unknown_by_graph(modules: &mut [ModuleNode], edges: &[ImportEdge]) {
    use std::collections::HashMap;

    let mut fan_in: HashMap<&str, usize> = HashMap::new();
    let mut fan_out: HashMap<&str, usize> = HashMap::new();

    for edge in edges {
        if !edge.is_external {
            *fan_in.entry(edge.to.as_str()).or_insert(0) += 1;
            *fan_out.entry(edge.from.as_str()).or_insert(0) += 1;
        }
    }

    // Calculate median fan-in to set adaptive thresholds
    let mut fan_in_values: Vec<usize> = modules
        .iter()
        .map(|m| *fan_in.get(m.path.as_str()).unwrap_or(&0))
        .filter(|v| *v > 0)
        .collect();
    fan_in_values.sort_unstable();
    let median_fan_in = fan_in_values
        .get(fan_in_values.len() / 2)
        .copied()
        .unwrap_or(1);
    let high_fan_in_threshold = (median_fan_in * 2).max(3);

    for module in modules.iter_mut() {
        if module.layer != ArchLayer::Unknown {
            continue;
        }

        let fi = *fan_in.get(module.path.as_str()).unwrap_or(&0);
        let fo = *fan_out.get(module.path.as_str()).unwrap_or(&0);

        if fi >= high_fan_in_threshold && fo <= 1 {
            module.layer = ArchLayer::Model;
        } else if fi >= high_fan_in_threshold {
            module.layer = ArchLayer::Utility;
        } else if fo >= high_fan_in_threshold && fi <= 1 {
            module.layer = ArchLayer::Controller;
        } else if fo > fi && fo >= 2 {
            module.layer = ArchLayer::Service;
        }
    }
}

// ── Deterministic overrides ────────────────────────────────────────────────

fn classify_by_path_keywords(path: &str) -> Option<ArchLayer> {
    let lower = path.to_lowercase();

    if lower.contains("test") || lower.contains("spec") || lower.starts_with("tests/") {
        return Some(ArchLayer::Test);
    }

    let config_suffixes = [
        "config.py",
        "config.js",
        "config.ts",
        "config.rs",
        "config.toml",
        "config.yaml",
        "config.yml",
    ];
    if lower.contains(".env") || config_suffixes.iter().any(|s| lower.ends_with(s)) {
        return Some(ArchLayer::Config);
    }

    if lower.ends_with("main.rs") || lower.ends_with("build.rs") || lower.ends_with("lib.rs") {
        return Some(ArchLayer::Utility);
    }

    None
}

// ── Scoring engine ─────────────────────────────────────────────────────────

/// Compute weighted scores for each architectural layer.
pub(crate) fn compute_layer_scores(path: &str, content: &str) -> Vec<(ArchLayer, i32)> {
    use std::collections::HashMap;
    let mut scores: HashMap<ArchLayer, i32> = HashMap::new();

    for signal in CONTENT_SIGNALS {
        if content.contains(signal.pattern) {
            *scores.entry(signal.layer).or_insert(0) += signal.weight;
        }
    }

    let parts: Vec<&str> = path.split('/').collect();
    for part in &parts {
        let p = part.to_lowercase();
        for (names, layer, bonus) in DIR_BONUS {
            if names.iter().any(|n| *n == p.as_str()) {
                *scores.entry(*layer).or_insert(0) += bonus;
            }
        }
    }

    scores.into_iter().collect()
}

// ── Signal & bonus tables ──────────────────────────────────────────────────

struct ContentSignal {
    pattern: &'static str,
    layer: ArchLayer,
    weight: i32,
}

/// Content signals — the primary classification method.
/// Covers: Rust, Python, JS/TS, Go, Dart/Flutter, Java/Kotlin,
///         and frameworks: Astro, React, Next.js, Vue, Angular, NestJS,
///         Express, FastAPI, Django, Flask, Gin, Echo, Fiber, Actix,
///         Rocket, Axum, Tauri, Shelf, dart_frog, Riverpod, Bloc.
const CONTENT_SIGNALS: &[ContentSignal] = &[
    // ── Controller signals (HTTP handlers, CLI commands, RPC, UI pages) ─────
    // Rust — Actix-web, Rocket, Axum, Tauri, MCP
    ContentSignal { pattern: "#[tauri::command]",   layer: ArchLayer::Controller, weight: 4 },
    ContentSignal { pattern: "CallToolResult",      layer: ArchLayer::Controller, weight: 4 },
    ContentSignal { pattern: "#[get(",              layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "#[post(",             layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "#[put(",              layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "#[delete(",           layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "#[derive(Args",       layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "#[derive(Subcommand", layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "#[command(",          layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "axum::Router",        layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "axum::extract",       layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "actix_web::web",      layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "HttpResponse",        layer: ArchLayer::Controller, weight: 2 },
    // Python — Flask, FastAPI, Django views
    ContentSignal { pattern: "@app.route(",         layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "@app.get(",           layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "@app.post(",          layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "@router.",            layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "APIRouter",           layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "def get(self, request",  layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "def post(self, request", layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "@api_view",           layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "class Meta:\n        model", layer: ArchLayer::Controller, weight: 2 },
    // JS/TS — Express, Fastify, Koa, Hapi
    ContentSignal { pattern: "app.get(",            layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "app.post(",           layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "app.put(",            layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "app.delete(",         layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "router.get(",         layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "router.post(",        layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "fastify.get(",        layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "fastify.post(",       layer: ArchLayer::Controller, weight: 3 },
    // NestJS
    ContentSignal { pattern: "@Controller(",        layer: ArchLayer::Controller, weight: 4 },
    ContentSignal { pattern: "@Get(",               layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "@Post(",              layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "@Put(",               layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "@Delete(",            layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "@Patch(",             layer: ArchLayer::Controller, weight: 3 },
    // Next.js
    ContentSignal { pattern: "export default function Page",  layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "getServerSideProps",  layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "getStaticProps",      layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "export async function GET(",  layer: ArchLayer::Controller, weight: 4 },
    ContentSignal { pattern: "export async function POST(", layer: ArchLayer::Controller, weight: 4 },
    ContentSignal { pattern: "NextRequest",         layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "NextResponse",        layer: ArchLayer::Controller, weight: 3 },
    // React
    ContentSignal { pattern: "useState(",           layer: ArchLayer::Controller, weight: 2 },
    ContentSignal { pattern: "useEffect(",          layer: ArchLayer::Controller, weight: 2 },
    ContentSignal { pattern: "return (<",           layer: ArchLayer::Controller, weight: 2 },
    ContentSignal { pattern: "return (\n",          layer: ArchLayer::Controller, weight: 1 },
    ContentSignal { pattern: "React.FC",            layer: ArchLayer::Controller, weight: 2 },
    ContentSignal { pattern: "JSX.Element",         layer: ArchLayer::Controller, weight: 2 },
    // Vue.js
    ContentSignal { pattern: "defineComponent(",    layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "<script setup",       layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "defineProps(",        layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "<template>",          layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "createApp(",          layer: ArchLayer::Controller, weight: 2 },
    // Angular
    ContentSignal { pattern: "@Component(",         layer: ArchLayer::Controller, weight: 5 },
    ContentSignal { pattern: "@NgModule(",          layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "@Directive(",         layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "@Pipe(",              layer: ArchLayer::Controller, weight: 2 },
    // Astro
    ContentSignal { pattern: "Astro.props",         layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "Astro.request",       layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "Astro.redirect(",     layer: ArchLayer::Controller, weight: 3 },
    // Go — Gin, Echo, Fiber, Chi, stdlib
    ContentSignal { pattern: "http.HandleFunc(",    layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "gin.Context",         layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "echo.Context",        layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "fiber.Ctx",           layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "chi.Router",          layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "http.Handler",        layer: ArchLayer::Controller, weight: 2 },
    ContentSignal { pattern: "func (w http.ResponseWriter", layer: ArchLayer::Controller, weight: 4 },
    // Dart/Flutter — shelf, dart_frog, UI widgets
    ContentSignal { pattern: "extends StatelessWidget",  layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "extends StatefulWidget",   layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "Widget build(",            layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "Route(",                   layer: ArchLayer::Controller, weight: 2 },
    ContentSignal { pattern: "shelf.Router",             layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "onRequest(",               layer: ArchLayer::Controller, weight: 3 },
    // Java/Kotlin — Spring MVC
    ContentSignal { pattern: "@RestController",     layer: ArchLayer::Controller, weight: 4 },
    ContentSignal { pattern: "@GetMapping",         layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "@PostMapping",        layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "@PutMapping",         layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "@DeleteMapping",      layer: ArchLayer::Controller, weight: 3 },
    ContentSignal { pattern: "@RequestMapping",     layer: ArchLayer::Controller, weight: 3 },

    // ── Service signals ─────────────────────────────────────────────────────
    // Rust
    ContentSignal { pattern: "#[async_trait]",      layer: ArchLayer::Service, weight: 2 },
    ContentSignal { pattern: "impl ",               layer: ArchLayer::Service, weight: 1 },
    // Python
    ContentSignal { pattern: "class ",              layer: ArchLayer::Service, weight: 1 },
    // JS/TS — NestJS, Angular
    ContentSignal { pattern: "@Injectable(",        layer: ArchLayer::Service, weight: 4 },
    ContentSignal { pattern: "@Injectable({",       layer: ArchLayer::Service, weight: 4 },
    ContentSignal { pattern: "providedIn:",         layer: ArchLayer::Service, weight: 3 },
    // Dart/Flutter — Riverpod, Bloc, Provider
    ContentSignal { pattern: "extends ChangeNotifier",  layer: ArchLayer::Service, weight: 4 },
    ContentSignal { pattern: "extends Bloc<",           layer: ArchLayer::Service, weight: 4 },
    ContentSignal { pattern: "extends Cubit<",          layer: ArchLayer::Service, weight: 4 },
    ContentSignal { pattern: "StateNotifierProvider",   layer: ArchLayer::Service, weight: 3 },
    ContentSignal { pattern: "riverpod",                layer: ArchLayer::Service, weight: 2 },
    ContentSignal { pattern: "FutureProvider",          layer: ArchLayer::Service, weight: 3 },
    ContentSignal { pattern: "StreamProvider",          layer: ArchLayer::Service, weight: 3 },
    // React/Vue — custom hooks / composables as services
    ContentSignal { pattern: "export function use",     layer: ArchLayer::Service, weight: 3 },
    ContentSignal { pattern: "export const use",        layer: ArchLayer::Service, weight: 3 },
    // Go
    ContentSignal { pattern: "type Service struct",     layer: ArchLayer::Service, weight: 3 },
    // Java
    ContentSignal { pattern: "@Service",            layer: ArchLayer::Service, weight: 4 },
    ContentSignal { pattern: "@Component",          layer: ArchLayer::Service, weight: 3 },
    ContentSignal { pattern: "@Transactional",      layer: ArchLayer::Service, weight: 2 },

    // ── Repository signals ──────────────────────────────────────────────────
    // Rust
    ContentSignal { pattern: "diesel::",            layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "sqlx::",              layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "sea_orm::",           layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "#[derive(Queryable",  layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "#[derive(Insertable", layer: ArchLayer::Repository, weight: 4 },
    // Python
    ContentSignal { pattern: "session.query(",      layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "cursor.execute(",     layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "objects.filter(",     layer: ArchLayer::Repository, weight: 3 },
    ContentSignal { pattern: "objects.get(",        layer: ArchLayer::Repository, weight: 3 },
    ContentSignal { pattern: "objects.create(",     layer: ArchLayer::Repository, weight: 3 },
    ContentSignal { pattern: "objects.all(",        layer: ArchLayer::Repository, weight: 3 },
    ContentSignal { pattern: "tortoise.models",     layer: ArchLayer::Repository, weight: 3 },
    // JS/TS
    ContentSignal { pattern: "prisma.",             layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "sequelize.",          layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "mongoose.",           layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "mongoose.model(",    layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "knex(",              layer: ArchLayer::Repository, weight: 3 },
    ContentSignal { pattern: "typeorm",            layer: ArchLayer::Repository, weight: 3 },
    ContentSignal { pattern: "drizzle(",           layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "getRepository(",     layer: ArchLayer::Repository, weight: 3 },
    ContentSignal { pattern: "@Entity(",           layer: ArchLayer::Repository, weight: 3 },
    // Go
    ContentSignal { pattern: "sql.DB",             layer: ArchLayer::Repository, weight: 3 },
    ContentSignal { pattern: "gorm.",              layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "gorm.Model",         layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "sqlx.DB",            layer: ArchLayer::Repository, weight: 3 },
    // Dart/Flutter
    ContentSignal { pattern: "extends DatabaseAccessor",  layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "@DriftDatabase(",           layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "@dao",                      layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "FloorDatabase",             layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "openDatabase(",             layer: ArchLayer::Repository, weight: 3 },
    ContentSignal { pattern: "Isar.open(",                layer: ArchLayer::Repository, weight: 3 },
    // Java
    ContentSignal { pattern: "@Repository",        layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "JpaRepository",      layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "CrudRepository",     layer: ArchLayer::Repository, weight: 4 },
    ContentSignal { pattern: "EntityManager",      layer: ArchLayer::Repository, weight: 3 },
    ContentSignal { pattern: "@Query(",            layer: ArchLayer::Repository, weight: 3 },
    // General SQL
    ContentSignal { pattern: "SELECT ",            layer: ArchLayer::Repository, weight: 2 },
    ContentSignal { pattern: "INSERT INTO",        layer: ArchLayer::Repository, weight: 2 },
    ContentSignal { pattern: "CREATE TABLE",       layer: ArchLayer::Repository, weight: 3 },

    // ── Model signals ───────────────────────────────────────────────────────
    // Rust
    ContentSignal { pattern: "#[derive(Serialize",    layer: ArchLayer::Model, weight: 2 },
    ContentSignal { pattern: "#[derive(Deserialize",  layer: ArchLayer::Model, weight: 2 },
    ContentSignal { pattern: "pub struct ",           layer: ArchLayer::Model, weight: 1 },
    ContentSignal { pattern: "pub enum ",             layer: ArchLayer::Model, weight: 1 },
    // Python
    ContentSignal { pattern: "class Meta:",           layer: ArchLayer::Model, weight: 3 },
    ContentSignal { pattern: "(BaseModel):",          layer: ArchLayer::Model, weight: 3 },
    ContentSignal { pattern: "(models.Model):",       layer: ArchLayer::Model, weight: 4 },
    ContentSignal { pattern: "@dataclass",            layer: ArchLayer::Model, weight: 3 },
    ContentSignal { pattern: "@attr.s",               layer: ArchLayer::Model, weight: 3 },
    ContentSignal { pattern: "Field(",                layer: ArchLayer::Model, weight: 1 },
    // JS/TS
    ContentSignal { pattern: "z.object(",             layer: ArchLayer::Model, weight: 3 },
    ContentSignal { pattern: "z.string(",             layer: ArchLayer::Model, weight: 2 },
    ContentSignal { pattern: "export interface ",     layer: ArchLayer::Model, weight: 2 },
    ContentSignal { pattern: "export type ",          layer: ArchLayer::Model, weight: 2 },
    ContentSignal { pattern: "new Schema(",           layer: ArchLayer::Model, weight: 3 },
    ContentSignal { pattern: "@Schema(",              layer: ArchLayer::Model, weight: 3 },
    ContentSignal { pattern: "@Prop(",                layer: ArchLayer::Model, weight: 2 },
    // Go
    ContentSignal { pattern: "`json:\"",              layer: ArchLayer::Model, weight: 2 },
    ContentSignal { pattern: "`xml:\"",               layer: ArchLayer::Model, weight: 2 },
    ContentSignal { pattern: "`db:\"",                layer: ArchLayer::Model, weight: 2 },
    // Dart/Flutter
    ContentSignal { pattern: "@JsonSerializable(",    layer: ArchLayer::Model, weight: 4 },
    ContentSignal { pattern: "@freezed",              layer: ArchLayer::Model, weight: 4 },
    ContentSignal { pattern: "factory ",              layer: ArchLayer::Model, weight: 1 },
    ContentSignal { pattern: "fromJson(",             layer: ArchLayer::Model, weight: 2 },
    ContentSignal { pattern: "toJson(",               layer: ArchLayer::Model, weight: 2 },
    // Java
    ContentSignal { pattern: "@Entity",               layer: ArchLayer::Model, weight: 4 },
    ContentSignal { pattern: "@Data",                 layer: ArchLayer::Model, weight: 3 },
    ContentSignal { pattern: "@Table(",               layer: ArchLayer::Model, weight: 3 },
    ContentSignal { pattern: "@Column(",              layer: ArchLayer::Model, weight: 2 },

    // ── Utility signals ─────────────────────────────────────────────────────
    ContentSignal { pattern: "pub fn ",               layer: ArchLayer::Utility, weight: 1 },
    ContentSignal { pattern: "function ",             layer: ArchLayer::Utility, weight: 1 },
    ContentSignal { pattern: "def ",                  layer: ArchLayer::Utility, weight: 1 },
    ContentSignal { pattern: "func ",                 layer: ArchLayer::Utility, weight: 1 },

    // ── Config signals ──────────────────────────────────────────────────────
    ContentSignal { pattern: "dotenv",                layer: ArchLayer::Config, weight: 3 },
    ContentSignal { pattern: "env::",                 layer: ArchLayer::Config, weight: 2 },
    ContentSignal { pattern: "process.env.",          layer: ArchLayer::Config, weight: 2 },
    ContentSignal { pattern: "os.environ",            layer: ArchLayer::Config, weight: 2 },
    ContentSignal { pattern: "os.Getenv(",            layer: ArchLayer::Config, weight: 2 },
    ContentSignal { pattern: "Platform.environment",  layer: ArchLayer::Config, weight: 2 },
    ContentSignal { pattern: "defineConfig(",         layer: ArchLayer::Config, weight: 3 },
    ContentSignal { pattern: "vite.config",           layer: ArchLayer::Config, weight: 3 },
    ContentSignal { pattern: "next.config",           layer: ArchLayer::Config, weight: 3 },
    ContentSignal { pattern: "nuxt.config",           layer: ArchLayer::Config, weight: 3 },
    ContentSignal { pattern: "astro.config",          layer: ArchLayer::Config, weight: 3 },
    ContentSignal { pattern: "angular.json",          layer: ArchLayer::Config, weight: 3 },
];

/// Directory name bonus — universal conventions only (not project-specific).
const DIR_BONUS: &[(&[&str], ArchLayer, i32)] = &[
    (
        &[
            "controllers", "controller", "routes", "routers", "handlers",
            "views", "endpoints", "api", "commands", "tools", "pages",
        ],
        ArchLayer::Controller,
        2,
    ),
    (
        &[
            "services", "service", "usecases", "use_cases", "domain",
            "business", "logic",
        ],
        ArchLayer::Service,
        2,
    ),
    (
        &[
            "repositories", "repository", "repos", "dao", "dal", "data",
            "db", "database", "persistence", "migration", "migrations",
        ],
        ArchLayer::Repository,
        2,
    ),
    (
        &[
            "models", "model", "entities", "entity", "schemas", "schema",
            "types", "dto", "dtos", "proto",
        ],
        ArchLayer::Model,
        2,
    ),
    (
        &[
            "utils", "util", "helpers", "helper", "common", "shared",
            "lib", "middleware",
        ],
        ArchLayer::Utility,
        2,
    ),
    (
        &["config", "configuration", "settings"],
        ArchLayer::Config,
        2,
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    // ── Rust ───────────────────────────────────────────────────────────────

    #[test]
    fn test_rust_controller_tauri() {
        let content = "#[tauri::command]\npub async fn list() -> Result<Vec<Service>, String> { todo!() }";
        assert_eq!(classify_layer("src/commands/services.rs", content), ArchLayer::Controller);
    }

    #[test]
    fn test_rust_controller_actix() {
        let content = "#[get(\"/api/users\")]\nasync fn get_users() -> HttpResponse { HttpResponse::Ok().finish() }";
        assert_eq!(classify_layer("src/handlers/users.rs", content), ArchLayer::Controller);
    }

    #[test]
    fn test_rust_service_impl() {
        let content = "#[async_trait]\nimpl Runner for LocalRunner {\n    async fn start(&self) -> Result<()> { todo!() }\n}";
        assert_eq!(classify_layer("src/runner/local.rs", content), ArchLayer::Service);
    }

    #[test]
    fn test_rust_model() {
        let content = "#[derive(Serialize, Deserialize, Clone)]\npub struct Project {\n    pub name: String,\n}\n#[derive(Serialize, Deserialize)]\npub enum RunnerType { Local, Docker }";
        assert_eq!(classify_layer("src/model.rs", content), ArchLayer::Model);
    }

    #[test]
    fn test_rust_repository_sqlx() {
        let content = concat!(
            "pub async fn get_user(pool: &sqlx::PgPool) -> Result<User> {\n",
            "    sqlx::query_as(\"SELECT * FROM users\").fetch_one(pool).await\n",
            "}\n",
        );
        assert_eq!(classify_layer("src/db/users.rs", content), ArchLayer::Repository);
    }

    // ── Python ─────────────────────────────────────────────────────────────

    #[test]
    fn test_python_fastapi() {
        let content = "from fastapi import APIRouter\nrouter = APIRouter()\n@router.get(\"/users\")\nasync def get_users(): pass";
        assert_eq!(classify_layer("app/routes/users.py", content), ArchLayer::Controller);
    }

    #[test]
    fn test_python_django_model() {
        let content = "from django.db import models\nclass User(models.Model):\n    name = models.CharField(max_length=100)\n    class Meta:\n        db_table = 'users'";
        assert_eq!(classify_layer("app/models.py", content), ArchLayer::Model);
    }

    #[test]
    fn test_python_sqlalchemy_repo() {
        let content = "def get_user(db):\n    return db.session.query(User).filter(User.id == 1).first()";
        assert_eq!(classify_layer("app/repos/user_repo.py", content), ArchLayer::Repository);
    }

    // ── JS/TS ──────────────────────────────────────────────────────────────

    #[test]
    fn test_nestjs_controller() {
        let content = "@Controller('users')\nexport class UsersController {\n    @Get()\n    findAll() {}\n    @Post()\n    create() {}\n}";
        assert_eq!(classify_layer("src/users/users.controller.ts", content), ArchLayer::Controller);
    }

    #[test]
    fn test_nestjs_service() {
        let content = "@Injectable()\nexport class UsersService {\n    constructor(private repo: UsersRepository) {}\n    findAll() { return this.repo.find(); }\n}";
        assert_eq!(classify_layer("src/users/users.service.ts", content), ArchLayer::Service);
    }

    #[test]
    fn test_nextjs_api_route() {
        let content = "import { NextRequest, NextResponse } from 'next/server';\nexport async function GET(request: NextRequest) {\n    return NextResponse.json({ users: [] });\n}";
        assert_eq!(classify_layer("app/api/users/route.ts", content), ArchLayer::Controller);
    }

    #[test]
    fn test_react_component() {
        let content = "import { useState, useEffect } from 'react';\nexport default function UserList() {\n    const [users, setUsers] = useState([]);\n    useEffect(() => { fetchUsers(); }, []);\n    return (<div>{users.map(u => <p>{u.name}</p>)}</div>);\n}";
        assert_eq!(classify_layer("src/components/UserList.tsx", content), ArchLayer::Controller);
    }

    #[test]
    fn test_vue_component() {
        let content = "<template>\n    <div>{{ message }}</div>\n</template>\n<script setup>\nimport { ref } from 'vue'\nconst message = ref('Hello')\n</script>";
        assert_eq!(classify_layer("src/components/Hello.vue", content), ArchLayer::Controller);
    }

    #[test]
    fn test_angular_component() {
        let content = "@Component({\n    selector: 'app-user-list',\n    templateUrl: './user-list.component.html'\n})\nexport class UserListComponent implements OnInit { }";
        assert_eq!(classify_layer("src/app/user-list.component.ts", content), ArchLayer::Controller);
    }

    #[test]
    fn test_prisma_repository() {
        let content = "const prisma = new PrismaClient();\nexport async function getUsers() {\n    return prisma.user.findMany();\n}";
        assert_eq!(classify_layer("src/repos/userRepo.ts", content), ArchLayer::Repository);
    }

    #[test]
    fn test_zod_model() {
        let content = "import { z } from 'zod';\nexport const UserSchema = z.object({\n    id: z.string(),\n    name: z.string(),\n});\nexport type User = z.infer<typeof UserSchema>;";
        assert_eq!(classify_layer("src/schemas/user.ts", content), ArchLayer::Model);
    }

    // ── Go ──────────────────────────────────────────────────────────────────

    #[test]
    fn test_go_handler() {
        let content = "func GetUsers(c *gin.Context) {\n    users := service.GetAll()\n    c.JSON(200, users)\n}";
        assert_eq!(classify_layer("handlers/users.go", content), ArchLayer::Controller);
    }

    #[test]
    fn test_go_gorm_repo() {
        let content = "func (r *UserRepo) FindAll(db *gorm.DB) []User {\n    var users []User\n    db.Find(&users)\n    return users\n}";
        assert_eq!(classify_layer("repos/user_repo.go", content), ArchLayer::Repository);
    }

    // ── Dart/Flutter ────────────────────────────────────────────────────────

    #[test]
    fn test_flutter_widget() {
        let content = "class HomePage extends StatefulWidget {\n    @override\n    State<HomePage> createState() => _HomePageState();\n}";
        assert_eq!(classify_layer("lib/pages/home_page.dart", content), ArchLayer::Controller);
    }

    #[test]
    fn test_flutter_bloc_service() {
        let content = "class UserBloc extends Bloc<UserEvent, UserState> {\n    final UserRepository repo;\n    UserBloc(this.repo) : super(UserInitial());\n}";
        assert_eq!(classify_layer("lib/blocs/user_bloc.dart", content), ArchLayer::Service);
    }

    #[test]
    fn test_dart_json_model() {
        let content = "@JsonSerializable()\nclass User {\n    final String id;\n    factory User.fromJson(Map<String, dynamic> json) => _\u{24}UserFromJson(json);\n    Map<String, dynamic> toJson() => _\u{24}UserToJson(this);\n}";
        assert_eq!(classify_layer("lib/models/user.dart", content), ArchLayer::Model);
    }

    // ── Astro & MCP ────────────────────────────────────────────────────────

    #[test]
    fn test_astro_page() {
        let content = "const data = Astro.props;\nconst resp = Astro.request;";
        assert_eq!(classify_layer("src/pages/index.astro", content), ArchLayer::Controller);
    }

    #[test]
    fn test_mcp_tool() {
        let content = "pub async fn handle(params: Value) -> CallToolResult {\n    CallToolResult::success(vec![])\n}";
        assert_eq!(classify_layer("src/tools/analysis.rs", content), ArchLayer::Controller);
    }

    // ── Deterministic overrides ────────────────────────────────────────────

    #[test]
    fn test_test_file() {
        assert_eq!(classify_layer("tests/test_users.py", "def test_create(): pass"), ArchLayer::Test);
        assert_eq!(classify_layer("src/__tests__/user.spec.ts", "describe('User', () => {})"), ArchLayer::Test);
    }

    #[test]
    fn test_config_file() {
        assert_eq!(classify_layer("src/config.py", "DEBUG = True"), ArchLayer::Config);
        assert_eq!(classify_layer(".env.production", "API_KEY=xxx"), ArchLayer::Config);
    }

    #[test]
    fn test_entry_points() {
        assert_eq!(classify_layer("src/main.rs", "fn main() {}"), ArchLayer::Utility);
        assert_eq!(classify_layer("src/lib.rs", "pub mod model;"), ArchLayer::Utility);
        assert_eq!(classify_layer("build.rs", "fn main() {}"), ArchLayer::Utility);
    }

    // ── Scoring behavior ───────────────────────────────────────────────────

    #[test]
    fn test_content_wins_over_path() {
        let content = "#[get(\"/health\")]\nasync fn health() -> HttpResponse { HttpResponse::Ok().finish() }";
        assert_eq!(classify_layer("src/misc/health.rs", content), ArchLayer::Controller);
    }

    #[test]
    fn test_unknown_minimal_content() {
        assert_eq!(classify_layer("src/foo.rs", "// empty"), ArchLayer::Unknown);
    }

    #[test]
    fn test_scores_accumulate() {
        let content = "#[get(\"/a\")]\n#[post(\"/b\")]\n#[put(\"/c\")]";
        let scores = compute_layer_scores("src/api.rs", content);
        let ctrl = scores.iter().find(|(l, _)| *l == ArchLayer::Controller).map(|(_, s)| *s).unwrap_or(0);
        assert!(ctrl >= 9, "3 controller signals should give >=9 pts, got {ctrl}");
    }

    #[test]
    fn test_dir_bonus_applied() {
        let scores = compute_layer_scores("controllers/user.rs", "");
        let ctrl = scores.iter().find(|(l, _)| *l == ArchLayer::Controller).map(|(_, s)| *s).unwrap_or(0);
        assert_eq!(ctrl, 2);
    }

    #[test]
    fn test_content_beats_dir() {
        let content = "@Controller('users')\n@Get()\n@Post()\n@Put()";
        let layer = classify_layer("models/user_controller.ts", content);
        assert_eq!(layer, ArchLayer::Controller);
    }

    // ── Fan-in/fan-out ─────────────────────────────────────────────────────

    #[test]
    fn test_fanin_refines_to_model() {
        let mut modules = vec![
            ModuleNode { path: "types.rs".into(), language: Language::Rust, layer: ArchLayer::Unknown, loc: 50, class_count: 0, function_count: 2 },
            ModuleNode { path: "a.rs".into(), language: Language::Rust, layer: ArchLayer::Service, loc: 100, class_count: 0, function_count: 5 },
            ModuleNode { path: "b.rs".into(), language: Language::Rust, layer: ArchLayer::Service, loc: 80, class_count: 0, function_count: 3 },
            ModuleNode { path: "c.rs".into(), language: Language::Rust, layer: ArchLayer::Controller, loc: 60, class_count: 0, function_count: 4 },
            ModuleNode { path: "d.rs".into(), language: Language::Rust, layer: ArchLayer::Controller, loc: 40, class_count: 0, function_count: 2 },
            ModuleNode { path: "e.rs".into(), language: Language::Rust, layer: ArchLayer::Service, loc: 70, class_count: 0, function_count: 3 },
            ModuleNode { path: "f.rs".into(), language: Language::Rust, layer: ArchLayer::Controller, loc: 50, class_count: 0, function_count: 2 },
            ModuleNode { path: "g.rs".into(), language: Language::Rust, layer: ArchLayer::Utility, loc: 30, class_count: 0, function_count: 1 },
            ModuleNode { path: "h.rs".into(), language: Language::Rust, layer: ArchLayer::Service, loc: 90, class_count: 0, function_count: 4 },
        ];
        let edges = vec![
            ImportEdge { from: "a.rs".into(), to: "types.rs".into(), is_external: false },
            ImportEdge { from: "b.rs".into(), to: "types.rs".into(), is_external: false },
            ImportEdge { from: "c.rs".into(), to: "types.rs".into(), is_external: false },
            ImportEdge { from: "d.rs".into(), to: "types.rs".into(), is_external: false },
            ImportEdge { from: "e.rs".into(), to: "types.rs".into(), is_external: false },
            ImportEdge { from: "f.rs".into(), to: "types.rs".into(), is_external: false },
            ImportEdge { from: "g.rs".into(), to: "types.rs".into(), is_external: false },
            ImportEdge { from: "h.rs".into(), to: "types.rs".into(), is_external: false },
            ImportEdge { from: "a.rs".into(), to: "b.rs".into(), is_external: false },
            ImportEdge { from: "c.rs".into(), to: "d.rs".into(), is_external: false },
        ];
        refine_unknown_by_graph(&mut modules, &edges);
        assert_eq!(modules[0].layer, ArchLayer::Model);
    }

    #[test]
    fn test_fanout_refines_to_controller() {
        let mut modules = vec![
            ModuleNode { path: "handler.rs".into(), language: Language::Rust, layer: ArchLayer::Unknown, loc: 200, class_count: 0, function_count: 10 },
            ModuleNode { path: "svc_a.rs".into(), language: Language::Rust, layer: ArchLayer::Service, loc: 50, class_count: 0, function_count: 3 },
            ModuleNode { path: "svc_b.rs".into(), language: Language::Rust, layer: ArchLayer::Service, loc: 50, class_count: 0, function_count: 3 },
            ModuleNode { path: "model.rs".into(), language: Language::Rust, layer: ArchLayer::Model, loc: 30, class_count: 0, function_count: 1 },
        ];
        let edges = vec![
            ImportEdge { from: "handler.rs".into(), to: "svc_a.rs".into(), is_external: false },
            ImportEdge { from: "handler.rs".into(), to: "svc_b.rs".into(), is_external: false },
            ImportEdge { from: "handler.rs".into(), to: "model.rs".into(), is_external: false },
            ImportEdge { from: "svc_a.rs".into(), to: "model.rs".into(), is_external: false },
        ];
        refine_unknown_by_graph(&mut modules, &edges);
        assert_eq!(modules[0].layer, ArchLayer::Controller);
    }

    #[test]
    fn test_fanout_gt_fanin_refines_to_service() {
        let mut modules = vec![
            ModuleNode { path: "orch.rs".into(), language: Language::Rust, layer: ArchLayer::Unknown, loc: 100, class_count: 0, function_count: 5 },
            ModuleNode { path: "dep_a.rs".into(), language: Language::Rust, layer: ArchLayer::Model, loc: 30, class_count: 0, function_count: 1 },
            ModuleNode { path: "dep_b.rs".into(), language: Language::Rust, layer: ArchLayer::Model, loc: 30, class_count: 0, function_count: 1 },
            ModuleNode { path: "caller.rs".into(), language: Language::Rust, layer: ArchLayer::Controller, loc: 50, class_count: 0, function_count: 3 },
        ];
        let edges = vec![
            ImportEdge { from: "orch.rs".into(), to: "dep_a.rs".into(), is_external: false },
            ImportEdge { from: "orch.rs".into(), to: "dep_b.rs".into(), is_external: false },
            ImportEdge { from: "caller.rs".into(), to: "orch.rs".into(), is_external: false },
        ];
        refine_unknown_by_graph(&mut modules, &edges);
        assert_eq!(modules[0].layer, ArchLayer::Service);
    }

    #[test]
    fn test_no_edges_stays_unknown() {
        let mut modules = vec![
            ModuleNode { path: "isolated.rs".into(), language: Language::Rust, layer: ArchLayer::Unknown, loc: 10, class_count: 0, function_count: 1 },
        ];
        refine_unknown_by_graph(&mut modules, &[]);
        assert_eq!(modules[0].layer, ArchLayer::Unknown);
    }

    #[test]
    fn test_already_classified_not_changed() {
        let mut modules = vec![
            ModuleNode { path: "service.rs".into(), language: Language::Rust, layer: ArchLayer::Service, loc: 100, class_count: 0, function_count: 5 },
            ModuleNode { path: "dep.rs".into(), language: Language::Rust, layer: ArchLayer::Model, loc: 30, class_count: 0, function_count: 1 },
        ];
        let edges = vec![
            ImportEdge { from: "dep.rs".into(), to: "service.rs".into(), is_external: false },
        ];
        refine_unknown_by_graph(&mut modules, &edges);
        assert_eq!(modules[0].layer, ArchLayer::Service);
    }

    // ── Express/Mongoose edge cases ────────────────────────────────────────

    #[test]
    fn test_express_router() {
        let content = "const router = require('express').Router();\nrouter.get('/users', getUsers);\nrouter.post('/users', createUser);";
        assert_eq!(classify_layer("routes/users.js", content), ArchLayer::Controller);
    }

    #[test]
    fn test_mongoose_model() {
        let content = "const mongoose = require('mongoose');\nconst userSchema = new Schema({\n    name: String,\n});\nmodule.exports = mongoose.model('User', userSchema);";
        assert_eq!(classify_layer("models/user.js", content), ArchLayer::Repository);
    }
}
