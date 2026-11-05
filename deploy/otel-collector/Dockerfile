FROM otel/opentelemetry-collector-contrib:0.113.0 AS builder

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends curl ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /otelcol-contrib /otelcol
COPY deploy/otel-collector/config.yaml /etc/otelcol/config.yaml
EXPOSE 4317 4318 9464 13133
CMD ["/otelcol", "--config=/etc/otelcol/config.yaml"]