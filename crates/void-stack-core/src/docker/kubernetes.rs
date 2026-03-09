//! Kubernetes manifest parser — extract resources from YAML files.

use std::path::Path;

use super::K8sResource;

/// Parse Kubernetes manifests found in a project directory.
pub fn parse_kubernetes(project_path: &Path) -> Vec<K8sResource> {
    let mut resources = Vec::new();
    let mut yaml_files = Vec::new();

    // Search in known k8s directories
    let k8s_dirs = ["k8s", "kubernetes", "manifests", "deploy", "deployment", "kube"];
    for dir_name in &k8s_dirs {
        let dir = project_path.join(dir_name);
        if dir.is_dir() {
            collect_yaml_files(&dir, &mut yaml_files, 0, 3);
        }
    }

    // Also check for common k8s files in root
    let root_patterns = [
        "deployment.yaml", "deployment.yml",
        "service.yaml", "service.yml",
        "ingress.yaml", "ingress.yml",
        "statefulset.yaml", "statefulset.yml",
    ];
    for pattern in &root_patterns {
        let p = project_path.join(pattern);
        if p.exists() && !yaml_files.contains(&p) {
            yaml_files.push(p);
        }
    }

    // Check for files matching *-deployment.yaml, *-service.yaml patterns in root
    if let Ok(entries) = std::fs::read_dir(project_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let name = entry.file_name().to_string_lossy().to_lowercase();
                if (name.ends_with("-deployment.yaml") || name.ends_with("-deployment.yml")
                    || name.ends_with("-service.yaml") || name.ends_with("-service.yml")
                    || name.ends_with("-ingress.yaml") || name.ends_with("-ingress.yml"))
                    && !yaml_files.contains(&path)
                {
                    yaml_files.push(path);
                }
            }
        }
    }

    for path in &yaml_files {
        if let Ok(content) = std::fs::read_to_string(path) {
            parse_k8s_yaml(&content, &mut resources);
        }
    }

    resources
}

fn collect_yaml_files(dir: &Path, files: &mut Vec<std::path::PathBuf>, depth: usize, max_depth: usize) {
    if depth >= max_depth {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_yaml_files(&path, files, depth + 1, max_depth);
        } else if path.is_file() {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext == "yaml" || ext == "yml" {
                files.push(path);
            }
        }
    }
}

/// Parse a YAML file that may contain one or more K8s resource documents.
fn parse_k8s_yaml(content: &str, resources: &mut Vec<K8sResource>) {
    // K8s YAML files may contain multiple documents separated by ---
    for doc_str in content.split("\n---") {
        let trimmed = doc_str.trim();
        if trimmed.is_empty() {
            continue;
        }

        let doc: serde_yaml::Value = match serde_yaml::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let kind = match doc.get("kind").and_then(|v| v.as_str()) {
            Some(k) => k.to_string(),
            None => continue,
        };

        // Only process known K8s resource types
        let known_kinds = [
            "Deployment", "Service", "Ingress", "StatefulSet",
            "ConfigMap", "Secret", "DaemonSet", "Job", "CronJob",
            "HorizontalPodAutoscaler",
        ];
        if !known_kinds.contains(&kind.as_str()) {
            continue;
        }

        let name = doc.get("metadata")
            .and_then(|m| m.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        let namespace = doc.get("metadata")
            .and_then(|m| m.get("namespace"))
            .and_then(|v| v.as_str())
            .map(String::from);

        let mut images = Vec::new();
        let mut ports = Vec::new();
        let mut replicas = None;

        // Extract replicas from spec
        if let Some(r) = doc.get("spec").and_then(|s| s.get("replicas")).and_then(|v| v.as_u64()) {
            replicas = Some(r as u32);
        }

        // Extract container images and ports from pod spec
        let pod_spec = doc.get("spec")
            .and_then(|s| s.get("template"))
            .and_then(|t| t.get("spec"));

        if let Some(spec) = pod_spec {
            extract_containers(spec, &mut images, &mut ports);
        }

        // For Service kind, extract ports from spec.ports
        if kind == "Service" {
            if let Some(svc_ports) = doc.get("spec").and_then(|s| s.get("ports")).and_then(|v| v.as_sequence()) {
                for port_val in svc_ports {
                    if let Some(port) = port_val.get("port").and_then(|v| v.as_u64()) {
                        if !ports.contains(&(port as u16)) {
                            ports.push(port as u16);
                        }
                    }
                    if let Some(target) = port_val.get("targetPort").and_then(|v| v.as_u64()) {
                        if !ports.contains(&(target as u16)) {
                            ports.push(target as u16);
                        }
                    }
                }
            }
        }

        // For Ingress, extract ports from rules
        if kind == "Ingress" {
            if let Some(rules) = doc.get("spec").and_then(|s| s.get("rules")).and_then(|v| v.as_sequence()) {
                for rule in rules {
                    if let Some(paths) = rule.get("http").and_then(|h| h.get("paths")).and_then(|v| v.as_sequence()) {
                        for path in paths {
                            if let Some(port) = path.get("backend")
                                .and_then(|b| b.get("service"))
                                .and_then(|s| s.get("port"))
                                .and_then(|p| p.get("number"))
                                .and_then(|v| v.as_u64())
                            {
                                if !ports.contains(&(port as u16)) {
                                    ports.push(port as u16);
                                }
                            }
                        }
                    }
                }
            }
        }

        resources.push(K8sResource {
            kind,
            name,
            namespace,
            images,
            ports,
            replicas,
        });
    }
}

/// Extract container images and ports from a pod spec.
fn extract_containers(spec: &serde_yaml::Value, images: &mut Vec<String>, ports: &mut Vec<u16>) {
    for container_key in &["containers", "initContainers"] {
        if let Some(containers) = spec.get(container_key).and_then(|v| v.as_sequence()) {
            for container in containers {
                if let Some(image) = container.get("image").and_then(|v| v.as_str()) {
                    if !images.contains(&image.to_string()) {
                        images.push(image.to_string());
                    }
                }

                if let Some(container_ports) = container.get("ports").and_then(|v| v.as_sequence()) {
                    for port_val in container_ports {
                        if let Some(port) = port_val.get("containerPort").and_then(|v| v.as_u64()) {
                            if !ports.contains(&(port as u16)) {
                                ports.push(port as u16);
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_kubernetes_deployment_and_service() {
        let dir = tempfile::tempdir().unwrap();
        let k8s_dir = dir.path().join("k8s");
        std::fs::create_dir(&k8s_dir).unwrap();

        std::fs::write(k8s_dir.join("deployment.yaml"), r#"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: api-server
  namespace: production
spec:
  replicas: 3
  template:
    spec:
      containers:
        - name: api
          image: myapp/api:1.2.3
          ports:
            - containerPort: 8080
            - containerPort: 9090
---
apiVersion: v1
kind: Service
metadata:
  name: api-service
  namespace: production
spec:
  ports:
    - port: 80
      targetPort: 8080
  selector:
    app: api
"#).unwrap();

        let resources = parse_kubernetes(dir.path());
        assert_eq!(resources.len(), 2);

        let deploy = &resources[0];
        assert_eq!(deploy.kind, "Deployment");
        assert_eq!(deploy.name, "api-server");
        assert_eq!(deploy.namespace.as_deref(), Some("production"));
        assert_eq!(deploy.replicas, Some(3));
        assert_eq!(deploy.images, vec!["myapp/api:1.2.3"]);
        assert!(deploy.ports.contains(&8080));
        assert!(deploy.ports.contains(&9090));

        let svc = &resources[1];
        assert_eq!(svc.kind, "Service");
        assert_eq!(svc.name, "api-service");
        assert!(svc.ports.contains(&80));
        assert!(svc.ports.contains(&8080));
    }

    #[test]
    fn test_parse_kubernetes_statefulset() {
        let dir = tempfile::tempdir().unwrap();
        let manifests_dir = dir.path().join("manifests");
        std::fs::create_dir(&manifests_dir).unwrap();

        std::fs::write(manifests_dir.join("db.yaml"), r#"
apiVersion: apps/v1
kind: StatefulSet
metadata:
  name: postgres
spec:
  replicas: 1
  template:
    spec:
      containers:
        - name: postgres
          image: postgres:16
          ports:
            - containerPort: 5432
"#).unwrap();

        let resources = parse_kubernetes(dir.path());
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].kind, "StatefulSet");
        assert_eq!(resources[0].name, "postgres");
        assert_eq!(resources[0].images, vec!["postgres:16"]);
        assert_eq!(resources[0].ports, vec![5432]);
    }

    #[test]
    fn test_parse_kubernetes_root_files() {
        let dir = tempfile::tempdir().unwrap();

        std::fs::write(dir.path().join("deployment.yaml"), r#"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: web
spec:
  replicas: 2
  template:
    spec:
      containers:
        - name: web
          image: nginx:latest
          ports:
            - containerPort: 80
"#).unwrap();

        let resources = parse_kubernetes(dir.path());
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].name, "web");
    }

    #[test]
    fn test_empty_project_no_kubernetes() {
        let dir = tempfile::tempdir().unwrap();
        let resources = parse_kubernetes(dir.path());
        assert!(resources.is_empty());
    }

    #[test]
    fn test_parse_kubernetes_ingress() {
        let dir = tempfile::tempdir().unwrap();
        let k8s_dir = dir.path().join("k8s");
        std::fs::create_dir(&k8s_dir).unwrap();

        std::fs::write(k8s_dir.join("ingress.yaml"), r#"
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: main-ingress
spec:
  rules:
    - host: example.com
      http:
        paths:
          - path: /
            pathType: Prefix
            backend:
              service:
                name: web
                port:
                  number: 80
"#).unwrap();

        let resources = parse_kubernetes(dir.path());
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].kind, "Ingress");
        assert_eq!(resources[0].name, "main-ingress");
        assert!(resources[0].ports.contains(&80));
    }
}
