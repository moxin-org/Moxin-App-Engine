use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use mofa_monitoring::{
    AgentMetrics, CardinalityLimits, DashboardConfig, DashboardServer, PrometheusExportConfig,
    WorkflowMetrics,
};
use std::time::Duration;
use tower::ServiceExt;

#[tokio::test]
async fn metrics_route_returns_prometheus_payload_with_histograms() {
    let mut server = DashboardServer::new(DashboardConfig::new().with_require_auth(false))
        .with_prometheus_export_config(
            PrometheusExportConfig::default()
                .with_refresh_interval(Duration::from_millis(10))
                .with_cardinality(CardinalityLimits::default()),
        );

    server
        .collector()
        .update_agent(AgentMetrics {
            agent_id: "agent-alpha".to_string(),
            tasks_completed: 42,
            avg_task_duration_ms: 120.0,
            ..Default::default()
        })
        .await;

    server
        .collector()
        .update_workflow(WorkflowMetrics {
            workflow_id: "wf-a".to_string(),
            total_executions: 10,
            avg_execution_time_ms: 200.0,
            ..Default::default()
        })
        .await;

    let _ = server.collector().collect().await;
    let app = server.build_router();

    server
        .prometheus_exporter()
        .expect("prom exporter")
        .refresh_once()
        .await
        .expect("refresh payload");

    let response = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("request success");

    assert_eq!(response.status(), StatusCode::OK);

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert!(content_type.starts_with("text/plain"));

    let body = to_bytes(response.into_body(), 5 * 1024 * 1024)
        .await
        .expect("read body");
    let body_str = String::from_utf8(body.to_vec()).expect("utf8");

    assert!(body_str.contains("mofa_agent_tasks_total{agent_id=\"agent-alpha\"} 42"));
    assert!(body_str.contains("mofa_agent_execution_duration_seconds_bucket"));
    assert!(body_str.contains("mofa_agent_execution_duration_seconds_sum"));
    assert!(body_str.contains("mofa_agent_execution_duration_seconds_count"));
}

#[tokio::test]
async fn metrics_route_applies_cardinality_overflow_bucket() {
    let mut server = DashboardServer::new(DashboardConfig::new().with_require_auth(false))
        .with_prometheus_export_config(
            PrometheusExportConfig::default()
                .with_refresh_interval(Duration::from_millis(10))
                .with_cardinality(
                    CardinalityLimits::default()
                        .with_agent_id(1)
                        .with_workflow_id(100)
                        .with_plugin_or_tool(100)
                        .with_provider_model(50),
                ),
        );

    for idx in 0..3 {
        server
            .collector()
            .update_agent(AgentMetrics {
                agent_id: format!("agent-{idx}"),
                tasks_completed: (10 + idx) as u64,
                avg_task_duration_ms: 100.0,
                ..Default::default()
            })
            .await;
    }

    let _ = server.collector().collect().await;
    let app = server.build_router();

    server
        .prometheus_exporter()
        .expect("prom exporter")
        .refresh_once()
        .await
        .expect("refresh payload");

    let response = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("request success");

    let body = to_bytes(response.into_body(), 5 * 1024 * 1024)
        .await
        .expect("read body");
    let body_str = String::from_utf8(body.to_vec()).expect("utf8");

    assert!(body_str.contains("agent_id=\"__other__\""));
    assert!(body_str.contains("mofa_exporter_dropped_series_total{label=\"agent_id\"}"));
}
