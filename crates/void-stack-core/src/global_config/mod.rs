mod paths;
mod project_ops;
mod scanner;

pub use paths::*;
pub use project_ops::*;
pub use scanner::*;

use std::fs;

use crate::error::{Result, VoidStackError};
use crate::model::Project;

/// Wrapper for the global config containing multiple projects.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct GlobalConfig {
    #[serde(default)]
    pub projects: Vec<Project>,
}

/// Load the global config. Returns empty config if file doesn't exist.
pub fn load_global_config() -> Result<GlobalConfig> {
    let path = global_config_path()?;
    if !path.exists() {
        return Ok(GlobalConfig::default());
    }
    let content = fs::read_to_string(&path)?;
    let config: GlobalConfig = toml::from_str(&content)?;
    Ok(config)
}

/// Save the global config, creating the directory if needed.
pub fn save_global_config(config: &GlobalConfig) -> Result<()> {
    let dir = global_config_dir()?;
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    let path = dir.join(GLOBAL_CONFIG_FILENAME);
    let content =
        toml::to_string_pretty(config).map_err(|e| VoidStackError::InvalidConfig(e.to_string()))?;
    fs::write(&path, content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_global_config_roundtrip() {
        use crate::model::*;

        let config = GlobalConfig {
            projects: vec![Project {
                name: "test-project".into(),
                description: "A test".into(),
                path: "F:\\test".into(),
                project_type: Some(ProjectType::Node),
                tags: vec![],
                services: vec![Service {
                    name: "web".into(),
                    command: "npm run dev".into(),
                    target: Target::Windows,
                    working_dir: Some("F:\\test\\frontend".into()),
                    enabled: true,
                    env_vars: vec![],
                    depends_on: vec![],
                    docker: None,
                }],
                hooks: None,
            }],
        };

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let loaded: GlobalConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(loaded.projects.len(), 1);
        assert_eq!(loaded.projects[0].name, "test-project");
        assert_eq!(
            loaded.projects[0].services[0].working_dir.as_deref(),
            Some("F:\\test\\frontend")
        );
    }

    #[test]
    fn test_scan_subprojects() {
        let dir = tempdir().unwrap();
        // Create a Node root
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        // Create a Python subdir
        let backend = dir.path().join("backend");
        std::fs::create_dir(&backend).unwrap();
        std::fs::write(backend.join("requirements.txt"), "flask").unwrap();

        let results = scan_subprojects(dir.path());
        assert!(results.len() >= 2);
        // Should find Node at root and Python in backend/
        let types: Vec<_> = results.iter().map(|(_, _, t)| *t).collect();
        assert!(types.contains(&crate::model::ProjectType::Node));
        assert!(types.contains(&crate::model::ProjectType::Python));
    }

    #[test]
    fn test_detect_fastapi_uvicorn() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "fastapi\nuvicorn\n").unwrap();
        std::fs::write(
            dir.path().join("main.py"),
            "from fastapi import FastAPI\n\napp = FastAPI()\n\n@app.get('/')\ndef root():\n    return {'ok': True}\n",
        ).unwrap();

        let cmd = detect_python_command(dir.path());
        assert_eq!(cmd, "uvicorn main:app --host 0.0.0.0 --port 8000");
    }

    #[test]
    fn test_detect_fastapi_custom_var() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.py"),
            "from fastapi import FastAPI\n\nserver = FastAPI(title='My API')\n",
        )
        .unwrap();

        let cmd = detect_python_command(dir.path());
        assert_eq!(cmd, "uvicorn app:server --host 0.0.0.0 --port 8000");
    }

    #[test]
    fn test_detect_fastapi_self_starting() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("main.py"),
            "from fastapi import FastAPI\nimport uvicorn\n\napp = FastAPI()\n\nif __name__ == '__main__':\n    uvicorn.run(app)\n",
        ).unwrap();

        let cmd = detect_python_command(dir.path());
        // Self-starting scripts should use `python main.py`
        assert_eq!(cmd, "python main.py");
    }

    #[test]
    fn test_detect_flask() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.py"),
            "from flask import Flask\n\napp = Flask(__name__)\n\n@app.route('/')\ndef index():\n    return 'hello'\n",
        ).unwrap();

        let cmd = detect_python_command(dir.path());
        assert_eq!(cmd, "flask --app app run --port 5000");
    }

    #[test]
    fn test_detect_flask_self_starting() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.py"),
            "from flask import Flask\n\napp = Flask(__name__)\n\nif __name__ == '__main__':\n    app.run(port=5000)\n",
        ).unwrap();

        let cmd = detect_python_command(dir.path());
        assert_eq!(cmd, "python app.py");
    }

    #[test]
    fn test_detect_django() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("manage.py"),
            "#!/usr/bin/env python\nimport django\n",
        )
        .unwrap();

        let cmd = detect_python_command(dir.path());
        assert_eq!(cmd, "python manage.py runserver");
    }

    #[test]
    fn test_detect_plain_main() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("main.py"),
            "import sys\n\ndef main():\n    print('hello')\n\nif __name__ == '__main__':\n    main()\n",
        ).unwrap();

        let cmd = detect_python_command(dir.path());
        assert_eq!(cmd, "python main.py");
    }

    #[test]
    fn test_detect_app_variable_default() {
        let content = "# no constructor here\nprint('hello')\n";
        assert_eq!(detect_app_variable(content, &["FastAPI("]), "app");
    }

    #[test]
    fn test_detect_app_variable_custom() {
        let content = "from fastapi import FastAPI\n\nmy_api = FastAPI(title='test')\n";
        assert_eq!(detect_app_variable(content, &["FastAPI("]), "my_api");
    }

    #[test]
    fn test_find_project() {
        let config = GlobalConfig {
            projects: vec![Project {
                name: "MyApp".into(),
                description: "test".into(),
                path: "/test".into(),
                project_type: None,
                tags: vec![],
                services: vec![],
                hooks: None,
            }],
        };
        assert!(find_project(&config, "myapp").is_some());
        assert!(find_project(&config, "MYAPP").is_some());
        assert!(find_project(&config, "unknown").is_none());
    }

    #[test]
    fn test_remove_project() {
        let mut config = GlobalConfig {
            projects: vec![
                Project {
                    name: "A".into(),
                    description: "".into(),
                    path: "/a".into(),
                    project_type: None,
                    tags: vec![],
                    services: vec![],
                    hooks: None,
                },
                Project {
                    name: "B".into(),
                    description: "".into(),
                    path: "/b".into(),
                    project_type: None,
                    tags: vec![],
                    services: vec![],
                    hooks: None,
                },
            ],
        };
        assert!(remove_project(&mut config, "a"));
        assert_eq!(config.projects.len(), 1);
        assert_eq!(config.projects[0].name, "B");
        assert!(!remove_project(&mut config, "nonexistent"));
    }

    #[test]
    fn test_remove_service() {
        use crate::model::*;
        let mut config = GlobalConfig {
            projects: vec![Project {
                name: "P".into(),
                description: "".into(),
                path: "/p".into(),
                project_type: None,
                tags: vec![],
                services: vec![
                    Service {
                        name: "svc1".into(),
                        command: "x".into(),
                        target: Target::Windows,
                        working_dir: None,
                        enabled: true,
                        env_vars: vec![],
                        depends_on: vec![],
                        docker: None,
                    },
                    Service {
                        name: "svc2".into(),
                        command: "y".into(),
                        target: Target::Windows,
                        working_dir: None,
                        enabled: true,
                        env_vars: vec![],
                        depends_on: vec![],
                        docker: None,
                    },
                ],
                hooks: None,
            }],
        };
        assert!(remove_service(&mut config, "P", "svc1"));
        assert_eq!(config.projects[0].services.len(), 1);
        assert!(!remove_service(&mut config, "P", "nonexistent"));
        assert!(!remove_service(&mut config, "NoProject", "svc2"));
    }

    #[test]
    fn test_default_command_for() {
        use crate::model::ProjectType;
        assert_eq!(default_command_for(ProjectType::Node), "npm run dev");
        assert_eq!(default_command_for(ProjectType::Rust), "cargo run");
        assert_eq!(default_command_for(ProjectType::Go), "go run .");
        assert_eq!(default_command_for(ProjectType::Flutter), "flutter run");
        assert_eq!(
            default_command_for(ProjectType::Docker),
            "docker compose up"
        );
        assert_eq!(default_command_for(ProjectType::Unknown), "echo 'hello'");
    }

    #[test]
    fn test_default_command_for_dir_python_no_entrypoint() {
        let dir = tempdir().unwrap();
        let cmd = default_command_for_dir(crate::model::ProjectType::Python, dir.path());
        assert_eq!(cmd, "python main.py");
    }

    #[test]
    fn test_has_entrypoint_node() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        assert!(has_entrypoint(crate::model::ProjectType::Node, dir.path()));
    }

    #[test]
    fn test_has_entrypoint_rust() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        assert!(has_entrypoint(crate::model::ProjectType::Rust, dir.path()));
    }

    #[test]
    fn test_has_entrypoint_python() {
        let dir = tempdir().unwrap();
        assert!(!has_entrypoint(
            crate::model::ProjectType::Python,
            dir.path()
        ));
        std::fs::write(dir.path().join("app.py"), "").unwrap();
        assert!(has_entrypoint(
            crate::model::ProjectType::Python,
            dir.path()
        ));
    }

    #[test]
    fn test_has_entrypoint_unknown() {
        let dir = tempdir().unwrap();
        assert!(!has_entrypoint(
            crate::model::ProjectType::Unknown,
            dir.path()
        ));
    }

    #[test]
    fn test_scan_subprojects_skips_hidden() {
        let dir = tempdir().unwrap();
        let hidden = dir.path().join(".hidden");
        std::fs::create_dir(&hidden).unwrap();
        std::fs::write(hidden.join("package.json"), "{}").unwrap();

        let results = scan_subprojects(dir.path());
        assert!(!results.iter().any(|(name, _, _)| name.contains(".hidden")));
    }

    #[test]
    fn test_scan_subprojects_deep() {
        let dir = tempdir().unwrap();
        let backends = dir.path().join("backends");
        let api = backends.join("api");
        std::fs::create_dir_all(&api).unwrap();
        std::fs::write(api.join("requirements.txt"), "flask\n").unwrap();

        let results = scan_subprojects(dir.path());
        assert!(results
            .iter()
            .any(|(name, _, t)| name.contains("api")
                && *t == crate::model::ProjectType::Python));
    }

    #[test]
    fn test_global_config_default() {
        let config = GlobalConfig::default();
        assert!(config.projects.is_empty());
    }

    #[test]
    fn test_detect_go_command_with_air() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module example.com/app").unwrap();
        std::fs::write(dir.path().join(".air.toml"), "[build]\ncmd = \"go build\"").unwrap();
        let cmd = detect_go_command(dir.path());
        assert_eq!(cmd, "air");
    }

    #[test]
    fn test_detect_go_command_without_air() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module example.com/app").unwrap();
        let cmd = detect_go_command(dir.path());
        assert_eq!(cmd, "go run .");
    }

    #[test]
    fn test_scan_air_services_multiple() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module example.com/app").unwrap();
        // Two air services in subdirs
        std::fs::create_dir_all(dir.path().join("cmd/api")).unwrap();
        std::fs::write(dir.path().join("cmd/api/.air.toml"), "[build]").unwrap();
        std::fs::create_dir_all(dir.path().join("cmd/worker")).unwrap();
        std::fs::write(dir.path().join("cmd/worker/.air.toml"), "[build]").unwrap();

        let results = scan_air_services(dir.path(), "myapp");
        assert_eq!(results.len(), 2);
        assert!(results.iter().any(|(name, _, _)| name == "myapp/cmd/api"));
        assert!(
            results
                .iter()
                .any(|(name, _, _)| name == "myapp/cmd/worker")
        );
    }

    #[test]
    fn test_scan_air_services_none() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module example.com/app").unwrap();
        let results = scan_air_services(dir.path(), "myapp");
        assert!(results.is_empty());
    }

    #[test]
    fn test_scan_air_services_root_and_subdir() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module example.com/app").unwrap();
        std::fs::write(dir.path().join(".air.toml"), "[build]").unwrap();
        std::fs::create_dir_all(dir.path().join("services/gateway")).unwrap();
        std::fs::write(dir.path().join("services/gateway/.air.toml"), "[build]").unwrap();

        let results = scan_air_services(dir.path(), "myapp");
        assert_eq!(results.len(), 2); // root + subdir
        assert!(results.iter().any(|(name, _, _)| name == "myapp"));
        assert!(
            results
                .iter()
                .any(|(name, _, _)| name == "myapp/services/gateway")
        );
    }
}
