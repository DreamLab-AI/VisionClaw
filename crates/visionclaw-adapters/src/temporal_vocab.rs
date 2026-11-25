//! The vocabulary of temporal contract v2 (ADR-2219), in one place.
//!
//! `temporal:` and `memory:` terms are taken from eljeffeg/memory-ontology
//! (CC BY 4.0); `dl:` terms are VisionClaw's own.

/// `dl:` namespace (decision layer; closure terms).
pub const DL_NS: &str = "https://narrativegoldmine.com/ns/dl#";
/// memory-ontology `temporal:` namespace.
pub const TEMPORAL_NS: &str = "http://nova.dev/temporal#";
/// memory-ontology `memory:` namespace.
pub const MEMORY_NS: &str = "http://nova.dev/memory#";

/// A version's valid-time start (`xsd:dateTimeStamp`).
pub const TEMPORAL_VALID_FROM: &str = "http://nova.dev/temporal#validFrom";
/// A version's recorded-time start (`xsd:dateTimeStamp`).
pub const TEMPORAL_RECORDED_FROM: &str = "http://nova.dev/temporal#recordedFrom";

/// Class of a closure record.
pub const DL_CLOSURE: &str = "https://narrativegoldmine.com/ns/dl#Closure";
/// Closure → the version it closes.
pub const DL_CLOSES_VERSION: &str = "https://narrativegoldmine.com/ns/dl#closesVersion";
/// Closure → a clock it closes.
pub const DL_CLOSES: &str = "https://narrativegoldmine.com/ns/dl#closes";
/// Exclusive valid end of a valid-time closure (`xsd:dateTimeStamp`).
pub const DL_VALID_UNTIL: &str = "https://narrativegoldmine.com/ns/dl#validUntil";
/// Optional application-policy reason on a closure.
pub const DL_REASON: &str = "https://narrativegoldmine.com/ns/dl#reason";

/// The valid-time clock.
pub const MEMORY_VALID_TIME: &str = "http://nova.dev/memory#ValidTime";
/// The recorded-time clock.
pub const MEMORY_RECORDED_TIME: &str = "http://nova.dev/memory#RecordedTime";

#[cfg(test)]
mod tests {
    use super::*;
    use oxigraph::model::vocab::xsd;
    use oxigraph::model::{GraphName, Literal, NamedNode, Quad};
    use oxigraph::sparql::QueryResults;
    use oxigraph::store::Store;

    #[test]
    fn prefixed_terms_agree_with_their_namespaces() {
        assert_eq!(TEMPORAL_VALID_FROM, format!("{TEMPORAL_NS}validFrom"));
        assert_eq!(TEMPORAL_RECORDED_FROM, format!("{TEMPORAL_NS}recordedFrom"));
        assert_eq!(MEMORY_VALID_TIME, format!("{MEMORY_NS}ValidTime"));
        assert_eq!(MEMORY_RECORDED_TIME, format!("{MEMORY_NS}RecordedTime"));
        for t in [
            DL_CLOSURE,
            DL_CLOSES_VERSION,
            DL_CLOSES,
            DL_VALID_UNTIL,
            DL_REASON,
        ] {
            assert!(t.starts_with(DL_NS), "{t}");
        }
    }

    /// An `xsd:dateTimeStamp` literal compares by value, against both
    /// `xsd:dateTimeStamp` and `xsd:dateTime` literals, in the pinned Oxigraph.
    #[test]
    fn date_time_stamp_compares_by_value_with_date_time() {
        let store = Store::new().unwrap();
        let s = NamedNode::new_unchecked("urn:t:s");
        let p = NamedNode::new_unchecked(TEMPORAL_VALID_FROM);
        let o = Literal::new_typed_literal("2026-11-19T13:26:13Z", xsd::DATE_TIME_STAMP);
        store
            .insert(&Quad::new(s, p, o, GraphName::DefaultGraph))
            .unwrap();
        let ask = |op: &str, rhs: &str| {
            let q = format!(
                "PREFIX t: <{TEMPORAL_NS}> PREFIX xsd: <http://www.w3.org/2001/XMLSchema#> \
                 ASK {{ ?s t:validFrom ?v FILTER (?v {op} {rhs}) }}"
            );
            matches!(store.query(&q).unwrap(), QueryResults::Boolean(true))
        };
        let dt = |v: &str| format!("\"{v}\"^^xsd:dateTime");
        let stamp = |v: &str| format!("\"{v}\"^^xsd:dateTimeStamp");
        for lit in [dt, stamp] {
            assert!(ask("<=", &lit("2026-11-19T13:26:13Z")), "equal instant");
            assert!(ask("<", &lit("2026-11-19T13:26:14Z")));
            assert!(!ask("<", &lit("2026-11-19T13:26:13Z")), "strict");
            assert!(ask(">", &lit("2026-11-19T13:26:12Z")));
            // The same instant written with an offset is equal by value.
            assert!(ask("<=", &lit("2026-11-19T14:26:13+01:00")));
            assert!(ask(">=", &lit("2026-11-19T14:26:13+01:00")));
        }
    }
}
