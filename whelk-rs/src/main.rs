#[macro_use]
extern crate log;

use clap::Parser;
use humantime::format_duration;
use std::error;
use std::path;
use std::time;
use whelk::read_input;
use whelk::whelk::owl::translate_ontology;
use whelk::whelk::reasoner::assert;

#[derive(Parser, Debug)]
#[clap(name = "whelk", about = "whelk")]
struct Options {
    #[clap(short = 'i', long = "input", long_help = "expects an *.owl file", required = true)]
    input: path::PathBuf,
}
fn main() -> Result<(), Box<dyn error::Error>> {
    let start = time::Instant::now();
    env_logger::init();

    let options = Options::parse();
    debug!("{:?}", options);

    let path: &path::PathBuf = &options.input;
    let ontology = read_input(&path).expect("unable to parse input");
    debug!("Loaded ontology in {}s", start.elapsed().as_secs());

    // let summary = horned_bin::summary::summarize(ont.clone());
    // debug!("Logical Axioms: {}, Annotation Axioms: {}", summary.logical_axiom, summary.annotation_axiom);

    let start_convert = time::Instant::now();
    let whelk_axioms = translate_ontology(&ontology);
    debug!("Converted axioms in {}ms", start_convert.elapsed().as_millis());
    debug!("whelk_axioms.len(): {}", whelk_axioms.len());

    let start_reason = time::Instant::now();
    let _whelk = assert(&whelk_axioms);
    debug!("Reasoned in {}s", start_reason.elapsed().as_secs());
    // for (sub, sup) in whelk.named_subsumptions() {
    //     println!("{}\t{}", sub.id, sup.id);
    // }

    // let ci = ConceptInclusion {
    //     subclass: Rc::new(Concept::AtomicConcept(Rc::new(AtomicConcept { id: "foo".to_string() }))),
    //     superclass: Rc::new(Concept::AtomicConcept(Rc::new(AtomicConcept { id: "bar".to_string() }))),
    // };
    // let new_axioms = hashset![Rc::new(ci)];
    // let whelk2 = assert_append(&new_axioms, &whelk);
    // println!("{}", whelk2 == whelk);
    // let whelk3 = whelk2.clone();
    // println!("{}", whelk3 == whelk);
    // println!("{}", whelk3 == whelk2);
    info!("Duration: {}", format_duration(start.elapsed()).to_string());
    Ok(())
}
