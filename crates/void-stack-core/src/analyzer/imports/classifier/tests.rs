use super::super::super::graph::*;
use super::*;

// ── Rust ───────────────────────────────────────────────────────────────

#[test]
fn test_rust_controller_tauri() {
    let content =
        "#[tauri::command]\npub async fn list() -> Result<Vec<Service>, String> { todo!() }";
    assert_eq!(
        classify_layer("src/commands/services.rs", content),
        ArchLayer::Controller
    );
}

#[test]
fn test_rust_controller_actix() {
    let content = "#[get(\"/api/users\")]\nasync fn get_users() -> HttpResponse { HttpResponse::Ok().finish() }";
    assert_eq!(
        classify_layer("src/handlers/users.rs", content),
        ArchLayer::Controller
    );
}

#[test]
fn test_rust_service_impl() {
    let content = "#[async_trait]\nimpl Runner for LocalRunner {\n    async fn start(&self) -> Result<()> { todo!() }\n}";
    assert_eq!(
        classify_layer("src/runner/local.rs", content),
        ArchLayer::Service
    );
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
    assert_eq!(
        classify_layer("src/db/users.rs", content),
        ArchLayer::Repository
    );
}

// ── Python ─────────────────────────────────────────────────────────────

#[test]
fn test_python_fastapi() {
    let content = "from fastapi import APIRouter\nrouter = APIRouter()\n@router.get(\"/users\")\nasync def get_users(): pass";
    assert_eq!(
        classify_layer("app/routes/users.py", content),
        ArchLayer::Controller
    );
}

#[test]
fn test_python_django_model() {
    let content = "from django.db import models\nclass User(models.Model):\n    name = models.CharField(max_length=100)\n    class Meta:\n        db_table = 'users'";
    assert_eq!(classify_layer("app/models.py", content), ArchLayer::Model);
}

#[test]
fn test_python_sqlalchemy_repo() {
    let content =
        "def get_user(db):\n    return db.session.query(User).filter(User.id == 1).first()";
    assert_eq!(
        classify_layer("app/repos/user_repo.py", content),
        ArchLayer::Repository
    );
}

// ── JS/TS ──────────────────────────────────────────────────────────────

#[test]
fn test_nestjs_controller() {
    let content = "@Controller('users')\nexport class UsersController {\n    @Get()\n    findAll() {}\n    @Post()\n    create() {}\n}";
    assert_eq!(
        classify_layer("src/users/users.controller.ts", content),
        ArchLayer::Controller
    );
}

#[test]
fn test_nestjs_service() {
    let content = "@Injectable()\nexport class UsersService {\n    constructor(private repo: UsersRepository) {}\n    findAll() { return this.repo.find(); }\n}";
    assert_eq!(
        classify_layer("src/users/users.service.ts", content),
        ArchLayer::Service
    );
}

#[test]
fn test_nextjs_api_route() {
    let content = "import { NextRequest, NextResponse } from 'next/server';\nexport async function GET(request: NextRequest) {\n    return NextResponse.json({ users: [] });\n}";
    assert_eq!(
        classify_layer("app/api/users/route.ts", content),
        ArchLayer::Controller
    );
}

#[test]
fn test_react_component() {
    let content = "import { useState, useEffect } from 'react';\nexport default function UserList() {\n    const [users, setUsers] = useState([]);\n    useEffect(() => { fetchUsers(); }, []);\n    return (<div>{users.map(u => <p>{u.name}</p>)}</div>);\n}";
    assert_eq!(
        classify_layer("src/components/UserList.tsx", content),
        ArchLayer::Controller
    );
}

#[test]
fn test_vue_component() {
    let content = "<template>\n    <div>{{ message }}</div>\n</template>\n<script setup>\nimport { ref } from 'vue'\nconst message = ref('Hello')\n</script>";
    assert_eq!(
        classify_layer("src/components/Hello.vue", content),
        ArchLayer::Controller
    );
}

#[test]
fn test_angular_component() {
    let content = "@Component({\n    selector: 'app-user-list',\n    templateUrl: './user-list.component.html'\n})\nexport class UserListComponent implements OnInit { }";
    assert_eq!(
        classify_layer("src/app/user-list.component.ts", content),
        ArchLayer::Controller
    );
}

#[test]
fn test_prisma_repository() {
    let content = "const prisma = new PrismaClient();\nexport async function getUsers() {\n    return prisma.user.findMany();\n}";
    assert_eq!(
        classify_layer("src/repos/userRepo.ts", content),
        ArchLayer::Repository
    );
}

#[test]
fn test_zod_model() {
    let content = "import { z } from 'zod';\nexport const UserSchema = z.object({\n    id: z.string(),\n    name: z.string(),\n});\nexport type User = z.infer<typeof UserSchema>;";
    assert_eq!(
        classify_layer("src/schemas/user.ts", content),
        ArchLayer::Model
    );
}

// ── Go ──────────────────────────────────────────────────────────────────

#[test]
fn test_go_handler() {
    let content =
        "func GetUsers(c *gin.Context) {\n    users := service.GetAll()\n    c.JSON(200, users)\n}";
    assert_eq!(
        classify_layer("handlers/users.go", content),
        ArchLayer::Controller
    );
}

#[test]
fn test_go_gorm_repo() {
    let content = "func (r *UserRepo) FindAll(db *gorm.DB) []User {\n    var users []User\n    db.Find(&users)\n    return users\n}";
    assert_eq!(
        classify_layer("repos/user_repo.go", content),
        ArchLayer::Repository
    );
}

// ── Dart/Flutter ────────────────────────────────────────────────────────

#[test]
fn test_flutter_widget() {
    let content = "class HomePage extends StatefulWidget {\n    @override\n    State<HomePage> createState() => _HomePageState();\n}";
    assert_eq!(
        classify_layer("lib/pages/home_page.dart", content),
        ArchLayer::Controller
    );
}

#[test]
fn test_flutter_bloc_service() {
    let content = "class UserBloc extends Bloc<UserEvent, UserState> {\n    final UserRepository repo;\n    UserBloc(this.repo) : super(UserInitial());\n}";
    assert_eq!(
        classify_layer("lib/blocs/user_bloc.dart", content),
        ArchLayer::Service
    );
}

#[test]
fn test_dart_json_model() {
    let content = "@JsonSerializable()\nclass User {\n    final String id;\n    factory User.fromJson(Map<String, dynamic> json) => _$UserFromJson(json);\n    Map<String, dynamic> toJson() => _$UserToJson(this);\n}";
    assert_eq!(
        classify_layer("lib/models/user.dart", content),
        ArchLayer::Model
    );
}

// ── Astro & MCP ────────────────────────────────────────────────────────

#[test]
fn test_astro_page() {
    let content = "const data = Astro.props;\nconst resp = Astro.request;";
    assert_eq!(
        classify_layer("src/pages/index.astro", content),
        ArchLayer::Controller
    );
}

#[test]
fn test_mcp_tool() {
    let content = "pub async fn handle(params: Value) -> CallToolResult {\n    CallToolResult::success(vec![])\n}";
    assert_eq!(
        classify_layer("src/tools/analysis.rs", content),
        ArchLayer::Controller
    );
}

// ── Deterministic overrides ────────────────────────────────────────────

#[test]
fn test_test_file() {
    assert_eq!(
        classify_layer("tests/test_users.py", "def test_create(): pass"),
        ArchLayer::Test
    );
    assert_eq!(
        classify_layer("src/__tests__/user.spec.ts", "describe('User', () => {})"),
        ArchLayer::Test
    );
}

#[test]
fn test_config_file() {
    assert_eq!(
        classify_layer("src/config.py", "DEBUG = True"),
        ArchLayer::Config
    );
    assert_eq!(
        classify_layer(".env.production", "API_KEY=xxx"),
        ArchLayer::Config
    );
}

#[test]
fn test_entry_points() {
    assert_eq!(
        classify_layer("src/main.rs", "fn main() {}"),
        ArchLayer::Utility
    );
    assert_eq!(
        classify_layer("src/lib.rs", "pub mod model;"),
        ArchLayer::Utility
    );
    assert_eq!(
        classify_layer("build.rs", "fn main() {}"),
        ArchLayer::Utility
    );
}

// ── Scoring behavior ───────────────────────────────────────────────────

#[test]
fn test_content_wins_over_path() {
    let content =
        "#[get(\"/health\")]\nasync fn health() -> HttpResponse { HttpResponse::Ok().finish() }";
    assert_eq!(
        classify_layer("src/misc/health.rs", content),
        ArchLayer::Controller
    );
}

#[test]
fn test_unknown_minimal_content() {
    assert_eq!(classify_layer("src/foo.rs", "// empty"), ArchLayer::Unknown);
}

#[test]
fn test_pub_super_fn_classifies_as_utility() {
    let content = "pub(super) fn flutter_web_dockerfile(_path: &Path) -> String { todo!() }";
    assert_eq!(
        classify_layer("src/docker/generate_dockerfile/flutter.rs", content),
        ArchLayer::Utility
    );
}

#[test]
fn test_pub_crate_fn_classifies_as_utility() {
    let content = "pub(crate) fn detect_url(line: &str) -> Option<String> { None }";
    assert_eq!(
        classify_layer("src/manager/url.rs", content),
        ArchLayer::Utility
    );
}

#[test]
fn test_scores_accumulate() {
    let content = "#[get(\"/a\")]\n#[post(\"/b\")]\n#[put(\"/c\")]";
    let scores = compute_layer_scores("src/api.rs", content);
    let ctrl = scores
        .iter()
        .find(|(l, _)| *l == ArchLayer::Controller)
        .map(|(_, s)| *s)
        .unwrap_or(0);
    assert!(
        ctrl >= 9,
        "3 controller signals should give >=9 pts, got {ctrl}"
    );
}

#[test]
fn test_dir_bonus_applied() {
    let scores = compute_layer_scores("controllers/user.rs", "");
    let ctrl = scores
        .iter()
        .find(|(l, _)| *l == ArchLayer::Controller)
        .map(|(_, s)| *s)
        .unwrap_or(0);
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
        ModuleNode {
            path: "types.rs".into(),
            language: Language::Rust,
            layer: ArchLayer::Unknown,
            loc: 50,
            class_count: 0,
            function_count: 2,
            is_hub: false,
            has_framework_macros: false,
        },
        ModuleNode {
            path: "a.rs".into(),
            language: Language::Rust,
            layer: ArchLayer::Service,
            loc: 100,
            class_count: 0,
            function_count: 5,
            is_hub: false,
            has_framework_macros: false,
        },
        ModuleNode {
            path: "b.rs".into(),
            language: Language::Rust,
            layer: ArchLayer::Service,
            loc: 80,
            class_count: 0,
            function_count: 3,
            is_hub: false,
            has_framework_macros: false,
        },
        ModuleNode {
            path: "c.rs".into(),
            language: Language::Rust,
            layer: ArchLayer::Controller,
            loc: 60,
            class_count: 0,
            function_count: 4,
            is_hub: false,
            has_framework_macros: false,
        },
        ModuleNode {
            path: "d.rs".into(),
            language: Language::Rust,
            layer: ArchLayer::Controller,
            loc: 40,
            class_count: 0,
            function_count: 2,
            is_hub: false,
            has_framework_macros: false,
        },
        ModuleNode {
            path: "e.rs".into(),
            language: Language::Rust,
            layer: ArchLayer::Service,
            loc: 70,
            class_count: 0,
            function_count: 3,
            is_hub: false,
            has_framework_macros: false,
        },
        ModuleNode {
            path: "f.rs".into(),
            language: Language::Rust,
            layer: ArchLayer::Controller,
            loc: 50,
            class_count: 0,
            function_count: 2,
            is_hub: false,
            has_framework_macros: false,
        },
        ModuleNode {
            path: "g.rs".into(),
            language: Language::Rust,
            layer: ArchLayer::Utility,
            loc: 30,
            class_count: 0,
            function_count: 1,
            is_hub: false,
            has_framework_macros: false,
        },
        ModuleNode {
            path: "h.rs".into(),
            language: Language::Rust,
            layer: ArchLayer::Service,
            loc: 90,
            class_count: 0,
            function_count: 4,
            is_hub: false,
            has_framework_macros: false,
        },
    ];
    let edges = vec![
        ImportEdge {
            from: "a.rs".into(),
            to: "types.rs".into(),
            is_external: false,
        },
        ImportEdge {
            from: "b.rs".into(),
            to: "types.rs".into(),
            is_external: false,
        },
        ImportEdge {
            from: "c.rs".into(),
            to: "types.rs".into(),
            is_external: false,
        },
        ImportEdge {
            from: "d.rs".into(),
            to: "types.rs".into(),
            is_external: false,
        },
        ImportEdge {
            from: "e.rs".into(),
            to: "types.rs".into(),
            is_external: false,
        },
        ImportEdge {
            from: "f.rs".into(),
            to: "types.rs".into(),
            is_external: false,
        },
        ImportEdge {
            from: "g.rs".into(),
            to: "types.rs".into(),
            is_external: false,
        },
        ImportEdge {
            from: "h.rs".into(),
            to: "types.rs".into(),
            is_external: false,
        },
        ImportEdge {
            from: "a.rs".into(),
            to: "b.rs".into(),
            is_external: false,
        },
        ImportEdge {
            from: "c.rs".into(),
            to: "d.rs".into(),
            is_external: false,
        },
    ];
    refine_unknown_by_graph(&mut modules, &edges);
    assert_eq!(modules[0].layer, ArchLayer::Model);
}

#[test]
fn test_fanout_refines_to_controller() {
    let mut modules = vec![
        ModuleNode {
            path: "handler.rs".into(),
            language: Language::Rust,
            layer: ArchLayer::Unknown,
            loc: 200,
            class_count: 0,
            function_count: 10,
            is_hub: false,
            has_framework_macros: false,
        },
        ModuleNode {
            path: "svc_a.rs".into(),
            language: Language::Rust,
            layer: ArchLayer::Service,
            loc: 50,
            class_count: 0,
            function_count: 3,
            is_hub: false,
            has_framework_macros: false,
        },
        ModuleNode {
            path: "svc_b.rs".into(),
            language: Language::Rust,
            layer: ArchLayer::Service,
            loc: 50,
            class_count: 0,
            function_count: 3,
            is_hub: false,
            has_framework_macros: false,
        },
        ModuleNode {
            path: "model.rs".into(),
            language: Language::Rust,
            layer: ArchLayer::Model,
            loc: 30,
            class_count: 0,
            function_count: 1,
            is_hub: false,
            has_framework_macros: false,
        },
    ];
    let edges = vec![
        ImportEdge {
            from: "handler.rs".into(),
            to: "svc_a.rs".into(),
            is_external: false,
        },
        ImportEdge {
            from: "handler.rs".into(),
            to: "svc_b.rs".into(),
            is_external: false,
        },
        ImportEdge {
            from: "handler.rs".into(),
            to: "model.rs".into(),
            is_external: false,
        },
        ImportEdge {
            from: "svc_a.rs".into(),
            to: "model.rs".into(),
            is_external: false,
        },
    ];
    refine_unknown_by_graph(&mut modules, &edges);
    assert_eq!(modules[0].layer, ArchLayer::Controller);
}

#[test]
fn test_fanout_gt_fanin_refines_to_service() {
    let mut modules = vec![
        ModuleNode {
            path: "orch.rs".into(),
            language: Language::Rust,
            layer: ArchLayer::Unknown,
            loc: 100,
            class_count: 0,
            function_count: 5,
            is_hub: false,
            has_framework_macros: false,
        },
        ModuleNode {
            path: "dep_a.rs".into(),
            language: Language::Rust,
            layer: ArchLayer::Model,
            loc: 30,
            class_count: 0,
            function_count: 1,
            is_hub: false,
            has_framework_macros: false,
        },
        ModuleNode {
            path: "dep_b.rs".into(),
            language: Language::Rust,
            layer: ArchLayer::Model,
            loc: 30,
            class_count: 0,
            function_count: 1,
            is_hub: false,
            has_framework_macros: false,
        },
        ModuleNode {
            path: "caller.rs".into(),
            language: Language::Rust,
            layer: ArchLayer::Controller,
            loc: 50,
            class_count: 0,
            function_count: 3,
            is_hub: false,
            has_framework_macros: false,
        },
    ];
    let edges = vec![
        ImportEdge {
            from: "orch.rs".into(),
            to: "dep_a.rs".into(),
            is_external: false,
        },
        ImportEdge {
            from: "orch.rs".into(),
            to: "dep_b.rs".into(),
            is_external: false,
        },
        ImportEdge {
            from: "caller.rs".into(),
            to: "orch.rs".into(),
            is_external: false,
        },
    ];
    refine_unknown_by_graph(&mut modules, &edges);
    assert_eq!(modules[0].layer, ArchLayer::Service);
}

#[test]
fn test_no_edges_stays_unknown() {
    let mut modules = vec![ModuleNode {
        path: "isolated.rs".into(),
        language: Language::Rust,
        layer: ArchLayer::Unknown,
        loc: 10,
        class_count: 0,
        function_count: 1,
        is_hub: false,
        has_framework_macros: false,
    }];
    refine_unknown_by_graph(&mut modules, &[]);
    assert_eq!(modules[0].layer, ArchLayer::Unknown);
}

#[test]
fn test_already_classified_not_changed() {
    let mut modules = vec![
        ModuleNode {
            path: "service.rs".into(),
            language: Language::Rust,
            layer: ArchLayer::Service,
            loc: 100,
            class_count: 0,
            function_count: 5,
            is_hub: false,
            has_framework_macros: false,
        },
        ModuleNode {
            path: "dep.rs".into(),
            language: Language::Rust,
            layer: ArchLayer::Model,
            loc: 30,
            class_count: 0,
            function_count: 1,
            is_hub: false,
            has_framework_macros: false,
        },
    ];
    let edges = vec![ImportEdge {
        from: "dep.rs".into(),
        to: "service.rs".into(),
        is_external: false,
    }];
    refine_unknown_by_graph(&mut modules, &edges);
    assert_eq!(modules[0].layer, ArchLayer::Service);
}

// ── Express/Mongoose edge cases ────────────────────────────────────────

#[test]
fn test_express_router() {
    let content = "const router = require('express').Router();\nrouter.get('/users', getUsers);\nrouter.post('/users', createUser);";
    assert_eq!(
        classify_layer("routes/users.js", content),
        ArchLayer::Controller
    );
}

#[test]
fn test_mongoose_model() {
    let content = "const mongoose = require('mongoose');\nconst userSchema = new Schema({\n    name: String,\n});\nmodule.exports = mongoose.model('User', userSchema);";
    assert_eq!(
        classify_layer("models/user.js", content),
        ArchLayer::Repository
    );
}
