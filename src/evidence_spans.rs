//! Model selects bounded source spans; only code writes durable quote text.
use crate::{AppResult, model::TurnEvent};
use serde_json::Value;
use std::collections::BTreeMap;

pub fn catalog(events: &[TurnEvent]) -> BTreeMap<String, BTreeMap<String, String>> {
    events
        .iter()
        .map(|event| {
            let mut candidates = BTreeMap::new();
            let mut part = String::new();
            let mut count = 0;
            // Paragraph boundaries retain soft wraps; long paragraphs are bounded by
            // Unicode character count. Every candidate remains an exact substring.
            for c in event.content.chars() {
                part.push(c);
                count += 1;
                if part.ends_with("\n\n") || count == 600 {
                    insert_candidate(&mut candidates, &part);
                    part.clear();
                    count = 0;
                }
            }
            insert_candidate(&mut candidates, &part);
            (format!("event:{}", event.seq), candidates)
        })
        .collect()
}

fn insert_candidate(candidates: &mut BTreeMap<String, String>, source: &str) {
    let quote = source.trim();
    if !quote.is_empty() {
        candidates.insert(format!("c{}", candidates.len() + 1), quote.to_owned());
    }
}

pub fn resolve(value: &mut Value, events: &[TurnEvent]) -> AppResult<()> {
    let catalog = catalog(events);
    for field in ["record_updates", "state_updates"] {
        if let Some(updates) = value.get_mut(field).and_then(Value::as_array_mut) {
            for update in updates {
                if let Some(evidence) = update.get_mut("evidence").and_then(Value::as_array_mut) {
                    for item in evidence {
                        resolve_one(item, &catalog)?;
                    }
                }
                if let Some(basis) = update.get_mut("decision_basis").filter(|v| !v.is_null()) {
                    resolve_one(basis, &catalog)?;
                }
            }
        }
    }
    Ok(())
}

fn resolve_one(
    value: &mut Value,
    catalog: &BTreeMap<String, BTreeMap<String, String>>,
) -> AppResult<()> {
    let object = value.as_object_mut().ok_or("invalid evidence selector")?;
    if object.contains_key("segment_start") || object.contains_key("segment_end") {
        return Err("obsolete evidence range: select a supplied candidate_id".into());
    }
    // Legacy quote fixtures still undergo exact journal validation. The v6 model
    // schema only permits candidate IDs, never free quotes or ranges.
    if !object.contains_key("candidate_id") {
        return Ok(());
    }
    if object.contains_key("quote") {
        return Err("mixed evidence candidate and quote".into());
    }
    let reference = object
        .get("reference")
        .and_then(Value::as_str)
        .ok_or("missing evidence reference")?;
    let candidates = catalog
        .get(reference)
        .ok_or("evidence candidate event not in batch")?;
    let id = object
        .get("candidate_id")
        .and_then(Value::as_str)
        .ok_or("invalid evidence candidate_id")?;
    let quote = candidates
        .get(id)
        .ok_or("unknown evidence candidate_id for supplied event")?
        .clone();
    object.remove("candidate_id");
    object.insert("quote".into(), Value::String(quote));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(content: &str) -> TurnEvent {
        serde_json::from_value(json!({"seq":25,"kind":"assistant_final",
            "session_id":"fixture","turn_id":null,"model":null,"content":content,
            "truncated":false,"recorded_at_unix":1,"cwd":"/fixture"}))
        .unwrap()
    }

    #[test]
    fn source_formatting_is_preserved_without_model_copying() {
        for content in [
            "이번 캡션에는 **3장의 규격 예시와 4장의 비교 조건**만 보충했습니다.",
            "[운영 Studio](https://example.invalid)에 반영했습니다.",
            "운영 배포 승인 → 운영에\n  서 최종 확인 → 문제 발생 시 이전 버전 복원",
        ] {
            let selector = json!({"reference":"event:25","candidate_id":"c1"});
            let mut value = json!({"record_updates":[{"evidence":[selector.clone()],
                "decision_basis":{"intent":"explicit_choice","reference":"event:25","candidate_id":"c1"}}],
                "state_updates":[{"evidence":[selector]}]});
            resolve(&mut value, &[event(content)]).unwrap();
            for pointer in [
                "/record_updates/0/evidence/0/quote",
                "/record_updates/0/decision_basis/quote",
                "/state_updates/0/evidence/0/quote",
            ] {
                assert_eq!(value.pointer(pointer).unwrap(), content);
            }
        }
    }

    #[test]
    fn invalid_and_obsolete_selectors_are_rejected() {
        for selector in [
            json!({"reference":"event:26","candidate_id":"c1"}),
            json!({"reference":"event:25","candidate_id":"c45"}),
            json!({"reference":"event:25","candidate_id":1}),
            json!({"reference":"event:25","candidate_id":"c1","quote":"원문"}),
            json!({"reference":"event:25","segment_start":45,"segment_end":45}),
        ] {
            let mut value = json!({"state_updates":[{"evidence":[selector]}]});
            assert!(resolve(&mut value, &[event("원문")]).is_err());
        }
    }

    #[test]
    fn every_offered_candidate_is_nonblank_bounded_and_exact() {
        // Regression: old line 45 was just a newline, offered as selectable proof.
        let content = format!("{}\n\n{}\n  \n끝", "완료\n".repeat(44), "가🦀".repeat(601));
        let events = [event(&content)];
        let candidates = catalog(&events);
        assert!(!candidates["event:25"].is_empty());
        for (id, quote) in &candidates["event:25"] {
            assert!(!quote.trim().is_empty());
            assert!(quote.chars().count() <= 600);
            assert!(content.contains(quote));
            let mut value = json!({"state_updates":[{"evidence":[{"reference":"event:25","candidate_id":id}]}]});
            resolve(&mut value, &events).unwrap();
            assert_eq!(value["state_updates"][0]["evidence"][0]["quote"], *quote);
        }
        for blank in ["", "\n", " \t\r\n", "　"] {
            assert!(catalog(&[event(blank)])["event:25"].is_empty());
        }
        for length in [599, 600, 601, 1200, 1201] {
            let input = "한".repeat(length);
            let candidates = catalog(&[event(&input)]);
            assert_eq!(
                candidates["event:25"]
                    .values()
                    .map(|s| s.chars().count())
                    .sum::<usize>(),
                length
            );
            assert!(
                candidates["event:25"]
                    .values()
                    .all(|s| s.chars().count() <= 600)
            );
        }
    }
}
