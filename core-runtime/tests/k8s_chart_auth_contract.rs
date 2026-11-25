//! Static chart/IPC source guards (#164). Helm rendering and installed
//! Kubernetes security/real-model acceptance are separate requirements.

const HELPERS: &str = include_str!("../../k8s/helm/gg-core/templates/_helpers.tpl");
const VALUES: &str = include_str!("../../k8s/helm/gg-core/values.yaml");
const NORMAL: &str = include_str!("../../k8s/helm/gg-core/templates/deployment.yaml");
const CANARY: &str = include_str!("../../k8s/helm/gg-core/templates/canary-deployment.yaml");
const BLUEGREEN: &str = include_str!("../../k8s/helm/gg-core/templates/bluegreen-service.yaml");
const GUIDE: &str = include_str!("../../docs/operations/QUICKSTART.md");

#[test]
fn all_serve_containers_reference_existing_secret_without_plaintext() {
    assert!(HELPERS.contains("define \"gg-core.authTokenEnv\""));
    assert!(HELPERS.contains("name: CORE_AUTH_TOKEN"));
    assert!(HELPERS.contains("valueFrom:"));
    assert!(HELPERS.contains("secretKeyRef:"));
    assert!(HELPERS.contains("optional: false"));
    assert!(!HELPERS.contains("value: {{ .Values.auth"));
    assert!(VALUES.contains("existingSecretName: \"\""));
    assert!(VALUES.contains("secretKey: token"));
    for (name, body, expected) in [
        ("normal", NORMAL, 1usize),
        ("canary", CANARY, 2usize),
        ("bluegreen", BLUEGREEN, 2usize),
    ] {
        assert_eq!(
            body.matches("args:\n            - serve").count(),
            expected,
            "{name}: unexpected number of serve containers"
        );
        assert_eq!(
            body.matches("include \"gg-core.authTokenEnv\"").count(),
            expected,
            "{name}: a serve container lacks the auth Secret reference"
        );
    }
}

#[test]
fn operator_guide_disavows_unverified_http_and_fake_registries() {
    for obsolete in [
        "helm repo add",
        "https://charts.GG-CORE.io",
        "http://localhost:8080",
        "curl -X POST",
        "GG-CORE-verify",
        "support@GG-CORE.io",
    ] {
        assert!(!GUIDE.contains(obsolete), "obsolete command: {obsolete}");
    }
    assert!(GUIDE.contains("qualification pending"));
    assert!(GUIDE.contains("auth.existingSecretName"));
    assert!(GUIDE.contains("socketVolumeInit.enabled"));
    assert!(GUIDE.contains("release gate remains **HOLD**"));
}
