use std::{
    env, fmt,
    iter::Peekable,
    path::{Path, PathBuf},
    str::Chars,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LexError {
    UnclosedSingleQuote,
    UnclosedDoubleQuote,
    UnclosedBrace,
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LexError::UnclosedSingleQuote => write!(f, "unexpected EOF while looking for matching `''"),
            LexError::UnclosedDoubleQuote => write!(f, "unexpected EOF while looking for matching `\"'"),
            LexError::UnclosedBrace => write!(f, "unexpected EOF while looking for matching `}}'"),
        }
    }
}

impl std::error::Error for LexError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenChar {
    Literal(char),
    GlobStar,     
    GlobQuestion, 
}


pub fn tokenize(input: &str, last_exit_code: i32) -> Result<Vec<String>, LexError> {
    let mut raw_tokens: Vec<Vec<TokenChar>> = Vec::new();
    let mut current: Vec<TokenChar> = Vec::new();
    let mut has_quotes = false;
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c.is_whitespace() {
            if !current.is_empty() || has_quotes {
                raw_tokens.push(std::mem::take(&mut current));
                has_quotes = false;
            }
            continue;
        }

        match c {
            '#' if current.is_empty() && !has_quotes => {
                break;
            }
            '\'' => {
                has_quotes = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(ch) => current.push(TokenChar::Literal(ch)),
                        None => return Err(LexError::UnclosedSingleQuote),
                    }
                }
            }
            '"' => {
                has_quotes = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.peek().copied() {
                            Some('"') | Some('\\') | Some('$') | Some('`') => {
                                let escaped = chars.next().unwrap();
                                current.push(TokenChar::Literal(escaped));
                            }
                            Some('\n') => {
                                chars.next();
                            }
                            _ => current.push(TokenChar::Literal('\\')),
                        },
                        Some('$') => {
                            let expanded = expand_variable(&mut chars, last_exit_code)?;
                            current.extend(expanded.chars().map(TokenChar::Literal));
                        }
                        Some(ch) => current.push(TokenChar::Literal(ch)),
                        None => return Err(LexError::UnclosedDoubleQuote),
                    }
                }
            }
            '\\' => {
                if let Some(escaped) = chars.next() {
                    current.push(TokenChar::Literal(escaped));
                }
            }
            '~' if current.is_empty() && !has_quotes => {
                let should_expand = match chars.peek().copied() {
                    None | Some('/') => true,
                    Some(next) if next.is_whitespace() => true,
                    _ => false,
                };

                if should_expand {
                    if let Ok(home) = env::var("HOME") {
                        if home.is_empty() {
                            has_quotes = true;
                        } else {
                            current.extend(home.chars().map(TokenChar::Literal));
                        }
                    } else {
                        current.push(TokenChar::Literal('~'));
                    }
                } else {
                    current.push(TokenChar::Literal('~'));
                }
            }
            '$' => {
                let expanded = expand_variable(&mut chars, last_exit_code)?;
                current.extend(expanded.chars().map(TokenChar::Literal));
            }
            '*' => current.push(TokenChar::GlobStar),
            '?' => current.push(TokenChar::GlobQuestion),
            _ => current.push(TokenChar::Literal(c)),
        }
    }

    if !current.is_empty() || has_quotes {
        raw_tokens.push(current);
    }

    Ok(expand_globs(raw_tokens))
}

fn expand_variable(
    chars: &mut Peekable<Chars<'_>>,
    last_exit_code: i32,
) -> Result<String, LexError> {
    match chars.peek().copied() {
        Some('?') => {
            chars.next();
            Ok(last_exit_code.to_string())
        }
        Some('$') => {
            chars.next();
            Ok(std::process::id().to_string())
        }
        Some('{') => {
            chars.next();
            let mut name = String::new();
            loop {
                match chars.next() {
                    Some('}') => break,
                    Some(ch) => name.push(ch),
                    None => return Err(LexError::UnclosedBrace),
                }
            }
            if name == "?" {
                Ok(last_exit_code.to_string())
            } else if name == "$" {
                Ok(std::process::id().to_string())
            } else {
                Ok(env::var(&name).unwrap_or_default())
            }
        }
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {
            let mut name = String::new();
            while let Some(&ch) = chars.peek() {
                if ch.is_ascii_alphanumeric() || ch == '_' {
                    name.push(ch);
                    chars.next();
                } else {
                    break;
                }
            }
            Ok(env::var(&name).unwrap_or_default())
        }
        _ => Ok("$".to_string()),
    }
}

fn token_has_glob(token: &[TokenChar]) -> bool {
    token
        .iter()
        .any(|tc| matches!(tc, TokenChar::GlobStar | TokenChar::GlobQuestion))
}

fn tokens_to_literal_string(token: &[TokenChar]) -> String {
    token
        .iter()
        .map(|tc| match tc {
            TokenChar::Literal(c) => *c,
            TokenChar::GlobStar => '*',
            TokenChar::GlobQuestion => '?',
        })
        .collect()
}

fn glob_match(pattern: &[TokenChar], text: &str) -> bool {
    let text_chars: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star_pi, mut star_ti) = (None, 0);

    while ti < text_chars.len() {
        if pi < pattern.len() {
            match pattern[pi] {
                TokenChar::GlobQuestion => {
                    pi += 1;
                    ti += 1;
                    continue;
                }
                TokenChar::Literal(c) if c == text_chars[ti] => {
                    pi += 1;
                    ti += 1;
                    continue;
                }
                TokenChar::GlobStar => {
                    star_pi = Some(pi);
                    star_ti = ti;
                    pi += 1;
                    continue;
                }
                _ => {}
            }
        }

        if let Some(sp) = star_pi {
            pi = sp + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }

    while pi < pattern.len() && pattern[pi] == TokenChar::GlobStar {
        pi += 1;
    }

    pi == pattern.len()
}

fn append_display_segment(base: &str, segment: &str) -> String {
    if base.is_empty() {
        segment.to_string()
    } else if base == "/" {
        format!("/{segment}")
    } else {
        format!("{base}/{segment}")
    }
}

fn expand_single_glob(token: &[TokenChar]) -> Vec<String> {
    let is_absolute = matches!(token.first(), Some(TokenChar::Literal('/')));
    let has_trailing_slash = matches!(token.last(), Some(TokenChar::Literal('/')));

    let components: Vec<&[TokenChar]> = token
        .split(|tc| *tc == TokenChar::Literal('/'))
        .filter(|comp| !comp.is_empty())
        .collect();

    if components.is_empty() {
        return vec![tokens_to_literal_string(token)];
    }

    let mut candidates: Vec<(PathBuf, String)> = if is_absolute {
        vec![(PathBuf::from("/"), "/".to_string())]
    } else {
        vec![(PathBuf::from("."), String::new())]
    };

    for (idx, comp) in components.iter().enumerate() {
        let is_last = idx + 1 == components.len();
        let needs_dir = !is_last || has_trailing_slash;
        let mut next_candidates = Vec::new();

        if token_has_glob(comp) {
            let match_hidden = matches!(comp.first(), Some(TokenChar::Literal('.')));

            for (fs_path, display) in candidates {
                let entries = match std::fs::read_dir(&fs_path) {
                    Ok(entries) => entries,
                    Err(_) => continue,
                };

                for entry in entries.flatten() {
                    let file_name = entry.file_name();
                    let Some(name) = file_name.to_str() else {
                        continue;
                    };

                    if !match_hidden && name.starts_with('.') {
                        continue;
                    }

                    if !glob_match(comp, name) {
                        continue;
                    }

                    let next_fs = fs_path.join(name);
                    if needs_dir && !next_fs.is_dir() {
                        continue;
                    }

                    let next_display = append_display_segment(&display, name);
                    next_candidates.push((next_fs, next_display));
                }
            }
        } else {
            let lit = tokens_to_literal_string(comp);
            for (fs_path, display) in candidates {
                let next_fs = fs_path.join(&lit);
                let valid = if needs_dir {
                    next_fs.is_dir()
                } else {
                    Path::new(&next_fs).symlink_metadata().is_ok()
                };

                if valid {
                    let next_display = append_display_segment(&display, &lit);
                    next_candidates.push((next_fs, next_display));
                }
            }
        }

        candidates = next_candidates;
        if candidates.is_empty() {
            break;
        }
    }

    if candidates.is_empty() {
        vec![tokens_to_literal_string(token)]
    } else {
        let mut results: Vec<String> = candidates
            .into_iter()
            .map(|(_, mut display)| {
                if has_trailing_slash && display != "/" {
                    display.push('/');
                }
                display
            })
            .collect();
        results.sort();
        results
    }
}

fn expand_globs(raw_tokens: Vec<Vec<TokenChar>>) -> Vec<String> {
    let mut expanded = Vec::with_capacity(raw_tokens.len());

    for token in raw_tokens {
        if token_has_glob(&token) {
            expanded.extend(expand_single_glob(&token));
        } else {
            expanded.push(tokens_to_literal_string(&token));
        }
    }

    expanded
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .expect("env mutex poisoned")
    }

    #[test]
    fn single_quotes_preserve_spaces_and_specials() {
        let tokens = tokenize("echo 'hello   world' 'a$HOMEb' '*'", 0).unwrap();
        assert_eq!(tokens, vec!["echo", "hello   world", "a$HOMEb", "*"]);
    }

    #[test]
    fn double_quotes_preserve_spaces_and_handle_escapes() {
        let _guard = env_lock();
        unsafe { env::set_var("RUSH_TEST_VAR", "rust") };

        let tokens = tokenize(
            r#"echo "hello   world" "and \"escaped\" \\ \$RUSH_TEST_VAR $RUSH_TEST_VAR $?" "a\nb""#,
            42,
        )
        .unwrap();

        assert_eq!(
            tokens,
            vec![
                "echo",
                "hello   world",
                r#"and "escaped" \ $RUSH_TEST_VAR rust 42"#,
                r#"a\nb"#,
            ]
        );

        unsafe { env::remove_var("RUSH_TEST_VAR") };
    }

    #[test]
    fn adjacent_quotes_concatenate_and_empty_quotes_persist() {
        let tokens = tokenize(r#"'hello '"world" foo"bar"'baz' "" ''"#, 0).unwrap();
        assert_eq!(tokens, vec!["hello world", "foobarbaz", "", ""]);
    }

    #[test]
    fn unquoted_backslash_escapes_special_chars() {
        let tokens = tokenize(r#"echo hello\ world \$HOME \* \?"#, 0).unwrap();
        assert_eq!(tokens, vec!["echo", "hello world", "$HOME", "*", "?"]);
    }

    #[test]
    fn tilde_expansion_cases() {
        let _guard = env_lock();
        let prev_home = env::var_os("HOME");
        unsafe { env::set_var("HOME", "/home/testuser") };

        let tokens = tokenize("~ ~/projects a~b ~other '~'", 0).unwrap();
        assert_eq!(
            tokens,
            vec![
                "/home/testuser",
                "/home/testuser/projects",
                "a~b",
                "~other",
                "~",
            ]
        );

        match prev_home {
            Some(v) => unsafe { env::set_var("HOME", v) },
            None => unsafe { env::remove_var("HOME") },
        }
    }

    #[test]
    fn variable_expansion_and_empty_unquoted_var() {
        let _guard = env_lock();
        unsafe {
            env::set_var("RUSH_FOO", "bar");
            env::remove_var("RUSH_UNSET");
        }

        let tokens = tokenize(r#"echo $RUSH_FOO ${RUSH_FOO}! $RUSH_UNSET "$RUSH_UNSET" $"#, 7).unwrap();
        assert_eq!(tokens, vec!["echo", "bar", "bar!", "", "$"]);

        unsafe { env::remove_var("RUSH_FOO") };
    }

    #[test]
    fn comments_ignored_only_at_token_start() {
        let tokens = tokenize("echo hi # ignored comment", 0).unwrap();
        assert_eq!(tokens, vec!["echo", "hi"]);

        let tokens2 = tokenize("echo hi#not_ignored", 0).unwrap();
        assert_eq!(tokens2, vec!["echo", "hi#not_ignored"]);
    }

    #[test]
    fn unclosed_delimiters_return_errors() {
        assert_eq!(tokenize("echo 'unclosed", 0), Err(LexError::UnclosedSingleQuote));
        assert_eq!(tokenize(r#"echo "unclosed"#, 0), Err(LexError::UnclosedDoubleQuote));
        assert_eq!(tokenize("echo ${UNCLOSED", 0), Err(LexError::UnclosedBrace));
    }

    #[test]
    fn glob_matches_files_and_falls_back_when_no_match() {
        let tokens = tokenize("echo *.toml *.definitely_nonexistent_ext", 0).unwrap();
        assert_eq!(tokens, vec!["echo", "Cargo.toml", "*.definitely_nonexistent_ext"]);

        let src_tokens = tokenize("echo src/?.rs src/*.rs", 0).unwrap();
        assert_eq!(src_tokens, vec!["echo", "src/?.rs", "src/lexer.rs", "src/main.rs"]);
    }
}
