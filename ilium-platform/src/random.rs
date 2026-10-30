//! Operating-system randomness for key material.

/// Fills a 32-byte seed from the operating system's random source.
pub fn random_seed() -> std::io::Result<[u8; 32]> {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|error| std::io::Error::other(error.to_string()))?;
    Ok(seed)
}
