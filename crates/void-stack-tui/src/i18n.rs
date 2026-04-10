/// Lightweight i18n for the TUI. Spanish (default) and English.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Es,
    En,
}

impl Lang {
    pub fn toggle(self) -> Self {
        match self {
            Lang::Es => Lang::En,
            Lang::En => Lang::Es,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Lang::Es => "ES",
            Lang::En => "EN",
        }
    }
}

/// Get a translated string by key.
pub fn t(lang: Lang, key: &str) -> &str {
    match lang {
        Lang::Es => es(key),
        Lang::En => en(key),
    }
}

fn es(key: &str) -> &str {
    match key {
        // Header
        "projects" => "proyectos",
        "services" => "servicios",
        "session" => "sesion",
        "ready" => "Listo",

        // Tab names
        "tab.services" => "Servicios",
        "tab.analysis" => "Analisis",
        "tab.security" => "Seguridad",
        "tab.debt" => "Deuda",
        "tab.space" => "Espacio",

        // Projects panel
        "panel.projects" => "Proyectos",

        // Services
        "panel.services" => "Servicios",
        "panel.logs" => "Logs",
        "panel.deps" => "Dependencias",
        "started" => "iniciados",
        "failed" => "fallidos",
        "all_stopped" => "todos detenidos",
        "checking_deps" => "Verificando dependencias...",
        "deps_ready" => "deps listas",

        // Analysis
        "analysis.title" => "Analisis de Arquitectura",
        "analysis.overview" => "Resumen de Arquitectura",
        "analysis.pattern" => "Patron",
        "analysis.confidence" => "confianza",
        "analysis.modules" => "Modulos",
        "analysis.loc" => "LOC",
        "analysis.deps" => "Deps",
        "analysis.lang" => "Lenguaje",
        "analysis.layers" => "Capas",
        "analysis.coverage" => "Cobertura",
        "analysis.antipatterns" => "Anti-patrones",
        "analysis.no_antipatterns" => "Sin anti-patrones detectados",
        "analysis.complexity" => "Funciones Complejas",
        "analysis.no_complexity" => "Sin datos de complejidad",
        "analysis.run_hint" => "Presiona R para analizar el proyecto actual",
        "analysis.running" => "Analizando...",
        "analysis.complete" => "Analisis completado",
        "analysis.no_code" => "No se encontraron archivos fuente analizables",

        // Security
        "security.title" => "Auditoria de Seguridad",
        "security.risk" => "Resumen de Riesgo",
        "security.score" => "Risk Score",
        "security.findings" => "Hallazgos",
        "security.total" => "Total hallazgos",
        "security.critical" => "criticos",
        "security.high" => "altos",
        "security.medium" => "medios",
        "security.low" => "bajos",
        "security.no_findings" => "Sin problemas de seguridad encontrados!",
        "security.run_hint" => "Presiona R para ejecutar auditoria de seguridad",
        "security.running" => "Ejecutando auditoria...",
        "security.complete" => "Auditoria completada",

        // Debt
        "debt.title" => "Deuda Tecnica Explicita",
        "debt.markers" => "marcadores",
        "debt.no_markers" => "Sin marcadores de deuda encontrados!",
        "debt.run_hint" => "Presiona R para escanear marcadores TODO/FIXME/HACK",
        "debt.running" => "Escaneando marcadores de deuda...",
        "debt.found" => "marcadores de deuda encontrados",

        // Space
        "space.title" => "Espacio en Disco",
        "space.entries" => "entradas",
        "space.total" => "total",
        "space.no_entries" => "Sin directorios limpiables encontrados",
        "space.run_hint" => "Presiona R para escanear uso de disco",
        "space.running" => "Escaneando espacio en disco...",

        // Claudeignore
        "claudeignore.generated" => ".claudeignore generado en",
        "claudeignore.patterns" => "patrones",
        "claudeignore.files_ignored" => "archivos ignorados",
        "claudeignore.error" => "Error generando .claudeignore",

        // Semantic search
        "search.title" => "Busqueda Semantica",
        "search.hint" => "Presiona / para buscar, I para indexar",
        "search.type_query" => "Escribe tu consulta y presiona Enter...",
        "search.indexing" => "Indexando proyecto...",

        // Voidignore
        "voidignore.generating" => "Generando .voidignore...",
        "voidignore.patterns" => "patrones",

        // AI suggestions
        "suggest.running" => "Generando sugerencias (requiere Ollama)...",
        "suggest.generated" => "sugerencias generadas",
        "suggest.no_ollama" => "Ollama no disponible — mostrando contexto",

        // Log filter
        "logs.filter_on" => "Filtrado de logs activado",
        "logs.filter_off" => "Filtrado de logs desactivado",

        // Stats
        "tab.stats" => "Stats",
        "stats.title" => "Token Savings",
        "stats.running" => "Cargando estadisticas...",
        "stats.run_hint" => "Presiona R para cargar estadisticas de ahorro",
        "stats.ops" => "ops",
        "stats.avg" => "ahorro prom.",
        "stats.lines_saved" => "lineas ahorradas",
        "stats.savings" => "Ahorro",
        "stats.by_project" => "Por proyecto",
        "stats.by_operation" => "Por operacion",
        "stats.ops_found" => "operaciones encontradas",

        // Status messages
        "status.all_refreshed" => "Todos los proyectos actualizados",
        "status.refreshed" => "Estado actualizado",
        "space.found" => "entradas de espacio encontradas",

        // Footer
        "footer.tabs" => "Tabs",
        "footer.panel" => "Panel",
        "footer.select" => "Seleccionar",
        "footer.start_all" => "Iniciar Todo",
        "footer.stop_all" => "Detener Todo",
        "footer.deps" => "Deps",
        "footer.quit" => "Salir",
        "footer.help" => "Ayuda",
        "footer.start" => "Iniciar",
        "footer.stop" => "Detener",
        "footer.logs" => "Logs",
        "footer.scroll" => "Scroll",
        "footer.run" => "Ejecutar",
        "footer.lang" => "Idioma",

        // Help
        "help.title" => "Ayuda",
        "help.shortcuts" => "Atajos de Teclado",
        "help.navigation" => "Navegacion:",
        "help.switch_tab" => "Cambiar tab",
        "help.switch_panel" => "Cambiar panel",
        "help.nav_down" => "Navegar abajo",
        "help.nav_up" => "Navegar arriba",
        "help.service_actions" => "Acciones de Servicios:",
        "help.start_all_svcs" => "Iniciar todos los servicios",
        "help.start_selected" => "Iniciar servicio seleccionado",
        "help.stop_selected" => "Detener servicio seleccionado",
        "help.stop_all_svcs" => "Detener todos los servicios",
        "help.analysis_section" => "Analisis:",
        "help.check_deps" => "Verificar dependencias",
        "help.run_action" => "Ejecutar analisis/auditoria/escaneo",
        "help.toggle_lang" => "Cambiar idioma (ES/EN)",
        "help.other" => "Otros:",
        "help.go_logs" => "Ir al panel de Logs",
        "help.go_back" => "Volver al panel de Servicios",
        "help.refresh" => "Refrescar estado",
        "help.search" => "Busqueda semantica",
        "help.index" => "Indexar proyecto",
        "help.claudeignore" => "Generar .claudeignore",
        "help.voidignore" => "Generar .voidignore",
        "help.suggest" => "Sugerencias IA",
        "help.filter_logs" => "Filtrar logs (compacto)",
        "help.quit_hint" => "Salir (detiene servicios)",
        "help.toggle_help" => "Mostrar/ocultar ayuda",

        // Table headers
        "th.severity" => "Sev",
        "th.kind" => "Tipo",
        "th.description" => "Descripcion",
        "th.file" => "Archivo",
        "th.line" => "Linea",
        "th.name" => "Nombre",
        "th.target" => "Target",
        "th.status" => "Estado",
        "th.pid" => "PID",
        "th.uptime" => "Uptime",
        "th.url" => "URL",
        "th.cc" => "CC",
        "th.function" => "Funcion",
        "th.cov" => "Cob",
        "th.text" => "Texto",
        "th.category" => "Categoria",
        "th.size" => "Tamano",
        "th.path" => "Ruta",

        _ => key,
    }
}

fn en(key: &str) -> &str {
    match key {
        // Header
        "projects" => "projects",
        "services" => "services",
        "session" => "session",
        "ready" => "Ready",

        // Tab names
        "tab.services" => "Services",
        "tab.analysis" => "Analysis",
        "tab.security" => "Security",
        "tab.debt" => "Debt",
        "tab.space" => "Space",

        // Projects panel
        "panel.projects" => "Projects",

        // Services
        "panel.services" => "Services",
        "panel.logs" => "Logs",
        "panel.deps" => "Dependencies",
        "started" => "started",
        "failed" => "failed",
        "all_stopped" => "all stopped",
        "checking_deps" => "Checking dependencies...",
        "deps_ready" => "deps ready",

        // Analysis
        "analysis.title" => "Architecture Analysis",
        "analysis.overview" => "Architecture Overview",
        "analysis.pattern" => "Pattern",
        "analysis.confidence" => "confidence",
        "analysis.modules" => "Modules",
        "analysis.loc" => "LOC",
        "analysis.deps" => "Deps",
        "analysis.lang" => "Language",
        "analysis.layers" => "Layers",
        "analysis.coverage" => "Coverage",
        "analysis.antipatterns" => "Anti-patterns",
        "analysis.no_antipatterns" => "No anti-patterns detected",
        "analysis.complexity" => "Top Complex Functions",
        "analysis.no_complexity" => "No complexity data",
        "analysis.run_hint" => "Press R to run analysis on the current project",
        "analysis.running" => "Analyzing...",
        "analysis.complete" => "Analysis complete",
        "analysis.no_code" => "No analyzable source files found",

        // Security
        "security.title" => "Security Audit",
        "security.risk" => "Risk Overview",
        "security.score" => "Risk Score",
        "security.findings" => "Findings",
        "security.total" => "Total findings",
        "security.critical" => "critical",
        "security.high" => "high",
        "security.medium" => "medium",
        "security.low" => "low",
        "security.no_findings" => "No security issues found!",
        "security.run_hint" => "Press R to run security audit",
        "security.running" => "Running audit...",
        "security.complete" => "Audit complete",

        // Debt
        "debt.title" => "Explicit Technical Debt",
        "debt.markers" => "markers",
        "debt.no_markers" => "No debt markers found!",
        "debt.run_hint" => "Press R to scan for TODO/FIXME/HACK markers",
        "debt.running" => "Scanning for debt markers...",
        "debt.found" => "debt markers found",

        // Space
        "space.title" => "Disk Space",
        "space.entries" => "entries",
        "space.total" => "total",
        "space.no_entries" => "No cleanable directories found",
        "space.run_hint" => "Press R to scan project + global disk usage",
        "space.running" => "Scanning disk space...",
        "space.found" => "space entries found",

        // Claudeignore
        "claudeignore.generated" => ".claudeignore generated at",
        "claudeignore.patterns" => "patterns",
        "claudeignore.files_ignored" => "files ignored",
        "claudeignore.error" => "Error generating .claudeignore",

        // Semantic search
        "search.title" => "Semantic Search",
        "search.hint" => "Press / to search, I to index",
        "search.type_query" => "Type your query and press Enter...",
        "search.indexing" => "Indexing project...",

        // Voidignore
        "voidignore.generating" => "Generating .voidignore...",
        "voidignore.patterns" => "patterns",

        // AI suggestions
        "suggest.running" => "Generating suggestions (requires Ollama)...",
        "suggest.generated" => "suggestions generated",
        "suggest.no_ollama" => "Ollama not available — showing analysis context",

        // Log filter
        "logs.filter_on" => "Log filtering enabled",
        "logs.filter_off" => "Log filtering disabled",

        // Stats
        "tab.stats" => "Stats",
        "stats.title" => "Token Savings",
        "stats.running" => "Loading statistics...",
        "stats.run_hint" => "Press R to load savings statistics",
        "stats.ops" => "ops",
        "stats.avg" => "avg savings",
        "stats.lines_saved" => "lines saved",
        "stats.savings" => "Savings",
        "stats.by_project" => "By project",
        "stats.by_operation" => "By operation",
        "stats.ops_found" => "operations found",

        // Status messages
        "status.all_refreshed" => "All projects refreshed",
        "status.refreshed" => "Status refreshed",

        // Footer
        "footer.tabs" => "Tabs",
        "footer.panel" => "Panel",
        "footer.select" => "Select",
        "footer.start_all" => "Start All",
        "footer.stop_all" => "Stop All",
        "footer.deps" => "Deps",
        "footer.quit" => "Quit",
        "footer.help" => "Help",
        "footer.start" => "Start",
        "footer.stop" => "Stop",
        "footer.logs" => "Logs",
        "footer.scroll" => "Scroll",
        "footer.run" => "Run",
        "footer.lang" => "Language",

        // Help
        "help.title" => "Help",
        "help.shortcuts" => "Keyboard Shortcuts",
        "help.navigation" => "Navigation:",
        "help.switch_tab" => "Switch tab",
        "help.switch_panel" => "Switch panel",
        "help.nav_down" => "Navigate down",
        "help.nav_up" => "Navigate up",
        "help.service_actions" => "Service Actions:",
        "help.start_all_svcs" => "Start all services",
        "help.start_selected" => "Start selected service",
        "help.stop_selected" => "Stop selected service",
        "help.stop_all_svcs" => "Stop all services",
        "help.analysis_section" => "Analysis:",
        "help.check_deps" => "Check dependencies",
        "help.run_action" => "Run analysis / audit / scan",
        "help.toggle_lang" => "Toggle language (ES/EN)",
        "help.other" => "Other:",
        "help.go_logs" => "Switch to Logs panel",
        "help.go_back" => "Back to Services panel",
        "help.refresh" => "Refresh status",
        "help.search" => "Semantic search",
        "help.index" => "Index project",
        "help.claudeignore" => "Generate .claudeignore",
        "help.voidignore" => "Generate .voidignore",
        "help.suggest" => "AI suggestions",
        "help.filter_logs" => "Filter logs (compact)",
        "help.quit_hint" => "Quit (stops running services)",
        "help.toggle_help" => "Toggle this help",

        // Table headers
        "th.severity" => "Sev",
        "th.kind" => "Kind",
        "th.description" => "Description",
        "th.file" => "File",
        "th.line" => "Line",
        "th.name" => "Name",
        "th.target" => "Target",
        "th.status" => "Status",
        "th.pid" => "PID",
        "th.uptime" => "Uptime",
        "th.url" => "URL",
        "th.cc" => "CC",
        "th.function" => "Function",
        "th.cov" => "Cov",
        "th.text" => "Text",
        "th.category" => "Category",
        "th.size" => "Size",
        "th.path" => "Path",

        _ => key,
    }
}
