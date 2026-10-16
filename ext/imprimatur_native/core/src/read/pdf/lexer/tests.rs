use super::*;

fn tokens(data: &[u8]) -> Vec<Token<'_>> {
    let mut lexer = Lexer::new(data, 0);
    std::iter::from_fn(|| lexer.next()).collect()
}

#[test]
fn splits_tokens() {
    let data = b"<< /Type /Font /W [1 -2.5 .5 +3 4.] >> (a (b) \\) c) <48 65 6C6C6f> \
          % comment\n true R obj";
    assert_eq!(
        tokens(data),
        [
            Token::DictStart,
            Token::Name(b"Type"),
            Token::Name(b"Font"),
            Token::Name(b"W"),
            Token::ArrayStart,
            Token::Integer(1),
            Token::Real(-2.5),
            Token::Real(0.5),
            Token::Integer(3),
            Token::Real(4.0),
            Token::ArrayEnd,
            Token::DictEnd,
            Token::String(b"a (b) \\) c"),
            Token::Hex(b"48 65 6C6C6f"),
            Token::Keyword(b"true"),
            Token::Keyword(b"R"),
            Token::Keyword(b"obj"),
        ]
    );
    assert_eq!(
        tokens(b"--5 1.2.3"),
        [Token::Real(-5.0), Token::Keyword(b"1.2.3")],
        "lenient numbers"
    );
    assert_eq!(tokens(b") } x"), [Token::Keyword(b"x")], "stray closers are skipped");
}

#[test]
fn decodes_strings_and_names() {
    const LITERALS: &[(&[u8], &[u8])] = &[
        (b"plain", b"plain"),
        (b"a\\nb\\tc\\(d\\)\\\\", b"a\nb\tc(d)\\"),
        (b"\\101\\60\\0", b"A0\0"),
        (b"split\\\r\nline", b"splitline"),
        (b"cr\r\nlf", b"cr\nlf"),
    ];
    for &(raw, decoded) in LITERALS {
        assert_eq!(&*literal_bytes(raw), decoded, "{:?}", String::from_utf8_lossy(raw));
    }
    assert_eq!(hex_bytes(b"48 65 6c6c 6"), b"Hell`");
    assert_eq!(&*name_bytes(b"A#20B#zz"), b"A B#zz");
}
