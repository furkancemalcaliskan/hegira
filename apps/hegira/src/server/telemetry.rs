use infrastructure::config::AppConfig;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

pub struct TelemetryGuard {
    #[cfg(feature = "otel-otlp")]
    provider: Option<opentelemetry_sdk::trace::SdkTracerProvider>,
}

impl TelemetryGuard {
    pub fn shutdown(self) -> Result<(), String> {
        #[cfg(feature = "otel-otlp")]
        if let Some(provider) = self.provider {
            provider
                .shutdown()
                .map_err(|err| format!("failed to flush OpenTelemetry traces: {err}"))?;
        }
        Ok(())
    }
}

pub fn init(config: &AppConfig) -> Result<TelemetryGuard, String> {
    #[cfg(feature = "otel-otlp")]
    {
        init_with_optional_otlp(config)
    }

    #[cfg(not(feature = "otel-otlp"))]
    {
        if config.telemetry.enabled {
            return Err(
                "telemetry.enabled=true requires building with --features otel-otlp".to_string(),
            );
        }
        tracing_subscriber::registry()
            .with(EnvFilter::new(config.logging.filter.clone()))
            .with(tracing_subscriber::fmt::layer())
            .try_init()
            .map_err(|err| format!("failed to initialize tracing subscriber: {err}"))?;
        Ok(TelemetryGuard {})
    }
}

#[cfg(feature = "otel-otlp")]
fn init_with_optional_otlp(config: &AppConfig) -> Result<TelemetryGuard, String> {
    use infrastructure::config::OtlpProtocol;
    use opentelemetry::{KeyValue, global, trace::TracerProvider as _};
    use opentelemetry_otlp::{Protocol, WithExportConfig};
    use opentelemetry_sdk::{Resource, propagation::TraceContextPropagator, trace::Sampler};
    use std::time::Duration;

    if !config.telemetry.enabled {
        tracing_subscriber::registry()
            .with(EnvFilter::new(config.logging.filter.clone()))
            .with(tracing_subscriber::fmt::layer())
            .try_init()
            .map_err(|err| format!("failed to initialize tracing subscriber: {err}"))?;
        return Ok(TelemetryGuard { provider: None });
    }

    let timeout = Duration::from_millis(config.telemetry.timeout_milliseconds);
    let exporter = match config.telemetry.protocol {
        OtlpProtocol::Grpc => opentelemetry_otlp::SpanExporter::builder()
            .with_tonic()
            .with_endpoint(config.telemetry.endpoint.clone())
            .with_timeout(timeout)
            .build(),
        OtlpProtocol::HttpProtobuf => opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_protocol(Protocol::HttpBinary)
            .with_endpoint(config.telemetry.endpoint.clone())
            .with_timeout(timeout)
            .build(),
    }
    .map_err(|err| format!("failed to build OTLP span exporter: {err}"))?;

    let resource = Resource::builder()
        .with_service_name(config.application.name.clone())
        .with_attributes([
            KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
            KeyValue::new("deployment.environment.name", config.environment.clone()),
            KeyValue::new(
                "service.instance.role",
                format!("{:?}", config.runtime.role).to_lowercase(),
            ),
        ])
        .build();
    let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_sampler(Sampler::ParentBased(Box::new(Sampler::TraceIdRatioBased(
            config.telemetry.sample_ratio,
        ))))
        .with_resource(resource)
        .build();
    let tracer = provider.tracer(env!("CARGO_PKG_NAME"));

    tracing_subscriber::registry()
        .with(EnvFilter::new(config.logging.filter.clone()))
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .try_init()
        .map_err(|err| format!("failed to initialize tracing subscriber: {err}"))?;
    global::set_text_map_propagator(TraceContextPropagator::new());
    global::set_tracer_provider(provider.clone());

    Ok(TelemetryGuard {
        provider: Some(provider),
    })
}
