//! Tests to verify benchmark fixture files parse correctly.

use std::fs;
use std::path::Path;

#[derive(Debug, serde::Deserialize)]
struct FixturePrompt {
    model_id: String,
    prompt_tokens: Vec<u32>,
    parameters: FixtureParams,
}

#[derive(Debug, serde::Deserialize)]
struct FixtureParams {
    max_tokens: u64,
    temperature: f64,
}

fn load_and_validate_fixture(name: &str, expected_token_count: usize) -> FixturePrompt {
    let path = format!("fixtures/prompts/{}.json", name);
    assert!(Path::new(&path).exists(), "Fixture file not found: {}", path);

    let content = fs::read_to_string(&path).expect("Failed to read fixture");
    let fixture: FixturePrompt = serde_json::from_str(&content).expect("Invalid JSON");

    assert_eq!(fixture.model_id, "test-model", "Unexpected model_id");
    assert_eq!(
        fixture.prompt_tokens.len(),
        expected_token_count,
        "Token count mismatch for {}: expected {}, got {}",
        name,
        expected_token_count,
        fixture.prompt_tokens.len()
    );
    assert!(fixture.parameters.max_tokens > 0, "max_tokens must be positive");
    assert!(fixture.parameters.temperature >= 0.0, "temperature must be non-negative");

    fixture
}

#[test]
fn test_small_fixture_parses() {
    let fixture = load_and_validate_fixture("small", 100);
    assert_eq!(fixture.prompt_tokens.len(), 100);
}

#[test]
fn test_medium_fixture_parses() {
    let fixture = load_and_validate_fixture("medium", 1000);
    assert_eq!(fixture.prompt_tokens.len(), 1000);
}

#[test]
fn test_large_fixture_parses() {
    let fixture = load_and_validate_fixture("large", 4000);
    assert_eq!(fixture.prompt_tokens.len(), 4000);
}

#[test]
fn test_all_fixtures_have_valid_token_sequences() {
    for (name, count) in [("small", 100), ("medium", 1000), ("large", 4000)] {
        let fixture = load_and_validate_fixture(name, count);
        // Verify tokens are sequential (1..=N as generated)
        for (i, &token) in fixture.prompt_tokens.iter().enumerate() {
            assert_eq!(
                token,
                (i + 1) as u32,
                "Token sequence mismatch at index {} in {}: expected {}, got {}",
                i,
                name,
                i + 1,
                token
            );
        }
    }
}

#[test]
fn test_fixtures_have_consistent_parameters() {
    for name in ["small", "medium", "large"] {
        let path = format!("fixtures/prompts/{}.json", name);
        let content = fs::read_to_string(&path).expect("Failed to read fixture");
        let fixture: FixturePrompt = serde_json::from_str(&content).expect("Invalid JSON");

        assert_eq!(fixture.parameters.max_tokens, 256, "max_tokens should be 256");
        assert!(
            (fixture.parameters.temperature - 0.7).abs() < 0.001,
            "temperature should be 0.7"
        );
    }
}
