use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Debug, Clone)]
pub struct TelemetrySettings {
    pub service_name: String,
    pub service_version: &'static str,
    pub environment: String,
    pub role: String,
    pub logging_filter: String,
    pub exporter: Option<OtlpExporterSettings>,
}

#[derive(Debug, Clone)]
pub struct OtlpExporterSettings {
    pub protocol: OtlpProtocol,
    pub endpoint: String,
    pub timeout: std::time::Duration,
    pub sample_ratio: f64,
}

#[derive(Debug, Clone, Copy)]
pub enum OtlpProtocol {
    Grpc,
    HttpProtobuf,
}

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

pub fn init(settings: &TelemetrySettings) -> Result<TelemetryGuard, String> {
    #[cfg(feature = "otel-otlp")]
    {
        init_with_optional_otlp(settings)
    }

    #[cfg(not(feature = "otel-otlp"))]
    {
        if settings.exporter.is_some() {
            return Err(
                "telemetry.enabled=true requires building with --features otel-otlp".to_string(),
            );
        }
        tracing_subscriber::registry()
            .with(EnvFilter::new(settings.logging_filter.clone()))
            .with(tracing_subscriber::fmt::layer())
            .try_init()
            .map_err(|err| format!("failed to initialize tracing subscriber: {err}"))?;
        Ok(TelemetryGuard {})
    }
}

#[cfg(feature = "otel-otlp")]
fn init_with_optional_otlp(settings: &TelemetrySettings) -> Result<TelemetryGuard, String> {
    use opentelemetry::{KeyValue, global, trace::TracerProvider as _};
    use opentelemetry_otlp::{Protocol, WithExportConfig};
    use opentelemetry_sdk::{Resource, propagation::TraceContextPropagator, trace::Sampler};
    let Some(exporter_settings) = &settings.exporter else {
        tracing_subscriber::registry()
            .with(EnvFilter::new(settings.logging_filter.clone()))
            .with(tracing_subscriber::fmt::layer())
            .try_init()
            .map_err(|err| format!("failed to initialize tracing subscriber: {err}"))?;
        return Ok(TelemetryGuard { provider: None });
    };

    let exporter = match exporter_settings.protocol {
        OtlpProtocol::Grpc => opentelemetry_otlp::SpanExporter::builder()
            .with_tonic()
            .with_endpoint(exporter_settings.endpoint.clone())
            .with_timeout(exporter_settings.timeout)
            .build(),
        OtlpProtocol::HttpProtobuf => opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_protocol(Protocol::HttpBinary)
            .with_endpoint(exporter_settings.endpoint.clone())
            .with_timeout(exporter_settings.timeout)
            .build(),
    }
    .map_err(|err| format!("failed to build OTLP span exporter: {err}"))?;

    let resource = Resource::builder()
        .with_service_name(settings.service_name.clone())
        .with_attributes([
            KeyValue::new("service.version", settings.service_version),
            KeyValue::new("deployment.environment.name", settings.environment.clone()),
            KeyValue::new("service.instance.role", settings.role.clone()),
        ])
        .build();
    let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_sampler(Sampler::ParentBased(Box::new(Sampler::TraceIdRatioBased(
            exporter_settings.sample_ratio,
        ))))
        .with_resource(resource)
        .build();
    let tracer = provider.tracer(settings.service_name.clone());

    tracing_subscriber::registry()
        .with(EnvFilter::new(settings.logging_filter.clone()))
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
