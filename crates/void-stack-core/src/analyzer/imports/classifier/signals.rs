//! Content signal and directory bonus tables for the layer classifier.

use super::super::super::graph::ArchLayer;

pub(super) struct ContentSignal {
    pub pattern: &'static str,
    pub layer: ArchLayer,
    pub weight: i32,
}

/// Content signals — the primary classification method.
/// Covers: Rust, Python, JS/TS, Go, Dart/Flutter, Java/Kotlin,
///         and frameworks: Astro, React, Next.js, Vue, Angular, NestJS,
///         Express, FastAPI, Django, Flask, Gin, Echo, Fiber, Actix,
///         Rocket, Axum, Tauri, Shelf, dart_frog, Riverpod, Bloc.
pub(super) const CONTENT_SIGNALS: &[ContentSignal] = &[
    // ── Controller signals (HTTP handlers, CLI commands, RPC, UI pages) ─────
    // Rust — Actix-web, Rocket, Axum, Tauri, MCP
    ContentSignal {
        pattern: "#[tauri::command]",
        layer: ArchLayer::Controller,
        weight: 4,
    },
    ContentSignal {
        pattern: "CallToolResult",
        layer: ArchLayer::Controller,
        weight: 4,
    },
    ContentSignal {
        pattern: "#[get(",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "#[post(",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "#[put(",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "#[delete(",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "#[derive(Args",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "#[derive(Subcommand",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "#[command(",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "axum::Router",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "axum::extract",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "actix_web::web",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "HttpResponse",
        layer: ArchLayer::Controller,
        weight: 2,
    },
    // Python — Flask, FastAPI, Django views
    ContentSignal {
        pattern: "@app.route(",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "@app.get(",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "@app.post(",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "@router.",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "APIRouter",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "def get(self, request",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "def post(self, request",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "@api_view",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "class Meta:\n        model",
        layer: ArchLayer::Controller,
        weight: 2,
    },
    // JS/TS — Express, Fastify, Koa, Hapi
    ContentSignal {
        pattern: "app.get(",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "app.post(",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "app.put(",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "app.delete(",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "router.get(",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "router.post(",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "fastify.get(",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "fastify.post(",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    // NestJS
    ContentSignal {
        pattern: "@Controller(",
        layer: ArchLayer::Controller,
        weight: 4,
    },
    ContentSignal {
        pattern: "@Get(",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "@Post(",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "@Put(",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "@Delete(",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "@Patch(",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    // Next.js
    ContentSignal {
        pattern: "export default function Page",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "getServerSideProps",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "getStaticProps",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "export async function GET(",
        layer: ArchLayer::Controller,
        weight: 4,
    },
    ContentSignal {
        pattern: "export async function POST(",
        layer: ArchLayer::Controller,
        weight: 4,
    },
    ContentSignal {
        pattern: "NextRequest",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "NextResponse",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    // React
    ContentSignal {
        pattern: "useState(",
        layer: ArchLayer::Controller,
        weight: 2,
    },
    ContentSignal {
        pattern: "useEffect(",
        layer: ArchLayer::Controller,
        weight: 2,
    },
    ContentSignal {
        pattern: "return (<",
        layer: ArchLayer::Controller,
        weight: 2,
    },
    ContentSignal {
        pattern: "return (\n",
        layer: ArchLayer::Controller,
        weight: 1,
    },
    ContentSignal {
        pattern: "React.FC",
        layer: ArchLayer::Controller,
        weight: 2,
    },
    ContentSignal {
        pattern: "JSX.Element",
        layer: ArchLayer::Controller,
        weight: 2,
    },
    // Vue.js
    ContentSignal {
        pattern: "defineComponent(",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "<script setup",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "defineProps(",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "<template>",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "createApp(",
        layer: ArchLayer::Controller,
        weight: 2,
    },
    // Angular
    ContentSignal {
        pattern: "@Component(",
        layer: ArchLayer::Controller,
        weight: 5,
    },
    ContentSignal {
        pattern: "@NgModule(",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "@Directive(",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "@Pipe(",
        layer: ArchLayer::Controller,
        weight: 2,
    },
    // Astro
    ContentSignal {
        pattern: "Astro.props",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "Astro.request",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "Astro.redirect(",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    // Go — Gin, Echo, Fiber, Chi, stdlib
    ContentSignal {
        pattern: "http.HandleFunc(",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "gin.Context",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "echo.Context",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "fiber.Ctx",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "chi.Router",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "http.Handler",
        layer: ArchLayer::Controller,
        weight: 2,
    },
    ContentSignal {
        pattern: "func (w http.ResponseWriter",
        layer: ArchLayer::Controller,
        weight: 4,
    },
    // Dart/Flutter — shelf, dart_frog, UI widgets
    ContentSignal {
        pattern: "extends StatelessWidget",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "extends StatefulWidget",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "Widget build(",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "Route(",
        layer: ArchLayer::Controller,
        weight: 2,
    },
    ContentSignal {
        pattern: "shelf.Router",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "onRequest(",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    // Java/Kotlin — Spring MVC
    ContentSignal {
        pattern: "@RestController",
        layer: ArchLayer::Controller,
        weight: 4,
    },
    ContentSignal {
        pattern: "@GetMapping",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "@PostMapping",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "@PutMapping",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "@DeleteMapping",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    ContentSignal {
        pattern: "@RequestMapping",
        layer: ArchLayer::Controller,
        weight: 3,
    },
    // ── Service signals ─────────────────────────────────────────────────────
    // Rust
    ContentSignal {
        pattern: "#[async_trait]",
        layer: ArchLayer::Service,
        weight: 2,
    },
    ContentSignal {
        pattern: "impl ",
        layer: ArchLayer::Service,
        weight: 1,
    },
    // Python
    ContentSignal {
        pattern: "class ",
        layer: ArchLayer::Service,
        weight: 1,
    },
    // JS/TS — NestJS, Angular
    ContentSignal {
        pattern: "@Injectable(",
        layer: ArchLayer::Service,
        weight: 4,
    },
    ContentSignal {
        pattern: "@Injectable({",
        layer: ArchLayer::Service,
        weight: 4,
    },
    ContentSignal {
        pattern: "providedIn:",
        layer: ArchLayer::Service,
        weight: 3,
    },
    // Dart/Flutter — Riverpod, Bloc, Provider
    ContentSignal {
        pattern: "extends ChangeNotifier",
        layer: ArchLayer::Service,
        weight: 4,
    },
    ContentSignal {
        pattern: "extends Bloc<",
        layer: ArchLayer::Service,
        weight: 4,
    },
    ContentSignal {
        pattern: "extends Cubit<",
        layer: ArchLayer::Service,
        weight: 4,
    },
    ContentSignal {
        pattern: "StateNotifierProvider",
        layer: ArchLayer::Service,
        weight: 3,
    },
    ContentSignal {
        pattern: "riverpod",
        layer: ArchLayer::Service,
        weight: 2,
    },
    ContentSignal {
        pattern: "FutureProvider",
        layer: ArchLayer::Service,
        weight: 3,
    },
    ContentSignal {
        pattern: "StreamProvider",
        layer: ArchLayer::Service,
        weight: 3,
    },
    // React/Vue — custom hooks / composables as services
    ContentSignal {
        pattern: "export function use",
        layer: ArchLayer::Service,
        weight: 3,
    },
    ContentSignal {
        pattern: "export const use",
        layer: ArchLayer::Service,
        weight: 3,
    },
    // Go
    ContentSignal {
        pattern: "type Service struct",
        layer: ArchLayer::Service,
        weight: 3,
    },
    // Java
    ContentSignal {
        pattern: "@Service",
        layer: ArchLayer::Service,
        weight: 4,
    },
    ContentSignal {
        pattern: "@Component",
        layer: ArchLayer::Service,
        weight: 3,
    },
    ContentSignal {
        pattern: "@Transactional",
        layer: ArchLayer::Service,
        weight: 2,
    },
    // ── Repository signals ──────────────────────────────────────────────────
    // Rust
    ContentSignal {
        pattern: "diesel::",
        layer: ArchLayer::Repository,
        weight: 4,
    },
    ContentSignal {
        pattern: "sqlx::",
        layer: ArchLayer::Repository,
        weight: 4,
    },
    ContentSignal {
        pattern: "sea_orm::",
        layer: ArchLayer::Repository,
        weight: 4,
    },
    ContentSignal {
        pattern: "#[derive(Queryable",
        layer: ArchLayer::Repository,
        weight: 4,
    },
    ContentSignal {
        pattern: "#[derive(Insertable",
        layer: ArchLayer::Repository,
        weight: 4,
    },
    // Python
    ContentSignal {
        pattern: "session.query(",
        layer: ArchLayer::Repository,
        weight: 4,
    },
    ContentSignal {
        pattern: "cursor.execute(",
        layer: ArchLayer::Repository,
        weight: 4,
    },
    ContentSignal {
        pattern: "objects.filter(",
        layer: ArchLayer::Repository,
        weight: 3,
    },
    ContentSignal {
        pattern: "objects.get(",
        layer: ArchLayer::Repository,
        weight: 3,
    },
    ContentSignal {
        pattern: "objects.create(",
        layer: ArchLayer::Repository,
        weight: 3,
    },
    ContentSignal {
        pattern: "objects.all(",
        layer: ArchLayer::Repository,
        weight: 3,
    },
    ContentSignal {
        pattern: "tortoise.models",
        layer: ArchLayer::Repository,
        weight: 3,
    },
    // JS/TS
    ContentSignal {
        pattern: "prisma.",
        layer: ArchLayer::Repository,
        weight: 4,
    },
    ContentSignal {
        pattern: "sequelize.",
        layer: ArchLayer::Repository,
        weight: 4,
    },
    ContentSignal {
        pattern: "mongoose.",
        layer: ArchLayer::Repository,
        weight: 4,
    },
    ContentSignal {
        pattern: "mongoose.model(",
        layer: ArchLayer::Repository,
        weight: 4,
    },
    ContentSignal {
        pattern: "knex(",
        layer: ArchLayer::Repository,
        weight: 3,
    },
    ContentSignal {
        pattern: "typeorm",
        layer: ArchLayer::Repository,
        weight: 3,
    },
    ContentSignal {
        pattern: "drizzle(",
        layer: ArchLayer::Repository,
        weight: 4,
    },
    ContentSignal {
        pattern: "getRepository(",
        layer: ArchLayer::Repository,
        weight: 3,
    },
    ContentSignal {
        pattern: "@Entity(",
        layer: ArchLayer::Repository,
        weight: 3,
    },
    // Go
    ContentSignal {
        pattern: "sql.DB",
        layer: ArchLayer::Repository,
        weight: 3,
    },
    ContentSignal {
        pattern: "gorm.",
        layer: ArchLayer::Repository,
        weight: 4,
    },
    ContentSignal {
        pattern: "gorm.Model",
        layer: ArchLayer::Repository,
        weight: 4,
    },
    ContentSignal {
        pattern: "sqlx.DB",
        layer: ArchLayer::Repository,
        weight: 3,
    },
    // Dart/Flutter
    ContentSignal {
        pattern: "extends DatabaseAccessor",
        layer: ArchLayer::Repository,
        weight: 4,
    },
    ContentSignal {
        pattern: "@DriftDatabase(",
        layer: ArchLayer::Repository,
        weight: 4,
    },
    ContentSignal {
        pattern: "@dao",
        layer: ArchLayer::Repository,
        weight: 4,
    },
    ContentSignal {
        pattern: "FloorDatabase",
        layer: ArchLayer::Repository,
        weight: 4,
    },
    ContentSignal {
        pattern: "openDatabase(",
        layer: ArchLayer::Repository,
        weight: 3,
    },
    ContentSignal {
        pattern: "Isar.open(",
        layer: ArchLayer::Repository,
        weight: 3,
    },
    // Java
    ContentSignal {
        pattern: "@Repository",
        layer: ArchLayer::Repository,
        weight: 4,
    },
    ContentSignal {
        pattern: "JpaRepository",
        layer: ArchLayer::Repository,
        weight: 4,
    },
    ContentSignal {
        pattern: "CrudRepository",
        layer: ArchLayer::Repository,
        weight: 4,
    },
    ContentSignal {
        pattern: "EntityManager",
        layer: ArchLayer::Repository,
        weight: 3,
    },
    ContentSignal {
        pattern: "@Query(",
        layer: ArchLayer::Repository,
        weight: 3,
    },
    // General SQL
    ContentSignal {
        pattern: "SELECT ",
        layer: ArchLayer::Repository,
        weight: 2,
    },
    ContentSignal {
        pattern: "INSERT INTO",
        layer: ArchLayer::Repository,
        weight: 2,
    },
    ContentSignal {
        pattern: "CREATE TABLE",
        layer: ArchLayer::Repository,
        weight: 3,
    },
    // ── Model signals ───────────────────────────────────────────────────────
    // Rust
    ContentSignal {
        pattern: "#[derive(Serialize",
        layer: ArchLayer::Model,
        weight: 2,
    },
    ContentSignal {
        pattern: "#[derive(Deserialize",
        layer: ArchLayer::Model,
        weight: 2,
    },
    ContentSignal {
        pattern: "pub struct ",
        layer: ArchLayer::Model,
        weight: 1,
    },
    ContentSignal {
        pattern: "pub enum ",
        layer: ArchLayer::Model,
        weight: 1,
    },
    // Python
    ContentSignal {
        pattern: "class Meta:",
        layer: ArchLayer::Model,
        weight: 3,
    },
    ContentSignal {
        pattern: "(BaseModel):",
        layer: ArchLayer::Model,
        weight: 3,
    },
    ContentSignal {
        pattern: "(models.Model):",
        layer: ArchLayer::Model,
        weight: 4,
    },
    ContentSignal {
        pattern: "@dataclass",
        layer: ArchLayer::Model,
        weight: 3,
    },
    ContentSignal {
        pattern: "@attr.s",
        layer: ArchLayer::Model,
        weight: 3,
    },
    ContentSignal {
        pattern: "Field(",
        layer: ArchLayer::Model,
        weight: 1,
    },
    // JS/TS
    ContentSignal {
        pattern: "z.object(",
        layer: ArchLayer::Model,
        weight: 3,
    },
    ContentSignal {
        pattern: "z.string(",
        layer: ArchLayer::Model,
        weight: 2,
    },
    ContentSignal {
        pattern: "export interface ",
        layer: ArchLayer::Model,
        weight: 2,
    },
    ContentSignal {
        pattern: "export type ",
        layer: ArchLayer::Model,
        weight: 2,
    },
    ContentSignal {
        pattern: "new Schema(",
        layer: ArchLayer::Model,
        weight: 3,
    },
    ContentSignal {
        pattern: "@Schema(",
        layer: ArchLayer::Model,
        weight: 3,
    },
    ContentSignal {
        pattern: "@Prop(",
        layer: ArchLayer::Model,
        weight: 2,
    },
    // Go
    ContentSignal {
        pattern: "`json:\"",
        layer: ArchLayer::Model,
        weight: 2,
    },
    ContentSignal {
        pattern: "`xml:\"",
        layer: ArchLayer::Model,
        weight: 2,
    },
    ContentSignal {
        pattern: "`db:\"",
        layer: ArchLayer::Model,
        weight: 2,
    },
    // Dart/Flutter
    ContentSignal {
        pattern: "@JsonSerializable(",
        layer: ArchLayer::Model,
        weight: 4,
    },
    ContentSignal {
        pattern: "@freezed",
        layer: ArchLayer::Model,
        weight: 4,
    },
    ContentSignal {
        pattern: "factory ",
        layer: ArchLayer::Model,
        weight: 1,
    },
    ContentSignal {
        pattern: "fromJson(",
        layer: ArchLayer::Model,
        weight: 2,
    },
    ContentSignal {
        pattern: "toJson(",
        layer: ArchLayer::Model,
        weight: 2,
    },
    // Java
    ContentSignal {
        pattern: "@Entity",
        layer: ArchLayer::Model,
        weight: 4,
    },
    ContentSignal {
        pattern: "@Data",
        layer: ArchLayer::Model,
        weight: 3,
    },
    ContentSignal {
        pattern: "@Table(",
        layer: ArchLayer::Model,
        weight: 3,
    },
    ContentSignal {
        pattern: "@Column(",
        layer: ArchLayer::Model,
        weight: 2,
    },
    // ── Utility signals ─────────────────────────────────────────────────────
    ContentSignal {
        pattern: "pub fn ",
        layer: ArchLayer::Utility,
        weight: 1,
    },
    ContentSignal {
        pattern: "pub(crate) fn ",
        layer: ArchLayer::Utility,
        weight: 1,
    },
    ContentSignal {
        pattern: "pub(super) fn ",
        layer: ArchLayer::Utility,
        weight: 1,
    },
    ContentSignal {
        pattern: "function ",
        layer: ArchLayer::Utility,
        weight: 1,
    },
    ContentSignal {
        pattern: "def ",
        layer: ArchLayer::Utility,
        weight: 1,
    },
    ContentSignal {
        pattern: "func ",
        layer: ArchLayer::Utility,
        weight: 1,
    },
    // ── Config signals ──────────────────────────────────────────────────────
    ContentSignal {
        pattern: "dotenv",
        layer: ArchLayer::Config,
        weight: 3,
    },
    ContentSignal {
        pattern: "env::",
        layer: ArchLayer::Config,
        weight: 2,
    },
    ContentSignal {
        pattern: "process.env.",
        layer: ArchLayer::Config,
        weight: 2,
    },
    ContentSignal {
        pattern: "os.environ",
        layer: ArchLayer::Config,
        weight: 2,
    },
    ContentSignal {
        pattern: "os.Getenv(",
        layer: ArchLayer::Config,
        weight: 2,
    },
    ContentSignal {
        pattern: "Platform.environment",
        layer: ArchLayer::Config,
        weight: 2,
    },
    ContentSignal {
        pattern: "defineConfig(",
        layer: ArchLayer::Config,
        weight: 3,
    },
    ContentSignal {
        pattern: "vite.config",
        layer: ArchLayer::Config,
        weight: 3,
    },
    ContentSignal {
        pattern: "next.config",
        layer: ArchLayer::Config,
        weight: 3,
    },
    ContentSignal {
        pattern: "nuxt.config",
        layer: ArchLayer::Config,
        weight: 3,
    },
    ContentSignal {
        pattern: "astro.config",
        layer: ArchLayer::Config,
        weight: 3,
    },
    ContentSignal {
        pattern: "angular.json",
        layer: ArchLayer::Config,
        weight: 3,
    },
];

/// Directory name bonus — universal conventions only (not project-specific).
pub(super) const DIR_BONUS: &[(&[&str], ArchLayer, i32)] = &[
    (
        &[
            "controllers",
            "controller",
            "routes",
            "routers",
            "handlers",
            "views",
            "endpoints",
            "api",
            "commands",
            "tools",
            "pages",
        ],
        ArchLayer::Controller,
        2,
    ),
    (
        &[
            "services",
            "service",
            "usecases",
            "use_cases",
            "domain",
            "business",
            "logic",
        ],
        ArchLayer::Service,
        2,
    ),
    (
        &[
            "repositories",
            "repository",
            "repos",
            "dao",
            "dal",
            "data",
            "db",
            "database",
            "persistence",
            "migration",
            "migrations",
        ],
        ArchLayer::Repository,
        2,
    ),
    (
        &[
            "models", "model", "entities", "entity", "schemas", "schema", "types", "dto", "dtos",
            "proto",
        ],
        ArchLayer::Model,
        2,
    ),
    (
        &[
            "utils",
            "util",
            "helpers",
            "helper",
            "common",
            "shared",
            "lib",
            "middleware",
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
