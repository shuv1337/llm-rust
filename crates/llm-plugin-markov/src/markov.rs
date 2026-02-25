//! Deterministic Markov-chain text generation.

use std::collections::HashMap;

fn tokenize(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|token| {
            token
                .trim_matches(|ch: char| ch.is_ascii_punctuation() && ch != '\'' && ch != '-')
                .to_string()
        })
        .filter(|token| !token.is_empty())
        .collect()
}

/// Generate deterministic Markov output for a given prompt.
pub fn generate_markov_text(input: &str, max_tokens: usize) -> String {
    let tokens = tokenize(input);

    if tokens.is_empty() {
        return "markov".to_string();
    }

    if tokens.len() == 1 {
        return tokens[0].clone();
    }

    let limit = max_tokens.clamp(1, 128);

    let mut chain: HashMap<&str, Vec<&str>> = HashMap::new();
    for pair in tokens.windows(2) {
        chain
            .entry(pair[0].as_str())
            .or_default()
            .push(pair[1].as_str());
    }

    let mut state = stable_seed(input);
    let start = (next_u64(&mut state) as usize) % tokens.len();
    let mut current = tokens[start].as_str();

    let mut output = Vec::with_capacity(limit);
    output.push(current.to_string());

    while output.len() < limit {
        let next_candidates = chain
            .get(current)
            .filter(|choices| !choices.is_empty())
            .cloned()
            .unwrap_or_else(|| {
                vec![tokens[(next_u64(&mut state) as usize) % tokens.len()].as_str()]
            });

        let next_index = (next_u64(&mut state) as usize) % next_candidates.len();
        current = next_candidates[next_index];
        output.push(current.to_string());
    }

    output.join(" ")
}

fn stable_seed(input: &str) -> u64 {
    // FNV-1a 64-bit
    let mut hash = 0xcbf29ce484222325u64;
    for byte in input.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn next_u64(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_for_same_input() {
        let prompt = "the quick brown fox jumps over the lazy dog";
        assert_eq!(
            generate_markov_text(prompt, 12),
            generate_markov_text(prompt, 12)
        );
    }

    #[test]
    fn enforces_token_limit() {
        let prompt = "one two three four five";
        let output = generate_markov_text(prompt, 5);
        assert_eq!(output.split_whitespace().count(), 5);
    }
}
