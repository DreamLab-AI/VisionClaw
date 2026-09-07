//! Boot-time effective-profile assertion (ADR-2038, closing ADR-2012 / ADR-2026 /
//! ADR-2027 / ADR-2037).
//!
//! ADR-2027 ratified three named deployment profiles but left them as prose:
//! "nothing at boot asserts the running env matches a named profile, so drift is
//! possible until a selector lands". ADR-2012 found the report-mode gap — a
//! non-debug build can disable RBAC enforcement with a current-date
//! acknowledgement, and the mode is captured once at middleware construction
//! and never re-checked. ADR-2037 asked for a rejection that runs *before the
//! listener binds*, so a mis-promoted artefact never accepts a request at all.
//!
//! This module is that selector. It is a pure function over an environment
//! snapshot, the build identity and the current UTC date:
//!
//! ```text
//!   EnvSnapshot + BuildIdentity + today  ->  Result<EffectiveProfile, ProfileRejection>
//! ```
//!
//! Nothing here reads the process environment or the clock on its own, so the
//! whole acceptance matrix runs as ordinary unit tests with no global state and
//! no test-ordering hazard. `main.rs` calls [`assert_effective_profile_or_exit`]
//! once, before `HttpServer::bind`.
//!
//! ## What counts as a rejection
//!
//! In a **production build** (no `debug_assertions`, no `dev-auth` feature)
//! every [`ProfileFinding`] below is fatal. In a development build the same
//! findings are reported and the boot continues — a developer's machine is
//! expected to carry them.
//!
//! * A forbidden development variable is **present**, whatever its value.
//!   `SETTINGS_AUTH_BYPASS=0` is a rejection, not a disabled feature: the
//!   variable's presence is the signal that a development configuration was
//!   promoted, and reading `0` as "off" is exactly the misreading that lets one
//!   through. Missing and zero-valued are therefore different outcomes, and the
//!   test matrix asserts both.
//! * RBAC report mode is requested. Report mode forwards denials instead of
//!   enforcing them; a dated acknowledgement makes it *auditable*, not
//!   *acceptable in production*.
//! * The artefact itself carries `dev-auth`. A dev-auth binary retains the
//!   loopback dev-token branch, so it must never be promoted (ADR-2037).
//! * `--allow-skip-auth` appears in argv.
//! * A declared profile does not match the observed flags (ADR-2027 drift).

use std::collections::BTreeMap;
use std::fmt;

/// Environment variable naming the deployment profile the operator intends.
///
/// When set, the observed flags must match that profile exactly; a mismatch is
/// configuration drift and is fatal in a production build. When unset, the
/// observed flags are still classified, but missing intent or an unrecognised
/// combination is a finding and prevents release builds from binding.
pub const SECURITY_PROFILE_ENV: &str = "VISIONCLAW_SECURITY_PROFILE";

/// Development variables whose mere presence indicates a promoted dev config.
/// Extends the ADR-2026 `SUSPECT_ENVS` list with the dev-token opt-in
/// (ADR-2012) that the original hygiene check did not cover.
pub const FORBIDDEN_DEV_VARS: &[&str] = &[
    "SETTINGS_AUTH_BYPASS",
    "ALLOW_INSECURE_DEFAULTS",
    "VISIONCLAW_DEV_MODE",
    "DEV_AUTH_LOOPBACK",
];

/// The six composable security flags the profile table names, in table order.
///
/// ADR-2027's Decision text originally said "four", omitting `RBAC_OWNER_PUBKEY`
/// and `RBAC_GATE_MODE`, which the table in `docs/SECURITY-profiles.md` has
/// always carried as rows. The count was corrected at the root in ADR-2027
/// (2026-09-05); this array was always six and is the authority.
pub const PROFILE_FLAGS: [&str; 6] = [
    "RBAC_PUBLIC_READS",
    "RBAC_ALLOW_OWNERLESS",
    "RBAC_OWNER_PUBKEY",
    "RBAC_DEFAULT_ROLE",
    "PUBKEY_VISIBILITY_FILTER",
    "RBAC_GATE_MODE",
];

/// A ratified deployment profile (ADR-2027).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DeploymentProfile {
    /// Public read-only kiosk: anonymous reads on, no Owner required.
    DemoOpen,
    /// One operator, private graph: reads require auth, ownerless tolerated.
    SingleTenant,
    /// Hardened multi-tenant: Owner mandatory, no anonymous reads, unknown
    /// signers read-only.
    MultiUserLocked,
}

impl DeploymentProfile {
    /// Every ratified profile, in increasing order of lockdown.
    pub const ALL: [DeploymentProfile; 3] = [
        DeploymentProfile::DemoOpen,
        DeploymentProfile::SingleTenant,
        DeploymentProfile::MultiUserLocked,
    ];

    /// Canonical wire/env name.
    pub fn as_str(&self) -> &'static str {
        match self {
            DeploymentProfile::DemoOpen => "demo-open",
            DeploymentProfile::SingleTenant => "single-tenant",
            DeploymentProfile::MultiUserLocked => "multi-user-locked",
        }
    }

    /// Parse a profile name, tolerating case and `_`/`-` spelling.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().replace('_', "-").as_str() {
            "demo-open" => Some(DeploymentProfile::DemoOpen),
            "single-tenant" => Some(DeploymentProfile::SingleTenant),
            "multi-user-locked" => Some(DeploymentProfile::MultiUserLocked),
            _ => None,
        }
    }

    /// The exact flag set for this profile, matching the table in
    /// `docs/SECURITY-profiles.md`. `None` means "must be unset".
    /// `Some(FlagExpectation::AnyNonEmpty)` covers `RBAC_OWNER_PUBKEY`, whose
    /// value is deployment-specific but whose presence is not.
    pub fn expected_flags(&self) -> BTreeMap<&'static str, FlagExpectation> {
        let (public_reads, ownerless, owner_key, default_role) = match self {
            DeploymentProfile::DemoOpen => ("1", "1", FlagExpectation::Unset, "editor"),
            DeploymentProfile::SingleTenant => ("0", "1", FlagExpectation::AnyNonEmpty, "editor"),
            DeploymentProfile::MultiUserLocked => {
                ("0", "0", FlagExpectation::AnyNonEmpty, "viewer")
            }
        };
        let mut m = BTreeMap::new();
        m.insert("RBAC_PUBLIC_READS", FlagExpectation::Exactly(public_reads));
        m.insert("RBAC_ALLOW_OWNERLESS", FlagExpectation::Exactly(ownerless));
        m.insert("RBAC_OWNER_PUBKEY", owner_key);
        m.insert("RBAC_DEFAULT_ROLE", FlagExpectation::Exactly(default_role));
        m.insert("PUBKEY_VISIBILITY_FILTER", FlagExpectation::Exactly("1"));
        m.insert("RBAC_GATE_MODE", FlagExpectation::Exactly("enforce"));
        m
    }
}

impl fmt::Display for DeploymentProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What a profile requires of one flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlagExpectation {
    /// The variable must be absent (or empty, which compose interpolation makes
    /// equivalent to absent for these flags).
    Unset,
    /// The variable must be present with any non-empty value.
    AnyNonEmpty,
    /// The variable must hold exactly this value, compared case-insensitively
    /// after trimming. `RBAC_GATE_MODE` unset also satisfies `enforce`, since
    /// enforce is the code default.
    Exactly(&'static str),
}

/// A read-only snapshot of the process environment plus argv, taken once at
/// boot so every check sees the same values.
#[derive(Clone, Debug, Default)]
pub struct EnvSnapshot {
    vars: BTreeMap<String, String>,
    argv: Vec<String>,
}

impl EnvSnapshot {
    /// Capture the real process environment and argv.
    pub fn from_process() -> Self {
        Self {
            vars: std::env::vars().collect(),
            argv: std::env::args().collect(),
        }
    }

    /// Build a snapshot from explicit pairs — the constructor the acceptance
    /// matrix uses, so no test mutates global process state.
    pub fn from_pairs<I, K, V>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        Self {
            vars: pairs
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
            argv: Vec::new(),
        }
    }

    /// Replace the argv the snapshot reports.
    pub fn with_argv<I, S>(mut self, argv: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.argv = argv.into_iter().map(Into::into).collect();
        self
    }

    /// The raw value of `name`, if the variable is present at all.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.vars.get(name).map(String::as_str)
    }

    /// Is `name` present, whatever its value? This — not truthiness — is what
    /// the forbidden-variable check asks, so `VAR=0` and `VAR=` both count.
    pub fn is_present(&self, name: &str) -> bool {
        self.vars.contains_key(name)
    }

    /// The trimmed value, treating an all-whitespace value as absent for flag
    /// comparison (compose writes `VAR: ""` for "leave at default").
    fn effective(&self, name: &str) -> Option<&str> {
        self.get(name).map(str::trim).filter(|v| !v.is_empty())
    }

    fn argv_contains(&self, flag: &str) -> bool {
        self.argv.iter().any(|a| a == flag)
    }
}

/// Which security-relevant code the running binary actually contains.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildIdentity {
    /// `cfg!(debug_assertions)` — a debug build.
    pub debug_assertions: bool,
    /// `cfg!(feature = "dev-auth")` — the dev-token branch is compiled in.
    pub dev_auth: bool,
}

impl BuildIdentity {
    /// The identity of *this* binary, resolved at compile time.
    pub const fn current() -> Self {
        Self {
            debug_assertions: cfg!(debug_assertions),
            dev_auth: cfg!(feature = "dev-auth"),
        }
    }

    /// A production artefact carries neither debug assertions nor dev-auth.
    /// Only such a build enforces the profile; a development build reports.
    pub const fn is_production_artefact(&self) -> bool {
        !self.debug_assertions && !self.dev_auth
    }

    /// Short label for logs and receipts.
    pub fn label(&self) -> &'static str {
        match (self.debug_assertions, self.dev_auth) {
            (false, false) => "release/no-dev-auth",
            (false, true) => "release/dev-auth",
            (true, false) => "debug/no-dev-auth",
            (true, true) => "debug/dev-auth",
        }
    }
}

/// One thing wrong with the effective profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileFinding {
    /// A forbidden development variable is present. `value` is retained so the
    /// operator can see that `=0` did not make it safe.
    ForbiddenDevVariable { name: String, value: String },
    /// `NODE_ENV=development` together with `DOCKER_ENV` (ADR-2026 §D11).
    DevelopmentNodeEnvInContainer,
    /// `--allow-skip-auth` present in argv.
    AllowSkipAuthArgv,
    /// The binary itself carries the `dev-auth` feature (ADR-2037).
    DevAuthFeatureInArtefact,
    /// RBAC report mode is requested. `acknowledged` records whether the dated
    /// acknowledgement was valid — either way it is fatal in production, which
    /// is precisely the ADR-2012 gap.
    ReportModeRequested {
        acknowledged: bool,
        ack_value: Option<String>,
        today: String,
    },
    /// The declared profile does not match an observed flag (ADR-2027 drift).
    ProfileDrift {
        profile: DeploymentProfile,
        flag: String,
        expected: String,
        observed: Option<String>,
    },
    /// `VISIONCLAW_SECURITY_PROFILE` names something that is not a ratified
    /// profile.
    UnknownDeclaredProfile { declared: String },
    /// Production deployments must name their intended security profile.
    MissingDeclaredProfile,
    /// Observed flags match none of the supported profiles.
    UnnamedEffectiveProfile,
    /// ADR-2043: anonymous reads are enabled while the visibility filter is
    /// disabled — the full-disclosure pair. `RBAC_PUBLIC_READS=1` serves
    /// `/api` reads to unauthenticated callers, and `PUBKEY_VISIBILITY_FILTER=0`
    /// puts private nodes on the wire unredacted. Either alone is a supported
    /// posture; together they publish the whole graph to anyone who can reach
    /// the port. Raised unconditionally, whether or not a profile is declared.
    FullDisclosureFlagPair {
        public_reads: String,
        visibility_filter: String,
    },
}

impl fmt::Display for ProfileFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProfileFinding::ForbiddenDevVariable { name, value } => write!(
                f,
                "development variable {name}={value:?} is present; its presence \
(not its value — {name}=0 counts) indicates a development configuration was promoted"
            ),
            ProfileFinding::DevelopmentNodeEnvInContainer => f.write_str(
                "NODE_ENV=development together with DOCKER_ENV indicates a development container image",
            ),
            ProfileFinding::AllowSkipAuthArgv => {
                f.write_str("--allow-skip-auth is present in argv")
            }
            ProfileFinding::DevAuthFeatureInArtefact => f.write_str(
                "the binary was built with --features dev-auth, so it retains the loopback \
dev-token bypass; a dev-auth artefact must never be promoted",
            ),
            ProfileFinding::ReportModeRequested {
                acknowledged,
                ack_value,
                today,
            } => write!(
                f,
                "RBAC_GATE_MODE=report is requested (acknowledged={acknowledged}, \
RBAC_REPORT_MODE_ACK={ack_value:?}, today={today}); report mode forwards denials \
instead of enforcing them and is never acceptable in a production profile"
            ),
            ProfileFinding::ProfileDrift {
                profile,
                flag,
                expected,
                observed,
            } => write!(
                f,
                "profile {profile} requires {flag}={expected}, observed {observed:?}"
            ),
            ProfileFinding::UnknownDeclaredProfile { declared } => write!(
                f,
                "{SECURITY_PROFILE_ENV}={declared:?} is not a ratified profile \
(expected demo-open, single-tenant or multi-user-locked)"
            ),
            ProfileFinding::MissingDeclaredProfile => f.write_str("VISIONCLAW_SECURITY_PROFILE must name the intended profile"),
            ProfileFinding::UnnamedEffectiveProfile => f.write_str("observed security flags match no supported profile"),
            ProfileFinding::FullDisclosureFlagPair {
                public_reads,
                visibility_filter,
            } => write!(
                f,
                "full-disclosure combination: RBAC_PUBLIC_READS={public_reads:?} serves /api reads \
to unauthenticated callers while PUBKEY_VISIBILITY_FILTER={visibility_filter:?} puts private \
nodes on the wire unredacted — every node of every user is published to anyone who can reach \
the port (ADR-2003, ADR-2043). Set PUBKEY_VISIBILITY_FILTER=1, or turn off RBAC_PUBLIC_READS"
            ),
        }
    }
}

/// The profile the running process actually has.
#[derive(Clone, Debug)]
pub struct EffectiveProfile {
    /// What the binary contains.
    pub build: BuildIdentity,
    /// The profile the operator declared, if any.
    pub declared: Option<DeploymentProfile>,
    /// The profile the observed flags match, if they match one exactly.
    pub classified: Option<DeploymentProfile>,
    /// The observed value of every profile flag, for the boot receipt.
    pub observed_flags: BTreeMap<String, Option<String>>,
    /// Everything wrong with it. Empty means a clean profile.
    pub findings: Vec<ProfileFinding>,
}

impl EffectiveProfile {
    /// Is the process allowed to bind its listener?
    ///
    /// Every non-debug artefact must have no findings, including release builds
    /// carrying dev-auth. Debug builds report findings and continue.
    pub fn may_bind_listener(&self) -> bool {
        self.build.debug_assertions || self.findings.is_empty()
    }

    /// A stable one-line summary for logs and for the boot receipt.
    pub fn summary(&self) -> String {
        format!(
            "build={} declared={} classified={} findings={}",
            self.build.label(),
            self.declared.map(|p| p.as_str()).unwrap_or("<none>"),
            self.classified.map(|p| p.as_str()).unwrap_or("<unnamed>"),
            self.findings.len()
        )
    }
}

/// Does `observed` satisfy `expectation`?
fn flag_satisfied(flag: &str, expectation: FlagExpectation, observed: Option<&str>) -> bool {
    match expectation {
        FlagExpectation::Unset => observed.is_none(),
        FlagExpectation::AnyNonEmpty => observed.is_some(),
        FlagExpectation::Exactly(want) => match observed {
            Some(got) => got.eq_ignore_ascii_case(want),
            // `RBAC_GATE_MODE` unset means enforce, the code default.
            None => flag == "RBAC_GATE_MODE" && want.eq_ignore_ascii_case("enforce"),
        },
    }
}

fn describe(expectation: FlagExpectation) -> String {
    match expectation {
        FlagExpectation::Unset => "<unset>".to_string(),
        FlagExpectation::AnyNonEmpty => "<any non-empty value>".to_string(),
        FlagExpectation::Exactly(v) => v.to_string(),
    }
}

/// Are anonymous `/api` reads enabled, read off an [`EnvSnapshot`]?
///
/// ADR-2043: this must agree exactly with `rbac_gate::public_reads_enabled`,
/// which is the function that actually admits the request at runtime — only
/// `1` or `true` (trimmed, case-insensitive for `true`) enable it, and absence
/// means auth-required. Duplicated here rather than called because the gate
/// reads the live process environment while the assertion reads the boot
/// snapshot, and the whole point of the snapshot is that every check sees the
/// same values.
pub fn public_reads_enabled_in(env: &EnvSnapshot) -> bool {
    env.get("RBAC_PUBLIC_READS")
        .map(|v| {
            let v = v.trim();
            v == "1" || v.eq_ignore_ascii_case("true")
        })
        .unwrap_or(false)
}

/// Is the ADR-060 pubkey visibility filter enabled, read off an [`EnvSnapshot`]?
///
/// ADR-2043: mirrors `position_updates::parse_visibility_flag` — the filter is
/// ON by default and only an explicit `0`, `false`, `off` or `no` disables it,
/// so an unrecognised value fails safe (filter stays on).
pub fn visibility_filter_enabled_in(env: &EnvSnapshot) -> bool {
    match env.get("PUBKEY_VISIBILITY_FILTER") {
        Some(v) => !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "off" | "no"
        ),
        None => true,
    }
}

/// Is RBAC report mode being requested at all?
pub fn report_mode_requested(env: &EnvSnapshot) -> bool {
    env.get("RBAC_GATE_MODE")
        .map(|v| v.trim().eq_ignore_ascii_case("report"))
        .unwrap_or(false)
}

/// Is a report-mode request acknowledged?
///
/// This is the single implementation of the ADR-2012 dated-acknowledgement rule,
/// shared with the RBAC gate so the two cannot drift:
///
/// * a debug build acknowledges implicitly;
/// * any other build needs `RBAC_REPORT_MODE_ACK` to equal `today` exactly,
///   so the acknowledgement expires at the UTC date rollover.
pub fn report_mode_acknowledged(env: &EnvSnapshot, build: BuildIdentity, today: &str) -> bool {
    if build.debug_assertions {
        return true;
    }
    env.get("RBAC_REPORT_MODE_ACK")
        .map(|v| v.trim() == today)
        .unwrap_or(false)
}

/// Compute the effective profile. Pure: no environment, clock or logging.
///
/// `today` is the current UTC date formatted `YYYY-MM-DD`.
pub fn evaluate_effective_profile(
    env: &EnvSnapshot,
    build: BuildIdentity,
    today: &str,
) -> EffectiveProfile {
    let mut findings = Vec::new();

    // 1. Forbidden development variables — presence, not truthiness.
    for name in FORBIDDEN_DEV_VARS {
        if env.is_present(name) {
            findings.push(ProfileFinding::ForbiddenDevVariable {
                name: (*name).to_string(),
                value: env.get(name).unwrap_or_default().to_string(),
            });
        }
    }

    // 2. Development container fingerprint (ADR-2026 §D11).
    let node_env_dev = env
        .get("NODE_ENV")
        .map(|v| v.trim().eq_ignore_ascii_case("development"))
        .unwrap_or(false);
    if node_env_dev && env.is_present("DOCKER_ENV") {
        findings.push(ProfileFinding::DevelopmentNodeEnvInContainer);
    }

    // 3. Argv refusal.
    if env.argv_contains("--allow-skip-auth") {
        findings.push(ProfileFinding::AllowSkipAuthArgv);
    }

    // 4. Artefact identity (ADR-2037): a dev-auth binary is never promotable.
    //    Only meaningful for a non-debug build — a debug build is not a
    //    candidate for promotion in the first place.
    if build.dev_auth && !build.debug_assertions {
        findings.push(ProfileFinding::DevAuthFeatureInArtefact);
    }

    // 5. Report mode (ADR-2012): requesting it at all is a production finding,
    //    acknowledged or not.
    if report_mode_requested(env) {
        findings.push(ProfileFinding::ReportModeRequested {
            acknowledged: report_mode_acknowledged(env, build, today),
            ack_value: env.get("RBAC_REPORT_MODE_ACK").map(str::to_string),
            today: today.to_string(),
        });
    }

    // 5b. ADR-2043 — the ADR-2003 full-disclosure pair, rejected on its own
    //     terms rather than only as profile drift.
    //
    //     Before ADR-2043 this combination was caught only indirectly: it
    //     matches no ratified profile, so a deployment that DECLARED one got
    //     `ProfileDrift` while a deployment that declared nothing was merely
    //     classified `Unnamed` and bound anyway. That is backwards relative to
    //     the risk — the careless deployment was the one that got through. The
    //     rule below is keyed on the pair itself and runs unconditionally.
    //
    //     Each flag is read with the SAME semantics as its live consumer, so
    //     the assertion cannot disagree with the runtime behaviour it predicts:
    //     `RBAC_PUBLIC_READS` mirrors `rbac_gate::public_reads_enabled` (only
    //     `1`/`true` enable, absence is off), and `PUBKEY_VISIBILITY_FILTER`
    //     mirrors `position_updates::parse_visibility_flag` (default ON, only
    //     `0`/`false`/`off`/`no` disable).
    if public_reads_enabled_in(env) && !visibility_filter_enabled_in(env) {
        findings.push(ProfileFinding::FullDisclosureFlagPair {
            public_reads: env.get("RBAC_PUBLIC_READS").unwrap_or_default().to_string(),
            visibility_filter: env
                .get("PUBKEY_VISIBILITY_FILTER")
                .unwrap_or_default()
                .to_string(),
        });
    }

    // 6. Declared-profile drift (ADR-2027).
    let declared_raw = env.effective(SECURITY_PROFILE_ENV);
    let declared = match declared_raw {
        None => None,
        Some(raw) => match DeploymentProfile::parse(raw) {
            Some(p) => Some(p),
            None => {
                findings.push(ProfileFinding::UnknownDeclaredProfile {
                    declared: raw.to_string(),
                });
                None
            }
        },
    };
    if let Some(profile) = declared {
        for (flag, expectation) in profile.expected_flags() {
            let observed = env.effective(flag);
            if !flag_satisfied(flag, expectation, observed) {
                findings.push(ProfileFinding::ProfileDrift {
                    profile,
                    flag: flag.to_string(),
                    expected: describe(expectation),
                    observed: observed.map(str::to_string),
                });
            }
        }
    }

    // 7. Classification: which ratified profile do the observed flags match?
    let classified = DeploymentProfile::ALL.into_iter().find(|profile| {
        profile
            .expected_flags()
            .into_iter()
            .all(|(flag, expectation)| flag_satisfied(flag, expectation, env.effective(flag)))
    });

    if declared_raw.is_none() {
        findings.push(ProfileFinding::MissingDeclaredProfile);
    }
    if classified.is_none() {
        findings.push(ProfileFinding::UnnamedEffectiveProfile);
    }

    let observed_flags = PROFILE_FLAGS
        .iter()
        .map(|f| ((*f).to_string(), env.effective(f).map(str::to_string)))
        .collect();

    EffectiveProfile {
        build,
        declared,
        classified,
        observed_flags,
        findings,
    }
}

/// Assert the effective profile before the listener binds.
///
/// Returns the profile when the process may continue. In a production artefact
/// with findings, this prints every finding to stderr and exits with status 2 —
/// the same code ADR-2026's env-hygiene check uses for a promoted development
/// configuration — so a mis-promoted image never accepts a single request.
///
/// Call this from `main` before `HttpServer::bind`.
pub fn assert_effective_profile_or_exit(
    env: &EnvSnapshot,
    build: BuildIdentity,
    today: &str,
) -> EffectiveProfile {
    let profile = evaluate_effective_profile(env, build, today);

    if profile.findings.is_empty() {
        log::info!("security profile OK — {}", profile.summary());
        return profile;
    }

    let fatal = !profile.may_bind_listener();
    for finding in &profile.findings {
        if fatal {
            eprintln!("FATAL: security profile violation: {finding}");
        } else {
            log::warn!("security profile finding (development build): {finding}");
        }
    }

    if fatal {
        eprintln!(
            "FATAL: refusing to bind a listener — {} (ADR-2038 boot-time profile assertion).",
            profile.summary()
        );
        eprintln!(
            "Remove the offending variables, rebuild without --features dev-auth, \
or set {SECURITY_PROFILE_ENV} to the profile this deployment actually is."
        );
        std::process::exit(2);
    }

    log::warn!(
        "security profile has {} finding(s) but this is a development build ({}) — continuing",
        profile.findings.len(),
        build.label()
    );
    profile
}

#[cfg(test)]
mod tests {
    use super::*;

    const TODAY: &str = "2026-09-05";
    const YESTERDAY: &str = "2026-09-04";
    const OWNER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    const PRODUCTION: BuildIdentity = BuildIdentity {
        debug_assertions: false,
        dev_auth: false,
    };
    const RELEASE_DEV_AUTH: BuildIdentity = BuildIdentity {
        debug_assertions: false,
        dev_auth: true,
    };
    const DEBUG: BuildIdentity = BuildIdentity {
        debug_assertions: true,
        dev_auth: false,
    };

    /// A clean `single-tenant` environment.
    fn clean_env() -> EnvSnapshot {
        EnvSnapshot::from_pairs([
            (SECURITY_PROFILE_ENV, "single-tenant"),
            ("RBAC_PUBLIC_READS", "0"),
            ("RBAC_ALLOW_OWNERLESS", "1"),
            ("RBAC_OWNER_PUBKEY", OWNER),
            ("RBAC_DEFAULT_ROLE", "editor"),
            ("PUBKEY_VISIBILITY_FILTER", "1"),
            ("RBAC_GATE_MODE", "enforce"),
        ])
    }

    fn with(env: &EnvSnapshot, name: &str, value: &str) -> EnvSnapshot {
        let mut pairs: Vec<(String, String)> = env
            .vars
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        pairs.push((name.to_string(), value.to_string()));
        EnvSnapshot::from_pairs(pairs)
    }

    fn without(env: &EnvSnapshot, name: &str) -> EnvSnapshot {
        let pairs: Vec<(String, String)> = env
            .vars
            .iter()
            .filter(|(k, _)| k.as_str() != name)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        EnvSnapshot::from_pairs(pairs)
    }

    fn findings(env: &EnvSnapshot, build: BuildIdentity) -> Vec<ProfileFinding> {
        evaluate_effective_profile(env, build, TODAY).findings
    }

    // ---- baseline -----------------------------------------------------------

    #[test]
    fn clean_single_tenant_profile_is_accepted() {
        let profile = evaluate_effective_profile(&clean_env(), PRODUCTION, TODAY);
        assert!(
            profile.findings.is_empty(),
            "unexpected findings: {:?}",
            profile.findings
        );
        assert_eq!(profile.declared, Some(DeploymentProfile::SingleTenant));
        assert_eq!(profile.classified, Some(DeploymentProfile::SingleTenant));
        assert!(profile.may_bind_listener());
    }

    #[test]
    fn every_ratified_profile_classifies_itself() {
        for profile in DeploymentProfile::ALL {
            let mut pairs: Vec<(String, String)> = vec![(
                SECURITY_PROFILE_ENV.to_string(),
                profile.as_str().to_string(),
            )];
            for (flag, expectation) in profile.expected_flags() {
                match expectation {
                    FlagExpectation::Unset => {}
                    FlagExpectation::AnyNonEmpty => {
                        pairs.push((flag.to_string(), OWNER.to_string()))
                    }
                    FlagExpectation::Exactly(v) => pairs.push((flag.to_string(), v.to_string())),
                }
            }
            let env = EnvSnapshot::from_pairs(pairs);
            let evaluated = evaluate_effective_profile(&env, PRODUCTION, TODAY);
            assert!(
                evaluated.findings.is_empty(),
                "{profile} produced findings: {:?}",
                evaluated.findings
            );
            assert_eq!(evaluated.classified, Some(profile));
        }
    }

    // ---- missing versus zero-valued variables -------------------------------

    /// The acceptance case. A forbidden variable set to `0` is *present*, and
    /// presence is the signal — reading `0` as "off" is the misreading that
    /// lets a promoted development configuration through.
    #[test]
    fn zero_valued_forbidden_variable_is_rejected() {
        for name in FORBIDDEN_DEV_VARS {
            let env = with(&clean_env(), name, "0");
            let found = findings(&env, PRODUCTION);
            assert!(
                found.iter().any(|f| matches!(
                    f,
                    ProfileFinding::ForbiddenDevVariable { name: n, value }
                        if n == name && value == "0"
                )),
                "{name}=0 must be rejected, got {found:?}"
            );
        }
    }

    /// An empty value is equally present.
    #[test]
    fn empty_valued_forbidden_variable_is_rejected() {
        let env = with(&clean_env(), "SETTINGS_AUTH_BYPASS", "");
        assert!(findings(&env, PRODUCTION)
            .iter()
            .any(|f| matches!(f, ProfileFinding::ForbiddenDevVariable { .. })));
    }

    /// A truthy value is rejected too, obviously — the point is that all three
    /// spellings land in the same place.
    #[test]
    fn truthy_forbidden_variable_is_rejected() {
        let env = with(&clean_env(), "VISIONCLAW_DEV_MODE", "1");
        assert!(findings(&env, PRODUCTION)
            .iter()
            .any(|f| matches!(f, ProfileFinding::ForbiddenDevVariable { .. })));
    }

    /// The other half of the matrix: a *missing* variable is not a finding.
    #[test]
    fn missing_forbidden_variable_is_not_a_finding() {
        let env = clean_env();
        for name in FORBIDDEN_DEV_VARS {
            assert!(!env.is_present(name));
        }
        assert!(findings(&env, PRODUCTION).is_empty());
    }

    /// `DEV_AUTH_LOOPBACK` is the ADR-2012 variable the original hygiene check
    /// did not cover; it is now in the forbidden set.
    #[test]
    fn dev_auth_loopback_is_covered() {
        assert!(FORBIDDEN_DEV_VARS.contains(&"DEV_AUTH_LOOPBACK"));
        let env = with(&clean_env(), "DEV_AUTH_LOOPBACK", "0");
        assert!(findings(&env, PRODUCTION).iter().any(
            |f| matches!(f, ProfileFinding::ForbiddenDevVariable { name, .. }
                if name == "DEV_AUTH_LOOPBACK")
        ));
    }

    // ---- report mode construction, rollover, restart -------------------------

    /// ADR-2012's reproduced gap: a non-debug build enables report mode with a
    /// current-date acknowledgement. The boot assertion rejects it regardless.
    #[test]
    fn acknowledged_report_mode_is_still_rejected_in_production() {
        let env = with(
            &with(&clean_env(), "RBAC_GATE_MODE", "report"),
            "RBAC_REPORT_MODE_ACK",
            TODAY,
        );
        let found = findings(&env, PRODUCTION);
        assert!(
            found.iter().any(|f| matches!(
                f,
                ProfileFinding::ReportModeRequested {
                    acknowledged: true,
                    ..
                }
            )),
            "an acknowledged report mode must still be rejected: {found:?}"
        );
    }

    /// Date rollover: yesterday's acknowledgement no longer acknowledges.
    #[test]
    fn report_mode_acknowledgement_expires_at_the_date_rollover() {
        let env = with(
            &with(&clean_env(), "RBAC_GATE_MODE", "report"),
            "RBAC_REPORT_MODE_ACK",
            YESTERDAY,
        );
        assert!(!report_mode_acknowledged(&env, PRODUCTION, TODAY));
        assert!(report_mode_acknowledged(&env, PRODUCTION, YESTERDAY));

        let found = findings(&env, PRODUCTION);
        assert!(found.iter().any(|f| matches!(
            f,
            ProfileFinding::ReportModeRequested {
                acknowledged: false,
                ..
            }
        )));
    }

    /// A tomorrow-dated acknowledgement does not pre-authorise anything.
    #[test]
    fn future_dated_acknowledgement_does_not_acknowledge() {
        let env = with(&clean_env(), "RBAC_REPORT_MODE_ACK", "2026-09-06");
        assert!(!report_mode_acknowledged(&env, PRODUCTION, TODAY));
    }

    /// A debug build acknowledges implicitly, with or without the variable —
    /// and the finding is non-fatal there.
    #[test]
    fn debug_build_acknowledges_implicitly_and_continues() {
        let env = with(&clean_env(), "RBAC_GATE_MODE", "report");
        assert!(report_mode_acknowledged(&env, DEBUG, TODAY));
        let profile = evaluate_effective_profile(&env, DEBUG, TODAY);
        assert!(!profile.findings.is_empty());
        assert!(
            profile.may_bind_listener(),
            "a development build reports rather than refuses"
        );
    }

    /// Report mode is evaluated per boot, so the acknowledgement is re-checked
    /// on restart against the then-current date. This is the "restart" arm of
    /// the acceptance matrix: the same environment, evaluated on two dates,
    /// gives two different acknowledgement answers.
    #[test]
    fn report_mode_is_re_evaluated_on_every_boot() {
        let env = with(
            &with(&clean_env(), "RBAC_GATE_MODE", "report"),
            "RBAC_REPORT_MODE_ACK",
            "2026-09-04",
        );
        assert!(
            report_mode_acknowledged(&env, PRODUCTION, "2026-09-04"),
            "the boot on the acknowledged date sees it as acknowledged"
        );
        assert!(
            !report_mode_acknowledged(&env, PRODUCTION, "2026-09-05"),
            "the next day's restart must not inherit yesterday's acknowledgement"
        );
    }

    /// `enforce` and an unset gate mode are both clean.
    #[test]
    fn enforce_and_unset_gate_mode_are_clean() {
        assert!(findings(&clean_env(), PRODUCTION).is_empty());
        let env = without(&clean_env(), "RBAC_GATE_MODE");
        assert!(
            findings(&env, PRODUCTION).is_empty(),
            "unset RBAC_GATE_MODE means enforce, the code default"
        );
    }

    // ---- artefact identity (ADR-2037) ---------------------------------------

    #[test]
    fn release_dev_auth_artefact_is_rejected() {
        let found = findings(&clean_env(), RELEASE_DEV_AUTH);
        assert!(
            found
                .iter()
                .any(|f| matches!(f, ProfileFinding::DevAuthFeatureInArtefact)),
            "a release build carrying dev-auth must be refused: {found:?}"
        );
    }

    /// A release dev-auth artefact is refused before binding.
    #[test]
    fn release_dev_auth_build_cannot_bind() {
        let profile = evaluate_effective_profile(&clean_env(), RELEASE_DEV_AUTH, TODAY);
        assert!(!profile.findings.is_empty());
        assert!(!profile.build.is_production_artefact());
        assert!(!profile.may_bind_listener());
    }

    #[test]
    fn debug_build_does_not_report_a_dev_auth_artefact() {
        let debug_dev_auth = BuildIdentity {
            debug_assertions: true,
            dev_auth: true,
        };
        assert!(!findings(&clean_env(), debug_dev_auth)
            .iter()
            .any(|f| matches!(f, ProfileFinding::DevAuthFeatureInArtefact)));
    }

    #[test]
    fn allow_skip_auth_argv_is_rejected() {
        let env = clean_env().with_argv(["visionclaw-server", "--allow-skip-auth"]);
        assert!(findings(&env, PRODUCTION)
            .iter()
            .any(|f| matches!(f, ProfileFinding::AllowSkipAuthArgv)));
    }

    #[test]
    fn development_container_fingerprint_is_rejected() {
        let env = with(
            &with(&clean_env(), "NODE_ENV", "development"),
            "DOCKER_ENV",
            "1",
        );
        assert!(findings(&env, PRODUCTION)
            .iter()
            .any(|f| matches!(f, ProfileFinding::DevelopmentNodeEnvInContainer)));

        // Either alone is not the fingerprint.
        let only_docker = with(&clean_env(), "DOCKER_ENV", "1");
        assert!(findings(&only_docker, PRODUCTION).is_empty());
        let only_node = with(&clean_env(), "NODE_ENV", "development");
        assert!(findings(&only_node, PRODUCTION).is_empty());
        let node_prod = with(
            &with(&clean_env(), "NODE_ENV", "production"),
            "DOCKER_ENV",
            "1",
        );
        assert!(findings(&node_prod, PRODUCTION).is_empty());
    }

    // ---- profile drift (ADR-2027) -------------------------------------------

    #[test]
    fn declared_profile_drift_is_rejected() {
        // multi-user-locked declared, but anonymous reads left on.
        let env = EnvSnapshot::from_pairs([
            (SECURITY_PROFILE_ENV, "multi-user-locked"),
            ("RBAC_PUBLIC_READS", "1"),
            ("RBAC_ALLOW_OWNERLESS", "0"),
            ("RBAC_OWNER_PUBKEY", OWNER),
            ("RBAC_DEFAULT_ROLE", "viewer"),
            ("PUBKEY_VISIBILITY_FILTER", "1"),
        ]);
        let found = findings(&env, PRODUCTION);
        assert!(
            found.iter().any(|f| matches!(
                f,
                ProfileFinding::ProfileDrift { flag, .. } if flag == "RBAC_PUBLIC_READS"
            )),
            "drift on RBAC_PUBLIC_READS must be reported: {found:?}"
        );
    }

    #[test]
    fn every_drifted_flag_is_reported_not_just_the_first() {
        let env = EnvSnapshot::from_pairs([
            (SECURITY_PROFILE_ENV, "multi-user-locked"),
            ("RBAC_PUBLIC_READS", "1"),
            ("RBAC_ALLOW_OWNERLESS", "1"),
            ("RBAC_DEFAULT_ROLE", "editor"),
            ("PUBKEY_VISIBILITY_FILTER", "0"),
        ]);
        let drift = findings(&env, PRODUCTION)
            .into_iter()
            .filter(|f| matches!(f, ProfileFinding::ProfileDrift { .. }))
            .count();
        assert_eq!(
            drift, 5,
            "public reads, ownerless, owner key, default role and visibility filter all drift"
        );
    }

    #[test]
    fn unknown_declared_profile_is_rejected() {
        let env = with(&clean_env(), SECURITY_PROFILE_ENV, "yolo-mode");
        assert!(findings(&env, PRODUCTION)
            .iter()
            .any(|f| matches!(f, ProfileFinding::UnknownDeclaredProfile { .. })));
    }

    /// An undeclared but recognisable environment is classified but is not
    /// allowed to bind in production: declaring intent is required.
    #[test]
    fn undeclared_environment_classifies_but_cannot_bind() {
        let env = without(&clean_env(), SECURITY_PROFILE_ENV);
        let profile = evaluate_effective_profile(&env, PRODUCTION, TODAY);
        assert!(profile
            .findings
            .contains(&ProfileFinding::MissingDeclaredProfile));
        assert!(!profile.may_bind_listener());
        assert_eq!(profile.declared, None);
        assert_eq!(profile.classified, Some(DeploymentProfile::SingleTenant));
    }

    /// An undeclared environment matching no ratified profile is reported as
    /// unnamed — ADR-2027 calls that unsupported, and the receipt says so.
    #[test]
    fn unnamed_flag_soup_is_classified_as_unnamed() {
        let env = EnvSnapshot::from_pairs([
            ("RBAC_PUBLIC_READS", "1"),
            ("RBAC_ALLOW_OWNERLESS", "0"),
            ("RBAC_DEFAULT_ROLE", "viewer"),
            ("PUBKEY_VISIBILITY_FILTER", "1"),
        ]);
        let profile = evaluate_effective_profile(&env, PRODUCTION, TODAY);
        assert_eq!(profile.classified, None);
        assert!(profile.summary().contains("classified=<unnamed>"));
        assert!(!profile.may_bind_listener());
        assert!(profile
            .findings
            .contains(&ProfileFinding::UnnamedEffectiveProfile));
    }

    /// A profile flag left blank falls back to its code default rather than
    /// being read as a literal empty value.
    #[test]
    fn blank_flag_values_are_treated_as_unset() {
        let env = with(&clean_env(), "RBAC_GATE_MODE", "   ");
        assert!(
            findings(&env, PRODUCTION).is_empty(),
            "a blank gate mode is the enforce default, not a drift"
        );
        assert!(!report_mode_requested(&env));
    }

    // ---- binding decision ----------------------------------------------------

    #[test]
    fn a_production_artefact_with_findings_may_not_bind() {
        let env = with(&clean_env(), "SETTINGS_AUTH_BYPASS", "0");
        let profile = evaluate_effective_profile(&env, PRODUCTION, TODAY);
        assert!(!profile.may_bind_listener());
    }

    #[test]
    fn a_development_build_with_findings_may_bind() {
        let env = with(&clean_env(), "SETTINGS_AUTH_BYPASS", "1");
        let profile = evaluate_effective_profile(&env, DEBUG, TODAY);
        assert!(!profile.findings.is_empty());
        assert!(profile.may_bind_listener());
    }

    #[test]
    fn the_receipt_records_every_profile_flag() {
        let profile = evaluate_effective_profile(&clean_env(), PRODUCTION, TODAY);
        for flag in PROFILE_FLAGS {
            assert!(
                profile.observed_flags.contains_key(flag),
                "the boot receipt must record {flag}"
            );
        }
        assert_eq!(
            profile.observed_flags.get("RBAC_PUBLIC_READS"),
            Some(&Some("0".to_string()))
        );
    }

    #[test]
    fn profile_names_round_trip() {
        for profile in DeploymentProfile::ALL {
            assert_eq!(DeploymentProfile::parse(profile.as_str()), Some(profile));
        }
        assert_eq!(
            DeploymentProfile::parse("MULTI_USER_LOCKED"),
            Some(DeploymentProfile::MultiUserLocked)
        );
        assert_eq!(DeploymentProfile::parse("nonsense"), None);
    }

    #[test]
    fn build_identity_labels_are_distinct() {
        let labels = [
            PRODUCTION.label(),
            RELEASE_DEV_AUTH.label(),
            DEBUG.label(),
            BuildIdentity {
                debug_assertions: true,
                dev_auth: true,
            }
            .label(),
        ];
        let unique: std::collections::BTreeSet<_> = labels.iter().collect();
        assert_eq!(unique.len(), 4);
        assert!(PRODUCTION.is_production_artefact());
        assert!(!RELEASE_DEV_AUTH.is_production_artefact());
        assert!(!DEBUG.is_production_artefact());
    }

    // ---------------------------------------------------------------- ADR-2043
    // The ADR-2003 full-disclosure pair: anonymous reads ON with the visibility
    // filter OFF. Rejected on its own terms, declared profile or not.

    fn has_full_disclosure(env: &EnvSnapshot) -> bool {
        evaluate_effective_profile(env, PRODUCTION, TODAY)
            .findings
            .iter()
            .any(|f| matches!(f, ProfileFinding::FullDisclosureFlagPair { .. }))
    }

    #[test]
    fn full_disclosure_pair_is_rejected_without_a_declared_profile() {
        // The case that previously slipped through entirely: nothing declared,
        // so the old code classified it `Unnamed` and bound anyway.
        let env = EnvSnapshot::from_pairs([
            ("RBAC_PUBLIC_READS", "1"),
            ("PUBKEY_VISIBILITY_FILTER", "0"),
        ]);
        assert!(has_full_disclosure(&env));
    }

    #[test]
    fn full_disclosure_pair_is_rejected_with_a_declared_profile_too() {
        let env = with(
            &with(&clean_env(), "RBAC_PUBLIC_READS", "1"),
            "PUBKEY_VISIBILITY_FILTER",
            "0",
        );
        assert!(has_full_disclosure(&env));
    }

    #[test]
    fn every_ratified_profile_is_free_of_the_pair() {
        // demo-open is the shipped compose posture (ADR-2027) and MUST stay
        // acceptable: it enables anonymous reads but keeps the filter on.
        for profile in DeploymentProfile::ALL {
            let mut pairs: Vec<(String, String)> = profile
                .expected_flags()
                .into_iter()
                .filter_map(|(k, v)| match v {
                    FlagExpectation::Unset => None,
                    FlagExpectation::AnyNonEmpty => Some((k.to_string(), OWNER.to_string())),
                    FlagExpectation::Exactly(val) => Some((k.to_string(), val.to_string())),
                })
                .collect();
            pairs.push((
                SECURITY_PROFILE_ENV.to_string(),
                profile.as_str().to_string(),
            ));
            let env = EnvSnapshot::from_pairs(pairs);
            assert!(
                !has_full_disclosure(&env),
                "ratified profile {profile} must not trip the full-disclosure rule"
            );
        }
    }

    #[test]
    fn either_flag_alone_is_not_the_pair() {
        // Anonymous reads with the filter ON is demo-open — supported.
        assert!(!has_full_disclosure(&EnvSnapshot::from_pairs([
            ("RBAC_PUBLIC_READS", "1"),
            ("PUBKEY_VISIBILITY_FILTER", "1"),
        ])));
        // Filter OFF with reads authenticated is a deliberate single-operator
        // choice — also supported.
        assert!(!has_full_disclosure(&EnvSnapshot::from_pairs([
            ("RBAC_PUBLIC_READS", "0"),
            ("PUBKEY_VISIBILITY_FILTER", "0"),
        ])));
    }

    #[test]
    fn absent_flags_do_not_trip_the_rule() {
        // Both absent = auth-required reads and filter-on: the fail-closed
        // default pair, which must be silent.
        assert!(!has_full_disclosure(&EnvSnapshot::from_pairs(Vec::<(
            String,
            String
        )>::new(
        ))));
    }

    #[test]
    fn public_reads_reader_matches_the_rbac_gate_semantics() {
        // Only `1` / `true` enable; everything else, including other truthy
        // spellings, is off — mirroring rbac_gate::public_reads_enabled.
        for on in ["1", "true", "TRUE", " true "] {
            let env = EnvSnapshot::from_pairs([("RBAC_PUBLIC_READS", on)]);
            assert!(public_reads_enabled_in(&env), "{on:?} should enable");
        }
        for off in ["0", "false", "yes", "on", "", "2"] {
            let env = EnvSnapshot::from_pairs([("RBAC_PUBLIC_READS", off)]);
            assert!(!public_reads_enabled_in(&env), "{off:?} should not enable");
        }
    }

    #[test]
    fn visibility_reader_matches_parse_visibility_flag_semantics() {
        // Default ON, only the four explicit opt-outs disable, unrecognised
        // values fail safe.
        assert!(visibility_filter_enabled_in(&EnvSnapshot::from_pairs(
            Vec::<(String, String)>::new()
        )));
        for off in ["0", "false", "off", "no", "OFF", " no "] {
            let env = EnvSnapshot::from_pairs([("PUBKEY_VISIBILITY_FILTER", off)]);
            assert!(
                !visibility_filter_enabled_in(&env),
                "{off:?} should disable"
            );
        }
        for on in ["1", "true", "banana", ""] {
            let env = EnvSnapshot::from_pairs([("PUBKEY_VISIBILITY_FILTER", on)]);
            assert!(visibility_filter_enabled_in(&env), "{on:?} should stay on");
        }
    }

    #[test]
    fn full_disclosure_is_fatal_in_a_production_artefact() {
        let env = EnvSnapshot::from_pairs([
            ("RBAC_PUBLIC_READS", "1"),
            ("PUBKEY_VISIBILITY_FILTER", "0"),
        ]);
        let profile = evaluate_effective_profile(&env, PRODUCTION, TODAY);
        assert!(!profile.may_bind_listener());
        let profile_dev = evaluate_effective_profile(&env, DEBUG, TODAY);
        assert!(
            profile_dev.may_bind_listener(),
            "a dev build reports and continues"
        );
    }
}
