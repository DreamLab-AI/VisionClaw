use std::collections::HashMap;

use super::block_node::{BlockNode, ClockEntry, TaskStatus};

const MAX_BLOCK_REFS: usize = 256;
const MAX_CLOCK_ENTRIES: usize = 64;
#[allow(dead_code)]
const MAX_EMBED_DEPTH: usize = 16;
#[allow(dead_code)]
const MAX_CLOZE_DEPTH: usize = 32;

/// O(N) deterministic stack-machine parser for Logseq markdown per ADR-068 D1.
///
/// Matches matryca's design decisions:
/// - Indentation alone arbitrates parent-child hierarchy (heading markers are content)
/// - Soft-break continuation: lines without bullet prefix merge into current block
/// - Property detection: `key:: value` on its own line at block terminus
/// - Drawer parsing: `:LOGBOOK:`/`:PROPERTIES:`/`:END:` consumed as opaque metadata
/// - Indentation fracture (0→4-space jump): no phantom intermediates
pub struct LogseqBlockParser {
    owner_hex: String,
    vault_cap: usize,
}

impl LogseqBlockParser {
    pub fn new(owner_hex: String) -> Self {
        Self {
            owner_hex,
            vault_cap: 50_000,
        }
    }

    pub fn with_vault_cap(mut self, cap: usize) -> Self {
        self.vault_cap = cap.min(200_000);
        self
    }

    /// Parse a single Logseq markdown file into blocks.
    ///
    /// `rel_path` is the vault-relative path (e.g., "pages/my-page.md").
    /// Returns blocks in document order with parent_id/left_id set.
    pub fn parse_file(&self, content: &str, rel_path: &str) -> ParseResult {
        let mut blocks: Vec<BlockNode> = Vec::new();
        let mut indent_stack: Vec<(u32, usize)> = Vec::new(); // (indent_level, block_index)
        let mut in_drawer = false;
        let mut drawer_name: Option<String> = None;
        let mut current_clock_entries: Vec<ClockEntry> = Vec::new();
        let mut current_properties: HashMap<String, String> = HashMap::new();
        let mut block_index: u32 = 0;

        let lines: Vec<&str> = content.lines().collect();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i];

            // Drawer handling
            if in_drawer {
                if line.trim() == ":END:" {
                    if drawer_name.as_deref() == Some("LOGBOOK") {
                        if let Some(last) = blocks.last_mut() {
                            let remaining = MAX_CLOCK_ENTRIES.saturating_sub(last.clock_entries.len());
                            last.clock_entries
                                .extend(current_clock_entries.drain(..remaining.min(current_clock_entries.len())));
                            if !current_clock_entries.is_empty() {
                                last.truncated = true;
                                current_clock_entries.clear();
                            }
                        }
                    } else if drawer_name.as_deref() == Some("PROPERTIES") {
                        if let Some(last) = blocks.last_mut() {
                            last.properties.extend(current_properties.drain());
                        }
                    }
                    in_drawer = false;
                    drawer_name = None;
                    i += 1;
                    continue;
                }

                if drawer_name.as_deref() == Some("LOGBOOK") {
                    if let Some(clock) = parse_clock_line(line.trim()) {
                        current_clock_entries.push(clock);
                    }
                } else if drawer_name.as_deref() == Some("PROPERTIES") {
                    if let Some((k, v)) = parse_property_line(line.trim()) {
                        current_properties.insert(k, v);
                    }
                }

                i += 1;
                continue;
            }

            // Check for drawer start
            let trimmed = line.trim();
            if trimmed.starts_with(':') && trimmed.ends_with(':') && trimmed.len() > 2 {
                let name = &trimmed[1..trimmed.len() - 1];
                if matches!(name, "LOGBOOK" | "PROPERTIES" | "CUSTOM") {
                    in_drawer = true;
                    drawer_name = Some(name.to_string());
                    current_clock_entries.clear();
                    current_properties.clear();
                    i += 1;
                    continue;
                }
            }

            // Detect bullet line (new block)
            if let Some((indent, rest)) = parse_bullet_line(line) {
                if blocks.len() >= self.vault_cap {
                    return ParseResult {
                        blocks,
                        truncated: true,
                        errors: vec![format!(
                            "vault cap {} exceeded at block {}",
                            self.vault_cap, block_index
                        )],
                    };
                }

                let (task_status, content_after_marker) = extract_task_marker(rest);
                let content_str = content_after_marker.to_string();
                let clean = strip_formatting(&content_str);
                let refs = extract_refs(&content_str);

                // Determine parent from indent stack
                while let Some(&(parent_indent, _)) = indent_stack.last() {
                    if parent_indent >= indent {
                        indent_stack.pop();
                    } else {
                        break;
                    }
                }

                let parent_id = indent_stack.last().map(|&(_, idx)| {
                    blocks[idx].uuid.clone()
                });

                // left_id: previous sibling at same indent level
                let left_id = find_left_sibling(&blocks, &indent_stack, indent);

                let uuid = self.mint_block_uuid(rel_path, block_index, &content_str);

                // Check explicit id:: property (will be processed if in trailing properties)
                let explicit_id = extract_inline_property(&content_str, "id");

                let mut block = BlockNode {
                    uuid: explicit_id
                        .map(|id| self.mint_explicit_uuid(&id))
                        .unwrap_or(uuid),
                    content: content_str,
                    clean_text: clean,
                    parent_id,
                    left_id,
                    indent_level: indent,
                    properties: HashMap::new(),
                    refs: if refs.len() > MAX_BLOCK_REFS {
                        let mut r = refs;
                        r.truncate(MAX_BLOCK_REFS);
                        r
                    } else {
                        refs
                    },
                    path_refs: Vec::new(),
                    task_status,
                    scheduled: None,
                    deadline: None,
                    repeater: None,
                    created_at: None,
                    journal_day: None,
                    block_index,
                    clock_entries: Vec::new(),
                    truncated: false,
                    cyclic_embed: false,
                };

                if block.refs.len() >= MAX_BLOCK_REFS {
                    block.truncated = true;
                }

                indent_stack.push((indent, blocks.len()));
                blocks.push(block);
                block_index += 1;
            } else if !trimmed.is_empty() && !blocks.is_empty() {
                // Soft-break continuation: merge into current block's content
                if let Some(last) = blocks.last_mut() {
                    last.content.push('\n');
                    last.content.push_str(trimmed);

                    // Check for trailing property lines
                    if let Some((k, v)) = parse_property_line(trimmed) {
                        last.properties.insert(k, v);
                        // Special property handling
                        match last.properties.get("id").cloned() {
                            Some(id) => last.uuid = self.mint_explicit_uuid(&id),
                            None => {}
                        }
                    }

                    // Check for SCHEDULED/DEADLINE
                    if let Some(sched) = extract_scheduled(trimmed) {
                        last.scheduled = Some(sched);
                    }
                    if let Some(dl) = extract_deadline(trimmed) {
                        last.deadline = Some(dl);
                    }

                    // Accumulate additional refs
                    let new_refs = extract_refs(trimmed);
                    let remaining = MAX_BLOCK_REFS.saturating_sub(last.refs.len());
                    last.refs.extend(new_refs.into_iter().take(remaining));
                }
            }

            i += 1;
        }

        // Compute path-refs (ADR-068 D5): union of own refs + all ancestor refs
        compute_path_refs(&mut blocks);

        ParseResult {
            blocks,
            truncated: false,
            errors: Vec::new(),
        }
    }

    /// Mint a deterministic block URN using BLAKE3 for cross-Rust-version stability (P1-24).
    fn mint_block_uuid(&self, rel_path: &str, block_index: u32, content: &str) -> String {
        let mut hasher = blake3::Hasher::new();
        hasher.update(self.owner_hex.as_bytes());
        hasher.update(b"|");
        hasher.update(rel_path.as_bytes());
        hasher.update(b"|");
        hasher.update(&block_index.to_le_bytes());
        hasher.update(b"|");
        hasher.update(content.as_bytes());
        let hash = hasher.finalize();
        let bytes = hash.as_bytes();
        // Take 8 bytes for a u64-width hex representation matching the old format.
        let hash_u64 = u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3],
            bytes[4], bytes[5], bytes[6], bytes[7],
        ]);

        format!(
            "urn:visionclaw:concept:{}:block:{:016x}",
            &self.owner_hex[..self.owner_hex.len().min(16)],
            hash_u64
        )
    }

    fn mint_explicit_uuid(&self, explicit_id: &str) -> String {
        format!(
            "urn:visionclaw:concept:{}:block:{}",
            &self.owner_hex[..self.owner_hex.len().min(16)],
            explicit_id
        )
    }
}

#[derive(Debug)]
pub struct ParseResult {
    pub blocks: Vec<BlockNode>,
    pub truncated: bool,
    pub errors: Vec<String>,
}

fn parse_bullet_line(line: &str) -> Option<(u32, &str)> {
    let bytes = line.as_bytes();
    let mut indent = 0u32;
    let mut pos = 0;

    while pos < bytes.len() {
        match bytes[pos] {
            b' ' => {
                indent += 1;
                pos += 1;
            }
            b'\t' => {
                indent += 2; // treat tab as 2 spaces (Logseq convention)
                pos += 1;
            }
            _ => break,
        }
    }

    // Check for bullet: "- " or "* "
    if pos + 1 < bytes.len() && (bytes[pos] == b'-' || bytes[pos] == b'*') && bytes[pos + 1] == b' '
    {
        Some((indent, &line[pos + 2..]))
    } else {
        None
    }
}

fn extract_task_marker(text: &str) -> (Option<TaskStatus>, &str) {
    let markers = [
        "TODO", "DOING", "DONE", "NOW", "LATER", "WAIT", "WAITING", "CANCELLED", "CANCELED",
    ];
    for marker in &markers {
        if text.starts_with(marker) {
            let rest = &text[marker.len()..];
            if rest.is_empty() || rest.starts_with(' ') {
                return (
                    TaskStatus::from_marker(marker),
                    rest.trim_start(),
                );
            }
        }
    }
    (None, text)
}

fn extract_refs(text: &str) -> Vec<String> {
    let mut refs = Vec::new();

    // WikiLinks: [[target]]
    let mut pos = 0;
    let bytes = text.as_bytes();
    while pos + 3 < bytes.len() {
        if bytes[pos] == b'[' && bytes[pos + 1] == b'[' {
            if let Some(end) = text[pos + 2..].find("]]") {
                let target = &text[pos + 2..pos + 2 + end];
                if !target.is_empty() {
                    refs.push(target.to_string());
                }
                pos = pos + 2 + end + 2;
                continue;
            }
        }
        // Block refs: ((uuid))
        if bytes[pos] == b'(' && pos + 1 < bytes.len() && bytes[pos + 1] == b'(' {
            if let Some(end) = text[pos + 2..].find("))") {
                let target = &text[pos + 2..pos + 2 + end];
                if !target.is_empty() {
                    refs.push(format!("block-ref:{}", target));
                }
                pos = pos + 2 + end + 2;
                continue;
            }
        }
        // Tags: #tag or #[[tag]]
        if bytes[pos] == b'#' {
            if pos + 2 < bytes.len() && bytes[pos + 1] == b'[' && bytes[pos + 2] == b'[' {
                if let Some(end) = text[pos + 3..].find("]]") {
                    let tag = &text[pos + 3..pos + 3 + end];
                    if !tag.is_empty() {
                        refs.push(format!("tag:{}", tag));
                    }
                    pos = pos + 3 + end + 2;
                    continue;
                }
            } else {
                let tag_start = pos + 1;
                let mut tag_end = tag_start;
                while tag_end < bytes.len()
                    && !bytes[tag_end].is_ascii_whitespace()
                    && bytes[tag_end] != b','
                    && bytes[tag_end] != b')'
                    && bytes[tag_end] != b']'
                {
                    tag_end += 1;
                }
                if tag_end > tag_start {
                    refs.push(format!("tag:{}", &text[tag_start..tag_end]));
                    pos = tag_end;
                    continue;
                }
            }
        }
        pos += 1;
    }

    refs
}

fn extract_inline_property<'a>(text: &'a str, key: &str) -> Option<String> {
    let pattern = format!("{}:: ", key);
    if let Some(pos) = text.find(&pattern) {
        let value_start = pos + pattern.len();
        let value_end = text[value_start..]
            .find('\n')
            .map(|p| value_start + p)
            .unwrap_or(text.len());
        Some(text[value_start..value_end].trim().to_string())
    } else {
        None
    }
}

fn parse_property_line(line: &str) -> Option<(String, String)> {
    if let Some(sep_pos) = line.find(":: ") {
        let key = line[..sep_pos].trim();
        let value = line[sep_pos + 3..].trim();
        if !key.is_empty() && !key.contains(' ') {
            return Some((key.to_string(), value.to_string()));
        }
    }
    None
}

fn parse_clock_line(line: &str) -> Option<ClockEntry> {
    if !line.starts_with("CLOCK:") {
        return None;
    }
    let rest = line[6..].trim();
    // Format: [2024-01-15 Mon 10:00]--[2024-01-15 Mon 11:30] =>  1:30
    if let Some(dash_pos) = rest.find("--") {
        let start = rest[1..dash_pos.min(rest.len())].trim_matches(|c| c == '[' || c == ']');
        let after_dash = &rest[dash_pos + 2..];
        let end_bracket = after_dash.find(']').unwrap_or(after_dash.len());
        let end = after_dash[..end_bracket].trim_start_matches('[');

        let duration = after_dash[end_bracket..]
            .find("=>")
            .and_then(|p| {
                let dur_str = after_dash[end_bracket + p + 2..].trim();
                parse_duration_to_min(dur_str)
            });

        Some(ClockEntry {
            start: start.to_string(),
            end: Some(end.to_string()),
            duration_min: duration,
        })
    } else {
        let start = rest.trim_matches(|c| c == '[' || c == ']');
        Some(ClockEntry {
            start: start.to_string(),
            end: None,
            duration_min: None,
        })
    }
}

fn parse_duration_to_min(s: &str) -> Option<u32> {
    // Format: "1:30" or "0:45"
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() == 2 {
        let hours: u32 = parts[0].trim().parse().ok()?;
        let mins: u32 = parts[1].trim().parse().ok()?;
        Some(hours * 60 + mins)
    } else {
        None
    }
}

fn extract_scheduled(line: &str) -> Option<String> {
    if let Some(pos) = line.find("SCHEDULED:") {
        let rest = &line[pos + 10..];
        let trimmed = rest.trim();
        if trimmed.starts_with('<') {
            let end = trimmed.find('>').unwrap_or(trimmed.len());
            return Some(trimmed[1..end].to_string());
        }
    }
    None
}

fn extract_deadline(line: &str) -> Option<String> {
    if let Some(pos) = line.find("DEADLINE:") {
        let rest = &line[pos + 9..];
        let trimmed = rest.trim();
        if trimmed.starts_with('<') {
            let end = trimmed.find('>').unwrap_or(trimmed.len());
            return Some(trimmed[1..end].to_string());
        }
    }
    None
}

fn strip_formatting(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '*' | '_' | '~' | '`' => {
                // Skip formatting markers, but keep content
                if chars.peek() == Some(&c) {
                    chars.next(); // skip double marker
                }
            }
            '[' if chars.peek() == Some(&'[') => {
                chars.next(); // skip [[
                // Copy content until ]]
                while let Some(inner) = chars.next() {
                    if inner == ']' && chars.peek() == Some(&']') {
                        chars.next();
                        break;
                    }
                    result.push(inner);
                }
            }
            '(' if chars.peek() == Some(&'(') => {
                chars.next(); // skip ((
                while let Some(inner) = chars.next() {
                    if inner == ')' && chars.peek() == Some(&')') {
                        chars.next();
                        break;
                    }
                    result.push(inner);
                }
            }
            '#' => {
                // Skip tag marker but keep tag text
            }
            _ => result.push(c),
        }
    }

    result.trim().to_string()
}

fn find_left_sibling(blocks: &[BlockNode], indent_stack: &[(u32, usize)], indent: u32) -> Option<String> {
    // Find the most recent block at the same indent level with the same parent
    let parent_idx = indent_stack.last().map(|&(_, idx)| idx);

    for block in blocks.iter().rev() {
        if block.indent_level == indent {
            // Check same parent
            let block_parent = block.parent_id.as_deref();
            let our_parent = parent_idx.map(|idx| blocks[idx].uuid.as_str());
            if block_parent == our_parent {
                return Some(block.uuid.clone());
            }
        }
        // Stop searching once we go above our indent level
        if block.indent_level < indent {
            break;
        }
    }
    None
}

fn compute_path_refs(blocks: &mut Vec<BlockNode>) {
    // Build parent index for traversal
    let parent_map: HashMap<String, Option<String>> = blocks
        .iter()
        .map(|b| (b.uuid.clone(), b.parent_id.clone()))
        .collect();

    let own_refs: HashMap<String, Vec<String>> = blocks
        .iter()
        .map(|b| (b.uuid.clone(), b.refs.clone()))
        .collect();

    for block in blocks.iter_mut() {
        let mut path_refs = block.refs.clone();
        let mut current = block.parent_id.clone();

        while let Some(pid) = current {
            if let Some(parent_refs) = own_refs.get(&pid) {
                for r in parent_refs {
                    if !path_refs.contains(r) {
                        path_refs.push(r.clone());
                    }
                }
            }
            current = parent_map.get(&pid).and_then(|p| p.clone());
        }

        block.path_refs = path_refs;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parser() -> LogseqBlockParser {
        LogseqBlockParser::new("deadbeef01234567".to_string())
    }

    #[test]
    fn parse_simple_blocks() {
        let content = "- Block one\n- Block two\n  - Child of two\n";
        let result = parser().parse_file(content, "pages/test.md");
        assert_eq!(result.blocks.len(), 3);
        assert!(result.blocks[2].parent_id.is_some());
        assert_eq!(result.blocks[2].indent_level, 2);
    }

    #[test]
    fn parse_wikilinks() {
        let content = "- This links to [[PageA]] and [[PageB]]\n";
        let result = parser().parse_file(content, "pages/test.md");
        assert_eq!(result.blocks[0].refs.len(), 2);
        assert!(result.blocks[0].refs.contains(&"PageA".to_string()));
    }

    #[test]
    fn parse_block_refs() {
        let content = "- Embeds ((some-uuid-here))\n";
        let result = parser().parse_file(content, "pages/test.md");
        assert!(result.blocks[0]
            .refs
            .iter()
            .any(|r| r.starts_with("block-ref:")));
    }

    #[test]
    fn parse_tags() {
        let content = "- Tagged #research and #[[multi word tag]]\n";
        let result = parser().parse_file(content, "pages/test.md");
        assert!(result.blocks[0].refs.iter().any(|r| r == "tag:research"));
        assert!(result.blocks[0]
            .refs
            .iter()
            .any(|r| r == "tag:multi word tag"));
    }

    #[test]
    fn parse_task_marker() {
        let content = "- TODO Buy groceries\n- DONE Write report\n- LATER Review PR\n";
        let result = parser().parse_file(content, "pages/test.md");
        assert_eq!(result.blocks[0].task_status, Some(TaskStatus::Todo));
        assert_eq!(result.blocks[1].task_status, Some(TaskStatus::Done));
        assert_eq!(result.blocks[2].task_status, Some(TaskStatus::Later));
    }

    #[test]
    fn parse_properties() {
        let content = "- My block\n  type:: article\n  author:: John\n";
        let result = parser().parse_file(content, "pages/test.md");
        assert_eq!(result.blocks[0].properties.get("type").unwrap(), "article");
        assert_eq!(result.blocks[0].properties.get("author").unwrap(), "John");
    }

    #[test]
    fn parse_logbook_drawer() {
        let content = "- Timed task\n  :LOGBOOK:\n  CLOCK: [2024-01-15 Mon 10:00]--[2024-01-15 Mon 11:30] =>  1:30\n  :END:\n";
        let result = parser().parse_file(content, "pages/test.md");
        assert_eq!(result.blocks[0].clock_entries.len(), 1);
        assert_eq!(result.blocks[0].clock_entries[0].duration_min, Some(90));
    }

    #[test]
    fn soft_break_continuation() {
        let content = "- First line\n  continues here\n  and here too\n";
        let result = parser().parse_file(content, "pages/test.md");
        assert_eq!(result.blocks.len(), 1);
        assert!(result.blocks[0].content.contains("continues here"));
    }

    #[test]
    fn indentation_fracture_no_phantom() {
        // 0→4 space jump should NOT create phantom intermediate
        let content = "- Root\n        - Deep child\n";
        let result = parser().parse_file(content, "pages/test.md");
        assert_eq!(result.blocks.len(), 2);
        assert!(result.blocks[1].parent_id.is_some());
    }

    #[test]
    fn vault_cap_enforced() {
        let parser = LogseqBlockParser::new("test".to_string()).with_vault_cap(5);
        let content = (0..10)
            .map(|i| format!("- Block {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let result = parser.parse_file(&content, "pages/big.md");
        assert_eq!(result.blocks.len(), 5);
        assert!(result.truncated);
    }

    #[test]
    fn explicit_id_minted_with_owner() {
        let content = "- My block\n  id:: abc-123-def\n";
        let result = parser().parse_file(content, "pages/test.md");
        assert!(result.blocks[0].uuid.contains("deadbeef01234567"));
        assert!(result.blocks[0].uuid.contains("abc-123-def"));
    }

    #[test]
    fn path_refs_inherited() {
        let content = "- Parent #research\n  - Child links to [[PageA]]\n    - Grandchild\n";
        let result = parser().parse_file(content, "pages/test.md");
        let grandchild = &result.blocks[2];
        assert!(
            grandchild.path_refs.iter().any(|r| r == "tag:research"),
            "grandchild should inherit parent's #research tag via path-refs"
        );
    }

    #[test]
    fn scheduled_and_deadline() {
        let content = "- TODO Review\n  SCHEDULED: <2024-02-01 Thu>\n  DEADLINE: <2024-02-15 Thu>\n";
        let result = parser().parse_file(content, "pages/test.md");
        assert!(result.blocks[0].scheduled.is_some());
        assert!(result.blocks[0].deadline.is_some());
    }

    #[test]
    fn empty_file_no_blocks() {
        let result = parser().parse_file("", "pages/empty.md");
        assert!(result.blocks.is_empty());
    }

    #[test]
    fn heading_is_content_not_structure() {
        let content = "- # Heading One\n  - Sub-block\n";
        let result = parser().parse_file(content, "pages/test.md");
        assert_eq!(result.blocks.len(), 2);
        assert!(result.blocks[0].content.starts_with("# Heading One"));
    }
}
