//! Conservative syntax and intent extraction; ambiguous language remains a candidate.
use super::{Evidence, Provenance, TaskReference, VerificationState};
use crate::AppResult;
use crate::work_links::{self, Kind};

pub(super) fn parse(
    input: &str,
    repository: Option<&str>,
    evidence: &Evidence,
    provenance: Provenance,
) -> AppResult<TaskReference> {
    let raw = input.trim();
    let (input, kind) = claimed_input(raw)?;
    let short = input.rsplit_once('#').filter(|(qualifier, _)| {
        !qualifier.is_empty()
            && !qualifier.contains('/')
            && qualifier
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
    });
    let short_repository = short.and_then(|(qualifier, _)| {
        repository.filter(|repo| {
            repo.rsplit('/')
                .next()
                .is_some_and(|name| name.eq_ignore_ascii_case(qualifier))
        })
    });
    let scoped_repository = if short.is_some() {
        short_repository
    } else {
        repository
    };
    let bare = short.map_or_else(
        || input.strip_prefix('#').unwrap_or(input),
        |(_, number)| number,
    );
    let (repository, number, url) = if !bare.is_empty()
        && bare.bytes().all(|byte| byte.is_ascii_digit())
    {
        let number: u64 = bare.parse()?;
        if number == 0 {
            return Err(crate::tr!(
                "참조 번호는 1 이상이어야 합니다",
                "Reference number must be at least 1"
            )
            .into());
        }
        if let Some(repo) = scoped_repository {
            let link = work_links::parse(kind.unwrap_or(Kind::Issue), &format!("{repo}#{number}"))?;
            (Some(link.repository), number, kind.map(|_| link.url))
        } else {
            (None, number, None)
        }
    } else {
        let link = work_links::parse(kind.unwrap_or(Kind::Issue), input)?;
        (Some(link.repository), link.number, kind.map(|_| link.url))
    };
    let unresolved_id = short.map_or_else(
        || "unresolved".into(),
        |(qualifier, _)| format!("unresolved:{qualifier}"),
    );
    let id = format!(
        "{}#{number}",
        repository
            .as_deref()
            .unwrap_or(&unresolved_id)
            .to_ascii_lowercase()
    );
    let unresolved = repository.is_none();
    Ok(TaskReference {
        id,
        repository,
        number,
        kind,
        raw: raw.into(),
        url,
        link: None,
        verification: if unresolved {
            VerificationState::Error
        } else {
            VerificationState::Checking
        },
        error: unresolved.then(|| {
            crate::tr!(
                "당시 GitHub 저장소 미확인 · 전체 URL로 지정하세요",
                "GitHub repository at that time unknown · Use a full URL"
            )
            .into()
        }),
        provenance,
        source: evidence.clone(),
        created_reported: false,
        related_reported: false,
        next_check_at: 0,
        consecutive_failures: 0,
    })
}

fn claimed_input(raw: &str) -> AppResult<(&str, Option<Kind>)> {
    let (input, claimed) = [
        ("이슈 ", Kind::Issue),
        ("issue ", Kind::Issue),
        ("PR ", Kind::Pr),
        ("pr ", Kind::Pr),
    ]
    .into_iter()
    .find_map(|(prefix, kind)| {
        raw.strip_prefix(prefix)
            .map(|rest| (rest.trim(), Some(kind)))
    })
    .unwrap_or((raw, None));
    let url_kind = if input.starts_with("https://github.com/") {
        if input.contains("/issues/") {
            Some(Kind::Issue)
        } else if input.contains("/pull/") {
            Some(Kind::Pr)
        } else {
            return Err(crate::tr!(
                "정확한 GitHub 이슈·PR URL이 필요합니다",
                "An exact GitHub issue/PR URL is required"
            )
            .into());
        }
    } else {
        None
    };
    if claimed
        .zip(url_kind)
        .is_some_and(|(claimed, actual)| claimed != actual)
    {
        return Err(crate::tr!(
            "입력 종류와 GitHub URL이 다릅니다",
            "Input type differs from GitHub URL"
        )
        .into());
    }
    Ok((input, url_kind.or(claimed)))
}

struct Token {
    raw: String,
    start: usize,
    end: usize,
    kind: Option<Kind>,
}

fn tokens(text: &str) -> Vec<Token> {
    let mut result = Vec::new();
    for (start, _) in text.match_indices("https://github.com/") {
        let rest = &text[start..];
        let end = rest
            .find(|c: char| c.is_whitespace() || "()[]{}<>\"'`“”‘’,;!?".contains(c))
            .unwrap_or(rest.len());
        let raw = rest[..end].trim_end_matches('.');
        if let Some(kind) = [Kind::Issue, Kind::Pr]
            .into_iter()
            .find(|kind| work_links::parse(*kind, raw).is_ok())
        {
            result.push(Token {
                raw: raw.into(),
                start,
                end: start + raw.len(),
                kind: Some(kind),
            });
        }
    }
    for (hash, _) in text.match_indices('#') {
        if result
            .iter()
            .any(|token| hash >= token.start && hash < token.end)
        {
            continue;
        }
        let after = &text[hash + 1..];
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            continue;
        }
        let end = hash + 1 + digits;
        // A fragment embedded in a URL/path or word is not a bare GitHub reference.
        if text[end..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '/')
        {
            continue;
        }
        let before = &text[..hash];
        let qualified_start = before
            .char_indices()
            .rev()
            .take_while(|(_, c)| c.is_ascii_alphanumeric() || "._-/".contains(*c))
            .last()
            .map_or(hash, |(at, _)| at);
        let prefix = &text[qualified_start..hash];
        let typed_prefix =
            prefix.eq_ignore_ascii_case("pr") || prefix.eq_ignore_ascii_case("issue");
        let start = if prefix.is_empty() || typed_prefix {
            hash
        } else {
            qualified_start
        };
        if prefix.contains('/') && work_links::parse(Kind::Issue, &text[start..end]).is_err() {
            continue;
        }
        if !prefix.contains('/') && before.ends_with('/') {
            continue;
        }
        let nearby: String = before
            .chars()
            .rev()
            .take(16)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        let lowered = nearby.to_ascii_lowercase();
        let kind = if lowered.trim_end().ends_with("pr")
            || lowered.trim_end().ends_with("pull request")
        {
            Some(Kind::Pr)
        } else if lowered.trim_end().ends_with("issue") || nearby.trim_end().ends_with("이슈") {
            Some(Kind::Issue)
        } else {
            None
        };
        result.push(Token {
            raw: text[start..end].into(),
            start,
            end,
            kind,
        });
    }
    result.sort_by_key(|token| token.start);
    result
}

fn unsafe_role(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    [
        "참고",
        "인용",
        "예시",
        "예를",
        "비교",
        "언급",
        "reference",
        "example",
        "quoted",
        "compare",
        "아닌",
        "말고",
        "제외",
        "하지 마",
        "하지마",
        "하지 않",
        "안 하고",
        "don't",
        "do not",
        "아직 작업하지",
        "나중에",
        "만약",
        "if ",
        "instead of",
        "not ",
        "after finishing",
        "어제",
        "예전에",
        "마쳤",
        "했어",
        "했습니다",
        "했던",
        "해봤",
        "해보고",
        "싶어",
    ]
    .iter()
    .any(|word| text.contains(word))
}

fn directive(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    let korean = [
        "작업해",
        "작업하자",
        "작업을 진행해",
        "작업을 해",
        "구현해",
        "수정해",
        "검토해",
        "리뷰해",
        "진행해",
        "해결해",
        "확인해",
        "연결해",
        "대상으로",
        "재개해",
        "작업해주세요",
        "구현해주세요",
        "검토해주세요",
    ]
    .iter()
    .any(|word| text.contains(word));
    let imperative = text
        .trim_start()
        .strip_prefix("please ")
        .unwrap_or(text.trim_start());
    let first = imperative
        .split_ascii_whitespace()
        .next()
        .unwrap_or_default();
    korean
        || [
            "implement",
            "fix",
            "review",
            "resume",
            "address",
            "investigate",
            "connect",
        ]
        .contains(&first)
        || imperative.starts_with("work on ")
}

fn connector(text: &str) -> bool {
    let cleaned = text
        .to_ascii_lowercase()
        .replace("issue", "")
        .replace("이슈", "")
        .replace("pr", "");
    let cleaned = cleaned.trim();
    matches!(cleaned, "와" | "과" | "및" | "," | "and" | "&" | "+" | "/")
}

fn simple_replacement(line: &str, refs: &[Token], index: usize) -> bool {
    if refs.len() != 2 || index != 1 {
        return false;
    }
    let prefix = line[..refs[0].start].trim().to_ascii_lowercase();
    let middle = line[refs[0].end..refs[1].start].trim();
    let suffix = line[refs[1].end..].trim().trim_end_matches('.');
    matches!(prefix.as_str(), "" | "이슈" | "issue" | "pr")
        && matches!(middle, "말고" | "대신")
        && suffix.is_empty()
}

fn quoted(text: &str, at: usize) -> bool {
    let prefix = &text[..at];
    prefix.matches('`').count() % 2 == 1
        || prefix.matches('"').count() % 2 == 1
        || prefix.matches('“').count() > prefix.matches('”').count()
        || prefix.matches('‘').count() > prefix.matches('’').count()
        || prefix.matches('\'').count() % 2 == 1
}

/// Every explicit entity is retained. Only simple, direct, unquoted user instructions are current.
pub(super) fn extract(
    text: &str,
    repository: Option<&str>,
    evidence: &Evidence,
    provenance: Provenance,
    truncated: bool,
) -> Vec<(TaskReference, bool)> {
    let mut result: Vec<(TaskReference, bool)> = Vec::new();
    let mut fenced = false;
    let mut line_offset = 0;
    for full_line in text.split_inclusive('\n') {
        let line = full_line.trim_end_matches('\n');
        let offset = line_offset;
        line_offset += full_line.len();
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        let refs = tokens(line);
        let comparison = line.contains("비교") || line.to_ascii_lowercase().contains("compare");
        for (index, token) in refs.iter().enumerate() {
            let raw = match token.kind {
                Some(Kind::Issue) if !token.raw.starts_with("https://") => {
                    format!("issue {}", token.raw)
                }
                Some(Kind::Pr) if !token.raw.starts_with("https://") => format!("pr {}", token.raw),
                _ => token.raw.clone(),
            };
            let Ok(mut reference) = parse(&raw, repository, evidence, provenance) else {
                continue;
            };
            let end = refs.get(index + 1).map_or(line.len(), |next| next.start);
            let suffix = line[token.end..end]
                .split(['.', ';', '!', '?'])
                .next()
                .unwrap_or_default();
            let prefix_start = refs
                .get(index.wrapping_sub(1))
                .map_or(0, |previous| previous.end);
            let mut prefix = line[prefix_start..token.start]
                .rsplit(['.', ';', '!', '?'])
                .next()
                .unwrap_or_default();
            for correction in [
                "말고",
                "아닌",
                "대신",
                "rather than",
                "하지 않고",
                "안 하고",
            ] {
                prefix = prefix.rsplit(correction).next().unwrap_or(prefix);
            }
            let parallel = refs
                .windows(2)
                .all(|pair| connector(&line[pair[0].end..pair[1].start]));
            let shared_directive = parallel
                && !unsafe_role(line)
                && (directive(&line[..refs[0].start])
                    || directive(&line[refs.last().unwrap().end..]));
            let direct = provenance == Provenance::UserPrompt
                && !truncated
                && !fenced
                && !line.trim_start().starts_with('>')
                && !comparison
                && !quoted(text, offset + token.start)
                && !unsafe_role(suffix)
                && !unsafe_role(prefix)
                && (directive(suffix)
                    || directive(prefix)
                    || shared_directive
                    || simple_replacement(line, &refs, index));
            reference.source.quote = line.chars().take(600).collect();
            if let Some((old, old_direct)) =
                result.iter_mut().find(|(old, _)| old.id == reference.id)
            {
                *old_direct |= direct;
                if direct {
                    *old = reference;
                }
            } else {
                result.push((reference, direct));
            }
        }
    }
    result
}

pub(super) fn direct_report(
    prompt: &str,
    quote: &str,
    references: &[TaskReference],
    repository: Option<&str>,
) -> bool {
    // Exact text alone does not make quoted/comparison material a present instruction.
    // Classify against the full captured prompt, retaining punctuation and quote boundaries.
    let full = extract(
        prompt,
        repository,
        &Evidence::default(),
        Provenance::UserPrompt,
        false,
    );
    let quoted = extract(
        quote,
        repository,
        &Evidence::default(),
        Provenance::UserPrompt,
        false,
    );
    references.iter().all(|reference| {
        full.iter()
            .any(|(r, direct)| *direct && r.id == reference.id)
            && quoted
                .iter()
                .any(|(r, direct)| *direct && r.id == reference.id)
    })
}

pub(super) fn clear_intent(text: &str) -> bool {
    !text.is_empty()
        && !unsafe_role(text)
        && [
            "연결 해제",
            "연결을 해제",
            "연결 안 함",
            "연결하지",
            "clear the link",
            "unlink",
        ]
        .iter()
        .any(|word| text.to_ascii_lowercase().contains(word))
}

pub(super) fn direct_clear(prompt: &str, quote: &str) -> bool {
    if !clear_intent(quote) {
        return false;
    }
    let phrases = [
        "연결 해제",
        "연결을 해제",
        "연결 안 함",
        "연결하지",
        "clear the link",
        "unlink",
    ];
    prompt.match_indices(quote).any(|(start, _)| {
        phrases.iter().any(|phrase| {
            quote.match_indices(phrase).any(|(inside, _)| {
                let at = start + inside;
                let before = &prompt[..at];
                let line_start = before.rfind('\n').map_or(0, |n| n + 1);
                let line_end = prompt[at..].find('\n').map_or(prompt.len(), |n| at + n);
                let line = &prompt[line_start..line_end];
                let prefix = prompt[line_start..at].trim().to_ascii_lowercase();
                let suffix = prompt[at + phrase.len()..line_end]
                    .trim()
                    .trim_end_matches(['.', '!'])
                    .to_ascii_lowercase();
                let command = if phrase.is_ascii() {
                    matches!(prefix.as_str(), "" | "please")
                        && matches!(suffix.as_str(), "" | "this task" | "for this task")
                } else {
                    matches!(
                        prefix.as_str(),
                        "" | "이 작업"
                            | "이 작업의"
                            | "현재 작업"
                            | "현재 작업의"
                            | "현재 대화"
                            | "현재 대화의"
                            | "이 대화"
                            | "이 대화의"
                            | "이번 작업"
                            | "이번 작업의"
                    ) && matches!(
                        suffix.as_str(),
                        "" | "해줘" | "해 줘" | "해주세요" | "해 주세요" | "해" | "하자" | "해줘요"
                    )
                };
                !quoted(prompt, at)
                    && before.matches("```").count().is_multiple_of(2)
                    && !line.trim_start().starts_with('>')
                    && !unsafe_role(line)
                    && command
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refs(text: &str) -> Vec<(TaskReference, bool)> {
        extract(
            text,
            Some("owner/repo"),
            &Evidence::default(),
            Provenance::UserPrompt,
            false,
        )
    }

    #[test]
    fn direct_requests_keep_all_entities_and_never_pick_a_primary() {
        let found = refs("이슈 #142와 #156 작업해줘");
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|(_, current)| *current));
        assert_eq!(found[0].0.kind, Some(Kind::Issue));
        assert_eq!(found[1].0.kind, None);
    }

    #[test]
    fn correction_targets_new_reference_and_retains_old_as_candidate() {
        let found = refs("#142 말고 #156 작업해줘");
        assert!(!found[0].1);
        assert!(found[1].1);
        assert_eq!(found[1].0.number, 156);
        for text in ["#142 말고 #156", "owner/repo#142 대신 owner/repo#156"] {
            let found = refs(text);
            assert!(!found[0].1, "{text}");
            assert!(found[1].1, "{text}");
        }
        for text in ["\"#142 말고 #156\"", "어제 #142 말고 #156 검토를 마쳤어"] {
            assert!(refs(text).iter().all(|(_, current)| !current), "{text}");
        }
    }

    #[test]
    fn quotations_references_comparisons_negations_and_truncation_are_candidates() {
        for text in [
            "#142 참고해",
            "#142와 #156 비교해줘",
            "#142 작업하지 마",
            "#142는 나중에 작업",
            "\"#142 작업해줘\"라고 말했던 기록",
            "“\n#142 작업해줘\n”라는 예시",
            "> #142 작업해줘",
            "```\n#142 작업해줘\n```",
            "`#142 작업해줘`",
        ] {
            let found = refs(text);
            assert!(!found.is_empty(), "{text}");
            assert!(found.iter().all(|(_, current)| !current), "{text}");
        }
        assert!(
            !extract(
                "#142 작업해줘",
                Some("owner/repo"),
                &Evidence::default(),
                Provenance::UserPrompt,
                true
            )[0]
            .1
        );
        assert!(
            !extract(
                "owner/repo#142 작업해줘",
                None,
                &Evidence::default(),
                Provenance::AssistantMention,
                false
            )[0]
            .1
        );
    }

    #[test]
    fn source_exact_quote_does_not_promote_quoted_or_unrelated_instructions() {
        let reference = parse(
            "#142",
            Some("owner/repo"),
            &Evidence::default(),
            Provenance::AgentReport,
        )
        .unwrap();
        assert!(!direct_report(
            "\"#142 작업해줘\"는 인용문",
            "#142 작업해줘",
            std::slice::from_ref(&reference),
            Some("owner/repo")
        ));
        assert!(!direct_report(
            "#142 참고하고 #156 작업해줘",
            "#142 참고하고 #156 작업해줘",
            &[reference],
            Some("owner/repo")
        ));
        assert!(!direct_clear("\"연결 해제\"는 예시", "연결 해제"));
        assert!(direct_clear("이 작업의 연결 해제", "이 작업의 연결 해제"));
        for text in [
            "\"연결 해제\"",
            "‘연결 해제’",
            "```\n연결 해제\n```",
            "> 연결 해제",
            "어제 연결 해제했어",
            "연결 해제는 참고용이야",
            "연결 해제하지 마",
            "unlink 함수 수정해줘",
            "연결 해제 기능 구현해줘",
        ] {
            assert!(!direct_clear(text, text), "{text}");
        }
        for text in [
            "연결 해제해줘",
            "unlink this task",
            "please unlink this task",
        ] {
            assert!(direct_clear(text, text), "{text}");
        }
    }

    #[test]
    fn completed_past_work_and_dependency_references_are_never_current() {
        assert!(!refs("어제 #142 검토를 마쳤어")[0].1);
        let mixed = refs("#142 작업해줘. #156은 예전에 검토했어");
        assert!(mixed[0].1);
        assert!(!mixed[1].1);
        let negated = refs("이번에는 #142 작업은 하지 않고 #156 검토해줘");
        assert!(!negated[0].1);
        assert!(negated[1].1);
        assert!(!refs("Fix #142 after finishing #205")[1].1);
        assert!(!refs("I fixed #142 yesterday")[0].1);
    }

    #[test]
    fn short_repository_qualifiers_are_preserved_and_only_resolved_on_exact_basename() {
        let found = refs("demo-space#142 작업해줘");
        assert_eq!(found[0].0.raw, "demo-space#142");
        assert!(found[0].0.repository.is_none());
        assert!(found[0].1);
        assert_eq!(found[0].0.verification, VerificationState::Error);
        let matching = refs("repo#142 작업해줘");
        assert_eq!(matching[0].0.repository.as_deref(), Some("owner/repo"));
        assert!(matching[0].1);
        let manual = parse(
            "demo-space#142",
            Some("owner/repo"),
            &Evidence::default(),
            Provenance::Manual,
        )
        .unwrap();
        assert!(manual.repository.is_none());
        assert_eq!(manual.raw, "demo-space#142");
    }

    #[test]
    fn parse_preserves_unknown_kind_and_refuses_wrong_hosts_or_pasted_mixed_kind() {
        let evidence = Evidence::default();
        let reference = parse("#142", Some("Owner/Repo"), &evidence, Provenance::Manual).unwrap();
        assert_eq!(reference.id, "owner/repo#142");
        assert_eq!(reference.kind, None);
        assert_eq!(reference.url, None);
        let unresolved = parse("#142", None, &evidence, Provenance::Manual).unwrap();
        assert!(unresolved.repository.is_none());
        assert_eq!(unresolved.verification, VerificationState::Error);
        for bad in [
            "0",
            "https://evil.test/owner/repo/issues/142",
            "https://github.com/owner/repo/issues/142?x=1",
            "issue https://github.com/owner/repo/pull/142",
        ] {
            assert!(
                parse(bad, Some("owner/repo"), &evidence, Provenance::Manual).is_err(),
                "{bad}"
            );
        }
        assert!(refs("https://example.test/#142 작업해줘").is_empty());
    }
}
