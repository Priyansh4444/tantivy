#!/usr/bin/env python3
"""Native synthetic differential checks; statistics are never overridden."""
import argparse
import itertools
import json
import math
import random
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parents[3]


def term(word):
    return {"type": "term", "term": word}


def boolean(clauses, minimum=None):
    ast = {"type": "bool", "clauses": [{"occur": occur, "query": query} for occur, query in clauses]}
    if minimum is not None:
        ast["minimum_should_match"] = minimum
    return ast


def queries():
    alpha, beta, gamma = map(term, ["alpha", "beta", "gamma"])
    definitions = [
        ("term_alpha", alpha), ("term_common", term("common")), ("term_absent", term("absent")),
        ("and_alpha_beta", boolean([("must", alpha), ("must", beta)])),
        ("or_alpha_beta", boolean([("should", alpha), ("should", beta)])),
        ("must_not_gamma", boolean([("must", alpha), ("must_not", gamma)])),
        ("filter_beta", boolean([("must", alpha), ("filter", beta)])),
        ("filter_only", boolean([("filter", beta)])),
        ("minimum_two", boolean([("should", alpha), ("should", beta), ("should", gamma)], 2)),
        ("required_and_optional", boolean([("must", alpha), ("should", beta), ("should", gamma)])),
        ("required_minimum_one", boolean([("must", alpha), ("should", beta), ("should", gamma)], 1)),
        ("duplicate_should", boolean([("should", alpha), ("should", alpha)])),
        ("boost_alpha", {"type": "boost", "boost": 3.25, "query": alpha}),
        ("boost_or", boolean([("should", {"type": "boost", "boost": 0.25, "query": alpha}), ("should", beta)])),
        ("nested", boolean([("must", boolean([("should", alpha), ("should", beta)])), ("must_not", gamma)])),
    ]
    for words in [["alpha", "beta"], ["alpha", "alpha"], ["alpha", "beta", "alpha"]]:
        definitions.append(("phrase_" + "_".join(words), {"type": "phrase", "terms": words, "slop": 0}))
    for words in [["alpha", "beta"], ["alpha", "alpha"]]:
        for slop in [1, 2, 3]:
            definitions.append(("slop_%d_%s" % (slop, "_".join(words)), {"type": "phrase", "terms": words, "slop": slop}))
    return [{"name": name, "query": ast} for name, ast in definitions]


def fixtures():
    patterns = [
        ["alpha", "alpha", "beta"], ["alpha", "beta"], ["beta", "alpha"],
        ["alpha", "gamma", "beta"], ["alpha", "gamma", "gamma", "beta"],
        ["alpha", "beta", "alpha"], ["alpha", "gamma", "alpha"],
        ["beta"], ["gamma"], ["alpha"] * 97, ["common", "beta"],
    ]
    for segments in [1, 3]:
        for missing in [False, True]:
            for deleted in [False, True]:
                docs = []
                for index in range(321):
                    tokens = ["common"] + patterns[index % len(patterns)]
                    if missing and index % 13 == 0:
                        tokens = [] if index % 2 else None
                    docs.append({"id": index, "tokens": tokens, "segment": min(segments - 1, index * segments // 321), "deleted": deleted and index % 17 == 0})
                yield {"name": f"deterministic_segments{segments}_missing{missing}_deleted{deleted}", "documents": docs, "queries": queries()}
    for fieldnorms in [True, False]:
        docs = [{"id": index, "tokens": None if index % 17 == 0 else ([] if index % 19 == 0 else patterns[index % len(patterns)]), "segment": index // 60, "deleted": index % 13 == 0} for index in range(180)]
        yield {"name": f"merged_missing_deleted_fieldnorms{fieldnorms}", "fieldnorms": fieldnorms, "merge": True, "documents": docs, "queries": queries()}
    docs = [{"id": 0, "tokens": ["alpha", "beta"]},
            {"id": 1, "tokens": ["alpha"] + ["padding"] * 260 + ["beta"]},
            {"id": 2, "tokens": ["alpha"] + ["padding"] * 260 + ["alpha"]},
            {"id": 3, "tokens": ["alpha"]},
            {"id": 4, "tokens": ["alpha"] + ["padding"] * 511 + ["beta"]}]
    definitions = [{"name": f"wide_slop_{slop}", "query": {"type": "phrase", "terms": ["alpha", "beta"], "slop": slop}} for slop in [255, 256, 260, 300, 511]]
    definitions.append({"name": "wide_repeated_slop_300", "query": {"type": "phrase", "terms": ["alpha", "alpha"], "slop": 300}})
    yield {"name": "wide_slop_carry", "documents": docs, "queries": definitions}
    carry_doc = ["alpha"] + ["x"] * 256 + ["beta", "x", "gamma"]
    yield {"name": "three_term_slop_carry_256", "documents": [{"id": 0, "tokens": carry_doc}], "queries": [{"name": f"three_term_slop_{slop}", "query": {"type": "phrase", "terms": ["alpha", "beta", "gamma"], "slop": slop}} for slop in [255, 256, 257]]}
    docs = [{"id": index, "tokens": words} for index, words in enumerate([
        ["alpha", "beta", "alpha", "beta"], ["alpha", "beta", "gamma", "alpha", "beta"],
        ["alpha", "gamma", "beta", "alpha", "gamma", "beta"], ["alpha", "beta", "gamma"],
        ["alpha", "alpha", "beta", "beta", "alpha", "beta"], ["alpha", "beta", "alpha", "beta", "alpha", "beta"]])]
    definitions = [{"name": f"interleaved_{'_'.join(words)}_{slop}", "query": {"type": "phrase", "terms": words, "slop": slop}} for words in [["alpha", "beta", "alpha"], ["alpha", "beta", "alpha", "beta"], ["alpha", "alpha", "alpha"]] for slop in [1, 2, 4]]
    yield {"name": "interleaved_repetition_groups", "documents": docs, "queries": definitions}
    for seed in [7, 19, 31]:
        rng = random.Random(seed)
        docs = []
        for index in range(90):
            tokens = [rng.choice(["common", "alpha", "beta", "gamma", "rare"]) for _ in range(rng.randrange(15))]
            if rng.randrange(15) == 0:
                tokens = None
            docs.append({"id": index, "tokens": tokens, "segment": index // 30, "deleted": rng.randrange(10) == 0})
        yield {"name": f"seed_{seed}", "documents": docs, "queries": queries()}


def matches(ast, tokens):
    tokens = tokens or []
    kind = ast["type"]
    if kind == "term":
        return ast["term"] in tokens
    if kind == "boost":
        return matches(ast["query"], tokens)
    if kind == "phrase":
        words, slop = ast["terms"], ast.get("slop", 0)
        if slop == 0:
            return any(tokens[start:start + len(words)] == words for start in range(len(tokens) - len(words) + 1))
        positions = [[position for position, token in enumerate(tokens) if token == word] for word in words]
        for assignment in itertools.product(*positions):
            if any(words[left] == words[right] and assignment[left] == assignment[right]
                   for left in range(len(words)) for right in range(left)):
                continue
            normalized = [position - offset for offset, position in enumerate(assignment)]
            if max(normalized) - min(normalized) <= slop:
                return True
        return False
    if kind == "bool":
        clauses = ast["clauses"]
        required = [clause for clause in clauses if clause["occur"] in ["must", "filter"]]
        optional = [clause for clause in clauses if clause["occur"] == "should"]
        prohibited = [clause for clause in clauses if clause["occur"] == "must_not"]
        if not required and not optional:
            return False
        minimum = ast.get("minimum_should_match", 0)
        if not required:
            minimum = max(1, minimum)
        return (all(matches(clause["query"], tokens) for clause in required)
                and not any(matches(clause["query"], tokens) for clause in prohibited)
                and sum(matches(clause["query"], tokens) for clause in optional) >= minimum)
    raise ValueError(kind)


def close(a, b):
    return math.isfinite(a) and math.isfinite(b) and abs(a - b) <= 4e-6 * max(1.0, abs(a), abs(b))


def validate_engine(case, output, engine):
    issues = []
    for definition, result in zip(case["queries"], output["queries"], strict=True):
        expected = {doc["id"] for doc in case["documents"] if not doc.get("deleted", False) and matches(definition["query"], doc.get("tokens"))}
        observed = {hit["id"] for hit in result["exhaustive"]}
        prefix = {"case": case["name"], "query": definition["name"], "engine": engine}
        if expected != observed:
            issues.append(dict(prefix, type="literal_matches", missing=sorted(expected - observed), extra=sorted(observed - expected)))
        if result["count"] != len(observed):
            issues.append(dict(prefix, type="count_vs_exhaustive", count=result["count"], exhaustive=len(observed)))
        all_scores = {hit["id"]: hit["score"] for hit in result["exhaustive"]}
        top = result["top"]
        if len(top) != min(10, len(observed)) or len({hit["id"] for hit in top}) != len(top):
            issues.append(dict(prefix, type="top_length"))
        if top:
            threshold = min(hit["score"] for hit in top)
            selected = {hit["id"] for hit in top}
            if any(hit["score"] > threshold and not close(hit["score"], threshold) and hit["id"] not in selected for hit in result["exhaustive"]):
                issues.append(dict(prefix, type="top_omitted_better_hit"))
        for hit in top:
            if hit["id"] not in all_scores or not close(hit["score"], all_scores[hit["id"]]):
                issues.append(dict(prefix, type="top_score", hit=hit))
    return issues


def compare(cases, tantivy, lucene):
    issues = []
    same_convention_mismatches = []
    raw_score_mismatches = []
    for case, t_output, l_output in zip(cases, tantivy, lucene, strict=True):
        issues += validate_engine(case, t_output, "tantivy")
        issues += validate_engine(case, l_output, "lucene")
        for t_query, l_query in zip(t_output["queries"], l_output["queries"], strict=True):
            t_hits = {hit["id"]: hit["score"] for hit in t_query["exhaustive"]}
            l_hits = {hit["id"]: hit["score"] for hit in l_query["exhaustive"]}
            prefix = {"case": case["name"], "query": t_query["name"]}
            if t_query["count"] != l_query["count"] or t_hits.keys() != l_hits.keys():
                issues.append(dict(prefix, type="cross_engine_matches", tantivy_count=t_query["count"], lucene_count=l_query["count"]))
            mismatch = []
            raw_mismatch = []
            for doc_id in t_hits.keys() & l_hits.keys():
                # Lucene omits the global (k1+1) numerator; this declared score
                # convention conversion changes neither statistics nor ranking.
                if not close(t_hits[doc_id], l_hits[doc_id]):
                    raw_mismatch.append({"id": doc_id, "tantivy_raw": t_hits[doc_id], "lucene_raw": l_hits[doc_id]})
                expected = 2.2 * l_hits[doc_id]
                if not close(t_hits[doc_id], expected):
                    mismatch.append({"id": doc_id, "tantivy": t_hits[doc_id], "lucene_raw": l_hits[doc_id], "lucene_same_convention": expected})
            if mismatch:
                same_convention_mismatches.append(dict(prefix, mismatched_documents=len(mismatch), examples=mismatch[:3]))
            if raw_mismatch:
                raw_score_mismatches.append(dict(prefix, mismatched_documents=len(raw_mismatch), examples=raw_mismatch[:3]))
            # Compare exact ID membership at every rank except mathematically
            # tied boundaries, where engine-internal segment tie breaking differs.
            t_rank = sorted(t_hits, key=lambda doc_id: (-t_hits[doc_id], doc_id))
            l_rank = sorted(l_hits, key=lambda doc_id: (-l_hits[doc_id], doc_id))
            if t_rank[:10] != l_rank[:10]:
                issues.append(dict(prefix, type="cross_engine_canonical_top_ids", tantivy=t_rank[:10], lucene=l_rank[:10]))
    return {"cases":len(cases),"queries":sum(len(case["queries"]) for case in cases),
            "structural_issues":issues,
            "native_raw_score_mismatches":raw_score_mismatches,
            "same_convention_statistic_and_phrase_mismatches":same_convention_mismatches,
            "semantic_and_ranking_passed":not issues,
            "same_convention_diagnostic_passed":not issues and not same_convention_mismatches,
            "passed":not issues and not raw_score_mismatches}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--jars",type=Path,default=REPO.parent/"search-benchmark-game/engines/lucene-10.4.0/build/dependencies")
    parser.add_argument("--target",type=Path,default=Path("/tmp/tantivy-parity-build-oct03"))
    parser.add_argument("--output",type=Path,default=Path("/tmp/tantivy-parity-results-oct03"))
    parser.add_argument("--minimal",action="store_true")
    parser.add_argument("--cases",type=Path,help="frozen JSONL cases, replacing generated cases")
    parser.add_argument("--skip-build",action="store_true")
    args=parser.parse_args()
    args.output.mkdir(parents=True,exist_ok=True)
    java_build=args.output/"java"
    java_build.mkdir(exist_ok=True)
    if not args.skip_build:
        subprocess.run(["cargo","build","--release","--manifest-path",str(ROOT/"rust/Cargo.toml"),"--target-dir",str(args.target)],check=True)
        subprocess.run(["javac","-cp",str(args.jars/"*"),"-d",str(java_build),str(ROOT/"LuceneParity.java")],check=True)
    cases_path = args.cases or (ROOT/"minimal.jsonl" if args.minimal else None)
    cases = [json.loads(line) for line in cases_path.read_text().splitlines()] if cases_path else list(fixtures())
    data="".join(json.dumps(case)+"\n" for case in cases)
    (args.output/"cases.jsonl").write_text(data)
    engines={}
    commands={"tantivy":[str(args.target/"release/tantivy-synthetic-parity")],"lucene":["java","--add-modules","jdk.incubator.vector","-cp",str(java_build)+":"+str(args.jars/"*"),"LuceneParity"]}
    for name,command in commands.items():
        result=subprocess.run(command,input=data,text=True,capture_output=True,check=True)
        (args.output/(name+".jsonl")).write_text(result.stdout)
        (args.output/(name+".stderr")).write_text(result.stderr)
        engines[name]=[json.loads(line) for line in result.stdout.splitlines()]
    report=compare(cases,engines["tantivy"],engines["lucene"])
    (args.output/"report.json").write_text(json.dumps(report,indent=2)+"\n")
    print(json.dumps({key: value if not isinstance(value,list) else len(value) for key,value in report.items()},indent=2))
    return 0 if report["passed"] else 1

if __name__ == "__main__":
    sys.exit(main())
