FROM lukemathwalker/cargo-chef:latest-rust-1.81-alpine AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-json recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-json recipe.json
COPY . .
RUN cargo build --release

# 运行阶段
FROM alpine:latest
RUN apk --no-cache add ca-certificates tzdata
RUN addgroup -S appgroup && adduser -S appuser -G appgroup
ENV TZ=Asia/Shanghai
WORKDIR /app
COPY --from=builder /app/target/release/hubp /app/hubp
# 通过 docker-compose 挂载 config.yaml 到 /app/config.yaml
RUN chown -R appuser:appgroup /app
USER appuser
EXPOSE 45000
ENTRYPOINT ["/app/hubp"]
