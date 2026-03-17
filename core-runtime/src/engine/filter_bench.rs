use criterion::{black_box, criterion_group, criterion_main, Criterion};

// Need to create a fake module to avoid pulling in the whole engine
use regex::Regex;
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone, Default)]
pub struct FilterConfig {
    pub blocklist: Vec<String>,
    pub regex_patterns: Vec<String>,
    pub max_output_chars: usize,
    pub replacement: String,
}

pub struct OutputFilter {
    config: FilterConfig,
    compiled_patterns: Vec<Regex>,
    normalized_blocklist: Vec<String>,
}

impl OutputFilter {
    pub fn new(config: FilterConfig) -> Result<Self, String> {
        let compiled = config
            .regex_patterns
            .iter()
            .map(|p| Regex::new(p))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("invalid regex: {}", e))?;

        let normalized_blocklist = config
            .blocklist
            .iter()
            .map(|s| s.nfc().collect::<String>().to_lowercase())
            .collect();

        Ok(Self {
            config,
            compiled_patterns: compiled,
            normalized_blocklist,
        })
    }

    pub fn filter_new(&self, text: &str) -> Result<String, String> {
        let mut result = std::borrow::Cow::Borrowed(text);

        if !self.normalized_blocklist.is_empty() {
            let lower = if text.is_ascii() {
                text.to_lowercase()
            } else {
                let normalized: String = text.nfc().collect();
                normalized.to_lowercase()
            };

            for (i, normalized_blocked) in self.normalized_blocklist.iter().enumerate() {
                if lower.contains(normalized_blocked) {
                    let new_str = result.replace(&self.config.blocklist[i], &self.config.replacement);
                    result = std::borrow::Cow::Owned(new_str);
                }
            }
        }

        for pattern in &self.compiled_patterns {
            let replaced = pattern.replace_all(&result, &self.config.replacement);
            if matches!(replaced, std::borrow::Cow::Owned(_)) {
                result = std::borrow::Cow::Owned(replaced.into_owned());
            }
        }

        let mut string_result = result.into_owned();

        if self.config.max_output_chars > 0 && string_result.len() > self.config.max_output_chars {
            string_result.truncate(self.config.max_output_chars);
        }

        Ok(string_result)
    }
}

fn bench_filter(c: &mut Criterion) {
    let mut config = FilterConfig::default();
    config.blocklist = vec!["badword".to_string(), "offensive".to_string()];
    config.replacement = "[filtered]".to_string();
    config.regex_patterns = vec![r"\b(?:badword|offensive)\b".to_string()];
    let filter = OutputFilter::new(config).unwrap();

    let clean_text = "This is a clean text without any of those words.";
    let dirty_text = "This text contains a badword and some offensive things.";

    let mut group = c.benchmark_group("filter");
    group.bench_function("new_clean", |b| b.iter(|| filter.filter_new(black_box(clean_text))));
    group.bench_function("new_dirty", |b| b.iter(|| filter.filter_new(black_box(dirty_text))));
    group.finish();
}

criterion_group!(benches, bench_filter);
criterion_main!(benches);
