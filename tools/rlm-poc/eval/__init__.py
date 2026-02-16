"""Evaluation harness for RLM learning extraction."""

import json
from dataclasses import dataclass
from pathlib import Path


@dataclass
class EvalMetrics:
    """Precision/recall/F1 for learning extraction."""
    precision: float
    recall: float
    f1: float
    matched: int
    total_predicted: int
    total_expected: int
    details: list[dict]


def normalize_principle(text: str) -> str:
    """Normalize a principle for fuzzy comparison."""
    return " ".join(text.lower().strip().split())


def principles_match(predicted: str, expected: str, threshold: float = 0.5) -> bool:
    """Check if two principles are semantically similar enough.

    Uses simple word overlap as a baseline. For production, use embeddings.
    """
    p_words = set(normalize_principle(predicted).split())
    e_words = set(normalize_principle(expected).split())

    if not p_words or not e_words:
        return False

    overlap = len(p_words & e_words)
    # Jaccard-like similarity
    similarity = overlap / len(p_words | e_words)
    return similarity >= threshold


def evaluate(
    predicted: list[dict],
    expected: list[dict],
    match_threshold: float = 0.3,
) -> EvalMetrics:
    """Evaluate predicted learnings against expected annotations.

    Args:
        predicted: List of {"principle": ..., "kind": ..., ...}
        expected: List of {"principle": ..., "kind": ..., ...}
        match_threshold: Minimum similarity for a match

    Returns:
        EvalMetrics with precision, recall, F1
    """
    matched_expected = set()
    matched_predicted = set()
    details = []

    for i, pred in enumerate(predicted):
        best_match = None
        best_score = 0.0

        for j, exp in enumerate(expected):
            if j in matched_expected:
                continue

            # Kind must match
            if pred.get("kind") != exp.get("kind"):
                continue

            p_words = set(normalize_principle(pred.get("principle", "")).split())
            e_words = set(normalize_principle(exp.get("principle", "")).split())

            if p_words and e_words:
                overlap = len(p_words & e_words)
                score = overlap / len(p_words | e_words)
                if score > best_score and score >= match_threshold:
                    best_score = score
                    best_match = j

        if best_match is not None:
            matched_expected.add(best_match)
            matched_predicted.add(i)
            details.append({
                "predicted": pred.get("principle", ""),
                "expected": expected[best_match].get("principle", ""),
                "kind": pred.get("kind", ""),
                "similarity": round(best_score, 3),
                "match": True,
            })
        else:
            details.append({
                "predicted": pred.get("principle", ""),
                "expected": None,
                "kind": pred.get("kind", ""),
                "similarity": 0.0,
                "match": False,
            })

    # Add unmatched expected
    for j, exp in enumerate(expected):
        if j not in matched_expected:
            details.append({
                "predicted": None,
                "expected": exp.get("principle", ""),
                "kind": exp.get("kind", ""),
                "similarity": 0.0,
                "match": False,
            })

    n_matched = len(matched_predicted)
    precision = n_matched / len(predicted) if predicted else 0.0
    recall = n_matched / len(expected) if expected else 0.0
    f1 = (2 * precision * recall / (precision + recall)) if (precision + recall) > 0 else 0.0

    return EvalMetrics(
        precision=round(precision, 3),
        recall=round(recall, 3),
        f1=round(f1, 3),
        matched=n_matched,
        total_predicted=len(predicted),
        total_expected=len(expected),
        details=details,
    )


def load_annotations(path: str | Path) -> list[dict]:
    """Load manual annotations from a JSON file.

    Expected format:
    [
        {"principle": "...", "kind": "guiding" | "cautionary", "evidence": "..."},
        ...
    ]
    """
    return json.loads(Path(path).read_text())
