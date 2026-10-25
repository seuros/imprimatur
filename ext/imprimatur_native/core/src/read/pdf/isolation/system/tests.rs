use super::*;

#[test]
fn resource_limits_fit_the_native_type_and_never_raise_existing_caps() {
    for (requested, soft, hard, expected) in [
        (0, 4096, 8192, 0),
        (1024, 4096, 8192, 1024),
        (8192, 4096, 8192, 4096),
        (8192, 8192, 4096, 4096),
        (u64::MAX, 4096, 8192, 4096),
        (u64::MAX, rlim_t::MAX, rlim_t::MAX, rlim_t::MAX),
    ] {
        assert_eq!(bounded_limit(requested, soft, hard).unwrap(), expected);
    }
}
