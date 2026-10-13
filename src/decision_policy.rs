//! Shared model/code contract. Meaning is model-classified, never inferred from punctuation.
use crate::{
    AppResult,
    journal::{Change, Kind, Proof},
    model::EventKind,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    ExplicitChoice,
    ExplicitApproval,
    ConditionalApproval,
    Request,
    Question,
    Proposal,
    Uncertain,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Basis {
    pub intent: Intent,
    pub reference: String,
    pub quote: String,
}

pub const PROMPT: &str = "\nDECISION ADMISSION CONTRACT v4 (authoritative):\n\
Classify the speech act in context, not punctuation. Distinguish a request, question, omission complaint, proposal, uncertain candidate, explicit choice, explicit approval and conditional approval.\n\
Every decision record MUST have decision_basis: {intent, reference, quote}. The basis is an exact short affirmative confirmation/approval passage from the SAME USER event as evidence[0], contained in evidence[0].quote. Do not use an assistant statement as user approval.\n\
Only explicit_choice, explicit_approval and conditional_approval are admissible decision intents. For conditional_approval retain the full prerequisite in condition; approval is not execution authorization. Do not invent a confirmation to fill required fields. If none is supplied, emit no decision record.\n\
Every admitted decision record MUST use the exact structural tuple kind=decision, stage=decision, outcome=confirmed. Never use stage=other or a work stage for a decision. A valid tuple does not make a request, question or proposal into approval; the decision_basis contract still applies.\n\
Missing-feature complaints such as '버튼은 어디갔어?' or '젠다이는 어디갔어?' are NOT by themselves new confirmed decisions. A direct task request such as '버튼을 복원해 줘' is a current request, not by itself a durable agreed policy. If actionable, describe it in state_updates (action/progress with owner and conditions); questions awaiting an answer may be actions only when warranted. Pure proposals/questions may be omitted from durable records. Never automatically execute the action.\n\
'그 방안으로 확정한다' supplies an explicit choice; '그 변경을 승인한다' supplies approval; '검증 통과하면 반영해도 된다' supplies conditional approval, not unconditional permission.\n\
A question elsewhere in the same event, or in supporting evidence, does not invalidate a distinct explicit confirmation passage. Do not copy the entire rhetorical question as the confirmation passage. If a confirmation refers to an unavailable earlier proposal, do not invent its scope.\n\
Classify each claim separately even when the SAME candidate passage contains several speech acts. An explicit choice for one claim never confirms a neighboring requirement or proposal. For example, 'follow the system language for now; model selection might be needed' confirms only the temporary language choice, not model selection. Preserve temporary scope such as 'for now' / '일단은' in condition. Put an actionable model-selection request in state_updates only when warranted; never borrow the language choice as its decision_basis.\n\
Completion records have decision_basis:null. Legacy decisions/progress/blockers/next_actions lists should be empty when record_updates/state_updates carry those facts. Requirements and requests belong in state_updates unless an explicit durable choice/approval is separately evidenced.\n";

pub fn validate(change: &Change, proof: &[Proof]) -> AppResult<()> {
    if change.kind != Kind::Decision {
        if change.decision_basis.is_some() {
            return Err("decision_basis_not_allowed_for_completion".into());
        }
        return Ok(());
    }
    let basis = change
        .decision_basis
        .as_ref()
        .ok_or("decision_basis_required: no explicit confirmation passage supplied")?;
    if !matches!(
        basis.intent,
        Intent::ExplicitChoice | Intent::ExplicitApproval | Intent::ConditionalApproval
    ) {
        return Err("decision_basis_not_confirmed: request/question/proposal/uncertain is not a durable decision; use state_updates if actionable".into());
    }
    let first = change
        .evidence
        .first()
        .ok_or("decision_basis_anchor_missing")?;
    if proof
        .first()
        .is_none_or(|p| p.source.kind != EventKind::UserPrompt)
        || basis.reference != first.reference
        || basis.quote.trim().is_empty()
        || basis.quote.chars().count() > 600
        || !first.quote.contains(&basis.quote)
    {
        return Err("decision_basis_anchor_mismatch: require exact confirmation passage in first user evidence".into());
    }
    if basis.intent == Intent::ConditionalApproval
        && change
            .condition
            .as_ref()
            .is_none_or(|c| c.trim().is_empty())
    {
        return Err("decision_basis_condition_required: retain approval prerequisite".into());
    }
    Ok(())
}
