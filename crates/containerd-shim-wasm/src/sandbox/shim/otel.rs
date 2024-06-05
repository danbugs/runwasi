//! OpenTelemetry Configuration Module
//!
//! This module provides a configuration structure and associated methods to initialize
//! OpenTelemetry tracing with the OTLP exporter. The configuration can be set up via
//! the `OtelConfig` struct and its builder pattern.
//!
//! # Usage
//!
//! ```rust
//! use containerd-shim-wasm::sandbox::shim::otel::{OtelConfig, OTEL_EXPORTER_OTLP_ENDPOINT};
//!
//! fn main() -> anyhow::Result<()> {
//!     let otel_endpoint = std::env::var(OTEL_EXPORTER_OTLP_ENDPOINT).expect("OTEL_EXPORTER_OTLP_ENDPOINT not set");
//!     let otel_config = OtelConfig::builder()
//!         .otel_endpoint(otel_endpoint)
//!         .name("my-service".to_string())
//!         .build()?;
//!
//!     let _guard = otel_config.init()?;
//!
//!     // Your application code here
//!
//!     Ok(())
//! }
//! ```

use std::collections::HashMap;

use opentelemetry::global::{self, set_text_map_propagator};
use opentelemetry::trace::TraceError;
use opentelemetry_otlp::{
    SpanExporterBuilder, WithExportConfig, OTEL_EXPORTER_OTLP_PROTOCOL_DEFAULT,
};
pub use opentelemetry_otlp::{OTEL_EXPORTER_OTLP_ENDPOINT, OTEL_EXPORTER_OTLP_PROTOCOL};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::{runtime, trace as sdktrace};
use tracing::Span;
use tracing_opentelemetry::OpenTelemetrySpanExt as _;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::{EnvFilter, Registry};

const OTEL_EXPORTER_OTLP_PROTOCOL_HTTP_PROTOBUF: &str = "http/protobuf";
const OTEL_EXPORTER_OTLP_PROTOCOL_GRPC: &str = "grpc";

/// Configuration struct for OpenTelemetry setup.
pub struct Config {
    otel_endpoint: String,
    otel_protocol: String,
}

/// Initializes a new OpenTelemetry tracer with the OTLP exporter.
///
/// Returns a `Result` containing the initialized tracer or a `TraceError` if initialization fails.
///
/// https://github.com/open-telemetry/opentelemetry-specification/blob/main/specification/protocol/exporter.md#configuration-options
impl Config {
    /// Creates a new builder for `OtelConfig`.
    pub fn builder() -> ConfigBuilder {
        ConfigBuilder::default()
    }

    /// Initializes the tracer, sets up the telemetry and subscriber layers, and sets the global subscriber.
    pub fn init(&self) -> anyhow::Result<ShutdownGuard> {
        let tracer = self.init_tracer()?;
        let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);
        set_text_map_propagator(TraceContextPropagator::new());

        let filter = EnvFilter::try_new("info,h2=off")?;

        let subscriber = Registry::default().with(telemetry).with(filter);

        tracing::subscriber::set_global_default(subscriber)?;
        Ok(ShutdownGuard)
    }

    pub fn get_trace_context() -> anyhow::Result<String> {
        // propogate the context
        let mut injector: HashMap<String, String> = HashMap::new();
        global::get_text_map_propagator(|propagator| {
            // retrieve the context from `tracing`
            propagator.inject_context(&Span::current().context(), &mut injector);
        });
        Ok(serde_json::to_string(&injector)?)
    }

    pub fn set_trace_context(trace_context: &str) -> anyhow::Result<()> {
        let extractor: HashMap<String, String> = serde_json::from_str(trace_context)?;
        let context = global::get_text_map_propagator(|propagator| propagator.extract(&extractor));
        Span::current().set_parent(context);
        Ok(())
    }

    fn init_tracer_http_protobuf(&self) -> SpanExporterBuilder {
        opentelemetry_otlp::new_exporter()
            .http()
            .with_endpoint(&self.otel_endpoint)
            .into()
    }

    fn init_tracer_grpc(&self) -> SpanExporterBuilder {
        opentelemetry_otlp::new_exporter()
            .tonic()
            .with_endpoint(&self.otel_endpoint)
            .into()
    }

    fn init_tracer(&self) -> Result<opentelemetry_sdk::trace::Tracer, TraceError> {
        let exporter = match self.otel_protocol.as_str() {
            OTEL_EXPORTER_OTLP_PROTOCOL_HTTP_PROTOBUF => self.init_tracer_http_protobuf(),
            OTEL_EXPORTER_OTLP_PROTOCOL_GRPC => self.init_tracer_grpc(),
            _ => Err(TraceError::from(
                "Invalid OTEL_EXPORTER_OTLP_PROTOCOL value",
            ))?,
        };

        opentelemetry_otlp::new_pipeline()
            .tracing()
            .with_exporter(exporter)
            .with_trace_config(sdktrace::config())
            .install_batch(runtime::Tokio)
    }
}

/// Shutdown of the open telemetry services will automatically called when the OtelConfig instance goes out of scope.
#[must_use]
pub struct ShutdownGuard;

impl Drop for ShutdownGuard {
    fn drop(&mut self) {
        // Give tracer provider a chance to flush any pending traces.
        opentelemetry::global::shutdown_tracer_provider();
    }
}

#[derive(Default)]
pub struct ConfigBuilder {
    otel_endpoint: Option<String>,
    otel_protocol: Option<String>,
}

impl ConfigBuilder {
    /// Sets the OpenTelemetry endpoint.
    pub fn otel_endpoint(mut self, otel_endpoint: String) -> Self {
        self.otel_endpoint = Some(otel_endpoint);
        self
    }

    pub fn otel_protocol(mut self, otel_protocol: String) -> Self {
        self.otel_protocol = Some(otel_protocol);
        self
    }

    /// Builds the `OtelConfig` instance.
    pub fn build(self) -> Result<Config, &'static str> {
        let otel_endpoint = self.otel_endpoint.ok_or("otel_endpoint is required")?;
        let otel_protocol = self
            .otel_protocol
            .unwrap_or_else(|| OTEL_EXPORTER_OTLP_PROTOCOL_DEFAULT.to_owned());
        Ok(Config {
            otel_endpoint,
            otel_protocol,
        })
    }
}
