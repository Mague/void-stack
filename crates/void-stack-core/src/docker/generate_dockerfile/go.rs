//! Go Dockerfile generation (distroless runtime).

use std::path::Path;

pub(super) fn go_dockerfile(_path: &Path) -> String {
    r#"# ── Build stage ──
FROM golang:1.22-alpine AS builder

WORKDIR /app

COPY go.mod go.sum ./
RUN go mod download

COPY . .
RUN CGO_ENABLED=0 GOOS=linux go build -ldflags="-s -w" -o /app/server .

# ── Runtime stage ──
FROM gcr.io/distroless/static-debian12

WORKDIR /app
COPY --from=builder /app/server .

USER nonroot:nonroot

EXPOSE 8080

CMD ["/app/server"]
"#.to_string()
}
