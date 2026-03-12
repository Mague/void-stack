//! Flutter Web Dockerfile generation.

use std::path::Path;

pub(super) fn flutter_web_dockerfile(_path: &Path) -> String {
    r#"# ── Build stage ──
FROM ghcr.io/cirruslabs/flutter:stable AS builder

WORKDIR /app
COPY . .

RUN flutter pub get
RUN flutter build web --release

# ── Runtime stage ──
FROM nginx:alpine

COPY --from=builder /app/build/web /usr/share/nginx/html

EXPOSE 80

CMD ["nginx", "-g", "daemon off;"]
"#.to_string()
}
