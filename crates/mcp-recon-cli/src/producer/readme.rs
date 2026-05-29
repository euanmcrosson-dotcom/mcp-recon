//! Extract a tool surface from a Markdown README.
//!
//! MCP server packages overwhelmingly document their tools in their
//! README rather than in `package.json` / `pyproject.toml`. The
//! producer wraps each registry's metadata fetcher around this module:
//! if the README parses into a richer per-tool surface, the classifier
//! gets real per-tool names + descriptions to fire R3/R5/R6/R7 against.
//! Otherwise the fetcher falls back to the existing bin/entry_point
//! synthesis path.
//!
//! Patterns recognised (best-effort, in priority order):
//!
//!   1. **Markdown table** with `Tool` / `Name` and `Description`
//!      columns under (or near) a heading containing `Tools`,
//!      `Available Tools`, `Tool Reference`, `API`, or `Capabilities`.
//!   2. **Sub-headings** (`### tool_name` or `### tool_name(...)`)
//!      under that section, with the next non-empty paragraph as the
//!      description.
//!   3. **Bullet list** entries like `- name - description`,
//!      `- **name** - description`, `- \`name\` - description`,
//!      or `- name: description` under that section.
//!
//! Returns an empty vec when no recognisable section is found — caller
//! falls back to whatever surface they had before.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedTool {
    pub name: String,
    pub description: Option<String>,
}

const TOOL_SECTION_HEADINGS: &[&str] = &[
    "tools",
    "available tools",
    "tool reference",
    "capabilities",
    "api",
    "api reference",
    "exposed tools",
    "supported tools",
];

/// Top-level entry. Locate a tool section in the README and try every
/// pattern until one yields entries. Returns empty when nothing
/// matches.
pub fn extract_tools(markdown: &str) -> Vec<ExtractedTool> {
    let section = match locate_tool_section(markdown) {
        Some(s) => s,
        None => return Vec::new(),
    };

    // Try table first — when present it's the most structured signal.
    let from_table = parse_table(section);
    if !from_table.is_empty() {
        return dedupe(from_table);
    }

    // Then sub-headings.
    let from_headings = parse_subheadings(section);
    if !from_headings.is_empty() {
        return dedupe(from_headings);
    }

    // Bullet list fallback.
    let from_bullets = parse_bullet_list(section);
    dedupe(from_bullets)
}

/// Find the body of the first heading that matches one of the
/// canonical tool-section titles. Returns the slice from end of the
/// matched heading to start of the next equal-or-higher-level heading
/// (or EOF).
fn locate_tool_section(markdown: &str) -> Option<&str> {
    let bytes = markdown.as_bytes();
    for line in markdown.lines() {
        let stripped = line.trim_start();
        let level = stripped.chars().take_while(|c| *c == '#').count();
        if level == 0 || level > 6 {
            continue;
        }
        let title = stripped[level..].trim();
        let title_lower = title.to_lowercase();
        if !TOOL_SECTION_HEADINGS.iter().any(|h| {
            // exact match OR section starts with that phrase followed by punctuation
            &title_lower == h
                || title_lower.starts_with(&format!("{h}:"))
                || title_lower.starts_with(&format!("{h} "))
        }) {
            continue;
        }

        // Found the heading. Find where its body starts in the original
        // string (skip past the line + newline).
        let body_start = byte_offset_after_line(markdown, line);
        if body_start >= bytes.len() {
            return Some("");
        }

        // Find the end: next heading at equal-or-shallower level, or EOF.
        let mut end = bytes.len();
        for line2 in markdown[body_start..].lines() {
            let stripped2 = line2.trim_start();
            let lvl2 = stripped2.chars().take_while(|c| *c == '#').count();
            if lvl2 > 0 && lvl2 <= level {
                let offset = byte_offset_of_line(markdown, body_start, line2);
                end = offset;
                break;
            }
        }
        return Some(&markdown[body_start..end]);
    }
    None
}

fn byte_offset_after_line<'a>(haystack: &'a str, line: &'a str) -> usize {
    // SAFETY assumption: `line` is a substring slice from `haystack`.
    let line_start = (line.as_ptr() as usize)
        .saturating_sub(haystack.as_ptr() as usize)
        .min(haystack.len());
    let line_end = (line_start + line.len()).min(haystack.len());
    // Skip the line's terminating newline if present.
    if line_end < haystack.len() && haystack.as_bytes()[line_end] == b'\n' {
        line_end + 1
    } else if line_end + 1 < haystack.len()
        && haystack.as_bytes()[line_end] == b'\r'
        && haystack.as_bytes()[line_end + 1] == b'\n'
    {
        line_end + 2
    } else {
        line_end
    }
}

fn byte_offset_of_line<'a>(haystack: &'a str, search_start: usize, line: &'a str) -> usize {
    let line_start_global = (line.as_ptr() as usize).saturating_sub(haystack.as_ptr() as usize);
    // Defensive: should be >= search_start; if not, return EOF.
    if line_start_global >= search_start && line_start_global <= haystack.len() {
        line_start_global
    } else {
        haystack.len()
    }
}

/// Parse a Markdown table whose first two columns name a tool and
/// describe it. Tolerates a "Parameters" or other third column.
fn parse_table(section: &str) -> Vec<ExtractedTool> {
    let mut out = Vec::new();
    let mut in_table = false;
    let mut header_seen = false;
    for line in section.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            in_table = false;
            header_seen = false;
            continue;
        }
        // Header divider row: `|---|---|...`
        if trimmed.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ')) {
            if in_table {
                header_seen = true;
            }
            continue;
        }
        if !in_table {
            // Header row — verify it looks like a tool table by checking
            // first two columns mention "tool"/"name" + "description".
            let cells = split_row(trimmed);
            if cells.len() < 2 {
                continue;
            }
            let h0 = cells[0].to_lowercase();
            let h1 = cells[1].to_lowercase();
            let name_col = h0.contains("tool") || h0.contains("name") || h0.contains("method");
            let desc_col = h1.contains("description")
                || h1.contains("summary")
                || h1.contains("what")
                || h1.contains("purpose");
            if name_col && desc_col {
                in_table = true;
            }
            continue;
        }
        if !header_seen {
            // Skip rows until we've cleared the divider.
            continue;
        }
        let cells = split_row(trimmed);
        if cells.len() < 2 {
            continue;
        }
        let name = strip_inline_md(cells[0]);
        if name.is_empty() {
            continue;
        }
        let desc = strip_inline_md(cells[1]);
        out.push(ExtractedTool {
            name,
            description: if desc.is_empty() { None } else { Some(desc) },
        });
    }
    out
}

fn split_row(line: &str) -> Vec<&str> {
    let trimmed = line.trim().trim_matches('|');
    trimmed.split('|').map(str::trim).collect()
}

/// Find `### tool_name` (or `#### tool_name`) sub-headings under the
/// section and pair each with the next non-empty, non-heading
/// paragraph as its description.
fn parse_subheadings(section: &str) -> Vec<ExtractedTool> {
    let mut out = Vec::new();
    let lines: Vec<&str> = section.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let raw = lines[i];
        let stripped = raw.trim_start();
        let level = stripped.chars().take_while(|c| *c == '#').count();
        if (3..=6).contains(&level) {
            let title = stripped[level..].trim();
            if let Some(name) = clean_tool_name(title) {
                // Find next non-empty, non-heading line as description.
                let mut desc: Option<String> = None;
                let mut j = i + 1;
                while j < lines.len() {
                    let l = lines[j].trim();
                    if l.is_empty() {
                        j += 1;
                        continue;
                    }
                    let lvl_j = l.chars().take_while(|c| *c == '#').count();
                    if lvl_j > 0 {
                        break;
                    }
                    desc = Some(strip_inline_md(l));
                    break;
                }
                out.push(ExtractedTool {
                    name,
                    description: desc,
                });
            }
        }
        i += 1;
    }
    out
}

/// Parse bullet-list entries shaped like:
///   - name - description
///   - **name** - description
///   - `name` - description
///   - name: description
fn parse_bullet_list(section: &str) -> Vec<ExtractedTool> {
    let mut out = Vec::new();
    for raw in section.lines() {
        let trimmed = raw.trim_start();
        if !trimmed.starts_with("- ") && !trimmed.starts_with("* ") {
            continue;
        }
        let body = &trimmed[2..];
        if let Some((name, desc)) = split_bullet(body) {
            out.push(ExtractedTool {
                name,
                description: if desc.is_empty() { None } else { Some(desc) },
            });
        }
    }
    out
}

fn split_bullet(body: &str) -> Option<(String, String)> {
    // Prefer `name - description`, fall back to `name: description`.
    let body = body.trim();
    // Strip leading inline-emphasis around the name: `**name**` or `\`name\``.
    let body_clean = body.strip_prefix('`').and_then(|s| {
        let idx = s.find('`')?;
        Some((s[..idx].to_string(), s[idx + 1..].to_string()))
    });
    if let Some((name, rest)) = body_clean {
        let rest = rest.trim_start_matches(['-', ':', '—', ' ']);
        return Some((name, strip_inline_md(rest)));
    }
    let body_clean = body.strip_prefix("**").and_then(|s| {
        let idx = s.find("**")?;
        Some((s[..idx].to_string(), s[idx + 2..].to_string()))
    });
    if let Some((name, rest)) = body_clean {
        let rest = rest.trim_start_matches(['-', ':', '—', ' ']);
        return Some((name, strip_inline_md(rest)));
    }

    if let Some((a, b)) = body.split_once(" - ") {
        let name = strip_inline_md(a);
        if !name.is_empty() {
            return Some((name, strip_inline_md(b)));
        }
    }
    if let Some((a, b)) = body.split_once(": ") {
        let name = strip_inline_md(a);
        if !name.is_empty() {
            return Some((name, strip_inline_md(b)));
        }
    }
    None
}

fn clean_tool_name(raw: &str) -> Option<String> {
    let mut s = raw.trim().to_string();
    if let Some(rest) = s.strip_prefix("Tool:").or_else(|| s.strip_prefix("tool:")) {
        s = rest.trim().to_string();
    }
    // If wrapped in backticks, pull the inner contents.
    if let Some(rest) = s.strip_prefix('`') {
        if let Some(end) = rest.find('`') {
            s = rest[..end].to_string();
        }
    }
    // Drop any `(parameter list)` after the name.
    if let Some(idx) = s.find('(') {
        s = s[..idx].trim().to_string();
    }
    let name = strip_inline_md(&s);
    // Reject pure prose headings.
    if name.is_empty()
        || (name.contains(' ') && name.split_whitespace().count() > 3)
        || name.chars().any(|c| matches!(c, '.' | '?' | '!'))
    {
        return None;
    }
    Some(name)
}

fn strip_inline_md(s: &str) -> String {
    let s = s
        .trim()
        .trim_matches(|c: char| matches!(c, '`' | '*' | '_'));
    s.to_string()
}

/// Deduplicate by name, preserving first-seen insertion order so the
/// emitted tool surface mirrors the README's documented order.
fn dedupe(tools: Vec<ExtractedTool>) -> Vec<ExtractedTool> {
    use std::collections::HashSet;
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(tools.len());
    for t in tools {
        if seen.insert(t.name.clone()) {
            out.push(t);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_tools_section_returns_empty() {
        let md = "# foo\n\nSome stuff about the project.\n";
        assert!(extract_tools(md).is_empty());
    }

    #[test]
    fn table_with_name_description_columns() {
        let md = "\
# Project

## Tools

| Tool | Description |
|------|-------------|
| read_file | Read contents of a file |
| write_file | Write to a file |
| run_command | Execute a shell command |
";
        let t = extract_tools(md);
        assert_eq!(t.len(), 3);
        assert_eq!(t[0].name, "read_file");
        assert_eq!(t[0].description.as_deref(), Some("Read contents of a file"));
        assert_eq!(t[2].name, "run_command");
    }

    #[test]
    fn subheadings_with_descriptions() {
        let md = "\
## Tools

### read_file
Reads contents of a file at the given path.

### fetch_url
Fetches an external URL.
";
        let t = extract_tools(md);
        assert_eq!(t.len(), 2);
        // Insertion order preserved.
        assert_eq!(t[0].name, "read_file");
        assert_eq!(t[1].name, "fetch_url");
        assert_eq!(
            t[1].description.as_deref(),
            Some("Fetches an external URL.")
        );
    }

    #[test]
    fn subheadings_with_tool_prefix_and_parens() {
        let md = "\
## Available Tools

### Tool: `query_db(sql)`
Runs a SQL query against the database.
";
        let t = extract_tools(md);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].name, "query_db");
    }

    #[test]
    fn bullet_list_with_dash_separator() {
        let md = "\
## Tools

- `read_file` - Reads a file
- `write_file` - Writes to a file
- exec - Runs a shell command
";
        let t = extract_tools(md);
        assert_eq!(t.len(), 3);
        let names: Vec<&str> = t.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, vec!["read_file", "write_file", "exec"]);
    }

    #[test]
    fn bullet_list_with_bold_names() {
        let md = "\
## Capabilities

- **fetch_url** — Fetches an external URL
- **send_email** — Sends an email
";
        let t = extract_tools(md);
        assert_eq!(t.len(), 2);
        assert!(t.iter().any(|x| x.name == "fetch_url"));
    }

    #[test]
    fn deduplicates_by_name() {
        let md = "\
## Tools

| Tool | Description |
|------|-------------|
| read_file | first row |
| read_file | second row (dupe) |
";
        let t = extract_tools(md);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].description.as_deref(), Some("first row"));
    }

    #[test]
    fn rejects_table_without_tool_name_column() {
        let md = "\
## Tools

| Step | Outcome |
|------|---------|
| install | done |
";
        let t = extract_tools(md);
        assert!(t.is_empty());
    }

    #[test]
    fn stops_at_next_h2_section() {
        let md = "\
## Tools

- read_file - reads
- write_file - writes

## Installation

- pip install x
- npm install x
";
        let t = extract_tools(md);
        let names: Vec<&str> = t.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, vec!["read_file", "write_file"]);
    }

    #[test]
    fn matches_tool_section_title_variants() {
        // Title with trailing colon.
        let md1 = "## Tools:\n\n- a - b\n";
        let md2 = "## API\n\n- a - b\n";
        let md3 = "## Available Tools\n\n- a - b\n";
        for md in [md1, md2, md3] {
            let t = extract_tools(md);
            assert_eq!(t.len(), 1, "should match: {md:?}");
            assert_eq!(t[0].name, "a");
        }
    }

    #[test]
    fn prefers_table_over_bullets_when_both_present() {
        // Real READMEs sometimes have both. Table wins.
        let md = "\
## Tools

| Tool | Description |
|------|-------------|
| from_table | yes |

- from_bullet - no
";
        let t = extract_tools(md);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].name, "from_table");
    }
}
