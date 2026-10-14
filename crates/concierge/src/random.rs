use rand::seq::IndexedRandom;

/// base64url's alphabet: nothing in it needs escaping in a URL or a cookie.
pub const URL_SAFE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// `length` characters drawn uniformly from `alphabet` by a CSPRNG.
pub fn string(alphabet: &[u8], length: usize) -> String {
    let mut rng = rand::rng();
    (0..length)
        .map(|_| char::from(*alphabet.choose(&mut rng).expect("the alphabet isn't empty")))
        .collect()
}

#[cfg(test)]
/// The bits of randomness in a [`string`] of that alphabet and length.
pub fn entropy_bits(alphabet: &[u8], length: usize) -> f64 {
    length as f64 * (alphabet.len() as f64).log2()
}
