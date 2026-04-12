pub fn generate_slug<S: AsRef<str>>(input: S) -> String {
    let input = input.as_ref();

    // Pre-allocate a string with the same capacity as the input
    // to reduce re-allocations during the push.
    let mut slug = String::with_capacity(input.len());
    let mut last_was_hyphen = true; // Start true to trim leading hyphens

    for c in input.chars() {
        match c.to_ascii_lowercase() {
            c @ 'a'..='z' | c @ '0'..='9' => {
                slug.push(c);
                last_was_hyphen = false;
            }
            ' ' | '-' | '_' => {
                if !last_was_hyphen {
                    slug.push('-');
                    last_was_hyphen = true;
                }
            }
            _ => (), // Ignore everything else
        }
    }

    // Trim a trailing hyphen if it exists
    if slug.ends_with('-') {
        slug.pop();
    }

    slug
}

pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
