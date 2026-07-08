use super::*;
use crate::models::registry::ModelHandle;
use crate::models::smart_loader::SmartLoaderConfig;
use crate::models::smart_loader_types::LoadCallback;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use tempfile::NamedTempFile;

fn test_callback() -> LoadCallback {
    let counter = std::sync::Arc::new(AtomicU64::new(100));
    Box::new(move |_path| {
        let id = counter.fetch_add(1, Ordering::SeqCst);
        Ok(ModelHandle::new(id))
    })
}

fn create_test_model(size: usize) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(&vec![0u8; size]).unwrap();
    file.flush().unwrap();
    file
}

#[tokio::test]
async fn test_synergy_auto_detect_mode() {
    let loader = Arc::new(SmartLoader::new(
        SmartLoaderConfig::default(),
        test_callback(),
    ));
    let synergy = TierSynergy::new(loader.clone());

    let light = create_test_model(100);
    let quality = create_test_model(200);

    loader
        .register("light".into(), light.path().to_path_buf(), ModelTier::Light)
        .await
        .unwrap();
    loader
        .register(
            "quality".into(),
            quality.path().to_path_buf(),
            ModelTier::Quality,
        )
        .await
        .unwrap();

    synergy.register_tier("light", ModelTier::Light).await;
    synergy.register_tier("quality", ModelTier::Quality).await;

    assert_eq!(synergy.mode().await, SynergyMode::SpeculativeLightQuality);
}

#[tokio::test]
async fn test_synergy_request_quick_query() {
    let loader = Arc::new(SmartLoader::new(
        SmartLoaderConfig::default(),
        test_callback(),
    ));
    let synergy = TierSynergy::new(loader.clone());

    let light = create_test_model(100);
    loader
        .register("light".into(), light.path().to_path_buf(), ModelTier::Light)
        .await
        .unwrap();
    synergy.register_tier("light", ModelTier::Light).await;

    let result = synergy.request(LoadHint::QuickQuery).await.unwrap();
    assert_eq!(result.mode, SynergyMode::Single);
    assert!(result.draft_handle.is_none());
}

#[tokio::test]
async fn test_synergy_complex_task_speculative() {
    let loader = Arc::new(SmartLoader::new(
        SmartLoaderConfig::default(),
        test_callback(),
    ));
    let synergy = TierSynergy::new(loader.clone());

    let light = create_test_model(100);
    let quality = create_test_model(200);

    loader
        .register("light".into(), light.path().to_path_buf(), ModelTier::Light)
        .await
        .unwrap();
    loader
        .register(
            "quality".into(),
            quality.path().to_path_buf(),
            ModelTier::Quality,
        )
        .await
        .unwrap();

    synergy.register_tier("light", ModelTier::Light).await;
    synergy.register_tier("quality", ModelTier::Quality).await;

    let result = synergy.request(LoadHint::ComplexTask).await.unwrap();
    assert_eq!(result.mode, SynergyMode::SpeculativeLightQuality);
}

#[tokio::test]
async fn test_synergy_fallback_single_tier() {
    let loader = Arc::new(SmartLoader::new(
        SmartLoaderConfig::default(),
        test_callback(),
    ));
    let synergy = TierSynergy::new(loader.clone());

    let balanced = create_test_model(150);
    loader
        .register(
            "balanced".into(),
            balanced.path().to_path_buf(),
            ModelTier::Balanced,
        )
        .await
        .unwrap();
    synergy.register_tier("balanced", ModelTier::Balanced).await;

    assert_eq!(synergy.mode().await, SynergyMode::Single);

    let result = synergy.request(LoadHint::ComplexTask).await.unwrap();
    assert_eq!(result.mode, SynergyMode::Single);
}
