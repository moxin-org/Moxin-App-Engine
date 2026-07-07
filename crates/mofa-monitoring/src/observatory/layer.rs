//! `tracing_subscriber::layer::Layer` implementation for Cognitive Observatory.

use super::types::{SpanField, SpanRecord};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

/// Internal per-span state stored in the tracing registry.
struct SpanState {
    span_id: String,
    parent_span_id: Option<String>,
    trace_id: String,
    target: String,
    name: String,
    start_time_us: u64,
    fields: Vec<SpanField>,
}

impl SpanState {
    fn new(id: &Id, attrs: &Attributes<'_>, parent_id: Option<String>, trace_id: String) -> Self {
        let start_time_us = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;

        let mut visitor = FieldVisitor::default();
        attrs.record(&mut visitor);

        Self {
            span_id: format!("{:016x}", id.into_u64()),
            parent_span_id: parent_id,
            trace_id,
            target: attrs.metadata().target().to_string(),
            name: attrs.metadata().name().to_string(),
            start_time_us,
            fields: visitor.fields,
        }
    }
}

/// The tracing Layer that captures spans and forwards them to Observatory.
pub struct ObservatoryLayer {
    tx: mpsc::Sender<SpanRecord>,
}

impl ObservatoryLayer {
    pub fn new(tx: mpsc::Sender<SpanRecord>) -> Self {
        Self { tx }
    }
}

impl<S> Layer<S> for ObservatoryLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        // Determine parent span and trace ID
        let (parent_span_id, trace_id) = ctx
            .span(id)
            .and_then(|span| span.parent())
            .and_then(|parent| {
                parent
                    .extensions()
                    .get::<SpanState>()
                    .map(|s| (Some(s.span_id.clone()), s.trace_id.clone()))
            })
            .unwrap_or_else(|| {
                // No parent → new trace; generate a trace ID from the span ID
                let trace_id = format!("{:032x}", rand_trace_id(id));
                (None, trace_id)
            });

        let state = SpanState::new(id, attrs, parent_span_id, trace_id);

        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(state);
        }
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id) {
            let mut extensions = span.extensions_mut();
            if let Some(state) = extensions.get_mut::<SpanState>() {
                let mut visitor = FieldVisitor::default();
                values.record(&mut visitor);
                state.fields.extend(visitor.fields);
            }
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        // Attach event fields to the current span, if any
        if let Some(span) = ctx.lookup_current() {
            let mut extensions = span.extensions_mut();
            if let Some(state) = extensions.get_mut::<SpanState>() {
                let mut visitor = FieldVisitor::default();
                event.record(&mut visitor);
                // Prefix event fields with "event." to distinguish from span fields
                for mut field in visitor.fields {
                    field.key = format!("event.{}", field.key);
                    state.fields.push(field);
                }
            }
        }
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let end_time_us = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;

        let record = ctx.span(&id).and_then(|span| {
            let extensions = span.extensions();
            extensions.get::<SpanState>().map(|state| {
                let latency_us = end_time_us.saturating_sub(state.start_time_us);

                // Extract well-known fields
                let tokens = state
                    .fields
                    .iter()
                    .find(|f| f.key == "tokens" || f.key == "event.tokens")
                    .and_then(|f| f.value.parse::<u64>().ok());
                let model = state
                    .fields
                    .iter()
                    .find(|f| f.key == "model" || f.key == "event.model")
                    .map(|f| f.value.clone());

                SpanRecord {
                    span_id: state.span_id.clone(),
                    parent_span_id: state.parent_span_id.clone(),
                    trace_id: state.trace_id.clone(),
                    target: state.target.clone(),
                    name: state.name.clone(),
                    start_time_us: state.start_time_us,
                    end_time_us,
                    latency_us,
                    fields: state.fields.clone(),
                    tokens,
                    model,
                }
            })
        });

        if let Some(record) = record {
            // Best-effort send — if buffer is full, drop the span rather than block
            let _ = self.tx.try_send(record);
        }
    }
}

/// Generates a pseudo-random u128 trace ID seeded from the span ID.
fn rand_trace_id(id: &Id) -> u128 {
    let v = id.into_u64() as u128;
    // Mix bits for better distribution
    let v = v ^ (v << 64);
    v.wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407)
}

/// Field visitor that collects all recorded fields into a Vec<SpanField>.
#[derive(Default)]
struct FieldVisitor {
    fields: Vec<SpanField>,
}

impl Visit for FieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.fields.push(SpanField {
            key: field.name().to_string(),
            value: format!("{:?}", value),
        });
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.fields.push(SpanField {
            key: field.name().to_string(),
            value: value.to_string(),
        });
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields.push(SpanField {
            key: field.name().to_string(),
            value: value.to_string(),
        });
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields.push(SpanField {
            key: field.name().to_string(),
            value: value.to_string(),
        });
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.fields.push(SpanField {
            key: field.name().to_string(),
            value: value.to_string(),
        });
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields.push(SpanField {
            key: field.name().to_string(),
            value: value.to_string(),
        });
    }
}
