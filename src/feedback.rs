//! 明示された編集判断の集計。差分がないことを不採用とは推測しない。

use super::EvaluationError;
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Decisions {
    schema_version: u32,
    decisions: Vec<Decision>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Decision {
    id: String,
    file: String,
    source_sha256: String,
    category: String,
    line: usize,
    decision: Outcome,
    reason: String,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Outcome {
    Accepted,
    Retained,
    Deferred,
    Unreviewed,
    Unaddressable,
}

impl Outcome {
    fn key(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Retained => "retained",
            Self::Deferred => "deferred",
            Self::Unreviewed => "unreviewed",
            Self::Unaddressable => "unaddressable",
        }
    }
}

pub fn feedback(path: &Path) -> Result<String, EvaluationError> {
    let source = suiko::read_source(path)?;
    let data: Decisions = serde_json::from_str(&source).map_err(|e| EvaluationError::Parse {
        path: path.display().to_string(),
        message: e.to_string(),
    })?;
    if data.schema_version != 1 {
        return Err(EvaluationError::Invalid(
            "編集判断はschema_version: 1を指定してください".to_owned(),
        ));
    }
    let mut ids = BTreeSet::new();
    let mut sources = BTreeSet::new();
    let mut categories = BTreeMap::<String, BTreeMap<&str, usize>>::new();
    for decision in &data.decisions {
        if decision.id.trim().is_empty()
            || !ids.insert(&decision.id)
            || decision.file.trim().is_empty()
            || decision.line == 0
            || !suiko::lint::is_known_rule(&decision.category)
            || decision.source_sha256.len() != 64
            || !decision
                .source_sha256
                .bytes()
                .all(|ch| ch.is_ascii_hexdigit())
            || !matches!(decision.decision, Outcome::Unreviewed)
                && decision.reason.trim().is_empty()
        {
            return Err(EvaluationError::Invalid(format!(
                "編集判断のID・出典・カテゴリ・理由を確認してください: {}",
                decision.id
            )));
        }
        sources.insert((&decision.file, &decision.source_sha256));
        *categories
            .entry(decision.category.clone())
            .or_default()
            .entry(decision.decision.key())
            .or_default() += 1;
    }
    let rows = categories.into_iter().map(|(category, counts)| {
        let count = |key| counts.get(key).copied().unwrap_or(0);
        let total: usize = counts.values().sum();
        let reviewed = count("accepted") + count("retained");
        json!({"category": category, "total": total, "reviewed": reviewed, "accepted": count("accepted"), "retained": count("retained"), "deferred": count("deferred"), "unreviewed": count("unreviewed"), "unaddressable": count("unaddressable"), "acceptance_rate": (reviewed > 0).then(|| count("accepted") as f64 / reviewed as f64), "reviewed_fraction": reviewed as f64 / total as f64})
    }).collect::<Vec<_>>();
    serde_json::to_string_pretty(&json!({"schema_version": 1, "source_count": sources.len(), "decision_count": data.decisions.len(), "categories": rows, "interpretation": "採否の集計です。維持した表現を誤検出とは断定せず、未確認・保留・対応不能を採用率の分母から除きます。"})).map_err(|e| EvaluationError::Invalid(e.to_string()))
}
