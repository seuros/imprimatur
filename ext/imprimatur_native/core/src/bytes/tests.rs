use super::*;

#[test]
fn reads_integers_and_rejects_truncation_or_overflow() {
    let data = [0, 1, 2, 3, 4];
    assert_eq!(u16_le(&data, 1), Some(0x0201));
    assert_eq!(u32_le(&data, 1), Some(0x0403_0201));
    for at in [data.len() - 1, data.len(), usize::MAX - 1, usize::MAX] {
        assert_eq!(u16_le(&data, at), None, "{at}");
        assert_eq!(u32_le(&data, at), None, "{at}");
    }
    assert_eq!(u16_le(&[], 0), None);
    assert_eq!(u32_le(&data, 2), None);
}
