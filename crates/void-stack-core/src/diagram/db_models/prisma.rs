//! Prisma schema detection.

use super::DbModel;

pub(super) fn scan_prisma_schema(dir: &std::path::Path, models: &mut Vec<DbModel>) {
    let prisma_path = dir.join("prisma").join("schema.prisma");
    let content = match std::fs::read_to_string(&prisma_path) {
        Ok(c) => c,
        Err(_) => return,
    };

    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        if trimmed.starts_with("model ") && trimmed.ends_with('{') {
            let model_name = trimmed
                .strip_prefix("model ")
                .and_then(|s| s.strip_suffix('{'))
                .unwrap_or("")
                .trim()
                .to_string();

            let mut fields = Vec::new();
            i += 1;

            while i < lines.len() {
                let field_line = lines[i].trim();
                if field_line == "}" {
                    break;
                }
                if field_line.is_empty()
                    || field_line.starts_with("//")
                    || field_line.starts_with("@@")
                {
                    i += 1;
                    continue;
                }

                let parts: Vec<&str> = field_line.split_whitespace().collect();
                if parts.len() >= 2 {
                    let name = parts[0].to_string();
                    let prisma_type = parts[1]
                        .trim_end_matches('?')
                        .trim_end_matches("[]")
                        .to_lowercase();
                    let mapped = match prisma_type.as_str() {
                        "string" => "string",
                        "int" | "bigint" => "int",
                        "float" | "decimal" => "float",
                        "boolean" => "bool",
                        "datetime" => "datetime",
                        "json" => "json",
                        _ => "FK",
                    };
                    fields.push((name, mapped.to_string()));
                }

                i += 1;
            }

            if !fields.is_empty() {
                models.push(DbModel {
                    name: model_name,
                    fields,
                });
            }
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prisma_basic_model() {
        let content = r#"
model User {
  id        Int      @id @default(autoincrement())
  email     String   @unique
  name      String?
  age       Int
  score     Float
  active    Boolean
  createdAt DateTime @default(now())
  profile   Json?
}
"#;
        let mut models = Vec::new();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("prisma")).unwrap();
        std::fs::write(dir.path().join("prisma/schema.prisma"), content).unwrap();
        scan_prisma_schema(dir.path(), &mut models);

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "User");
        assert_eq!(models[0].fields.len(), 8);

        let field_map: std::collections::HashMap<&str, &str> = models[0]
            .fields
            .iter()
            .map(|(n, t)| (n.as_str(), t.as_str()))
            .collect();
        assert_eq!(field_map["id"], "int");
        assert_eq!(field_map["email"], "string");
        assert_eq!(field_map["score"], "float");
        assert_eq!(field_map["active"], "bool");
        assert_eq!(field_map["createdAt"], "datetime");
        assert_eq!(field_map["profile"], "json");
    }

    #[test]
    fn test_prisma_relation_as_fk() {
        let content = r#"
model Post {
  id       Int    @id
  title    String
  author   User   @relation(fields: [authorId], references: [id])
  authorId Int
}
"#;
        let mut models = Vec::new();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("prisma")).unwrap();
        std::fs::write(dir.path().join("prisma/schema.prisma"), content).unwrap();
        scan_prisma_schema(dir.path(), &mut models);

        assert_eq!(models.len(), 1);
        let field_map: std::collections::HashMap<&str, &str> = models[0]
            .fields
            .iter()
            .map(|(n, t)| (n.as_str(), t.as_str()))
            .collect();
        assert_eq!(field_map["author"], "FK");
    }

    #[test]
    fn test_prisma_multiple_models() {
        let content = r#"
model User {
  id   Int    @id
  name String
}

model Post {
  id    Int    @id
  title String
}
"#;
        let mut models = Vec::new();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("prisma")).unwrap();
        std::fs::write(dir.path().join("prisma/schema.prisma"), content).unwrap();
        scan_prisma_schema(dir.path(), &mut models);

        assert_eq!(models.len(), 2);
        assert_eq!(models[0].name, "User");
        assert_eq!(models[1].name, "Post");
    }

    #[test]
    fn test_prisma_skips_comments_and_annotations() {
        let content = r#"
model Item {
  // This is a comment
  id   Int    @id
  name String

  @@map("items")
  @@index([name])
}
"#;
        let mut models = Vec::new();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("prisma")).unwrap();
        std::fs::write(dir.path().join("prisma/schema.prisma"), content).unwrap();
        scan_prisma_schema(dir.path(), &mut models);

        assert_eq!(models[0].fields.len(), 2);
    }

    #[test]
    fn test_prisma_no_schema_file() {
        let mut models = Vec::new();
        let dir = tempfile::tempdir().unwrap();
        scan_prisma_schema(dir.path(), &mut models);
        assert!(models.is_empty());
    }
}
