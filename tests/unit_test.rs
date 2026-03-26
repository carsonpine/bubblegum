#[cfg(test)]
mod tests {
    use bubblegum::idl::compute_discriminator;

    #[test]
    fn test_discriminator_consistency() {
        let disc1 = compute_discriminator("initialize");
        let disc2 = compute_discriminator("initialize");
        assert_eq!(disc1, disc2);
    }

    #[test]
    fn test_discriminator_uniqueness() {
        let names = ["swap", "deposit", "withdraw", "claim", "stake"];
        let mut discriminators = Vec::new();

        for name in &names {
            let disc = compute_discriminator(name);
            assert!(
                !discriminators.contains(&disc),
                "Duplicate discriminator found for {}",
                name
            );
            discriminators.push(disc);
        }
    }

    #[test]
    fn test_discriminator_byte_distribution() {
        let names = ["a", "ab", "abc", "abcd", "initialize", "swap", "deposit"];
        let discriminators: Vec<[u8; 8]> = names.iter().map(|n| compute_discriminator(n)).collect();

        for i in 0..discriminators.len() {
            for j in (i + 1)..discriminators.len() {
                assert_ne!(discriminators[i], discriminators[j]);
            }
        }
    }

    #[test]
    fn test_discriminator_different_lengths() {
        let short = compute_discriminator("a");
        let long = compute_discriminator("initialize_account");
        assert_ne!(short, long);
    }

    #[test]
    fn test_discriminator_empty_string() {
        let empty = compute_discriminator("");
        let non_empty = compute_discriminator("swap");
        assert_ne!(empty, non_empty);
    }

    #[test]
    fn test_discriminator_case_sensitive() {
        let lower = compute_discriminator("swap");
        let upper = compute_discriminator("SWAP");
        assert_ne!(lower, upper);
    }

    #[test]
    fn test_discriminator_special_chars() {
        let normal = compute_discriminator("swap");
        let with_underscore = compute_discriminator("swap_now");
        let with_space = compute_discriminator("swap now");

        assert_ne!(normal, with_underscore);
        assert_ne!(normal, with_space);
        assert_ne!(with_underscore, with_space);
    }
}
