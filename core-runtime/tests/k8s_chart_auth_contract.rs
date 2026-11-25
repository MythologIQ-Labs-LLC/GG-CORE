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
            body.lines().filter(|line| line.trim() == "- serve").count(),
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

const NETWORK: &str = include_str!("../../k8s/helm/gg-core/templates/networkpolicy.yaml");

#[test]
fn all_serve_pods_disable_kubernetes_api_token_automount() {
    for (name, body, expected) in [
        ("normal", NORMAL, 1usize),
        ("canary", CANARY, 2usize),
        ("bluegreen", BLUEGREEN, 2usize),
    ] {
        assert_eq!(
            body.lines()
                .filter(|line| line.trim() == "automountServiceAccountToken: false")
                .count(),
            expected,
            "{name}: all pod templates must opt out even for external ServiceAccounts"
        );
    }
}

#[test]
fn default_deny_networkpolicy_selects_all_release_pods_without_allow_rules() {
    assert!(VALUES
        .replace("\r\n", "\n")
        .contains("networkPolicy:\n  enabled: true\n  denyAll: true"));
    assert!(NETWORK.contains("if and .Values.networkPolicy.enabled .Values.networkPolicy.denyAll"));
    assert!(NETWORK.contains("apiVersion: networking.k8s.io/v1"));
    assert!(NETWORK.contains("kind: NetworkPolicy"));
    assert!(NETWORK.contains("include \"gg-core.selectorLabels\""));
    assert!(NORMAL.contains("include \"gg-core.selectorLabels\""));
    assert!(CANARY.contains("include \"gg-core.selectorLabels\""));
    assert!(BLUEGREEN.contains("include \"gg-core.selectorLabels\""));
    assert!(NETWORK.contains("- Ingress"));
    assert!(NETWORK.contains("- Egress"));
    assert!(NETWORK.contains("ingress: []"));
    assert!(NETWORK.contains("egress: []"));
    assert!(!NETWORK.contains("namespaceSelector:"));
    assert!(!NETWORK.contains("ipBlock:"));
    assert!(!NETWORK.contains("ports:"));
}
