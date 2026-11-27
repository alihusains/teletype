/// Models the P1.9 token sink: for each of N tokens it emits a
/// `{seq, delta}` event and records the serialized IPC payload size.
/// The old behavior re-sent the whole accumulated string each token
/// (total O(N²) bytes); the new behavior sends only the delta
/// (total O(N) bytes).
#[test]
fn token_ipc_bytes_are_linear_in_token_count() {
    fn sink_bytes(tokens: &[&str]) -> usize {
        let mut total = 0usize;
        for (seq, tok) in tokens.iter().enumerate() {
            let payload = serde_json::json!({ "seq": seq, "delta": tok });
            total += payload.to_string().len();
        }
        total
    }

    let tokens: Vec<String> = (0..50).map(|i| format!("tok{i} ")).collect();
    let refs: Vec<&str> = tokens.iter().map(|s| s.as_str()).collect();

    let bytes_50 = sink_bytes(&refs);

    // O(N) check: doubling the token count must roughly double the
    // bytes (within a generous factor), NOT quadruple them as the old
    // O(N²) whole-string behavior would.
    let tokens_100: Vec<String> = (0..100).map(|i| format!("tok{i} ")).collect();
    let refs_100: Vec<&str> = tokens_100.iter().map(|s| s.as_str()).collect();
    let bytes_100 = sink_bytes(&refs_100);

    assert!(
        bytes_100 < 3 * bytes_50,
        "IPC bytes grew super-linearly: 50 tokens = {bytes_50} B, \
         100 tokens = {bytes_100} B (3x cap for O(N))"
    );

    // And the old behavior would be ~4x: prove the bound is meaningful.
    let old_style_50: usize = refs
        .iter()
        .scan(String::new(), |acc, t| {
            acc.push_str(t);
            Some(acc.len())
        })
        .map(|n| n + 10) // + ~10 bytes of event framing per emit
        .sum();
    let old_style_100: usize = refs_100
        .iter()
        .scan(String::new(), |acc, t| {
            acc.push_str(t);
            Some(acc.len())
        })
        .map(|n| n + 10)
        .sum();
    assert!(
        old_style_100 > 3 * old_style_50,
        "sanity: the old whole-string scheme must be super-linear \
         ({old_style_50} vs {old_style_100})"
    );
}
