//! Many downloads at once, as IDM's Tasks menu adds them: a numbered pattern ("Add batch
//! download": file*.zip from 1 to 50), and the links in some text (the clipboard, a text file).

/// The most one batch adds (a typo in "to" shouldn't add a million downloads).
pub const MOST: usize = 10_000;

/// The addresses a pattern stands for (or why it doesn't make sense yet).
pub fn expand(pattern: &str, from: &str, to: &str, digits: usize) -> Result<Vec<String>, String> {
    let pattern = pattern.trim();
    if !pattern.contains('*') {
        return Err("The address needs a * where the number goes".into());
    }
    let (from, to) = (from.trim(), to.trim());
    let items: Vec<String> = match (from.parse::<u64>(), to.parse::<u64>()) {
        (Ok(a), Ok(b)) if a <= b && (b - a) < MOST as u64 => (a..=b).map(|n| format!("{n:0digits$}")).collect(),
        (Ok(a), Ok(b)) if a <= b => return Err(format!("That's more than {MOST} downloads")),
        (Ok(_), Ok(_)) => return Err("\"From\" must not be bigger than \"To\"".into()),
        _ => {
            let letter = |s: &str| (s.len() == 1 && s.chars().all(|c| c.is_ascii_alphabetic())).then(|| s.as_bytes()[0]);
            match (letter(from), letter(to)) {
                (Some(a), Some(b)) if a <= b && a.is_ascii_lowercase() == b.is_ascii_lowercase() => {
                    (a..=b).map(|c| (c as char).to_string()).collect()
                }
                _ => return Err("\"From\" and \"To\" are numbers (1 to 50) or letters (a to z)".into()),
            }
        }
    };
    Ok(items.iter().map(|item| pattern.replacen('*', item, 1)).collect())
}

/// The download links in some text: http(s), ftp and magnet links, once each, in order.
pub fn links_in(text: &str) -> Vec<String> {
    let mut found: Vec<String> = vec![];
    for word in text.split(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '<' || c == '>') {
        let word = word.trim_start_matches(['(', '[', '{']).trim_end_matches([',', '.', ';', ')', ']', '}']);
        let link = ["http://", "https://", "ftp://", "magnet:?"].iter().any(|p| word.to_ascii_lowercase().starts_with(p));
        if link && word.len() > 10 && !found.iter().any(|f| f == word) {
            found.push(word.to_string());
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns() {
        let urls = expand("https://x.org/img*.jpg", "8", "10", 2).unwrap();
        assert_eq!(urls, ["https://x.org/img08.jpg", "https://x.org/img09.jpg", "https://x.org/img10.jpg"]);
        assert_eq!(expand("https://x.org/*.zip", "a", "c", 1).unwrap().len(), 3);
        assert!(expand("https://x.org/a.zip", "1", "2", 1).is_err());
        assert!(expand("https://x.org/*.zip", "5", "1", 1).is_err());
    }

    #[test]
    fn links() {
        let text = "see https://a.org/f.zip, and (https://b.org/g.iso) twice https://a.org/f.zip magnet:?xt=urn:btih:abc";
        assert_eq!(links_in(text), ["https://a.org/f.zip", "https://b.org/g.iso", "magnet:?xt=urn:btih:abc"]);
    }
}
