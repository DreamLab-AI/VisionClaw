//! FactKG rows from Memorilla's parquet splits (`data/factkg/<split>-*.parquet`).

use anyhow::{bail, Context, Result};
use parquet::file::reader::{FileReader, SerializedFileReader};
use parquet::record::{Field, List};
use std::fs::File;
use std::path::Path;

/// One question with its collection of documents.
#[derive(Debug, Clone)]
pub struct Row {
    pub collection_id: i64,
    pub question: String,
    pub answer: String,
    pub choices: Vec<String>,
    /// Document texts, in the same order as the collection's stored embeddings.
    pub documents: Vec<String>,
}

fn strings(list: &List) -> Result<Vec<String>> {
    list.elements()
        .iter()
        .map(|f| match f {
            Field::Str(s) => Ok(s.clone()),
            other => bail!("expected a string list element, found {other:?}"),
        })
        .collect()
}

/// Every row of one split, in file order (sorted shard names, then row order).
pub fn load_split(root: &Path, config: &str, split: &str) -> Result<Vec<Row>> {
    let dir = root.join("data").join(config);
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .with_context(|| format!("read {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(&format!("{split}-")) && n.ends_with(".parquet"))
        })
        .collect();
    files.sort();
    let mut rows = Vec::new();
    for path in files {
        let reader = SerializedFileReader::new(File::open(&path)?)?;
        for record in reader.get_row_iter(None)? {
            let record = record?;
            let mut row = Row { collection_id: 0, question: String::new(), answer: String::new(), choices: vec![], documents: vec![] };
            for (name, field) in record.get_column_iter() {
                match (name.as_str(), field) {
                    ("collection_id", Field::Long(v)) => row.collection_id = *v,
                    ("question", Field::Str(s)) => row.question = s.clone(),
                    ("answer", Field::Str(s)) => row.answer = s.clone(),
                    ("choices", Field::ListInternal(l)) => row.choices = strings(l)?,
                    ("documents", Field::ListInternal(l)) => row.documents = strings(l)?,
                    _ => {}
                }
            }
            rows.push(row);
        }
    }
    Ok(rows)
}
