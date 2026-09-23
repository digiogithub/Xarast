//! The small amount of argument parsing the subcommands share.
//!
//! Hand-rolled on purpose: the tool has a handful of flags, and a parser
//! dependency would cost more build time than it saves code.

use std::str::FromStr;

/// A cursor over one subcommand's arguments.
///
/// `--flag=value` is split into `--flag value` up front, so both
/// spellings work everywhere.
#[derive(Debug)]
pub struct Args {
    items: Vec<String>,
    at: usize,
}

impl Args {
    /// Wraps an argument list.
    #[must_use]
    pub fn new(argv: &[String]) -> Args {
        let mut items = Vec::with_capacity(argv.len());
        for a in argv {
            match a.split_once('=') {
                Some((f, v)) if f.starts_with("--") => {
                    items.push(f.to_owned());
                    items.push(v.to_owned());
                }
                _ => items.push(a.clone()),
            }
        }
        Args { items, at: 0 }
    }

    /// The next raw argument.
    pub fn next_arg(&mut self) -> Option<String> {
        let a = self.items.get(self.at).cloned();
        self.at += usize::from(a.is_some());
        a
    }

    /// The value of `flag`, which must follow it.
    ///
    /// # Errors
    ///
    /// When the argument list ends first.
    pub fn value(&mut self, flag: &str) -> Result<String, String> {
        self.next_arg()
            .ok_or_else(|| format!("{flag} needs a value"))
    }

    /// The value of `flag`, parsed.
    ///
    /// # Errors
    ///
    /// When it is missing or does not parse.
    pub fn parsed<T: FromStr>(&mut self, flag: &str) -> Result<T, String> {
        let v = self.value(flag)?;
        v.parse()
            .map_err(|_| format!("{flag}: `{v}` is not a valid value"))
    }
}

/// Parses a zoom given as a percentage: `100`, `100%` and `12.5%` all
/// work. Returns the zoom factor, where `1.0` is 100 %.
///
/// # Errors
///
/// When the text is not a finite positive number.
pub fn parse_zoom(text: &str) -> Result<f64, String> {
    let n: f64 = text
        .trim_end_matches('%')
        .parse()
        .map_err(|_| format!("--zoom: `{text}` is not a percentage"))?;
    if n.is_finite() && n > 0.0 {
        Ok(n / 100.0)
    } else {
        Err(format!("--zoom: `{text}` must be a positive percentage"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_accepts_both_spellings() {
        assert_eq!(parse_zoom("100"), Ok(1.0));
        assert_eq!(parse_zoom("250%"), Ok(2.5));
        assert!(parse_zoom("0").is_err());
        assert!(parse_zoom("-5%").is_err());
        assert!(parse_zoom("inf").is_err());
        assert!(parse_zoom("abc").is_err());
    }

    #[test]
    fn equals_form_splits_only_long_flags() {
        let argv: Vec<String> = ["--zoom=50", "a=b.xar", "--dpi"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        let mut a = Args::new(&argv);
        assert_eq!(a.next_arg().as_deref(), Some("--zoom"));
        assert_eq!(a.value("--zoom").as_deref(), Ok("50"));
        assert_eq!(a.next_arg().as_deref(), Some("a=b.xar"));
        assert_eq!(a.next_arg().as_deref(), Some("--dpi"));
        assert!(a.value("--dpi").is_err());
    }
}
