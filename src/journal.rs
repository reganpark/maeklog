//! Evidence-backed cumulative records and independent current state.
use crate::model::{EventKind, Summary, TurnEvent};
use crate::storage::{SpaceStore, atomic_write_json, lock_file, read_json_or_default};
use crate::{AppResult, branch_context as context};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub reference: String,
    pub quote: String,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Decision,
    Completion,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Add,
    Restate,
    Replace,
    Withdraw,
    FollowUp,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Decision,
    Implementation,
    Verification,
    Merge,
    Deployment,
    Other,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Confirmed,
    Reported,
    Failed,
    Rework,
    Passed,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Change {
    pub kind: Kind,
    pub operation: Operation,
    pub key: String,
    pub target: Option<String>,
    pub text: String,
    pub condition: Option<String>,
    pub stage: Stage,
    pub outcome: Outcome,
    pub evidence: Vec<Evidence>,
    #[serde(default)]
    pub decision_basis: Option<crate::decision_policy::Basis>,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StateKind {
    Progress,
    Blocker,
    Action,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StateStatus {
    Active,
    Resolved,
    Unconfirmed,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StateEdit {
    Same,
    Changed,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StateChange {
    pub key: String,
    pub kind: StateKind,
    pub text: String,
    pub owner: String,
    pub condition: Option<String>,
    pub status: StateStatus,
    pub change: StateEdit,
    pub evidence: Vec<Evidence>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Proof {
    pub source: context::Source,
    pub quote: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Version {
    pub operation: Operation,
    pub text: String,
    pub condition: Option<String>,
    pub at_unix_utc: Option<u64>,
    pub order: u64,
    pub target: Option<String>,
    pub proof: Vec<Proof>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_basis: Option<crate::decision_policy::Basis>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Record {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub corrections: Vec<crate::corrections::Correction>,
    pub id: String,
    pub key: String,
    pub kind: Kind,
    pub stage: Stage,
    pub outcome: Outcome,
    pub versions: Vec<Version>,
    pub replaced_by: Option<String>,
    pub withdrawn: bool,
    pub follows: Option<String>,
    #[serde(default)]
    pub relation_order: u64,
}
impl Record {
    pub fn latest(&self) -> &Version {
        self.versions
            .iter()
            .max_by_key(|v| v.order)
            .expect("record has a version")
    }
    pub fn text(&self) -> &str {
        crate::corrections::current(self).map_or(&self.latest().text, |c| &c.text)
    }
    pub fn condition(&self) -> Option<&str> {
        crate::corrections::current(self).map_or(self.latest().condition.as_deref(), |c| {
            c.condition.as_deref()
        })
    }
    pub fn excluded(&self) -> bool {
        crate::corrections::current(self).is_some_and(|c| c.excluded)
    }
    pub fn active(&self) -> bool {
        !self.excluded() && !self.withdrawn && self.replaced_by.is_none()
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct State {
    pub key: String,
    pub kind: StateKind,
    pub text: String,
    pub owner: String,
    pub condition: Option<String>,
    pub status: StateStatus,
    pub at_unix_utc: Option<u64>,
    pub order: u64,
    pub proof: Vec<Proof>,
    #[serde(default)]
    pub uncertain_by_omission: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Gap {
    pub space: String,
    pub generation: u64,
    pub reason: String,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct Journal {
    pub correction_revision: u64,
    pub revision: u64,
    pub through_order: u64,
    pub through_unix_utc: Option<u64>,
    pub updated_at_unix_utc: Option<u64>,
    pub records: BTreeMap<String, Record>,
    pub states: BTreeMap<String, State>,
    pub applied: BTreeSet<String>,
    pub processed_events: BTreeSet<String>,
    pub gaps: Vec<Gap>,
}

pub fn load(root: &std::path::Path, scope: &context::Context) -> AppResult<Journal> {
    let journal: Journal =
        read_json_or_default(&context::directory(root, scope).join("journal.json"))?;
    if journal
        .records
        .iter()
        .any(|(id, r)| r.versions.is_empty() || &r.id != id)
    {
        return Err(
            "invalid journal record identity or empty version history; source file retained".into(),
        );
    }
    Ok(journal)
}
pub fn resolve_proof(
    store: &SpaceStore,
    evidence: &[Evidence],
    events: &[TurnEvent],
) -> AppResult<Vec<Proof>> {
    if evidence.is_empty() || evidence.len() > 8 {
        return Err("journal change requires 1..8 evidence quotes".into());
    }
    evidence
        .iter()
        .map(|e| {
            let seq = e
                .reference
                .strip_prefix("event:")
                .and_then(|n| n.parse::<u64>().ok())
                .ok_or("journal evidence must reference a supplied event")?;
            let event = events
                .iter()
                .find(|v| v.seq == seq)
                .ok_or("journal evidence not in batch")?;
            if e.quote.trim().is_empty() {
                return Err(format!("journal evidence empty_quote at event:{seq}").into());
            }
            if e.quote.chars().count() > 600 {
                return Err(format!(
                    "journal evidence quote_too_long at event:{seq} (limit 600 characters)"
                )
                .into());
            }
            if !event.content.contains(&e.quote) {
                return Err(format!("journal evidence quote_not_in_event at event:{seq}").into());
            }
            Ok(Proof {
                source: context::source(store, event)?,
                quote: e.quote.clone(),
            })
        })
        .collect()
}
fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 120
        && key
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._:-".contains(&c))
}
fn at(proof: &[Proof]) -> Option<u64> {
    proof.first().and_then(|p| p.source.at_unix_utc)
}
fn order(proof: &[Proof]) -> u64 {
    proof.first().map_or(0, |p| p.source.order)
}
fn proof_key(proof: &[Proof]) -> String {
    let mut keys: Vec<_> = proof
        .iter()
        .map(|p| format!("{}:{}", p.source.uid, p.quote))
        .collect();
    keys.sort();
    context::hash(&keys.join("\0"))
}

pub fn apply(store: &SpaceStore, events: &[TurnEvent], summary: &Summary) -> AppResult<()> {
    if events.is_empty() {
        return Ok(());
    }
    if summary.record_updates.len() > 12 || summary.state_updates.len() > 12 {
        return Err("too many journal updates".into());
    }
    let scope = context::context_for_events(store, events)?;
    let dir = context::directory(&store.state_root, &scope);
    let _lock = lock_file(&dir.join(".journal.lock"), false)?;
    let mut journal = load(&store.state_root, &scope)?;
    crate::corrections::check_revision(&journal, summary.correction_revision)?;
    atomic_write_json(&dir.join("context.json"), &scope)?;
    let old = journal.clone();
    let batch_order = events
        .iter()
        .map(|e| context::source(store, e).map(|s| s.order))
        .collect::<AppResult<Vec<_>>>()?
        .into_iter()
        .max()
        .unwrap_or(0);
    for change in &summary.record_updates {
        let proof = resolve_proof(store, &change.evidence, events)?;
        if proof.iter().any(|p| p.source.context.key != scope.key) {
            return Err("cross-branch durable evidence".into());
        }
        if already_seen(&old, &proof) {
            continue;
        }
        apply_record(&mut journal, change, proof)?;
    }
    let seen = update_states(&mut journal, store, events, summary, batch_order, &scope)?;
    if batch_order > journal.through_order {
        for (key, state) in &mut journal.states {
            if state.kind == StateKind::Blocker
                && state.status == StateStatus::Active
                && !seen.contains(key)
            {
                state.status = StateStatus::Unconfirmed;
                state.uncertain_by_omission = true;
            }
        }
        journal.through_order = batch_order;
        journal.through_unix_utc = events
            .iter()
            .filter_map(|e| context::source(store, e).ok()?.at_unix_utc)
            .max()
            .or(journal.through_unix_utc);
    }
    for event in events {
        journal
            .processed_events
            .insert(context::source(store, event)?.uid);
    }
    if journal != old {
        journal.revision += 1;
        journal.updated_at_unix_utc = Some(crate::storage::unix_now());
        atomic_write_json(&dir.join("journal.json"), &journal)?;
    }
    Ok(())
}

fn already_seen(journal: &Journal, proof: &[Proof]) -> bool {
    proof
        .first()
        .is_some_and(|p| journal.processed_events.contains(&p.source.uid))
}

/// Preview validation runs the same durable operations on an in-memory clone.
/// It does not register facts, consume events, or write journal/clock metadata.
pub fn validate_updates(
    store: &SpaceStore,
    events: &[TurnEvent],
    summary: &Summary,
) -> AppResult<()> {
    if summary.record_updates.len() > 12 || summary.state_updates.len() > 12 {
        return Err("too many journal updates".into());
    }
    let scope = context::context_for_events(store, events)?;
    let mut journal = load(&store.state_root, &scope)?;
    crate::corrections::check_revision(&journal, summary.correction_revision)?;
    for change in &summary.record_updates {
        let proof = resolve_proof(store, &change.evidence, events)?;
        if proof.iter().any(|p| p.source.context.key != scope.key) {
            return Err("cross-branch preview evidence".into());
        }
        if already_seen(&journal, &proof) {
            continue;
        }
        apply_record(&mut journal, change, proof)?;
    }
    for change in &summary.state_updates {
        let proof = resolve_proof(store, &change.evidence, events)?;
        if proof.iter().any(|p| p.source.context.key != scope.key)
            || !valid_key(&change.key)
            || change.text.trim().is_empty()
            || change.text.chars().count() > 1200
        {
            return Err("invalid current state evidence".into());
        }
    }
    Ok(())
}

fn apply_record(journal: &mut Journal, c: &Change, proof: Vec<Proof>) -> AppResult<()> {
    validate_record(c, &proof)?;
    if c.target
        .as_ref()
        .and_then(|id| journal.records.get(id))
        .is_some_and(Record::excluded)
    {
        return Err("user-excluded record is not a valid relation target; do not restore the rejected interpretation".into());
    }
    let op_key = format!(
        "{:?}:{:?}:{:?}:{}:{}",
        c.kind,
        c.stage,
        c.operation,
        c.target.as_deref().unwrap_or(&c.key),
        proof_key(&proof)
    );
    if journal.applied.contains(&op_key) {
        return Ok(());
    }
    let existing = journal
        .records
        .values()
        .find(|r| {
            r.kind == c.kind
                && r.stage == c.stage
                && (r.key == c.key
                    || r.versions
                        .iter()
                        .any(|v| proof_key(&v.proof) == proof_key(&proof)))
        })
        .map(|r| r.id.clone());
    if c.operation == Operation::Restate {
        let id = c
            .target
            .as_ref()
            .or(existing.as_ref())
            .ok_or("restate target required")?;
        let r = journal.records.get(id).ok_or("unknown restate target")?;
        if r.kind != c.kind || r.stage != c.stage || r.condition() != c.condition.as_deref() {
            return Err("restate cannot change kind, stage or condition".into());
        }
        journal.applied.insert(op_key);
        return Ok(());
    }
    if c.operation == Operation::Withdraw {
        let r = journal
            .records
            .get_mut(c.target.as_ref().ok_or("withdraw target required")?)
            .ok_or("unknown withdrawal target")?;
        if r.kind != Kind::Decision {
            return Err("completion report cannot be withdrawn".into());
        }
        let newer = order(&proof) >= r.latest().order.max(r.relation_order);
        let source_order = order(&proof);
        r.versions.push(version(c, proof));
        if newer {
            r.withdrawn = true;
            r.relation_order = source_order;
        }
        journal.applied.insert(op_key);
        return Ok(());
    }
    if c.operation == Operation::Add
        && let Some(existing) = &existing
    {
        let previous = &journal.records[existing];
        if !previous.corrections.is_empty() {
            journal.applied.insert(op_key);
            return Ok(());
        }
        if previous.latest().condition != c.condition || previous.outcome != c.outcome {
            return Err(
                "changed condition/outcome requires an explicit replacement/follow-up".into(),
            );
        }
        journal.applied.insert(op_key);
        return Ok(());
    }
    insert_related(journal, c, proof)?;
    journal.applied.insert(op_key);
    Ok(())
}
fn version(c: &Change, proof: Vec<Proof>) -> Version {
    Version {
        operation: c.operation,
        text: c.text.clone(),
        condition: c.condition.clone(),
        at_unix_utc: at(&proof),
        order: order(&proof),
        target: c.target.clone(),
        decision_basis: c.decision_basis.clone(),
        proof,
    }
}

pub fn mark_gap(store: &SpaceStore, reason: &str) -> AppResult<()> {
    let generation = crate::participation::load(store)?.generation;
    let scopes: BTreeMap<_, _> = context::sources(store)?
        .values()
        .map(|s| (s.context.key.clone(), s.context.clone()))
        .collect();
    for scope in scopes.values() {
        let dir = context::directory(&store.state_root, scope);
        let _lock = lock_file(&dir.join(".journal.lock"), false)?;
        let mut journal = load(&store.state_root, scope)?;
        if !journal
            .gaps
            .iter()
            .any(|g| g.space == store.key && g.generation == generation && g.reason == reason)
        {
            journal.gaps.push(Gap {
                space: store.key.clone(),
                generation,
                reason: reason.into(),
            });
            journal.revision += 1;
            atomic_write_json(&dir.join("journal.json"), &journal)?;
        }
    }
    Ok(())
}
pub fn catalog(store: &SpaceStore, events: &[TurnEvent]) -> AppResult<String> {
    let scope = context::context_for_events(store, events)?;
    let journal = load(&store.state_root, &scope)?;
    let mut records: Vec<_> = journal.records.values().collect();
    records.sort_by_key(|r| std::cmp::Reverse(r.latest().order));
    let mut remaining = 12_000;
    let states=bounded_catalog(journal.states.values().filter(|s|s.status!=StateStatus::Resolved).take(12).map(|s|serde_json::json!({"key":s.key,"kind":s.kind,"text":s.text,"status":s.status,"condition":s.condition})),&mut remaining);
    let records=bounded_catalog(records.into_iter().take(24).map(|r|serde_json::json!({"id":r.id,"key":r.key,"kind":r.kind,"stage":r.stage,"outcome":r.outcome,"source_order":r.latest().order,"source_time":r.latest().at_unix_utc,"text":r.text(),"condition":r.condition(),"active":r.active(),"user_corrected":crate::corrections::current(r).is_some(),"user_excluded":r.excluded()})),&mut remaining);
    let source_orders = events
        .iter()
        .map(|e| context::source(store, e).map(|source| (e.seq, source.order)))
        .collect::<AppResult<BTreeMap<_, _>>>()?;
    Ok(serde_json::to_string(
        &serde_json::json!({"scope":scope,"batch_source_orders":source_orders,"journal_through_order":journal.through_order,"recent_record_catalog":records,"current_state_catalog":states,"catalog_is_partial":true}),
    )?)
}

fn bounded_catalog(
    values: impl Iterator<Item = serde_json::Value>,
    remaining: &mut usize,
) -> Vec<serde_json::Value> {
    values
        .filter(|value| {
            let size = value.to_string().chars().count();
            if size > *remaining {
                return false;
            }
            *remaining -= size;
            true
        })
        .collect()
}

fn insert_related(journal: &mut Journal, c: &Change, proof: Vec<Proof>) -> AppResult<()> {
    if matches!(c.operation, Operation::Replace | Operation::FollowUp) && c.target.is_none() {
        return Err("relation target required".into());
    }
    let id = format!(
        "{}-{}",
        if c.kind == Kind::Decision { "D" } else { "C" },
        context::hash(&format!("{:?}:{}:{:?}", c.kind, c.key, c.stage))
    );
    if journal.records.contains_key(&id) {
        return Err("existing key needs restate or a new replacement key".into());
    }
    let mut historical_successor = None;
    let mut superseded_by = None;
    let mut older_predecessor = false;
    if let Some(target) = &c.target {
        let previous = journal
            .records
            .get_mut(target)
            .ok_or("unknown related record")?;
        if c.operation == Operation::Replace {
            if previous.kind != Kind::Decision || c.kind != Kind::Decision {
                return Err("replace only applies to decisions".into());
            }
            if order(&proof) >= previous.latest().order.max(previous.relation_order) {
                historical_successor = previous.replaced_by.take();
                previous.replaced_by = Some(id.clone());
                previous.relation_order = order(&proof);
            } else {
                superseded_by = previous
                    .replaced_by
                    .clone()
                    .or_else(|| Some(target.clone()));
            }
        } else if c.operation == Operation::FollowUp
            && (previous.kind != Kind::Completion || c.kind != Kind::Completion)
        {
            return Err("follow-up must connect completion reports".into());
        }
        if c.operation == Operation::FollowUp && order(&proof) < previous.latest().order {
            if previous.follows.is_some() {
                return Err(
                    "newer report already has a predecessor; do not guess a second parent".into(),
                );
            }
            previous.follows = Some(id.clone());
            older_predecessor = true;
        }
    }
    if let Some(old_id) = historical_successor.filter(|old_id| old_id != &id)
        && let Some(old) = journal.records.get_mut(&old_id)
    {
        old.replaced_by = Some(id.clone());
        old.relation_order = order(&proof);
    }
    journal.records.insert(
        id.clone(),
        Record {
            corrections: Vec::new(),
            id,
            key: c.key.clone(),
            kind: c.kind,
            stage: c.stage,
            outcome: c.outcome,
            versions: vec![version(c, proof)],
            replaced_by: superseded_by,
            withdrawn: false,
            follows: if c.operation == Operation::FollowUp && !older_predecessor {
                c.target.clone()
            } else {
                None
            },
            relation_order: 0,
        },
    );
    Ok(())
}

fn validate_record(c: &Change, proof: &[Proof]) -> AppResult<()> {
    if !valid_key(&c.key)
        || c.text.trim().is_empty()
        || c.text.chars().count() > 1200
        || c.condition
            .as_ref()
            .is_some_and(|v| v.chars().count() > 600)
    {
        return Err("invalid durable record".into());
    }
    if c.kind == Kind::Decision {
        if c.stage != Stage::Decision {
            return Err("decision_stage_required: kind=decision requires stage=decision".into());
        }
        if c.outcome != Outcome::Confirmed {
            return Err(
                "decision_outcome_required: kind=decision requires outcome=confirmed".into(),
            );
        }
        if proof
            .first()
            .is_none_or(|p| p.source.kind != EventKind::UserPrompt)
        {
            return Err(
                "decision_user_evidence_required: first evidence must cite a user event".into(),
            );
        }
    }
    crate::decision_policy::validate(c, proof)?;
    if c.kind == Kind::Completion && (c.stage == Stage::Decision || c.outcome == Outcome::Confirmed)
    {
        return Err("completion needs a work stage".into());
    }
    if matches!(c.operation, Operation::Withdraw | Operation::Replace) && c.kind != Kind::Decision
        || c.operation == Operation::FollowUp && c.kind != Kind::Completion
        || c.operation == Operation::Add && c.target.is_some()
    {
        return Err("invalid operation for record kind".into());
    }
    Ok(())
}

fn update_states(
    journal: &mut Journal,
    store: &SpaceStore,
    events: &[TurnEvent],
    summary: &Summary,
    batch_order: u64,
    scope: &context::Context,
) -> AppResult<BTreeSet<String>> {
    let mut seen = BTreeSet::new();
    for change in &summary.state_updates {
        let proof = resolve_proof(store, &change.evidence, events)?;
        if proof.iter().any(|p| p.source.context.key != scope.key) {
            return Err("cross-branch state evidence".into());
        }
        if already_seen(journal, &proof) {
            continue;
        }
        if !valid_key(&change.key)
            || change.text.trim().is_empty()
            || change.text.chars().count() > 1200
            || change.owner.chars().count() > 160
            || change
                .condition
                .as_ref()
                .is_some_and(|v| v.chars().count() > 600)
        {
            return Err("invalid state update".into());
        }
        let key = format!("{:?}:{}", change.kind, change.key);
        seen.insert(key.clone());
        if batch_order < journal.through_order {
            continue;
        }
        let previous = journal.states.get(&key);
        if previous.is_some_and(|s| s.order > order(&proof)) {
            continue;
        }
        let same = previous.is_some_and(|s| {
            (s.status == change.status
                || s.uncertain_by_omission && change.status == StateStatus::Active)
                && s.owner == change.owner
                && s.condition == change.condition
                && (change.change == StateEdit::Same || s.text == change.text)
        });
        let time = if same {
            previous.and_then(|s| s.at_unix_utc)
        } else {
            at(&proof)
        };
        // Preserve the original timestamp's evidence, not only the number.
        let anchor_proof = if same {
            previous.map_or_else(|| proof.clone(), |s| s.proof.clone())
        } else {
            proof.clone()
        };
        journal.states.insert(
            key,
            State {
                key: change.key.clone(),
                kind: change.kind,
                text: change.text.clone(),
                owner: change.owner.clone(),
                condition: change.condition.clone(),
                status: change.status,
                at_unix_utc: time,
                order: order(&proof),
                proof: anchor_proof,
                uncertain_by_omission: false,
            },
        );
    }
    Ok(seen)
}

#[cfg(test)]
mod decision_record_tests {
    use super::*;
    use crate::decision_policy::{Basis, Intent};

    fn confirmed() -> (Change, Vec<Proof>) {
        let quote = "그 방안으로 확정한다.";
        let change = Change {
            kind: Kind::Decision,
            operation: Operation::Add,
            key: "confirmed-choice".into(),
            target: None,
            text: "확정한 방안을 따른다".into(),
            condition: None,
            stage: Stage::Decision,
            outcome: Outcome::Confirmed,
            evidence: vec![Evidence {
                reference: "event:1".into(),
                quote: quote.into(),
            }],
            decision_basis: Some(Basis {
                intent: Intent::ExplicitChoice,
                reference: "event:1".into(),
                quote: quote.into(),
            }),
        };
        let proof = vec![Proof {
            source: context::Source {
                uid: "fixture-source".into(),
                order: 1,
                space: "fixture".into(),
                session: "fixture".into(),
                turn: None,
                kind: EventKind::UserPrompt,
                at_unix_utc: Some(1),
                context: context::Context::default(),
            },
            quote: quote.into(),
        }];
        (change, proof)
    }

    #[test]
    fn decision_shape_errors_identify_the_actual_violated_guard() {
        let (change, proof) = confirmed();
        assert!(validate_record(&change, &proof).is_ok());
        let mut wrong_stage = change.clone();
        wrong_stage.stage = Stage::Other;
        assert!(
            validate_record(&wrong_stage, &proof)
                .unwrap_err()
                .to_string()
                .starts_with("decision_stage_required:")
        );
        let mut wrong_outcome = change.clone();
        wrong_outcome.outcome = Outcome::Reported;
        assert!(
            validate_record(&wrong_outcome, &proof)
                .unwrap_err()
                .to_string()
                .starts_with("decision_outcome_required:")
        );
        let mut assistant_proof = proof.clone();
        assistant_proof[0].source.kind = EventKind::AssistantFinal;
        for sources in [&assistant_proof[..], &[][..]] {
            assert!(
                validate_record(&change, sources)
                    .unwrap_err()
                    .to_string()
                    .starts_with("decision_user_evidence_required:")
            );
        }
    }

    #[test]
    fn correct_shape_does_not_admit_requests_or_weaken_confirmation_anchors() {
        let (mut change, proof) = confirmed();
        for intent in [
            Intent::Request,
            Intent::Question,
            Intent::Proposal,
            Intent::Uncertain,
        ] {
            change.decision_basis.as_mut().unwrap().intent = intent;
            assert!(
                validate_record(&change, &proof)
                    .unwrap_err()
                    .to_string()
                    .starts_with("decision_basis_not_confirmed:")
            );
        }
        change.decision_basis = None;
        assert!(
            validate_record(&change, &proof)
                .unwrap_err()
                .to_string()
                .starts_with("decision_basis_required:")
        );
        let (mut change, proof) = confirmed();
        change.decision_basis.as_mut().unwrap().reference = "event:2".into();
        assert!(
            validate_record(&change, &proof)
                .unwrap_err()
                .to_string()
                .starts_with("decision_basis_anchor_mismatch:")
        );
    }

    #[test]
    fn conditional_approval_still_requires_and_preserves_its_prerequisite() {
        let (mut change, mut proof) = confirmed();
        let quote = "검증 통과하면 반영해도 된다.";
        change.evidence[0].quote = quote.into();
        proof[0].quote = quote.into();
        change.decision_basis.as_mut().unwrap().quote = quote.into();
        change.decision_basis.as_mut().unwrap().intent = Intent::ConditionalApproval;
        assert!(
            validate_record(&change, &proof)
                .unwrap_err()
                .to_string()
                .starts_with("decision_basis_condition_required:")
        );
        change.condition = Some("검증 통과 후".into());
        assert!(validate_record(&change, &proof).is_ok());
        let stored = version(&change, proof.clone());
        assert_eq!(stored.condition.as_deref(), Some("검증 통과 후"));
        assert_eq!(stored.proof, proof);
        assert_eq!(stored.at_unix_utc, Some(1));
        assert_eq!(stored.decision_basis, change.decision_basis);
    }
}
