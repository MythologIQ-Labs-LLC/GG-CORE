//! TDD-Light tests for SIMD tokenizer.

use gg_core::engine::SimdTokenizer;

fn create_test_vocab() -> Vec<u8> {
    // Simple test vocabulary: each line is a token
    b"hello\nworld\nthe\na\n \n!\n".to_vec()
}

#[test]
fn simd_whitespace_matches_scalar() {
    let text = b"hello world\tthis\nis a test";
    // Positions: h=0..4, space=5, w=6..10, tab=11, t=12..15, newline=16, is=17..18, space=19, a=20, space=21
    let positions = SimdTokenizer::find_whitespace(text);

    // Expected: 5 (space), 11 (tab), 16 (newline), 19 (space), 21 (space)
    assert_eq!(positions, vec![5, 11, 16, 19, 21]);
}

#[test]
fn simd_whitespace_empty_input() {
    let text = b"";
    let positions = SimdTokenizer::find_whitespace(text);
    assert!(positions.is_empty());
}

#[test]
fn simd_whitespace_no_whitespace() {
    let text = b"helloworld";
    let positions = SimdTokenizer::find_whitespace(text);
    assert!(positions.is_empty());
}

#[test]
fn simd_whitespace_all_whitespace() {
    let text = b"   \t\n  ";
    let positions = SimdTokenizer::find_whitespace(text);
    assert_eq!(positions.len(), 7);
}

#[test]
fn simd_whitespace_large_input() {
    // Test with input larger than 32 bytes (SIMD chunk size)
    let text: Vec<u8> = (0..100).map(|i| if i % 10 == 0 { b' ' } else { b'a' }).collect();
    let positions = SimdTokenizer::find_whitespace(&text);

    // Should find spaces at 0, 10, 20, 30, 40, 50, 60, 70, 80, 90
    assert_eq!(positions.len(), 10);
    for (i, &pos) in positions.iter().enumerate() {
        assert_eq!(pos, i * 10);
    }
}

#[test]
fn simd_encode_empty() {
    let vocab = create_test_vocab();
    let tokenizer = SimdTokenizer::from_vocab(&vocab, 0, 0).unwrap();

    let tokens = tokenizer.encode("");
    assert!(tokens.is_empty());
}

#[test]
fn simd_encode_single_token() {
    let vocab = create_test_vocab();
    let tokenizer = SimdTokenizer::from_vocab(&vocab, 0, 0).unwrap();

    let tokens = tokenizer.encode("hello");
    assert_eq!(tokens, vec![0]); // "hello" is first token (id 0)
}

#[test]
fn simd_encode_multiple_tokens() {
    let vocab = create_test_vocab();
    let tokenizer = SimdTokenizer::from_vocab(&vocab, 0, 0).unwrap();

    // "hello world" should be: hello(0) + space(4) + world(1)
    let tokens = tokenizer.encode("hello world");
    assert_eq!(tokens, vec![0, 4, 1]);
}

#[test]
fn simd_encode_unknown_bytes() {
    let vocab = create_test_vocab();
    let tokenizer = SimdTokenizer::from_vocab(&vocab, 0, 0).unwrap();

    // 'x' is not in vocab, should fall back to byte value
    let tokens = tokenizer.encode("x");
    assert_eq!(tokens, vec![b'x' as u32]);
}

#[test]
fn simd_decode_valid() {
    let vocab = create_test_vocab();
    let tokenizer = SimdTokenizer::from_vocab(&vocab, 0, 0).unwrap();

    let text = tokenizer.decode(&[0, 4, 1]).unwrap();
    assert_eq!(text, "hello world");
}

#[test]
fn simd_decode_invalid_token() {
    let vocab = create_test_vocab();
    let tokenizer = SimdTokenizer::from_vocab(&vocab, 0, 0).unwrap();

    let result = tokenizer.decode(&[9999]);
    assert!(result.is_err());
}

#[test]
fn simd_vocab_size() {
    let vocab = create_test_vocab();
    let tokenizer = SimdTokenizer::from_vocab(&vocab, 0, 0).unwrap();

    // 6 non-empty lines in vocab
    assert_eq!(tokenizer.vocab_size(), 6);
}

#[test]
fn simd_special_tokens() {
    let vocab = create_test_vocab();
    let tokenizer = SimdTokenizer::from_vocab(&vocab, 100, 101).unwrap();

    assert_eq!(tokenizer.eos_token(), 100);
    assert_eq!(tokenizer.bos_token(), 101);
}

#[test]
fn simd_encode_roundtrip() {
    let vocab = create_test_vocab();
    let tokenizer = SimdTokenizer::from_vocab(&vocab, 0, 0).unwrap();

    let original = "hello world!";
    let tokens = tokenizer.encode(original);
    let decoded = tokenizer.decode(&tokens).unwrap();

    assert_eq!(decoded, original);
}
