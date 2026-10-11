//! A small reader for Valve's text KeyValues format (`libraryfolders.vdf`,
//! `appmanifest_*.acf`): quoted keys, quoted values or `{ … }` blocks, `//`
//! comments, and `\\` / `\"` escapes.

#[derive(Debug, Clone, PartialEq)]
pub enum Vdf {
    Str(String),
    Obj(Vec<(String, Vdf)>),
}

impl Vdf {
    /// Child by key, ignoring case (Valve's files aren't consistent).
    pub fn get(&self, key: &str) -> Option<&Vdf> {
        match self {
            Vdf::Obj(items) => items
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v),
            Vdf::Str(_) => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Vdf::Str(s) => Some(s),
            Vdf::Obj(_) => None,
        }
    }

    pub fn entries(&self) -> &[(String, Vdf)] {
        match self {
            Vdf::Obj(items) => items,
            Vdf::Str(_) => &[],
        }
    }
}

enum Tok {
    Str(String),
    Open,
    Close,
}

fn tokens(src: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' => out.push(Tok::Open),
            '}' => out.push(Tok::Close),
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            '"' => {
                let mut s = String::new();
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some(other) => s.push(other),
                            None => break,
                        },
                        other => s.push(other),
                    }
                }
                out.push(Tok::Str(s));
            }
            c if c.is_whitespace() => {}
            c => {
                // Unquoted token (rare): read up to whitespace or a brace.
                let mut s = String::from(c);
                while let Some(&n) = chars.peek() {
                    if n.is_whitespace() || n == '{' || n == '}' || n == '"' {
                        break;
                    }
                    s.push(n);
                    chars.next();
                }
                out.push(Tok::Str(s));
            }
        }
    }
    out
}

fn parse_obj(toks: &[Tok], pos: &mut usize) -> Vec<(String, Vdf)> {
    let mut items = Vec::new();
    while *pos < toks.len() {
        match &toks[*pos] {
            Tok::Close => {
                *pos += 1;
                break;
            }
            Tok::Open => {
                *pos += 1; // stray brace: skip
            }
            Tok::Str(key) => {
                let key = key.clone();
                *pos += 1;
                match toks.get(*pos) {
                    Some(Tok::Str(v)) => {
                        items.push((key, Vdf::Str(v.clone())));
                        *pos += 1;
                    }
                    Some(Tok::Open) => {
                        *pos += 1;
                        let child = parse_obj(toks, pos);
                        items.push((key, Vdf::Obj(child)));
                    }
                    _ => break,
                }
            }
        }
    }
    items
}

/// Parse a whole file into its top-level object.
pub fn parse(src: &str) -> Vdf {
    let toks = tokens(src);
    let mut pos = 0;
    Vdf::Obj(parse_obj(&toks, &mut pos))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_library_folders() {
        let src = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"apps" { "228980" "1" }
	}
	// a comment
	"1"
	{
		"path"		"D:\\SteamLibrary"
		"apps"
		{
			"2240620"		"5021437614"
		}
	}
}"#;
        let v = parse(src);
        let libs = v.get("LibraryFolders").unwrap();
        assert_eq!(libs.entries().len(), 2);
        assert_eq!(
            libs.get("1").unwrap().get("path").unwrap().as_str(),
            Some("D:\\SteamLibrary")
        );
        assert!(libs
            .get("1")
            .unwrap()
            .get("apps")
            .unwrap()
            .get("2240620")
            .is_some());
    }

    #[test]
    fn handles_escapes_and_unterminated_input() {
        let v = parse(r#""a" { "b" "say \"hi\"" "c" { "#);
        assert_eq!(
            v.get("a").unwrap().get("b").unwrap().as_str(),
            Some("say \"hi\"")
        );
    }
}
