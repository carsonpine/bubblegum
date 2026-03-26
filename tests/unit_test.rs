#[cfg(test)]
mod tests {
    use crate::decoder::BorshReader;
    use crate::idl::compute_discriminator;

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
            assert!(!discriminators.contains(&disc), "Duplicate discriminator found for {}", name);
            discriminators.push(disc);
        }
    }

    #[test]
    fn test_borsh_reader_basic_types() {
        let data = vec![255u8];
        let mut reader = BorshReader::new(&data);
        assert_eq!(reader.read_u8().unwrap(), 255);

        let data = vec![0xFFu8];
        let mut reader = BorshReader::new(&data);
        assert_eq!(reader.read_i8().unwrap(), -1);
    }

    #[test]
    fn test_borsh_reader_endianness() {
        let data = vec![0x02, 0x01];
        let mut reader = BorshReader::new(&data);
        assert_eq!(reader.read_u16().unwrap(), 514);

        let data = vec![0x04, 0x03, 0x02, 0x01];
        let mut reader = BorshReader::new(&data);
        assert_eq!(reader.read_u32().unwrap(), 0x01020304);
    }

    #[test]
    fn test_borsh_reader_string_various_lengths() {
        let data = vec![0x00, 0x00, 0x00, 0x00];
        let mut reader = BorshReader::new(&data);
        assert_eq!(reader.read_string().unwrap(), "");

        let long_string = "This is a longer test string with various characters!@#$%";
        let mut data = (long_string.len() as u32).to_le_bytes().to_vec();
        data.extend_from_slice(long_string.as_bytes());
        
        let mut reader = BorshReader::new(&data);
        assert_eq!(reader.read_string().unwrap(), long_string);
    }

    #[test]
    fn test_borsh_reader_invalid_utf8() {
        let data = vec![0x02, 0x00, 0x00, 0x00, 0xFF, 0xFF];
        let mut reader = BorshReader::new(&data);
        let result = reader.read_string();
        assert!(result.is_err());
    }

    #[test]
    fn test_borsh_reader_remaining_bytes() {
        let data = vec![1, 2, 3, 4, 5];
        let mut reader = BorshReader::new(&data);
        
        assert_eq!(reader.remaining(), 5);
        reader.read_u8().unwrap();
        assert_eq!(reader.remaining(), 4);
        reader.read_u8().unwrap();
        assert_eq!(reader.remaining(), 3);
    }

    #[test]
    fn test_borsh_reader_f32_f64() {
        let value: f32 = 3.14159;
        let data = value.to_le_bytes().to_vec();
        let mut reader = BorshReader::new(&data);
        let read_value = reader.read_f32().unwrap();
        assert!((read_value - value).abs() < f32::EPSILON);

        let value: f64 = 2.718281828459045;
        let data = value.to_le_bytes().to_vec();
        let mut reader = BorshReader::new(&data);
        let read_value = reader.read_f64().unwrap();
        assert!((read_value - value).abs() < f64::EPSILON);
    }

    #[test]
    fn test_borsh_reader_pubkey_size() {
        let data = vec![0xFF; 32];
        let mut reader = BorshReader::new(&data);
        let pubkey = reader.read_bytes(32).unwrap();
        assert_eq!(pubkey.len(), 32);
        assert!(pubkey.iter().all(|&b| b == 0xFF));
    }

    #[test]
    fn test_borsh_reader_chained_reads() {
        let mut data = Vec::new();
        data.extend_from_slice(&42u8.to_le_bytes());
        data.extend_from_slice(&1000u16.to_le_bytes());
        data.extend_from_slice(&50000u32.to_le_bytes());
        
        let mut reader = BorshReader::new(&data);
        assert_eq!(reader.read_u8().unwrap(), 42);
        assert_eq!(reader.read_u16().unwrap(), 1000);
        assert_eq!(reader.read_u32().unwrap(), 50000);
    }

    #[test]
    fn test_discriminator_byte_distribution() {
        let names = vec!["a", "ab", "abc", "abcd", "initialize", "swap", "deposit"];
        let discriminators: Vec<[u8; 8]> = names.iter()
            .map(|n| compute_discriminator(n))
            .collect();

        for i in 0..discriminators.len() {
            for j in (i + 1)..discriminators.len() {
                assert_ne!(discriminators[i], discriminators[j]);
            }
        }
    }

    #[test]
    fn test_borsh_reader_max_values() {
        let data = vec![u8::MAX];
        let mut reader = BorshReader::new(&data);
        assert_eq!(reader.read_u8().unwrap(), u8::MAX);

        let data = u16::MAX.to_le_bytes().to_vec();
        let mut reader = BorshReader::new(&data);
        assert_eq!(reader.read_u16().unwrap(), u16::MAX);

        let data = u32::MAX.to_le_bytes().to_vec();
        let mut reader = BorshReader::new(&data);
        assert_eq!(reader.read_u32().unwrap(), u32::MAX);

        let data = u64::MAX.to_le_bytes().to_vec();
        let mut reader = BorshReader::new(&data);
        assert_eq!(reader.read_u64().unwrap(), u64::MAX);
    }

    #[test]
    fn test_borsh_reader_zero_values() {
        let data = vec![0u8; 16];
        let mut reader = BorshReader::new(&data);
        
        assert_eq!(reader.read_u8().unwrap(), 0);
        assert_eq!(reader.read_u16().unwrap(), 0);
        assert_eq!(reader.read_u32().unwrap(), 0);
        assert_eq!(reader.read_u64().unwrap(), 0);
    }

    #[test]
    fn test_borsh_reader_min_negative_values() {
        let data = vec![0x80];
        let mut reader = BorshReader::new(&data);
        assert_eq!(reader.read_i8().unwrap(), i8::MIN);

        let data = vec![0x00, 0x80];
        let mut reader = BorshReader::new(&data);
        assert_eq!(reader.read_i16().unwrap(), i16::MIN);
    }
}
