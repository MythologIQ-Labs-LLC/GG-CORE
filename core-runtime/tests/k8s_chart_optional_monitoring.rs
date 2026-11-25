//! Source guards for opt-in Kubernetes monitoring and RBAC (#164).
//! Helm rendering and installed Kubernetes acceptance are separate gates.

const VALUES: &str = include_str!("../../k8s/helm/gg-core/values.yaml");
const RBAC: &str = include_str!("../../k8s/helm/gg-core/templates/rbac.yaml");
const GRAFANA: &str = include_str!("../../k8s/helm/gg-core/templates/grafana-dashboard.yaml");
const PROMETHEUS: &str = include_str!("../../k8s/helm/gg-core/templates/prometheus-rules.yaml");
const CANARY: &str = include_str!("../../k8s/helm/gg-core/templates/canary-deployment.yaml");
const BLUEGREEN: &str = include_str!("../../k8s/helm/gg-core/templates/bluegreen-service.yaml");

#[test]
fn unqualified_monitoring_is_disabled_by_default() {
    let values = VALUES.replace("\r\n", "\n");
    let monitor = values.split_once("monitoring:").expect("monitoring block").1;
    let grafana = monitor.split_once("grafana:").expect("grafana block").1;
    let grafana = grafana.split_once("prometheus:").expect("prometheus block").0;
    let prometheus = monitor.split_once("prometheus:").expect("prometheus block").1;
    let prometheus = prometheus.split_once("serviceMonitor:").expect("service monitor").0;
    assert!(grafana.contains("enabled: false"));
    assert!(prometheus.contains("enabled: false"));
    assert!(monitor.contains("serviceMonitor:\n    enabled: false"));
}

#[test]
fn monitoring_templates_use_defined_lowercase_helpers_when_opted_in() {
    for (name, body) in [("grafana", GRAFANA), ("prometheus", PROMETHEUS)] {
        assert!(
            !body.contains("include \"GG-CORE."),
            "{name}: undefined upper-case Helm helper"
        );
        assert!(body.contains("include \"gg-core."));
    }
}

#[test]
fn legacy_inference_pods_do_not_claim_an_http_metrics_scrape_port() {
    for (name, body) in [("canary", CANARY), ("bluegreen", BLUEGREEN)] {
        assert!(
            !body.contains("prometheus.io/scrape"),
            "{name}: IPC-only inference must not claim an HTTP metrics listener"
        );
    }
}

#[test]
fn inference_serviceaccount_does_not_receive_default_api_read_permissions() {
    let values = VALUES.replace("\r\n", "\n");
    assert!(values.contains("rbac:\n  create: false"));
    assert!(RBAC.starts_with("{{- if .Values.rbac.create -}}"));
    assert!(!RBAC.starts_with("{{- if .Values.serviceAccount.create -}}"));
}
