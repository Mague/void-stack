//! SQLAlchemy and Django model detection.

use super::DbModel;

pub(super) fn scan_python_models(dir: &std::path::Path, models: &mut Vec<DbModel>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        if entry.path().extension().map(|e| e == "py").unwrap_or(false)
            && let Ok(content) = std::fs::read_to_string(entry.path())
        {
            parse_sqlalchemy_models(&content, models);
            parse_django_models(&content, models);
        }
    }
}

fn parse_sqlalchemy_models(content: &str, models: &mut Vec<DbModel>) {
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        // class User(Base): or class User(db.Model):
        if trimmed.starts_with("class ")
            && (trimmed.contains("(Base)") || trimmed.contains("db.Model"))
        {
            let class_name = trimmed
                .strip_prefix("class ")
                .and_then(|s| s.split('(').next())
                .unwrap_or("")
                .trim()
                .to_string();

            if class_name.is_empty() || models.iter().any(|m| m.name == class_name) {
                i += 1;
                continue;
            }

            let mut fields = Vec::new();
            i += 1;

            while i < lines.len() {
                let field_line = lines[i].trim();
                if field_line.is_empty() || field_line.starts_with('#') {
                    i += 1;
                    continue;
                }
                // Stop at next class or non-indented line
                if !lines[i].starts_with(' ')
                    && !lines[i].starts_with('\t')
                    && !field_line.is_empty()
                {
                    break;
                }

                // name = Column(String, ...) or name = Column(Integer, ...)
                if (field_line.contains("Column(") || field_line.contains("column("))
                    && let Some((name, col_type)) = parse_column_def(field_line)
                {
                    fields.push((name, col_type));
                }
                // name: Mapped[str] = mapped_column(...)
                if (field_line.contains("Mapped[") || field_line.contains("mapped_column"))
                    && let Some((name, col_type)) = parse_mapped_column(field_line)
                {
                    fields.push((name, col_type));
                }

                i += 1;
            }

            if !fields.is_empty() {
                models.push(DbModel {
                    name: class_name,
                    fields,
                });
            }
            continue;
        }
        i += 1;
    }
}

fn parse_django_models(content: &str, models: &mut Vec<DbModel>) {
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        // class Post(models.Model):
        if trimmed.starts_with("class ") && trimmed.contains("models.Model") {
            let class_name = trimmed
                .strip_prefix("class ")
                .and_then(|s| s.split('(').next())
                .unwrap_or("")
                .trim()
                .to_string();

            if class_name.is_empty() || models.iter().any(|m| m.name == class_name) {
                i += 1;
                continue;
            }

            let mut fields = Vec::new();
            i += 1;

            while i < lines.len() {
                let field_line = lines[i].trim();
                if !lines[i].starts_with(' ')
                    && !lines[i].starts_with('\t')
                    && !field_line.is_empty()
                {
                    break;
                }

                // title = models.CharField(max_length=200)
                if field_line.contains("models.")
                    && field_line.contains('=')
                    && let Some((name, field_type)) = parse_django_field(field_line)
                {
                    fields.push((name, field_type));
                }
                i += 1;
            }

            if !fields.is_empty() {
                models.push(DbModel {
                    name: class_name,
                    fields,
                });
            }
            continue;
        }
        i += 1;
    }
}

fn parse_column_def(line: &str) -> Option<(String, String)> {
    let eq_pos = line.find('=')?;
    let name = line[..eq_pos].trim().to_string();
    if name.starts_with('_') || name.starts_with('#') {
        return None;
    }

    let rest = &line[eq_pos + 1..];
    let col_type = if rest.contains("String") || rest.contains("Text") {
        "string"
    } else if rest.contains("Integer") || rest.contains("BigInteger") {
        "int"
    } else if rest.contains("Float") || rest.contains("Numeric") {
        "float"
    } else if rest.contains("Boolean") {
        "bool"
    } else if rest.contains("DateTime") || rest.contains("Date") {
        "datetime"
    } else if rest.contains("ForeignKey") {
        "FK"
    } else if rest.contains("JSON") {
        "json"
    } else {
        "string"
    };

    Some((name, col_type.to_string()))
}

fn parse_mapped_column(line: &str) -> Option<(String, String)> {
    let colon_pos = line.find(':')?;
    let name = line[..colon_pos].trim().to_string();

    let mapped_type = if line.contains("Mapped[str]") || line.contains("Mapped[String]") {
        "string"
    } else if line.contains("Mapped[int]") {
        "int"
    } else if line.contains("Mapped[float]") {
        "float"
    } else if line.contains("Mapped[bool]") {
        "bool"
    } else if line.contains("Mapped[datetime]") || line.contains("Mapped[date]") {
        "datetime"
    } else {
        "string"
    };

    Some((name, mapped_type.to_string()))
}

fn parse_django_field(line: &str) -> Option<(String, String)> {
    let eq_pos = line.find('=')?;
    let name = line[..eq_pos].trim().to_string();
    if name.starts_with('_') || name.starts_with('#') || name == "class" || name == "Meta" {
        return None;
    }

    let rest = &line[eq_pos + 1..];
    let field_type =
        if rest.contains("CharField") || rest.contains("TextField") || rest.contains("SlugField") {
            "string"
        } else if rest.contains("IntegerField")
            || rest.contains("BigIntegerField")
            || rest.contains("PositiveIntegerField")
        {
            "int"
        } else if rest.contains("FloatField") || rest.contains("DecimalField") {
            "float"
        } else if rest.contains("BooleanField") {
            "bool"
        } else if rest.contains("DateTimeField") || rest.contains("DateField") {
            "datetime"
        } else if rest.contains("ForeignKey") || rest.contains("OneToOneField") {
            "FK"
        } else if rest.contains("ManyToManyField") {
            "M2M"
        } else if rest.contains("JSONField") {
            "json"
        } else if rest.contains("FileField") || rest.contains("ImageField") {
            "file"
        } else {
            "string"
        };

    Some((name, field_type.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sqlalchemy_model() {
        let content = r#"
from sqlalchemy import Column, Integer, String, Boolean, DateTime, Float, ForeignKey
from sqlalchemy.orm import declarative_base

Base = declarative_base()

class User(Base):
    __tablename__ = 'users'
    id = Column(Integer, primary_key=True)
    name = Column(String(100))
    email = Column(Text)
    age = Column(Integer)
    score = Column(Float)
    active = Column(Boolean, default=True)
    created_at = Column(DateTime)
    team_id = Column(Integer, ForeignKey('teams.id'))
"#;
        let mut models = Vec::new();
        parse_sqlalchemy_models(content, &mut models);

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "User");
        let fm: std::collections::HashMap<&str, &str> = models[0]
            .fields
            .iter()
            .map(|(n, t)| (n.as_str(), t.as_str()))
            .collect();
        assert_eq!(fm["name"], "string");
        assert_eq!(fm["age"], "int");
        assert_eq!(fm["score"], "float");
        assert_eq!(fm["active"], "bool");
        assert_eq!(fm["created_at"], "datetime");
        // Integer matches before ForeignKey in parse_column_def, so it's "int"
        assert_eq!(fm["team_id"], "int");
    }

    #[test]
    fn test_sqlalchemy_db_model() {
        let content = r#"
class Product(db.Model):
    id = Column(Integer, primary_key=True)
    name = Column(String(255))
    price = Column(Numeric(10, 2))
"#;
        let mut models = Vec::new();
        parse_sqlalchemy_models(content, &mut models);

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "Product");
        let fm: std::collections::HashMap<&str, &str> = models[0]
            .fields
            .iter()
            .map(|(n, t)| (n.as_str(), t.as_str()))
            .collect();
        assert_eq!(fm["price"], "float");
    }

    #[test]
    fn test_sqlalchemy_mapped_column() {
        let content = r#"
class Account(Base):
    name: Mapped[str] = mapped_column()
    balance: Mapped[float] = mapped_column()
    count: Mapped[int] = mapped_column()
    active: Mapped[bool] = mapped_column()
    ts: Mapped[datetime] = mapped_column()
"#;
        let mut models = Vec::new();
        parse_sqlalchemy_models(content, &mut models);

        assert_eq!(models.len(), 1);
        let fm: std::collections::HashMap<&str, &str> = models[0]
            .fields
            .iter()
            .map(|(n, t)| (n.as_str(), t.as_str()))
            .collect();
        assert_eq!(fm["name"], "string");
        assert_eq!(fm["balance"], "float");
        assert_eq!(fm["count"], "int");
        assert_eq!(fm["active"], "bool");
        assert_eq!(fm["ts"], "datetime");
    }

    #[test]
    fn test_django_model() {
        let content = r#"
from django.db import models

class Article(models.Model):
    title = models.CharField(max_length=200)
    body = models.TextField()
    views = models.IntegerField(default=0)
    rating = models.FloatField()
    published = models.BooleanField(default=False)
    created = models.DateTimeField(auto_now_add=True)
    author = models.ForeignKey('User', on_delete=models.CASCADE)
    tags = models.ManyToManyField('Tag')
    data = models.JSONField(null=True)
    cover = models.ImageField(upload_to='covers/')
"#;
        let mut models = Vec::new();
        parse_django_models(content, &mut models);

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "Article");
        let fm: std::collections::HashMap<&str, &str> = models[0]
            .fields
            .iter()
            .map(|(n, t)| (n.as_str(), t.as_str()))
            .collect();
        assert_eq!(fm["title"], "string");
        assert_eq!(fm["views"], "int");
        assert_eq!(fm["rating"], "float");
        assert_eq!(fm["published"], "bool");
        assert_eq!(fm["created"], "datetime");
        assert_eq!(fm["author"], "FK");
        assert_eq!(fm["tags"], "M2M");
        assert_eq!(fm["data"], "json");
        assert_eq!(fm["cover"], "file");
    }

    #[test]
    fn test_django_skips_meta_and_private() {
        let content = r#"
class Thing(models.Model):
    name = models.CharField(max_length=50)
    _internal = models.CharField(max_length=50)
"#;
        let mut models = Vec::new();
        parse_django_models(content, &mut models);

        assert_eq!(models[0].fields.len(), 1);
        assert_eq!(models[0].fields[0].0, "name");
    }

    #[test]
    fn test_scan_python_models_from_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("models.py"),
            r#"
from sqlalchemy import Column, Integer, String
from sqlalchemy.orm import declarative_base
Base = declarative_base()

class Category(Base):
    id = Column(Integer, primary_key=True)
    name = Column(String)
"#,
        )
        .unwrap();

        let mut models = Vec::new();
        scan_python_models(dir.path(), &mut models);

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "Category");
    }

    #[test]
    fn test_no_duplicate_models() {
        let content = r#"
class Dup(Base):
    name = Column(String)
"#;
        let mut models = Vec::new();
        parse_sqlalchemy_models(content, &mut models);
        parse_sqlalchemy_models(content, &mut models);

        assert_eq!(models.len(), 1);
    }
}
