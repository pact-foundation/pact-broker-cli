use opentelemetry::KeyValue;
use opentelemetry::global;
use opentelemetry::trace::TraceContextExt;
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::Protocol;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::{
    logs::SdkLoggerProvider, propagation::TraceContextPropagator, trace::SdkTracerProvider,
};
use std::sync::OnceLock;
use tracing::Level;
use tracing::info;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use tracing_subscriber::Layer;
use tracing_subscriber::Registry;
use tracing_subscriber::layer::SubscriberExt;

#[derive(Debug)]
pub struct OtelConfig {
    pub exporter: Option<Vec<String>>,
    pub endpoint: Option<String>,
    pub protocol: Option<String>,
    pub enable_otel: Option<bool>,
    pub enable_traces: Option<bool>,
    pub enable_logs: Option<bool>,
    pub log_level: Option<Level>,
}

pub struct TracerProviderDropper(pub opentelemetry_sdk::trace::SdkTracerProvider);

impl Drop for TracerProviderDropper {
    fn drop(&mut self) {
        match self.0.force_flush() {
            Ok(_) => (),
            Err(e) => eprintln!("Failed to flush OpenTelemetry tracing: {e}"),
        }
    }
}

fn get_resource() -> Resource {
    static RESOURCE: OnceLock<Resource> = OnceLock::new();
    RESOURCE
        .get_or_init(|| {
            Resource::builder()
                .with_service_name("pact-broker-cli")
                .with_attributes(vec![
                    KeyValue::new("service.name", env!("CARGO_CRATE_NAME")),
                    KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
                    KeyValue::new(
                        "service.instance.id",
                        std::env::var("HOSTNAME").unwrap_or_default(),
                    ),
                    KeyValue::new("service.auto.version", env!("CARGO_PKG_VERSION")),
                ])
                .build()
        })
        .clone()
}

pub fn init_logging(otel_config: OtelConfig) -> Option<SdkTracerProvider> {
    // If log_level is None, disable logs and tracing
    if otel_config.log_level.is_none() {
        info!("Log level not set, skipping logging and tracing initialization.");
        return None;
    }
    global::set_text_map_propagator(TraceContextPropagator::new());
    let resource = get_resource();

    let mut layers: Vec<Box<dyn Layer<Registry> + Send + Sync>> = Vec::new();

    // Stdout log output if log_level is set
    layers.push(
        tracing_subscriber::fmt::layer()
            .with_level(true)
            .with_filter(tracing_subscriber::filter::LevelFilter::from_level(
                otel_config.log_level.unwrap(),
            ))
            .boxed(),
    );

    let mut tracer_provider: Option<SdkTracerProvider> = None;

    // OTEL trace output
    if otel_config.enable_traces.unwrap_or(false) {
        let otlp_exporter = if let Some(exporters) = &otel_config.exporter {
            if exporters.iter().any(|e| e == "otlp") {
                let endpoint = otel_config
                    .endpoint
                    .unwrap_or_else(|| "http://localhost:4318".to_string());
                let protocol = otel_config.protocol.unwrap_or_else(|| "http".to_string());
                let exporter = match protocol.as_str() {
                    "grpc" => opentelemetry_otlp::SpanExporter::builder()
                        .with_tonic()
                        .with_endpoint(endpoint.to_string())
                        .build()
                        .expect("Failed to configure grpc exporter"),
                    _ => opentelemetry_otlp::SpanExporter::builder()
                        .with_http()
                        .with_protocol(Protocol::HttpBinary)
                        .build()
                        .expect("Failed to configure http exporter"),
                };
                Some(exporter)
            } else {
                None
            }
        } else {
            None
        };

        // Add OTLP exporter as batch if present
        tracer_provider = if let Some(exporters_list) = &otel_config.exporter {
            let mut builder = SdkTracerProvider::builder().with_resource(resource.clone());

            if let Some(exporter) = otlp_exporter {
                builder = builder.with_batch_exporter(exporter);
            }

            // Add stdout exporter as simple if "stdout" is in the exporters list
            if exporters_list
                .iter()
                .any(|e| e == "stdout" || e == "console")
            {
                println!("Adding stdout exporter for tracing");
                let stdout_exporter = opentelemetry_stdout::SpanExporter::default();
                builder = builder.with_simple_exporter(stdout_exporter);
            }

            Some(builder.build())
        } else {
            Some(
                SdkTracerProvider::builder()
                    .with_resource(resource.clone())
                    .build(),
            )
        };

        if let Some(ref provider) = tracer_provider {
            global::set_tracer_provider(provider.clone());
        }

        let tracer = global::tracer("pact-broker-cli");

        let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);
        layers.push(Box::new(telemetry));
    }

    // // OTEL log output
    if otel_config.enable_logs.unwrap_or(false) {
        let otel_log_stdout_exporter = opentelemetry_stdout::LogExporter::default();

        let otel_logger_provider = if otel_config.enable_logs.unwrap_or(false) {
            let otel_otlp_stdout_exporter = opentelemetry_otlp::LogExporter::builder()
                .with_http()
                .with_protocol(Protocol::HttpBinary)
                .build()
                .expect("Failed to create log exporter");
            SdkLoggerProvider::builder()
                .with_resource(get_resource())
                .with_simple_exporter(otel_log_stdout_exporter)
                .with_batch_exporter(otel_otlp_stdout_exporter)
                .build()
        } else {
            SdkLoggerProvider::builder()
                .with_resource(get_resource())
                .with_simple_exporter(otel_log_stdout_exporter)
                .build()
        };
        let otel_layer = OpenTelemetryTracingBridge::new(&otel_logger_provider);
        layers.push(Box::new(otel_layer));
    }
    // create a layered subscriber
    let subscriber = tracing_subscriber::registry().with(layers);

    if tracing::subscriber::set_global_default(subscriber).is_err() {
        info!(
            "Global tracing subscriber already set, attaching layers is not supported at runtime."
        );
    }
    tracer_provider
}

pub fn capture_telemetry(args: &[String], exit_code: i32, error_message: Option<&str>) {
    let span = tracing::Span::current();
    let _enter = span.enter();
    let span_context = span.context();
    let otel_span = span_context.span();

    if let Some(binary) = args.get(0) {
        otel_span.set_attribute(KeyValue::new("binary", binary.clone()));
    }
    if let Some(command) = args.get(1) {
        otel_span.set_attribute(KeyValue::new("command", command.clone()));
    }
    if let Some(subcommand) = args.get(2) {
        otel_span.set_attribute(KeyValue::new("subcommand", subcommand.clone()));
    }
    if args.len() > 3 {
        otel_span.set_attribute(KeyValue::new("args", format!("{:?}", &args[3..])));
    }
    otel_span.set_attribute(KeyValue::new("exit_code", exit_code.to_string()));
    if let Some(message) = error_message {
        otel_span.set_attribute(KeyValue::new("error_message", message.to_string()));
    }
}
