//! Per-page header census through visionclaw-domain PageMeta (read-only). Usage: census_domain <repo> <out.jsonl>
use std::io::Write;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let root = std::path::Path::new(&a[1]);
    let mut out = std::io::BufWriter::new(std::fs::File::create(&a[2]).unwrap());
    let mut files = Vec::new();
    for vault in ["knowledge", "working"] {
        let mut stack = vec![root.join(vault)];
        while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else {
                continue;
            };
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                let ft = e.file_type().unwrap();
                if n.starts_with('.') {
                    continue;
                }
                if ft.is_dir() {
                    if matches!(n.as_str(), "templates" | "bases" | "assets") {
                        continue;
                    }
                    stack.push(e.path());
                } else if ft.is_file() && n.ends_with(".md") {
                    files.push(e.path());
                }
            }
        }
    }
    files.sort();
    for p in files {
        let rel = p.strip_prefix(root).unwrap().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&p).unwrap_or_default();
        let m = visionclaw_domain::vault::parse(&text);
        let line = serde_json::json!({
            "path": rel,
            "kg_included": m.is_kg_included(),
            "publishable": m.is_publishable(),
            "generated_by": m.generated.as_ref().map(|g| g.by.clone()),
        });
        writeln!(out, "{line}").unwrap();
    }
}
