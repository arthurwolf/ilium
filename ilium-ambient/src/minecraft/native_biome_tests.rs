use super::*;

#[test]
fn guava_hash_long_uses_low_digest_bytes_as_little_endian_signed_long() {
    for (seed, expected) in [
        (0, 8_794_265_229_978_523_055),
        (1, -6_467_378_160_175_308_932),
        (-1, 6_759_447_113_877_070_610),
        (i64::MAX, 7_179_146_226_492_139_882),
        (i64::MIN, 6_374_347_445_474_471_398),
        (0x0123_4567_89ab_cdef, -2_535_419_513_661_138_008),
    ] {
        assert_eq!(hash_zoom_seed(seed), expected);
    }
}

#[test]
fn signed_and_negative_block_coordinates_select_one_of_eight_real_quarts() {
    // Independent source-derived Python arithmetic vectors; native JVM
    // execution and a saved-world tint-pixel oracle are still separate gates.
    for (seed, block, expected) in [
        (0, [0, 64, 0], [0, 15, 0]),
        (1, [3, 62, -7], [0, 15, -2]),
        (-1, [-17, -64, 31], [-5, -16, 7]),
        (0x0123_4567_89ab_cdef, [125, 77, -503], [31, 19, -127]),
        (i64::MIN, [-2, 319, -2], [-1, 79, -1]),
        (i64::MAX, [32767, 0, -32768], [8191, 0, -8193]),
    ] {
        assert_eq!(choose_quart(seed, block), Ok(expected));
    }
    assert_eq!(choose_quart(0, [i32::MIN, 0, 0]), Err(Error::Coordinate));
}

#[test]
fn seed_mixer_retains_all_sixty_four_bits_and_java_wraparound() {
    assert_ne!(
        next(0x0123_4567_89ab_cdef, i64::MAX),
        next(0x0123_4567_89ab_cdef, -1)
    );
    assert_eq!(next(0, 0), 0);
    assert_eq!(next(0, -1), -1);
}
