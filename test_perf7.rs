use std::time::Instant;
use regex::Regex;
use unicode_normalization::UnicodeNormalization;

fn main() {
    let clean_text = "This is a clean text without any of those words.";
    let dirty_text = "This text contains a badword and some offensive things.";
    let blocklist = vec!["badword".to_string(), "offensive".to_string()];
    let normalized_blocklist: Vec<String> = blocklist.iter().map(|s| s.nfc().collect::<String>().to_lowercase()).collect();
    let compiled_patterns = vec![Regex::new(r"\b(?:badword|offensive)\b").unwrap()];
    let replacement = "[filtered]".to_string();

    let iters = 100000;

    let start = Instant::now();
    for _ in 0..iters {
        let mut result = clean_text.to_string();
        let normalized: String = result.nfc().collect();
        let lower = normalized.to_lowercase();

        for (i, normalized_blocked) in normalized_blocklist.iter().enumerate() {
            if lower.contains(normalized_blocked) {
                result = result.replace(&blocklist[i], &replacement);
            }
        }

        for pattern in &compiled_patterns {
            result = pattern
                .replace_all(&result, &replacement)
                .to_string();
        }
    }
    println!("Old (Clean): {:?}", start.elapsed());

    let start = Instant::now();
    for _ in 0..iters {
        let mut result = std::borrow::Cow::Borrowed(clean_text);

        if !normalized_blocklist.is_empty() {
            let lower = if clean_text.is_ascii() {
                clean_text.to_lowercase()
            } else {
                let normalized: String = clean_text.nfc().collect();
                normalized.to_lowercase()
            };

            for (i, normalized_blocked) in normalized_blocklist.iter().enumerate() {
                if lower.contains(normalized_blocked) {
                    let new_str = result.replace(&blocklist[i], &replacement);
                    result = std::borrow::Cow::Owned(new_str);
                }
            }
        }

        for pattern in &compiled_patterns {
            let replaced = pattern.replace_all(&result, &replacement);
            if matches!(replaced, std::borrow::Cow::Owned(_)) {
                result = std::borrow::Cow::Owned(replaced.into_owned());
            }
        }

        let mut _string_result = result.into_owned();
    }
    println!("New (Clean): {:?}", start.elapsed());

    let start = Instant::now();
    for _ in 0..iters {
        let mut result = dirty_text.to_string();
        let normalized: String = result.nfc().collect();
        let lower = normalized.to_lowercase();

        for (i, normalized_blocked) in normalized_blocklist.iter().enumerate() {
            if lower.contains(normalized_blocked) {
                result = result.replace(&blocklist[i], &replacement);
            }
        }

        for pattern in &compiled_patterns {
            result = pattern
                .replace_all(&result, &replacement)
                .to_string();
        }
    }
    println!("Old (Dirty): {:?}", start.elapsed());

    let start = Instant::now();
    for _ in 0..iters {
        let mut result = std::borrow::Cow::Borrowed(dirty_text);

        if !normalized_blocklist.is_empty() {
            let lower = if dirty_text.is_ascii() {
                dirty_text.to_lowercase()
            } else {
                let normalized: String = dirty_text.nfc().collect();
                normalized.to_lowercase()
            };

            for (i, normalized_blocked) in normalized_blocklist.iter().enumerate() {
                if lower.contains(normalized_blocked) {
                    let new_str = result.replace(&blocklist[i], &replacement);
                    result = std::borrow::Cow::Owned(new_str);
                }
            }
        }

        for pattern in &compiled_patterns {
            let replaced = pattern.replace_all(&result, &replacement);
            if matches!(replaced, std::borrow::Cow::Owned(_)) {
                result = std::borrow::Cow::Owned(replaced.into_owned());
            }
        }

        let mut _string_result = result.into_owned();
    }
    println!("New (Dirty): {:?}", start.elapsed());
}
