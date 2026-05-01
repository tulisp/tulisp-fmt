//! Token-stream lexer with trivia capture.
//!
//! Produces a flat `Vec<Token>` from source text. Comments and
//! line-break runs are emitted as their own tokens so the formatter
//! can place them back; structural whitespace (spaces / tabs) is
//! discarded since the renderer re-emits its own indentation.

/// A single token. `start..end` is the byte range in the source.
#[derive(Clone, Debug)]
pub struct Token {
    pub kind: TokenKind,
    pub start: usize,
    pub end: usize,
}

impl Token {
    /// The token's exact source text.
    pub fn text<'a>(&self, source: &'a str) -> &'a str {
        &source[self.start..self.end]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    LParen,    // (
    RParen,    // )
    Quote,     // '
    Backquote, // `
    Unquote,   // ,    — not followed by `@`
    Splice,    // ,@
    Sharpquote, // #'
    /// Symbol, number, string, character literal, etc. The kind is
    /// recovered by the parser (and ultimately the renderer) from the
    /// token text.
    Atom,
    /// Line comment, including the leading `;`s but not the trailing
    /// newline. The renderer counts the leading semicolons to apply
    /// indentation conventions (`;` / `;;` / `;;;`).
    Comment,
    /// Run of newlines. `count` is the number of `\n` characters; `1`
    /// means a normal line break, `>= 2` means at least one blank
    /// line between the surrounding tokens.
    LineBreak { count: u32 },
}

/// Tokenize `source`. Spaces and tabs are skipped silently;
/// everything else lands in the returned vector.
pub fn lex(source: &str) -> Vec<Token> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b' ' | b'\t' | b'\r' => {
                i += 1;
            }
            b'\n' => {
                let start = i;
                let mut count: u32 = 0;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\n' => {
                            count += 1;
                            i += 1;
                        }
                        b' ' | b'\t' | b'\r' => {
                            i += 1;
                        }
                        _ => break,
                    }
                }
                out.push(Token {
                    kind: TokenKind::LineBreak { count },
                    start,
                    end: i,
                });
            }
            b'(' => {
                out.push(Token { kind: TokenKind::LParen, start: i, end: i + 1 });
                i += 1;
            }
            b')' => {
                out.push(Token { kind: TokenKind::RParen, start: i, end: i + 1 });
                i += 1;
            }
            b'\'' => {
                out.push(Token { kind: TokenKind::Quote, start: i, end: i + 1 });
                i += 1;
            }
            b'`' => {
                out.push(Token { kind: TokenKind::Backquote, start: i, end: i + 1 });
                i += 1;
            }
            b',' => {
                let (kind, end) = if bytes.get(i + 1) == Some(&b'@') {
                    (TokenKind::Splice, i + 2)
                } else {
                    (TokenKind::Unquote, i + 1)
                };
                out.push(Token { kind, start: i, end });
                i = end;
            }
            b'#' if bytes.get(i + 1) == Some(&b'\'') => {
                out.push(Token { kind: TokenKind::Sharpquote, start: i, end: i + 2 });
                i += 2;
            }
            b';' => {
                let start = i;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                out.push(Token { kind: TokenKind::Comment, start, end: i });
            }
            b'"' => {
                let start = i;
                i += 1;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' if i + 1 < bytes.len() => {
                            // Skip the backslash and its escapee, including
                            // a backslash-newline that some sources use.
                            i += 2;
                        }
                        b'"' => {
                            i += 1;
                            break;
                        }
                        _ => i += 1,
                    }
                }
                out.push(Token { kind: TokenKind::Atom, start, end: i });
            }
            b'?' => {
                // `?X` character literal. The byte after `?` is part
                // of the atom even when it would normally be a token
                // delimiter (`?'`, `?(`, `? `, …), so `?` gets its
                // own arm rather than falling through to bare-atom
                // reading. A backslash opens an escape sequence
                // whose escapee plus any continuation (hex digits
                // for `?\xNN`, control letters for `?\C-x`, …) are
                // also consumed.
                let start = i;
                i += 1;
                if i < bytes.len() {
                    let next = bytes[i];
                    i += 1;
                    if next == b'\\' && i < bytes.len() {
                        i += 1;
                        while i < bytes.len() && !is_atom_terminator(bytes[i]) {
                            i += 1;
                        }
                    }
                }
                out.push(Token {
                    kind: TokenKind::Atom,
                    start,
                    end: i,
                });
            }
            _ => {
                // Bare atom: read until we hit a structural delimiter.
                let start = i;
                while i < bytes.len() && !is_atom_terminator(bytes[i]) {
                    i += 1;
                }
                if i == start {
                    // Defensive: an unrecognised single byte. Emit it
                    // as a one-char atom rather than infinite-looping.
                    i += 1;
                }
                out.push(Token { kind: TokenKind::Atom, start, end: i });
            }
        }
    }
    out
}

fn is_atom_terminator(b: u8) -> bool {
    matches!(
        b,
        b' ' | b'\t' | b'\r' | b'\n' | b'(' | b')' | b'\'' | b'`' | b',' | b'"' | b';'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<TokenKind> {
        lex(source).into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn empty() {
        assert!(lex("").is_empty());
    }

    #[test]
    fn simple_call() {
        assert_eq!(
            kinds("(foo 1 2)"),
            vec![
                TokenKind::LParen,
                TokenKind::Atom,
                TokenKind::Atom,
                TokenKind::Atom,
                TokenKind::RParen,
            ]
        );
    }

    #[test]
    fn line_breaks_and_blank_lines() {
        let toks = lex("(a)\n(b)\n\n(c)");
        let breaks: Vec<_> = toks
            .iter()
            .filter_map(|t| match t.kind {
                TokenKind::LineBreak { count } => Some(count),
                _ => None,
            })
            .collect();
        assert_eq!(breaks, vec![1, 2]);
    }

    #[test]
    fn comments_and_reader_macros() {
        let src = ";; top\n'(a ,b ,@c #'d)";
        let kinds = kinds(src);
        assert_eq!(
            kinds,
            vec![
                TokenKind::Comment,
                TokenKind::LineBreak { count: 1 },
                TokenKind::Quote,
                TokenKind::LParen,
                TokenKind::Atom,
                TokenKind::Unquote,
                TokenKind::Atom,
                TokenKind::Splice,
                TokenKind::Atom,
                TokenKind::Sharpquote,
                TokenKind::Atom,
                TokenKind::RParen,
            ]
        );
    }

    #[test]
    fn string_with_escapes() {
        let src = r#"(message "hi\nthere")"#;
        let toks = lex(src);
        // Three structural tokens + the string atom.
        let atoms: Vec<_> = toks
            .iter()
            .filter(|t| t.kind == TokenKind::Atom)
            .map(|t| t.text(src))
            .collect();
        assert_eq!(atoms, vec!["message", r#""hi\nthere""#]);
    }

    #[test]
    fn span_round_trip() {
        // Every token's span maps back to the source verbatim.
        let src = "(let ((x 1)) ;; bind\n  (+ x 2))\n";
        for t in lex(src) {
            assert!(t.end <= src.len());
            assert_eq!(&src[t.start..t.end], t.text(src));
        }
    }

    #[test]
    fn character_literals_with_structural_chars() {
        // `?'`, `?(`, `?,`, `?;`, `?"` — the byte after `?` is part of
        // the char literal even when it would otherwise be a token
        // delimiter. Previously these split into a bare `?` atom plus
        // a structural token (Quote, LParen, etc.) and tripped the
        // parser.
        for src in ["?'", "?(", "?)", "?,", "?;", r#"?""#] {
            let toks = lex(src);
            assert_eq!(toks.len(), 1, "{src:?} lexed to {} tokens", toks.len());
            assert_eq!(toks[0].kind, TokenKind::Atom);
            assert_eq!(toks[0].text(src), src);
        }
    }

    #[test]
    fn character_literal_escapes() {
        // `?\n`, `?\\`, `?\(`, `?\C-x` and friends — backslash opens
        // an escape sequence; the escapee plus any continuation
        // (control prefixes, hex digits) gets folded into the atom.
        for src in [r"?\n", r"?\\", r"?\(", r"?\C-x", r"?\xff"] {
            let toks = lex(src);
            assert_eq!(toks.len(), 1, "{src:?} lexed to {} tokens", toks.len());
            assert_eq!(toks[0].text(src), src);
        }
    }

    #[test]
    fn character_literal_stops_before_next_token() {
        // `?<space>` is the space character literal; the source it
        // sits in usually continues with another form afterwards.
        // The literal must end at the space without absorbing the
        // following atom.
        let toks = lex("? foo");
        let atoms: Vec<&str> = toks
            .iter()
            .filter(|t| t.kind == TokenKind::Atom)
            .map(|t| t.text("? foo"))
            .collect();
        assert_eq!(atoms, vec!["? ", "foo"]);

        // Same shape with a structural-char literal followed by code.
        let src = "(setq c ?( body)";
        let atoms: Vec<&str> = lex(src)
            .iter()
            .filter(|t| t.kind == TokenKind::Atom)
            .map(|t| t.text(src))
            .collect();
        assert_eq!(atoms, vec!["setq", "c", "?(", "body"]);
    }
}
