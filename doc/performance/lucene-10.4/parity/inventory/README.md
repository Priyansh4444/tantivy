# Lucene 10.4 public API and feature inventory

This is an extraction and source audit, not a declaration of implementation parity. It accounts for every module in the [official Lucene 10.4 documentation catalog](https://lucene.apache.org/core/10_4_0/), the separately published Luke artifact, and distribution launchers/public `main` entrypoints. No production files were edited; no Lucene/Tantivy classes, tests or benchmarks were executed or built. `javap` reads existing published class files.

Pinned sources: Lucene annotated tag `releases/lucene/10.4.0`, tag object `c6a89ffb8b4eaa2962a5530fa10012365b6f5c22`, peeled source commit `9983b7ce7fdd04f4d357688fb85c14277c15ea8d`; Tantivy `65c4e15e6` (full commit in `manifest.json`). Published Maven artifacts are version 10.4.0. Every downloaded catalog/search index/JAR has a URL, byte length and SHA-256 in the manifest; the mapper records hashes of pinned Tantivy source evidence. Versioned URLs alone are not a content hash.

## Coverage

| Item | Count |
| --- | ---: |
| Official documented modules | 31 |
| Additional Luke module | 1 |
| Accounted modules | 32 |
| Documented packages | 292 |
| Documented types | 3,427 |
| Union documented/binary public-or-protected API types | 3,637 |
| Documented types lacking binary extraction | 0 |
| Declared public members | 23,596 |
| Declared protected members | 3,226 |
| Declared constructors | 4,012 |
| Declared methods | 18,946 |
| Declared fields | 3,864 |
| Declared members total | 26,822 |
| Javadoc member-search labels | 25,542 |
| Download failures / javap warning-or-failure batches | 0 / 0 |
| Distribution launchers | 2 |
| Public main entrypoints | 33 |

`manifest.json` reports the generated public-main count and per-module totals; `cli-entrypoints.tsv` contains each entrypoint. All documented classes were cross-checked against the binary extraction. The extra binary-visible types include Luke and implementation/extension classes outside the published Javadoc surface. Binary visibility and documented status remain separate columns.

All documented types and every base-JAR class were processed for **declared API extraction within the stated scope**. The accessibility/annotation/multi-release omissions below prevent a claim of complete Java public API compatibility coverage. It is not a receiver-expanded inventory of inherited methods, an API behavior specification, or Java source/binary compatibility certification. A class count is not a method count. Protected extension members are explicitly extracted with `javap -protected -s`.

## Artifacts

* `generate_inventory.py`: standard-library downloader, pinned-source/tool extractor and binary/Javadoc inventory generator.
* `modules.tsv`, `packages.tsv`, `types.tsv`, `members.tsv`: module/package/type inventory and public/protected declarations with JVM descriptors.
* `documented-member-labels.tsv`: raw Javadoc member labels, which do not separately encode access or complete method signatures.
* `inventory.json`: combined machine-readable inventory.
* `tools.tsv`, `cli-entrypoints.tsv`: pinned distribution launchers and public executable entrypoints; CLI option/behavior equivalence is not tested.
* `unmatched-documented-types.tsv`: empty apart from its header when extraction is complete.
* `manifest.json`: hashes, pins, counts, retrieval errors, tool version and coverage caveats.
* `feature_families.json`, `map_features.py`, `feature-parity.tsv`, `module-coverage.tsv`, `parity-matrix.json`: finite family mapping to verified pinned Tantivy source references and separate compatibility axes.
* `api-worklist.tsv`: one uniquely identified pending behavioral/API contract review per declared member. Coarse family mapping is not per-member equivalence.
* `raw/`, `jars/`: inputs for hash-checked offline regeneration. These make the full directory approximately 88 MB; a source-controlled handoff can ship the generators, mappings, manifest and TSV/JSON outputs without vendoring Maven artifacts.
* `package_inventory.py`, `ship/lucene-10.4-api-inventory.tar.gz`: reproducible compact handoff containing generators, hashes/counts/matrix and gzip-compressed declaration/worklist tables. It excludes binary JARs, raw Javadocs/disassemblies and the redundant uncompressed combined JSON. Run online extraction after unpacking to reconstruct omitted inputs.

## Reproduction

From this directory, regenerate online extraction and source mapping with:

```bash
python generate_inventory.py --lucene-repo /path/to/lucene --tantivy-repo /path/to/tantivy --tantivy-rev 65c4e15e6
python map_features.py --tantivy-repo /path/to/tantivy
```

Regenerate with retained inputs and hash verification:

```bash
python generate_inventory.py --offline --lucene-repo /path/to/lucene --tantivy-repo /path/to/tantivy --tantivy-rev 65c4e15e6
python map_features.py --tantivy-repo /path/to/tantivy
```

The online command reuses cached files only when their saved URL and SHA-256 match. Delete inputs/manifest to force new retrieval. Offline mode rejects altered input bytes. Scripts write beside themselves, so the directory may be moved for eventual shipping; update command paths accordingly. `javap` 21.0.12.1 successfully read all published artifacts with no warnings.

## Feature and compatibility axes

47 finite families cover all 32 modules. Source-supported native surfaces are recorded as **partial**, not tested Lucene parity. The matrix has 20 partial, 26 missing and one unverified family. Missing means no built-in equivalent identified in the pinned tree; external plugins are outside this audit. No family is marked verified behavioral parity by this pass.

Each family independently records behavior, Rust API surface, Java facade, index-format status and runtime-test status. The major absent built-in categories include KNN/vector/HNSW/quantization, BKD and spatial geometry, span/interval queries, joins, grouped TopDocs, suggest/spellchecking, specialized analyzer suites, classification, monitoring and replication. Partial categories include full-text queries, BM25, range queries, fast fields, facets, snippets, expressions, indexing lifecycle and storage. Actual Rust source references and specific caveats are in the matrix.

At the inventory pin `65c4e15e6`, native BM25 was explicitly incomplete: fixed `k1=1.2,b=0.75`, raw `k1+1` multiplier 2.2, all-document field population and approximate deleted-merge token totals differ from Lucene. Existing matched benchmark overrides cannot certify native behavior. Rust traits/plugins are not a Java subclass ABI. Lucene public/protected superclass hooks, checked exceptions, JVM descriptors, module exports, service loaders and callback/lifetime behavior need a separate facade/JNI design if Java compatibility is required. Native Tantivy index files are not Lucene files; codec algorithms with similar names do not establish file interchange.

Later bounded corrections and measurements are tracked in the [current acceptance ledger](../ACCEPTANCE.md). They do not rewrite this pinned matrix or establish exhaustive family/API parity.

## Explicit remaining work

There are **zero unaccounted catalog modules and zero unextracted documented types**. Behavioral compatibility remains open for all 47 families, 3,637 type contracts and 26,822 declared-member worklist entries. Those entries are review units, not an estimate of independent implementation tasks: inherited/common contracts should be consolidated before implementation. No Java facade or Lucene index read/write compatibility is implemented by this inventory.

Remaining extraction omissions: inherited members are stored only at their Lucene declaring types; external dependencies and `java.*` inherited APIs are excluded; annotations, annotation defaults and constant values are not extracted; source accessibility of nested types/enclosing classes and JPMS exports is not certified; compiler bridges are retained; multi-release alternative class bodies are counted but not separately disassembled. No CLI options or behavioral contracts were exhaustively extracted from documentation prose. Internal build/JMH/analysis test modules outside the published API catalog are not advertised as documented API modules.

Published `element-list` and `package-list` URLs returned HTTP 404. The available type/member/package search indexes plus all base-JAR declarations supply the inventory; there is no fabricated element-list coverage.

Architectural implication: the requested full scope needs a Rust behavioral compatibility program with finite family acceptance suites, a Java facade with extension-contract tests, and Lucene file-format interoperability. These are required workstreams whose completion remains unverified. Matching current full-text rankings or speeding up a postings loop cannot satisfy those broader compatibility contracts.
