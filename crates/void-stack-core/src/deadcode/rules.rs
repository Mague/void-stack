//! Per-language rules for dead-code detection: declaration visibility,
//! and the entry points game engines and UI frameworks call by convention
//! (no call site in the project ever shows up in the call graph for them).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use super::{NodeRow, cached_lines};

// ── Rust ────────────────────────────────────────────────────

/// Trait-impl / interface methods that are invoked by the runtime or via
/// dynamic dispatch — zero direct CALLS edges says nothing about them.
pub(super) const TRAIT_LIKE_NAMES: &[&str] = &[
    "fmt",
    "eq",
    "ne",
    "hash",
    "clone",
    "default",
    "drop",
    "from",
    "into",
    "try_from",
    "deserialize",
    "serialize",
    "next",
    "poll",
    "deref",
    "deref_mut",
    "as_ref",
    "as_mut",
    "partial_cmp",
    "cmp",
    "to_string",
    "build",
    "visit",
    "index",
    "add",
    "sub",
    "mul",
    "call",
    "call_once",
    "call_mut",
    "new",
];

/// Attribute markers (the lines just above a Rust item) that mean the
/// symbol is registered by a macro/framework, not called directly.
/// Derive macros whose types are constructed/consumed by frameworks, not
/// by direct calls: clap parses into them, serde deserializes into them.
const FRAMEWORK_DERIVES: &[&str] = &[
    "Parser",
    "Subcommand",
    "Args",
    "ValueEnum",
    "Deserialize",
    "Serialize",
];

const REGISTRATION_MARKERS: &[&str] = &[
    "#[tool",
    "#[tauri::command",
    "#[test",
    "#[tokio::test",
    "#[get",
    "#[post",
    "#[put",
    "#[delete",
    "#[route",
    "#[handler",
    "#[no_mangle",
    "#[wasm_bindgen",
    "#[pyfunction",
];

/// Registered by a macro/framework rather than called directly: attribute
/// markers, or clap/serde derive types (the framework macro is the caller).
pub(super) fn is_macro_registered(above: &str) -> bool {
    REGISTRATION_MARKERS.iter().any(|m| above.contains(m))
        || (above.contains("derive(") && FRAMEWORK_DERIVES.iter().any(|d| above.contains(d)))
}

// ── Game engines and UI frameworks ──────────────────────────

/// Methods a game engine / UI framework calls by name — no call site in
/// the project will ever show up in the graph.
const UNITY_MESSAGES: &[&str] = &[
    "Awake",
    "Start",
    "Update",
    "FixedUpdate",
    "LateUpdate",
    "OnEnable",
    "OnDisable",
    "OnDestroy",
    "OnValidate",
    "Reset",
    "OnGUI",
    "OnDrawGizmos",
    "OnDrawGizmosSelected",
    "OnTriggerEnter",
    "OnTriggerExit",
    "OnTriggerStay",
    "OnTriggerEnter2D",
    "OnTriggerExit2D",
    "OnTriggerStay2D",
    "OnCollisionEnter",
    "OnCollisionExit",
    "OnCollisionStay",
    "OnCollisionEnter2D",
    "OnCollisionExit2D",
    "OnCollisionStay2D",
    "OnControllerColliderHit",
    "OnParticleCollision",
    "OnApplicationQuit",
    "OnApplicationPause",
    "OnApplicationFocus",
    "OnBecameVisible",
    "OnBecameInvisible",
    "OnMouseDown",
    "OnMouseUp",
    "OnMouseEnter",
    "OnMouseExit",
    "OnMouseOver",
    "OnMouseDrag",
    "OnAnimatorMove",
    "OnAnimatorIK",
    "OnRenderObject",
    "OnWillRenderObject",
    "OnPreRender",
    "OnPostRender",
    "OnRenderImage",
    "OnTransformParentChanged",
    "OnTransformChildrenChanged",
    "OnRectTransformDimensionsChange",
    "OnInspectorGUI",
    "OnSceneGUI",
    "CreateInspectorGUI",
    "CreateGUI",
    "OnSelectionChange",
    "OnHierarchyChange",
    "OnProjectChange",
    "OnFocus",
    "OnLostFocus",
    "OnBeforeSerialize",
    "OnAfterDeserialize",
    "OnStateEnter",
    "OnStateExit",
    "OnStateUpdate",
    "OnStateMove",
    "OnStateIK",
    "OnPointerClick",
    "OnPointerDown",
    "OnPointerUp",
    "OnPointerEnter",
    "OnPointerExit",
    "OnBeginDrag",
    "OnDrag",
    "OnEndDrag",
    "OnDrop",
    "OnSubmit",
    "OnCancel",
    // .NET overrides called through the runtime.
    "Dispose",
    "Equals",
    "GetHashCode",
    "ToString",
    "CompareTo",
    "GetEnumerator",
    // Godot C# virtuals.
    "_Ready",
    "_Process",
    "_PhysicsProcess",
    "_Input",
    "_UnhandledInput",
    "_UnhandledKeyInput",
    "_EnterTree",
    "_ExitTree",
    "_Notification",
    "_Draw",
    "_GuiInput",
    "_IntegrateForces",
    "_GetConfigurationWarnings",
];

const GODOT_VIRTUALS: &[&str] = &[
    "_init",
    "_ready",
    "_process",
    "_physics_process",
    "_input",
    "_unhandled_input",
    "_unhandled_key_input",
    "_shortcut_input",
    "_enter_tree",
    "_exit_tree",
    "_notification",
    "_draw",
    "_gui_input",
    "_integrate_forces",
    "_get_configuration_warnings",
    "_to_string",
    "_get",
    "_set",
    "_get_property_list",
    "_property_can_revert",
    "_property_get_revert",
    "_validate_property",
    "_static_init",
];

/// Unreal overrides the engine calls (actors, components, widgets, modules).
const UNREAL_OVERRIDES: &[&str] = &[
    "BeginPlay",
    "EndPlay",
    "Tick",
    "PostInitializeComponents",
    "PreInitializeComponents",
    "SetupPlayerInputComponent",
    "OnConstruction",
    "PostEditChangeProperty",
    "PostEditChangeChainProperty",
    "GetLifetimeReplicatedProps",
    "StartupModule",
    "ShutdownModule",
    "NativeConstruct",
    "NativeDestruct",
    "NativeTick",
    "NativeOnInitialized",
    "NativePreConstruct",
    "InitializeComponent",
    "UninitializeComponent",
    "TickComponent",
    "OnRegister",
    "OnUnregister",
    "PossessedBy",
    "UnPossessed",
    "OnRep_PlayerState",
    "Destroyed",
    "PostLoad",
    "PostActorCreated",
    "NotifyActorBeginOverlap",
    "NotifyActorEndOverlap",
    "NotifyHit",
    "Initialize",
    "Deinitialize",
    "ShouldCreateSubsystem",
    "GetToolsetName",
    "RunTest",
    "Define",
];

/// Apple / SwiftUI lifecycle methods called by the frameworks.
const SWIFT_LIFECYCLE: &[&str] = &[
    "body",
    "init",
    "deinit",
    "application",
    "scene",
    "sceneDidBecomeActive",
    "sceneWillResignActive",
    "sceneWillEnterForeground",
    "sceneDidEnterBackground",
    "viewDidLoad",
    "viewWillAppear",
    "viewDidAppear",
    "viewWillDisappear",
    "viewDidDisappear",
    "viewWillLayoutSubviews",
    "viewDidLayoutSubviews",
    "layoutSubviews",
    "draw",
    "awakeFromNib",
    "prepare",
    "encode",
    "hash",
    "makeBody",
    "makeUIView",
    "updateUIView",
    "makeNSView",
    "updateNSView",
    "makeUIViewController",
    "updateUIViewController",
    "makeCoordinator",
    "tableView",
    "collectionView",
    "numberOfSections",
    "setUp",
    "tearDown",
    "setUpWithError",
    "tearDownWithError",
    "didMove",
    "update",
    "touchesBegan",
    "touchesMoved",
    "touchesEnded",
    "didBegin",
];

/// Base types whose subclasses the engine instantiates from scenes, assets
/// or menus — the class itself never has a `new` call site.
const ENGINE_BASES: &[&str] = &[
    "MonoBehaviour",
    "ScriptableObject",
    "EditorWindow",
    "Editor",
    "PropertyDrawer",
    "DecoratorDrawer",
    "StateMachineBehaviour",
    "NetworkBehaviour",
    "AssetPostprocessor",
    "ScriptedImporter",
    "Node",
    "Node2D",
    "Node3D",
    "Control",
    "Resource",
    "RefCounted",
    "CharacterBody2D",
    "CharacterBody3D",
    "RigidBody2D",
    "RigidBody3D",
    "Area2D",
    "Area3D",
    "EditorPlugin",
    "View",
    "App",
    "UIViewController",
    "UIView",
    "UIApplicationDelegate",
    "XCTestCase",
];

/// Attributes / macros that register a symbol with an engine or framework.
const ENGINE_MARKERS: &[&str] = &[
    // Unity
    "[MenuItem",
    "[ContextMenu",
    "[RuntimeInitializeOnLoadMethod",
    "[InitializeOnLoad",
    "[CustomEditor",
    "[CustomPropertyDrawer",
    "[CreateAssetMenu",
    "[Test",
    "[UnityTest",
    "[SetUp",
    "[TearDown",
    "[OneTimeSetUp",
    "[DidReloadScripts",
    "[PostProcessBuild",
    "[MenuCommand",
    "[Shortcut",
    // Godot C#
    "[Export",
    "[Signal",
    "[GlobalClass",
    "[Tool",
    // Unreal reflection: Blueprint-callable, RPCs, exec, delegates.
    "UFUNCTION(",
    "UCLASS(",
    "USTRUCT(",
    "UENUM(",
    "UINTERFACE(",
    "IMPLEMENT_MODULE",
    // Swift / Objective-C runtime
    "@main",
    "@objc",
    "@IBAction",
    "@IBOutlet",
    "@NSManaged",
    "@UIApplicationMain",
    "@NSApplicationMain",
    // GDScript
    "@export",
    "@rpc",
    "@onready",
];

/// MetaTrader event handlers (MQL4/MQL5, parsed with the C++ grammar):
/// the terminal calls them, nothing in the project does. `init` / `deinit`
/// / `start` are the pre-build-600 MQL4 names still found in old EAs.
const MQL_EVENT_HANDLERS: &[&str] = &[
    "OnInit",
    "OnDeinit",
    "OnTick",
    "OnTimer",
    "OnCalculate",
    "OnStart",
    "OnChartEvent",
    "OnTrade",
    "OnTradeTransaction",
    "OnTester",
    "OnTesterInit",
    "OnTesterPass",
    "OnTesterDeinit",
    "OnBookEvent",
    "init",
    "deinit",
    "start",
];

/// Symbols an engine/framework calls by convention: lifecycle methods,
/// engine base-class subclasses, attribute-registered members, Godot
/// signal handlers (`_on_*`, wired in `.tscn`), Unreal constructors.
pub(super) fn is_engine_entrypoint(
    language: &str,
    name: &str,
    parent: Option<&str>,
    decl: &str,
    above: &str,
) -> bool {
    if ENGINE_MARKERS
        .iter()
        .any(|m| above.contains(m) || decl.contains(m))
    {
        return true;
    }
    match language {
        "csharp" => {
            UNITY_MESSAGES.contains(&name)
                || decl.contains("override ")
                || inherits_engine_base(decl)
        }
        "gdscript" => GODOT_VIRTUALS.contains(&name) || name.starts_with("_on_"),
        "c" | "cpp" => {
            UNREAL_OVERRIDES.contains(&name)
                || MQL_EVENT_HANDLERS.contains(&name)
                || decl.contains("override")
                || parent.is_some_and(|p| p == name) // constructor
        }
        "swift" => {
            SWIFT_LIFECYCLE.contains(&name)
                || decl.contains("override ")
                || inherits_engine_base(decl)
        }
        _ => false,
    }
}

/// `class Foo : MonoBehaviour` / `struct V: View` / `class A: UIView, B`.
fn inherits_engine_base(decl: &str) -> bool {
    let Some((head, bases)) = decl.split_once(':') else {
        return false;
    };
    if !["class ", "struct ", "record "]
        .iter()
        .any(|k| head.contains(k))
    {
        return false;
    }
    bases
        .split([',', '{', ' ', '<'])
        .map(str::trim)
        .any(|b| ENGINE_BASES.contains(&b))
}

/// Names of methods declared under `UFUNCTION(...)` in the project's
/// headers. Unreal calls them through reflection (Blueprints, RPCs, exec
/// commands, delegates), and their definitions in `.cpp` files carry no
/// marker of their own.
pub(super) fn unreal_reflected_names(
    nodes: &[NodeRow],
    root: &Path,
    file_cache: &mut HashMap<String, Vec<String>>,
) -> HashSet<String> {
    let headers: HashSet<String> = nodes
        .iter()
        .map(|n| n.2.replace('\\', "/"))
        .filter(|f| f.ends_with(".h") || f.ends_with(".hpp"))
        .collect();
    let mut names = HashSet::new();
    for header in headers {
        let lines = cached_lines(file_cache, root, &header);
        for (i, line) in lines.iter().enumerate() {
            if !line.contains("UFUNCTION(") {
                continue;
            }
            // The declaration is the next line that opens a parameter list.
            if let Some(name) = lines
                .iter()
                .skip(i + 1)
                .take(4)
                .find(|l| l.contains('(') && !l.trim_start().starts_with("//"))
                .and_then(|l| l.split('(').next())
                .and_then(|head| head.split_whitespace().last())
            {
                let name = name.trim_start_matches(['*', '&']);
                if !name.is_empty() {
                    names.insert(name.to_string());
                }
            }
        }
    }
    names
}

/// Per-language visibility heuristic from the declaration line/name.
pub(super) fn is_exported(language: &str, decl: &str, name: &str) -> bool {
    match language {
        "rust" => decl.trim_start().starts_with("pub"),
        "go" => name.chars().next().is_some_and(|c| c.is_ascii_uppercase()),
        "dart" => !name.starts_with('_'),
        "javascript" | "typescript" => decl.contains("export"),
        "csharp" => ["public ", "protected ", "internal "]
            .iter()
            .any(|m| decl.contains(m)),
        "swift" => !decl.contains("private "),
        "gdscript" => !name.starts_with('_'),
        _ => true, // unknown visibility — stay conservative
    }
}
