//! Line-level function-signature heuristics for languages without a
//! dedicated parser in the analyzers (C#, Swift, C++, GDScript).
//!
//! Shared by the complexity analyzer (needs the function name) and the
//! semantic-index chunker (only needs "does a function start here").
//! Each returns the declared name, or `None` when the line is not a
//! signature. They are pragmatic, single-line heuristics: the structural
//! graph (tree-sitter) is the precise source; these keep metrics and
//! chunk boundaries sensible without a full parse.

/// Strip balanced `<…>` generic arguments from an identifier.
fn base_name(token: &str) -> &str {
    token.split('<').next().unwrap_or(token)
}

fn is_identifier(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '~')
        && s.chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '~')
}

/// C#: `[mods] ReturnType Name(` or `[mods] Name(` (constructor). Not a
/// control statement, call, assignment, attribute or a `;`-terminated
/// abstract/interface member.
pub fn csharp_method_name(line: &str) -> Option<String> {
    let t = line.trim();
    if t.ends_with(';') || t.starts_with('[') || t.starts_with("//") {
        return None;
    }
    let open = t.find('(')?;
    let head = &t[..open];
    if head.contains('=') || head.contains('.') {
        return None; // assignment or member call
    }
    let tokens: Vec<&str> = head.split_whitespace().collect();
    let first = *tokens.first()?;
    const NOT_A_SIGNATURE: &[&str] = &[
        "if", "else", "for", "foreach", "while", "do", "switch", "catch", "using", "return", "new",
        "await", "throw", "lock", "case", "when", "nameof", "typeof", "yield", "var", "base",
        "this",
    ];
    if NOT_A_SIGNATURE.contains(&first) {
        return None;
    }
    const MODIFIERS: &[&str] = &[
        "public",
        "private",
        "protected",
        "internal",
        "static",
        "virtual",
        "override",
        "abstract",
        "sealed",
        "async",
        "extern",
        "unsafe",
        "partial",
        "new",
        "readonly",
    ];
    let non_modifiers = tokens.iter().filter(|t| !MODIFIERS.contains(t)).count();
    // A constructor needs a modifier (`public Foo(`); a bare `Foo(` is a call.
    if !(non_modifiers >= 2 || (non_modifiers == 1 && tokens.len() >= 2)) {
        return None;
    }
    let name = base_name(tokens.last()?);
    is_identifier(name).then(|| name.to_string())
}

/// Swift: `func name` / `init(` after any modifiers and `@attributes`
/// (`public static func`, `@MainActor func`, `override init(`).
pub fn swift_function_name(line: &str) -> Option<String> {
    const MODIFIERS: &[&str] = &[
        "public",
        "private",
        "fileprivate",
        "internal",
        "open",
        "static",
        "class",
        "final",
        "override",
        "mutating",
        "nonmutating",
        "convenience",
        "required",
        "async",
        "nonisolated",
        "dynamic",
    ];
    let mut tokens = line.split_whitespace();
    while let Some(tok) = tokens.next() {
        if tok == "func" {
            let rest = tokens.next()?;
            let name = rest.split(['(', '<']).next()?;
            return is_identifier(name).then(|| name.to_string());
        }
        if tok.starts_with("init(") || tok.starts_with("init?(") || tok.starts_with("init<") {
            return Some("init".to_string());
        }
        let is_modifier = MODIFIERS.contains(&tok) || tok.starts_with('@') || tok.contains("(set)");
        if !is_modifier {
            return None;
        }
    }
    None
}

/// C / C++ (incl. Unreal): a function *definition* line —
/// `ReturnType Name(`, `ReturnType Class::Name(`, `Class::Class(` — not a
/// prototype (`;`), control statement, call or macro like `UFUNCTION(`.
pub fn cpp_function_name(line: &str) -> Option<String> {
    let t = line.trim();
    if t.ends_with(';') || t.starts_with('#') || t.starts_with("//") {
        return None;
    }
    let open = t.find('(')?;
    let head = t[..open].trim();
    if head.contains('=') || head.contains("->") || head.contains('.') {
        return None;
    }
    let tokens: Vec<&str> = head.split_whitespace().collect();
    let first = *tokens.first()?;
    const NOT_A_SIGNATURE: &[&str] = &[
        "if",
        "else",
        "for",
        "while",
        "do",
        "switch",
        "catch",
        "return",
        "new",
        "delete",
        "throw",
        "case",
        "sizeof",
        "co_return",
        "co_await",
    ];
    if NOT_A_SIGNATURE.contains(&first) {
        return None;
    }
    let last = *tokens.last()?;
    let qualified = last.contains("::");
    if tokens.len() < 2 && !qualified {
        return None; // a bare `Foo(` is a call or a macro (UFUNCTION(...))
    }
    let name = base_name(
        last.rsplit("::")
            .next()
            .unwrap_or(last)
            .trim_start_matches(['*', '&']),
    );
    is_identifier(name).then(|| name.to_string())
}

/// GDScript: `func name(` / `static func name(`.
pub fn gdscript_function_name(line: &str) -> Option<String> {
    let t = line.trim();
    let rest = t
        .strip_prefix("static func ")
        .or_else(|| t.strip_prefix("func "))?;
    let name = rest.split('(').next()?.trim();
    is_identifier(name).then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csharp_signatures() {
        assert_eq!(
            csharp_method_name("public void Start()").as_deref(),
            Some("Start")
        );
        assert_eq!(
            csharp_method_name("void Update() {").as_deref(),
            Some("Update")
        );
        assert_eq!(
            csharp_method_name("public async Task<int> LoadAsync()").as_deref(),
            Some("LoadAsync")
        );
        assert_eq!(
            csharp_method_name("public T Get<T>()").as_deref(),
            Some("Get")
        );
        assert_eq!(
            csharp_method_name("public Player()").as_deref(),
            Some("Player")
        );
        for not in [
            "if (hp <= 0) {",
            "foreach (var e in list)",
            "Debug.Log(\"x\");",
            "var h = GetComponent<Health>();",
            "Respawn();",
            "Respawn()",
            "[SerializeField] private float speed;",
            "void Hit();",
            "return Compute(x);",
        ] {
            assert_eq!(csharp_method_name(not), None, "{not}");
        }
    }

    #[test]
    fn swift_signatures() {
        assert_eq!(
            swift_function_name("func start() {").as_deref(),
            Some("start")
        );
        assert_eq!(
            swift_function_name("public static func make<T>() -> T {").as_deref(),
            Some("make")
        );
        assert_eq!(
            swift_function_name("@MainActor func refresh() async {").as_deref(),
            Some("refresh")
        );
        assert_eq!(
            swift_function_name("override init(frame: CGRect) {").as_deref(),
            Some("init")
        );
        assert_eq!(swift_function_name("let p = Player()"), None);
        assert_eq!(swift_function_name("print(\"x\")"), None);
    }

    #[test]
    fn cpp_signatures() {
        assert_eq!(
            cpp_function_name("void AMyActor::BeginPlay()").as_deref(),
            Some("BeginPlay")
        );
        assert_eq!(
            cpp_function_name("int main(int argc, char** argv) {").as_deref(),
            Some("main")
        );
        assert_eq!(
            cpp_function_name("AMyActor::AMyActor()").as_deref(),
            Some("AMyActor")
        );
        assert_eq!(
            cpp_function_name("static FString* Helper::Name(int x)").as_deref(),
            Some("Name")
        );
        for not in [
            "UFUNCTION(BlueprintCallable)",
            "void BeginPlay() override;",
            "if (bReady) {",
            "Super::BeginPlay();",
            "return Compute(x);",
            "x = Foo(1)",
            "#define MAX(a, b) a",
            "Mesh->SetupAttachment(Root)",
        ] {
            assert_eq!(cpp_function_name(not), None, "{not}");
        }
    }

    #[test]
    fn gdscript_signatures() {
        assert_eq!(
            gdscript_function_name("func _ready() -> void:").as_deref(),
            Some("_ready")
        );
        assert_eq!(
            gdscript_function_name("static func make():").as_deref(),
            Some("make")
        );
        assert_eq!(gdscript_function_name("var f = func(): pass"), None);
        assert_eq!(gdscript_function_name("# func old():"), None);
    }
}
