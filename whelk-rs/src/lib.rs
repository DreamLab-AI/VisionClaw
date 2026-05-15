extern crate log;

use horned_owl::io::ParserConfiguration;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::fs::File;
use std::io::BufReader;
use std::{error, path};

pub mod whelk {
    pub mod model;
    pub mod owl;
    pub mod reasoner;
}

pub fn read_input(input_path: &path::PathBuf) -> Result<SetOntology<RcStr>, Box<dyn error::Error>> {
    let file = File::open(&input_path)?;
    let mut bufreader = BufReader::new(file);
    let config = ParserConfiguration::default();
    match input_path.extension().and_then(|s| s.to_str()) {
        Some("owx") => {
            let ret = horned_owl::io::owx::reader::read(&mut bufreader, config)?;
            Ok(ret.0)
        }
        Some("owl") => {
            let ret = horned_owl::io::rdf::reader::read(&mut bufreader, config)?;
            Ok(ret.0.into())
        }
        _ => Err(Box::<dyn error::Error>::from("unable to parse input")),
    }
}
