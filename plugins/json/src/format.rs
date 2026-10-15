//! Whitespace-only formatting: numeric literals, duplicate keys and key order survive.
pub fn layout(text: &str, pretty: bool) -> String {
    let mut output = String::new();
    let mut quoted = false; let mut escaped = false; let mut depth = 0usize;
    let mut previous = None;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if quoted {
            output.push(ch);
            if escaped { escaped = false; }
            else if ch == '\\' { escaped = true; }
            else if ch == '"' { quoted = false; }
            continue;
        }
        if ch.is_whitespace() { continue; }
        match ch {
            '"' => { quoted = true; output.push(ch); }
            '{' | '[' => {
                output.push(ch); depth += 1;
                let next = chars.clone().find(|c| !c.is_whitespace());
                if pretty && !matches!(next, Some('}') | Some(']')) { newline(&mut output, depth); }
            }
            '}' | ']' => {
                depth = depth.saturating_sub(1);
                if pretty && !matches!(previous, Some('{') | Some('[')) { newline(&mut output, depth); }
                output.push(ch);
            }
            ',' => { output.push(ch); if pretty { newline(&mut output, depth); } }
            ':' => { output.push(ch); if pretty { output.push(' '); } }
            _ => output.push(ch),
        }
        previous = Some(ch);
    }
    output
}
fn newline(output: &mut String, depth: usize) {
    output.push('\n');
    for _ in 0..depth { output.push_str("  "); }
}
