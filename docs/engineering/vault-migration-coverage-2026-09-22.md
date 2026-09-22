# `vault migrate` coverage against the live corpus — 2026-09-22

Produced by WS-C so WS-B can finish `ontology/vocabulary.yaml` and WS-D can run
the migration. Reproduce with:

```bash
cp -r visionGraph/knowledge /tmp/full/ && cp <vocab> /tmp/full/ontology/
vault --repo /tmp/full migrate --fences-to-properties --dry-run --json
```

The vocabulary used was `VisionClaw/crates/vault/tests/golden/fixture/ontology/vocabulary.yaml`,
which covers every fence key the 50-page golden fixture carries. Everything
listed below is a key that fixture did **not** contain, so the real vocabulary
must decide each one: map it to a frontmatter key, or list it under `ignore:` /
`logseq_keys: <key>: null`. Until then `vault migrate` exits **2** and writes
nothing — that refusal is the point.

## Scale

| measure | count |
|---|---|
| markdown files examined | 8635 |
| pages carrying fences (converted) | 8446 |
| `json-ld` fences removed | 20596 |
| Logseq `key:: value` lines | 84058 |
| `{{embed [[Page]]}}` rewritten to `![[Page]]` | 0 |
| `{{embed ((uuid))}}` block references **refused** | 35 on 14 pages |
| aliases added so a reference label still resolves | 4748 |
| long-tail references normalised onto `urn:ngm:class:` | 30445 |

The alias count is the interesting one: 4748 times, the corpus
cites a class under a label that is not that class's title. Those become
`aliases:` on the target page, which is what keeps `[[Localization]]` resolving
to `Localisation` after the fences are gone.

## Uncovered fence fields (46)

Mostly one-off predicates that escaped the twelve the pipeline reads. Each is
either a real relation (declare it under `relations:` and map it) or a typo to
be fixed at source — `relations.bridges To`, `relations.contrasts-with`,
`relations.isSubclassOf` and `relations.subClassOf` are clearly the latter.

| fence key | occurrences |
|---|---|
| `vc:plainGloss` | 56 |
| `relations.subClassOf` | 27 |
| `vc:termId` | 8 |
| `relations.depends-on` | 6 |
| `relations.informs` | 5 |
| `relations.relatedTo_2` | 5 |
| `relations.contrasts-with` | 4 |
| `relations.governs` | 4 |
| `relations.isSubclassOf` | 4 |
| `relations.constrains` | 3 |
| `relations.reduces` | 3 |
| `relations.affects` | 2 |
| `relations.detectedBy` | 2 |
| `relations.enforcedBy` | 2 |
| `relations.implementedBy` | 2 |
| `relations.informedBy` | 2 |
| `relations.mitigatedBy` | 2 |
| `relations.appliesTo` | 1 |
| `relations.bridges To` | 1 |
| `relations.causes` | 1 |
| `relations.controls` | 1 |
| `relations.defines` | 1 |
| `relations.describes` | 1 |
| `relations.detects` | 1 |
| `relations.evaluates` | 1 |
| `relations.exploits` | 1 |
| `relations.feedsInto` | 1 |
| `relations.has-part` | 1 |
| `relations.impacts` | 1 |
| `relations.improvedBy` | 1 |
| `relations.improves` | 1 |
| `relations.influences` | 1 |
| `relations.manages` | 1 |
| `relations.measuredBy` | 1 |
| `relations.mitigates` | 1 |
| `relations.precedes` | 1 |
| `relations.provides` | 1 |
| `relations.regulatedBy` | 1 |
| `relations.relatesTo_engine` | 1 |
| `relations.sameAs` | 1 |
| `relations.settledOn` | 1 |
| `relations.supportedBy` | 1 |
| `relations.underpins` | 1 |
| `relations.uses_ai` | 1 |
| `relations.uses_hw` | 1 |
| `relations.verifies` | 1 |

`vc:plainGloss` (56) and `vc:termId` (8) are page-level content, not relations.

## Uncovered Logseq keys (565)

84058 `key:: value` lines survive in page bodies. The fixture
vocabulary already covers the 565 most common
ones by mapping them to `null` — they duplicate a fence field, and the fence is
authoritative because it carries the resolved IRI where the `key:: [[Label]]`
line carries only a label. The remainder, in descending frequency:

| logseq key | occurrences |
|---|---|
| `is-part-of` | 246 |
| `enrichment-date` | 228 |
| `domain-correction` | 153 |
| `definition` | 117 |
| `public-access` | 117 |
| `research-cache` | 84 |
| `enrichment-worker` | 81 |
| `source` | 69 |
| `enrichment-phase` | 39 |
| `enrichment-model` | 36 |
| `generatedBy` | 34 |
| `inference-rule` | 33 |
| `enriched-by` | 30 |
| `enriched-date` | 24 |
| `attributed-to` | 21 |
| `generatedAt` | 21 |
| `authority-score` | 20 |
| `domain-corrected` | 19 |
| `maturity` | 19 |
| `legacy-term-id` | 18 |
| `reference-count` | 18 |
| `accuracy` | 17 |
| `completeness` | 17 |
| `applied-in` | 14 |
| `worker-model` | 14 |
| `generated-by` | 13 |
| `owl-axiom-count` | 13 |
| `component-of` | 12 |
| `domain-validation` | 12 |
| `naming-note` | 12 |
| `references-count` | 11 |
| `domain` | 10 |
| `domain-correction-note` | 10 |
| `domain-note` | 10 |
| `enrichment-sprint` | 10 |
| `phase` | 10 |
| `quality-score` | 10 |
| `characterized-by` | 9 |
| `enrichment-notes` | 9 |
| `iri-correction` | 9 |
| `relationship-count` | 9 |
| `wikilink-count` | 9 |
| `characterizedBy` | 8 |
| `legacy-term-id-assigned` | 8 |
| `alternative-to` | 7 |
| `enrichment-version` | 7 |
| `governs` | 7 |
| `owl-domain-tag` | 7 |
| `quality-notes` | 7 |
| `source-stub-lines` | 7 |
| `affects` | 6 |
| `enrichment-wave` | 6 |
| `informs` | 6 |
| `owl-axioms-count` | 6 |
| `reduces` | 6 |
| `used-by` | 6 |
| `web-searches-performed` | 6 |
| `applies-to` | 5 |
| `axiom-count` | 5 |
| `evaluated-by` | 5 |
| `iri-updated` | 5 |
| `is-instance-of` | 5 |
| `measured-by` | 5 |
| `src-lines` | 5 |
| `alternative-terms` | 4 |
| `background-color` | 4 |
| `domain-confirmed` | 4 |
| `domain-correction-rationale` | 4 |
| `domain-remap` | 4 |
| `improves` | 4 |
| `includes` | 4 |
| `related-pages` | 4 |
| `term-id` | 4 |
| `uri-correction` | 4 |
| `used-in` | 4 |
| `validator-outcome` | 4 |
| `authority-basis` | 3 |
| `axiom-families` | 3 |
| `connected-to` | 3 |
| `detectedBy` | 3 |
| `detects` | 3 |
| `domain-remap-note` | 3 |
| `enrichment-note` | 3 |
| `extended-by` | 3 |
| `has-subtype` | 3 |
| `hasSubClass` | 3 |
| `iri` | 3 |
| `iri-corrected` | 3 |
| `key-concepts` | 3 |
| `key-references` | 3 |
| `lines-target` | 3 |
| `mitigatedBy` | 3 |
| `note` | 3 |
| `owl-axiom-families` | 3 |
| `provides` | 3 |
| `related-concepts` | 3 |
| `related-standards` | 3 |
| `research-augmented` | 3 |
| `status` | 3 |
| `uk-context` | 3 |
| `uri` | 3 |
| `uri-corrected` | 3 |
| `uri-updated` | 3 |
| `usedIn` | 3 |
| `wikilink-relationship-count` | 3 |
| `wikilinks-count` | 3 |
| `word-count` | 3 |
| `academic-sources` | 2 |
| `applied-to` | 2 |
| `appliesTo` | 2 |
| `child-of` | 2 |
| `complements` | 2 |
| `composability` | 2 |
| `computes` | 2 |
| `constrains` | 2 |
| `cross-domain-bridges` | 2 |
| `cross-references` | 2 |
| `defines` | 2 |
| `detected-by` | 2 |
| `domain-correction-reason` | 2 |
| `enforcedBy` | 2 |
| `enrichment-agent` | 2 |
| `enrichment-completed` | 2 |
| `enrichment-method` | 2 |
| `enrichment-pass` | 2 |
| `estimates` | 2 |
| `exploits` | 2 |
| `extends` | 2 |
| `formalises` | 2 |
| `foundational-to` | 2 |
| `governed-by` | 2 |
| `has-implementation` | 2 |
| `implemented-in` | 2 |
| `implementedBy` | 2 |
| `influenced-by` | 2 |
| `influences` | 2 |
| `informedBy` | 2 |
| `inverse-of` | 2 |
| `key-dates` | 2 |
| `key-institutions` | 2 |
| `line-count` | 2 |
| `manages` | 2 |
| `measures` | 2 |
| `mitigated-by` | 2 |
| `owl-class-correction` | 2 |
| `owl-class-updated` | 2 |
| `owl-layer-tag` | 2 |
| `owl-maturity` | 2 |
| `owl-related-standard` | 2 |
| `phase6-enrichment` | 2 |
| `precedes` | 2 |
| `preferred-term-correction` | 2 |
| `primary-sources` | 2 |
| `primary-standards` | 2 |
| `protects` | 2 |
| `public` | 2 |
| `references` | 2 |
| `related-ontology-terms` | 2 |
| `relatedStandard` | 2 |
| `review-status` | 2 |
| `same-as` | 2 |
| `same-as-corrected` | 2 |
| `satisfies` | 2 |
| `see-also` | 2 |
| `source-line-count` | 2 |
| `subclass-of` | 2 |
| `target-line-count` | 2 |
| `total-owl-axioms` | 2 |
| `total-references` | 2 |
| `type-of` | 2 |
| `underpins` | 2 |
| `uses-technique` | 2 |
| `version` | 2 |
| `web-search-queries` | 2 |
| `webSearchPerformed` | 2 |
| `ARR-milestones` | 1 |
| `academic-source` | 1 |
| `academic-venues` | 1 |
| `accumulates` | 1 |
| `actor-profile` | 1 |
| `additional-sources` | 1 |
| `adjacent-concepts` | 1 |
| `adoption-source` | 1 |
| `agent-categories` | 1 |
| `ai-risk-intersection` | 1 |
| `aiRelevance` | 1 |
| `annual-cycle` | 1 |
| `application-domains` | 1 |
| `authority-justification` | 1 |
| `authority-rationale` | 1 |
| `avg_block_time` | 1 |
| `avg_daily_transactions` | 1 |
| `avoids` | 1 |
| `award` | 1 |
| `axiom-family-count` | 1 |
| `axioms-families` | 1 |
| `base-models` | 1 |
| `belongsToDomain` | 1 |
| `benchmark-coverage` | 1 |
| `bitcoinSpecific` | 1 |
| `blockchainRelevance` | 1 |
| `body-note` | 1 |
| `california-effect-domains` | 1 |
| `captioners` | 1 |
| `catastrophic-forgetting-prevention` | 1 |
| `catastrophic-forgetting-resistance` | 1 |
| `causes` | 1 |
| `cfg-compatibility` | 1 |
| `challenges` | 1 |
| `characterises` | 1 |
| `checkpoint-compatibility` | 1 |
| `checkpoint-transplantability` | 1 |
| `citations-semantic-scholar-2025` | 1 |
| `cites` | 1 |
| `classification-notes` | 1 |
| `claudeCodeAnnualisedRevenueUSD` | 1 |
| `co-applicable-with` | 1 |
| `co-sponsored-by` | 1 |
| `combined-with` | 1 |
| `community-checkpoints-hub` | 1 |
| `completed-at` | 1 |
| `conceptual-note` | 1 |
| `conditioning-modalities` | 1 |
| `content-classification` | 1 |
| `content-subsections` | 1 |
| `controls` | 1 |
| `coordinates-with` | 1 |
| `core-mechanism` | 1 |
| `corrected-iri` | 1 |
| `corrected-uri` | 1 |
| `correction-note` | 1 |
| `correction-rationale` | 1 |
| `countermeasure-regime` | 1 |
| `coverage-gaps` | 1 |
| `coverage-notes` | 1 |
| `coverage-summary` | 1 |
| `created` | 1 |
| `crewAIGitHubStarsApril2026` | 1 |
| `crewAIGitHubStarsJan2024` | 1 |
| `critical-for` | 1 |
| `data-quality-notes` | 1 |
| `data-sources` | 1 |
| `dataset-sources` | 1 |
| `date-corrections` | 1 |
| `deprecation_timeline` | 1 |
| `derived-by` | 1 |
| `describes` | 1 |
| `disambiguation` | 1 |
| `disambiguation-note` | 1 |
| `distinguishing-characteristics` | 1 |
| `domain-aliases` | 1 |
| `domain-categories-covered` | 1 |
| `domain-corrected-from` | 1 |
| `domain-corrected-to` | 1 |
| `domain-correction-date` | 1 |
| `domain-correction-detail` | 1 |
| `domain-correction-log` | 1 |
| `domain-namespace` | 1 |
| `domain-notes` | 1 |
| `domain-original` | 1 |
| `domain-prefix` | 1 |
| `domain-retention` | 1 |
| `domain-type` | 1 |
| `domain-verification` | 1 |
| `e2bMonthlySessionsMarch2024` | 1 |
| `e2bMonthlySessionsMarch2025` | 1 |
| `ecosystem-components` | 1 |
| `editorial-notes` | 1 |
| `effective-regulation-date-eu` | 1 |
| `elevated-from` | 1 |
| `elevation-method` | 1 |
| `empirical-data-points` | 1 |
| `enabledBy` | 1 |
| `enables-via-surrogate` | 1 |
| `enforcement-mechanism` | 1 |
| `enhances` | 1 |
| `enriched-at` | 1 |
| `enrichment-basis` | 1 |
| `enrichment-pattern` | 1 |
| `enrichment-started` | 1 |
| `enrichment-worker-model` | 1 |
| `enterprise-deployments` | 1 |
| `evaluates` | 1 |
| `evidenceSource` | 1 |
| `evolved-into` | 1 |
| `exemplar-of` | 1 |
| `exemplar-parity` | 1 |
| `exemplified-by` | 1 |
| `exhibits` | 1 |
| `facilitation-contexts` | 1 |
| `feedsInto` | 1 |
| `foundation-of` | 1 |
| `freshness-checked` | 1 |
| `freshness-updated` | 1 |
| `freshness-updates` | 1 |
| `funding-rounds` | 1 |
| `generalized-by` | 1 |
| `generates` | 1 |
| `geographic-scope` | 1 |
| `github-stars-original-repo` | 1 |
| `global-anchor-institutions` | 1 |
| `governance-model` | 1 |
| `governance-regime` | 1 |
| `grounded-in` | 1 |
| `hardware-targets` | 1 |
| `has-benchmark` | 1 |
| `has-instance` | 1 |
| `has-types` | 1 |
| `has-variant` | 1 |
| `hasApplication` | 1 |
| `historical-origin` | 1 |
| `implementation-source` | 1 |
| `implemented-by` | 1 |
| `important-dates` | 1 |
| `imposed-by` | 1 |
| `improved-by` | 1 |
| `improvedBy` | 1 |
| `industry-sources` | 1 |
| `inference-frontends` | 1 |
| `inference-vram-flux` | 1 |
| `inference-vram-sd15` | 1 |
| `inference-vram-sdxl` | 1 |
| `inherits-security-from` | 1 |
| `injection-mechanism` | 1 |
| `institutional-actors` | 1 |
| `institutional-validation` | 1 |
| `integrated-with` | 1 |
| `interacts-with` | 1 |
| `iri-correction-note` | 1 |
| `iri-namespace-correction` | 1 |
| `iri-pattern` | 1 |
| `is-governed-by` | 1 |
| `is-mitigated-by` | 1 |
| `is-monitored-by` | 1 |
| `jurisdiction-count` | 1 |
| `key-actors` | 1 |
| `key-algorithms` | 1 |
| `key-claims-verified` | 1 |
| `key-entities` | 1 |
| `key-frameworks` | 1 |
| `key-implementations` | 1 |
| `key-methods-covered` | 1 |
| `key-organisations` | 1 |
| `key-people` | 1 |
| `key-phenomena` | 1 |
| `key-platforms` | 1 |
| `key-primitives` | 1 |
| `key-relations` | 1 |
| `key-standards-tiers` | 1 |
| `key-theorists` | 1 |
| `key-vendors` | 1 |
| `lacks` | 1 |
| `last-enriched` | 1 |
| `last-reviewed` | 1 |
| `last-updated` | 1 |
| `last-validated` | 1 |
| `lastUpdated` | 1 |
| `legacy-content-preserved` | 1 |
| `legacy-term-id-assignment` | 1 |
| `legacy-term-id-correction` | 1 |
| `legacy-term-id-format` | 1 |
| `legacy-term-id-note` | 1 |
| `legacy-term-id-rationale` | 1 |
| `legal-landmarks` | 1 |
| `legislative-instruments` | 1 |
| `line-count-estimate` | 1 |
| `line-count-estimated` | 1 |
| `lines` | 1 |
| `lines-added` | 1 |
| `lines-at-completion` | 1 |
| `lines-source` | 1 |
| `live-deployments-2026` | 1 |
| `maintained_by` | 1 |
| `managed-by` | 1 |
| `manchesterAIHubGBP` | 1 |
| `marketplace` | 1 |
| `marr-prize-winning-mechanism` | 1 |
| `max-conditioning-modalities` | 1 |
| `mayHave` | 1 |
| `mcpCommunityServers` | 1 |
| `mcpMonthlySDKDownloads` | 1 |
| `measurable-by` | 1 |
| `measuredBy` | 1 |
| `methods` | 1 |
| `min-training-data` | 1 |
| `minimizes` | 1 |
| `mitigates` | 1 |
| `modern-computational-extensions` | 1 |
| `network_types` | 1 |
| `notable-sota-results` | 1 |
| `ontology` | 1 |
| `ontology-note` | 1 |
| `ontology-prefix` | 1 |
| `ontology-version` | 1 |
| `openHandsFundingUSD` | 1 |
| `operationalised-through` | 1 |
| `original-stub-sources` | 1 |
| `owl-axioms` | 1 |
| `owl-class-corrected` | 1 |
| `owl-class-iri` | 1 |
| `owl-class-prefix` | 1 |
| `owl-connects-to-complexity` | 1 |
| `owl-emergence-threshold` | 1 |
| `owl-key-benchmarks` | 1 |
| `owl-key-contributors` | 1 |
| `owl-key-variants` | 1 |
| `owl-modern-relevance` | 1 |
| `owl-physical-interpretation` | 1 |
| `owl-primary-applications` | 1 |
| `owl-primary-derived-equations` | 1 |
| `owl-production-examples` | 1 |
| `owl-software-ecosystem` | 1 |
| `owl-test-method` | 1 |
| `owl-violations-indicate` | 1 |
| `parent-concept` | 1 |
| `parent-of` | 1 |
| `pedagogical-patterns` | 1 |
| `phase-6-quality-bar-target` | 1 |
| `phase-6-rewrite-date` | 1 |
| `phase6-enrichment-notes` | 1 |
| `platform-implementations` | 1 |
| `preceded-by` | 1 |
| `preemption-status` | 1 |
| `preferred-term-note` | 1 |
| `primary-architecture` | 1 |
| `primary-authors` | 1 |
| `primary-citation` | 1 |
| `primary-concepts` | 1 |
| `primary-funder` | 1 |
| `primary-legislation-text` | 1 |
| `primary-mathematical-tools` | 1 |
| `primary-references` | 1 |
| `primary-regulatory-bodies` | 1 |
| `primary-solvers` | 1 |
| `primary-source` | 1 |
| `primary-standard` | 1 |
| `primary-theorists` | 1 |
| `primaryDomain` | 1 |
| `primary_use` | 1 |
| `prior-authority-score` | 1 |
| `prior-content-note` | 1 |
| `prior-enrichment-worker` | 1 |
| `prior-quality-score` | 1 |
| `prior-status` | 1 |
| `produced-by` | 1 |
| `production-lines` | 1 |
| `production-words` | 1 |
| `products` | 1 |
| `property-of` | 1 |
| `protocol-family` | 1 |
| `quality-assessment` | 1 |
| `quality-assurance` | 1 |
| `quality-check` | 1 |
| `quality-gate` | 1 |
| `quality-rationale` | 1 |
| `qualityScore` | 1 |
| `quantisation-levels` | 1 |
| `rationale` | 1 |
| `regulated-by` | 1 |
| `regulatedBy` | 1 |
| `regulatory-compliance` | 1 |
| `regulatory-context` | 1 |
| `regulatory-frameworks` | 1 |
| `regulatory-paradigm` | 1 |
| `regulatory-source` | 1 |
| `regulatory-status-2026` | 1 |
| `related-ontology-concepts` | 1 |
| `related-ontology-entries` | 1 |
| `related-ontology-nodes` | 1 |
| `related-ontology-pages` | 1 |
| `related-pages-verified` | 1 |
| `related-standard` | 1 |
| `related-terms` | 1 |
| `related-uk-infrastructure` | 1 |
| `related-uk-policy` | 1 |
| `relationships-count` | 1 |
| `release-year` | 1 |
| `research-queries` | 1 |
| `research-queries-used` | 1 |
| `rewrite-reason` | 1 |
| `rewrite-worker` | 1 |
| `same-as-correction` | 1 |
| `scope-disambiguation` | 1 |
| `scope-pivot` | 1 |
| `secondary-concepts` | 1 |
| `secondary-references` | 1 |
| `secondary-sources` | 1 |
| `secondary-standards` | 1 |
| `sense-disambiguation` | 1 |
| `serves` | 1 |
| `settledOn` | 1 |
| `sibling-page` | 1 |
| `source-count` | 1 |
| `source-domain` | 1 |
| `source-stub` | 1 |
| `source-stub-assessment` | 1 |
| `source-stub-words` | 1 |
| `source-urls-consulted` | 1 |
| `special-case-of` | 1 |
| `specification-count` | 1 |
| `specification-source` | 1 |
| `specification-sources` | 1 |
| `specifies` | 1 |
| `src-stub-lines` | 1 |
| `src-stub-words` | 1 |
| `standards` | 1 |
| `started-at` | 1 |
| `state_size` | 1 |
| `structural-changes` | 1 |
| `sub-domain` | 1 |
| `subdomain` | 1 |
| `subsumes` | 1 |
| `subtype-taxonomy` | 1 |
| `suffers-from` | 1 |
| `supported-base-models` | 1 |
| `supportedBy` | 1 |
| `sweBenchBestScore` | 1 |
| `sweBenchProBestScore` | 1 |
| `taproot-assets-source` | 1 |
| `target-lines` | 1 |
| `target-word-count` | 1 |
| `technique-for` | 1 |
| `temporal-scope` | 1 |
| `term-count` | 1 |
| `term-id-note` | 1 |
| `termID` | 1 |
| `terminalBenchBestScore` | 1 |
| `terminalBenchVersion` | 1 |
| `theoretical-foundation` | 1 |
| `threat-tier` | 1 |
| `threatens` | 1 |
| `threshold-type` | 1 |
| `toolchain` | 1 |
| `toolchains` | 1 |
| `total-relationships` | 1 |
| `total-wikilinks` | 1 |
| `trained-via` | 1 |
| `training-dataset-scale` | 1 |
| `uk-academic` | 1 |
| `uk-anchor-institutions` | 1 |
| `uk-anchors` | 1 |
| `uk-context-source` | 1 |
| `uk-industry` | 1 |
| `uk-significance` | 1 |
| `uk-sources` | 1 |
| `validates` | 1 |
| `validation-note` | 1 |
| `validator_count` | 1 |
| `variant-of` | 1 |
| `verifies` | 1 |
| `version-bump` | 1 |
| `violated-by` | 1 |
| `vulnerable-to` | 1 |
| `wikilink-relationship-types` | 1 |
| `wikilink-relationships-count` | 1 |
| `wikilink-types-covered` | 1 |
| `wikilinks` | 1 |
| `wikilinks-by-type` | 1 |
| `windSurfAcquisitionUSD` | 1 |
| `word-count-estimate` | 1 |
| `word-count-estimated` | 1 |
| `words-at-completion` | 1 |
| `words-estimate` | 1 |
| `words-target` | 1 |
| `zero-convolution-based` | 1 |

## Unconvertible embeds (35 on 14 pages)

`{{embed [[Page]]}}` becomes `![[Page]]`. A Logseq **block** reference,
`{{embed ((66f13d66-…))}}`, names a block in a database that no longer
exists; there is no Obsidian equivalent and no way to guess one. The migration
refuses rather than leaving residue that `vault validate` would fail on later.

| page | embeds |
|---|---|
| `Token Embedding` | 11 |
| `Treatment Planning AI` | 6 |
| `AI Development` | 2 |
| `AI Frontier Capability Survey` | 2 |
| `Dr O'Hare Writing for LogSeq` | 2 |
| `Social Impact` | 2 |
| `Style Transfer` | 2 |
| `Task Specific Head` | 2 |
| `AI Literacy Training for Designers` | 1 |
| `Agentic AI Practitioner Training Programme` | 1 |
| `Knowledge Graph Presentation Session Artefact` | 1 |
| `Landvaettir Generative AI Art Research` | 1 |
| `Metaverse and Telecollaboration` | 1 |
| `Technical History (extended CV)` | 1 |

`working/` carries a further 41 by the same rule.

Each needs an owner decision at source: resolve to a page embed, inline the
text, or delete the line.

## Recommendation

Three buckets, and the vocabulary should say which each key is in:

1. **Duplicates a fence field** — `logseq_keys: <key>: null`. The large
   majority.
2. **Real content the fences never captured** — declare a scalar or relation
   and map it. `accuracy`, `authority-score`, `academic-sources` and the
   `alternative-terms` family look like this.
3. **Noise from an earlier tool** — `background-color`, `collapsed`, `id`.
   `null`, and delete the lines.

Nothing here blocks `vault build`; it blocks `vault migrate`, by design.
