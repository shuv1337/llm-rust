use llm_plugin_markov::markov::generate_markov_text;

#[test]
fn golden_output_for_canonical_prompt() {
    let prompt = "the quick brown fox jumps over the lazy dog";
    let output = generate_markov_text(prompt, 12);
    assert_eq!(
        output,
        "quick brown fox jumps over the lazy dog the lazy dog the"
    );
}
