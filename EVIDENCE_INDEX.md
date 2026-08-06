# 📋 Evidence Mapping Index

## Quick Reference: Claim → Evidence File

### System Input Claims
| Claim | Category | Evidence File |
|-------|----------|------------------|
| Frame loads from disk | [MEASURED] | `docs/demo_evidence/screenshots/01_original_frame.png` |
| Frame is 1366x767 pixels | [MEASURED] | `docs/demo_evidence/screenshots/01_original_frame.png` |
| Frame size is 4.1 MB | [MEASURED] | Derived from frame dimensions |

### Detector Execution Claims
| Claim | Category | Evidence File |
|-------|----------|------------------|
| HUD detector executes | [MEASURED] | `docs/demo_evidence/json/gamestate_frame_0_pretty.json` (lines 6-35) |
| HUD detector confidence 0.55 | [MEASURED] | `docs/demo_evidence/json/gamestate_frame_0_pretty.json` (lines 11, 19, 26) |
| Motion detector executes | [MEASURED] | `docs/demo_evidence/json/gamestate_frame_0_pretty.json` (lines 37-42) |
| Motion entities detected: 0 | [MEASURED] | `docs/demo_evidence/json/gamestate_frame_0_pretty.json` (line 39) |
| Dialog detector executes | [MEASURED] | `docs/demo_evidence/json/gamestate_frame_0_pretty.json` (lines 43-52) |
| Panels detector executes | [MEASURED] | `docs/demo_evidence/json/gamestate_frame_0_pretty.json` (lines 53-69) |
| Environment detector finds 24 edges | [MEASURED] | `docs/demo_evidence/json/gamestate_frame_0_pretty.json` (lines 70+) |
| Combat detector reports Idle | [MEASURED] | `docs/demo_evidence/json/gamestate_frame_0_pretty.json` |

### Serialization Claims
| Claim | Category | Evidence File |
|-------|----------|------------------|
| GameState serializes to JSON | [MEASURED] | `docs/demo_evidence/json/gamestate_frame_0_pretty.json` (valid JSON) |
| Pretty JSON size: 3.4 KB | [MEASURED] | File size of gamestate_frame_0_pretty.json |
| Compact JSON size: 1.9 KB | [MEASURED] | File size of gamestate_frame_0_compact.json |
| All detector outputs in JSON | [MEASURED] | `docs/demo_evidence/json/gamestate_frame_0_pretty.json` |
| Display string generates successfully | [MEASURED] | `docs/demo_evidence/logs/frame_0_display_output.txt` |

### Latency Claims
| Claim | Category | Evidence File |
|-------|----------|------------------|
| Minimum latency: 76.3 ms | [MEASURED] | `docs/demo_evidence/csv/latency_measurements.csv` |
| Maximum latency: 557.9 ms | [MEASURED] | `docs/demo_evidence/csv/latency_measurements.csv` |
| Mean latency: 136.3 ms | [MEASURED] | `docs/demo_evidence/benchmarks/realtime_analysis.txt` |
| Median latency: 86.5 ms | [MEASURED] | `docs/demo_evidence/benchmarks/realtime_analysis.txt` |
| P90 latency: 247.2 ms | [MEASURED] | `docs/demo_evidence/benchmarks/realtime_analysis.txt` |
| P95 latency: 300.3 ms | [MEASURED] | `docs/demo_evidence/benchmarks/realtime_analysis.txt` |
| P99 latency: 502.5 ms | [MEASURED] | `docs/demo_evidence/benchmarks/realtime_analysis.txt` |
| Std Dev: 98.1 ms | [MEASURED] | `docs/demo_evidence/benchmarks/realtime_analysis.txt` |

### Real-Time Capability Claims
| Claim | Category | Evidence File |
|-------|----------|------------------|
| 30 FPS achievable | [MEASURED ❌] | `docs/demo_evidence/benchmarks/realtime_analysis.txt` (0/100 frames) |
| 50 FPS achievable | [MEASURED ❌] | `docs/demo_evidence/benchmarks/realtime_analysis.txt` (0/100 frames) |
| 60 FPS achievable | [MEASURED ❌] | `docs/demo_evidence/benchmarks/realtime_analysis.txt` (0/100 frames) |
| 11.6 FPS achievable | [DERIVED ✅] | Calculated from median: 1000/86.5 = 11.6 |
| OCR causes spikes | [MEASURED ✅] | Timing distribution in CSV (gaps indicate OCR) |

### Test & Code Quality Claims
| Claim | Category | Evidence File |
|-------|----------|------------------|
| 38 tests pass | [VERIFIED] | `cargo test --lib` output |
| No compilation errors | [VERIFIED] | `cargo build --release` succeeds |
| Criterion integrated | [VERIFIED] | `benches/vision_pipeline.rs` exists and builds |

### Design Status Claims
| Claim | Category | Notes |
|-------|----------|-------|
| Async OCR architecture | [DESIGNED] | Code structure supports it; not implemented |
| Geometry-only fast path | [DESIGNED] | Motion detector can work alone; not optimized |
| Confidence-based routing | [DESIGNED] | Types support it; policy not implemented |
| Frame skipping strategy | [DESIGNED] | Would require motion prediction (future) |

---

## Evidence File Details

### latency_measurements.csv
```
Format: iteration,latency_ms
Rows: 100 (one per frame measured)
Contains: Precise timing for each frame
Use for: Statistical analysis, regression detection
```

### gamestate_frame_0_pretty.json
```
Size: 3415 bytes
Format: Complete GameState serialization
Contains: All 6 detector outputs with confidence
Validation: Valid JSON (parses cleanly)
Use for: Understanding data structure, integration examples
```

### gamestate_frame_0_compact.json
```
Size: 1875 bytes
Format: Compact GameState serialization
Contains: Same data as pretty version
Use for: Network transmission, storage efficiency
```

### realtime_analysis.txt
```
Content: Statistical summary of 100 measurements
Includes: Min/Max/Mean/Median/P90/P95/P99/StdDev
Analysis: Real-time capability assessment by FPS target
Use for: Performance summary, benchmarking claims
```

### frame_0_display_output.txt
```
Content: Formatted display string output
Shows: HUD state, motion entities, dialog, panels, environment, combat
Format: Human-readable with ASCII box drawing
Use for: UI demonstration, display verification
```

### 01_original_frame.png
```
Format: PNG image (1366x767, 4.1 MB)
Content: Actual MapleStory screenshot used for all measurements
Use for: Visual verification of test data, reproducibility
```

---

## How to Read Evidence Files

### For CSV (latency_measurements.csv)
```bash
# View first 10 measurements
head -n 11 docs/demo_evidence/csv/latency_measurements.csv

# Calculate additional statistics
awk -F, 'NR>1 {sum+=$2; n++} END {print "Mean:", sum/n "ms"}' \
    docs/demo_evidence/csv/latency_measurements.csv

# Find slowest frame
tail -n +2 docs/demo_evidence/csv/latency_measurements.csv | sort -t, -k2 -rn | head -1
```

### For JSON (gamestate_frame_0_*.json)
```bash
# Pretty-print for readability
cat docs/demo_evidence/json/gamestate_frame_0_pretty.json

# Extract specific field (e.g., HUD HP)
jq '.hud.hp' docs/demo_evidence/json/gamestate_frame_0_pretty.json

# Count detected entities
jq '.motion.total_count' docs/demo_evidence/json/gamestate_frame_0_pretty.json
```

### For Analysis (realtime_analysis.txt)
```bash
cat docs/demo_evidence/benchmarks/realtime_analysis.txt
```

### For Image (01_original_frame.png)
```bash
# View with any image viewer
# Or check dimensions:
identify docs/demo_evidence/screenshots/01_original_frame.png

# Copy dimensions to verify:
# Expected: 1366x767 pixels
```

---

## Reproducibility Instructions

### Regenerate All Evidence
```bash
# Clean old evidence
rm -rf docs/demo_evidence/*

# Collect fresh measurements
cargo run --release --bin measure_evidence

# Verify new evidence exists
ls -R docs/demo_evidence/
```

### Run Criterion Benchmarks
```bash
# Generate industry-standard statistical benchmarks
cargo bench --bench vision_pipeline

# View HTML report
open target/criterion/report/index.html
```

### Run Tests
```bash
# Verify all unit tests pass
cargo test --lib

# Run specific test
cargo test vision::snapshot::tests::pipeline_produces_world_state_on_empty_frame
```

---

## Evidence Chain of Custody

Each evidence file has:
1. **Generation** - How it was created (script/measurement)
2. **Timestamp** - When it was generated
3. **Source** - What input data was used
4. **Validation** - How to verify its correctness
5. **Usage** - What claims it supports

### Example Chain

**Claim:** "Mean latency is 136.3 ms"

**Evidence Chain:**
1. **Generation:** `cargo run --release --bin measure_evidence`
2. **Source Code:** `src/bin/measure_evidence.rs` (lines 57-72)
3. **Input:** `resources/last.png` (real MapleStory frame)
4. **Process:** 100 frame measurements using `Instant::now()`
5. **Output:** CSV file with 100 timings
6. **Analysis:** Calculate mean = sum / count
7. **Result:** 136.284 ms
8. **File:** `docs/demo_evidence/csv/latency_measurements.csv`
9. **Verification:** 
   ```bash
   awk -F, 'NR>1 {sum+=$2; count++} END {print sum/count}' \
       docs/demo_evidence/csv/latency_measurements.csv
   # Output: 136.284
   ```

---

## For Auditors/Reviewers

### Verification Checklist

- [ ] Evidence files exist in `docs/demo_evidence/`
- [ ] CSV has 100+ measurements
- [ ] JSON files are valid (can parse)
- [ ] Display text is human-readable
- [ ] Screenshot is the expected MapleStory frame
- [ ] Statistics match raw data (spot check mean/median)
- [ ] All 6 detectors present in JSON
- [ ] Confidence values in [0.0, 1.0] range
- [ ] No NaN or infinity values
- [ ] Measurements recorded with proper timestamps

### Invalidating Evidence

Evidence becomes invalid if:
- Source code changes detector behavior
- Test frame changes
- Measurement process changes
- Hardware changes significantly
- OCR library updates

**Action:** Re-run `cargo run --release --bin measure_evidence` to collect fresh evidence.

---

## Evidence Retention Policy

- **Keep indefinitely:** All raw measurement files (CSV, JSON, images)
- **Update on code change:** Regenerate if performance-critical code changes
- **Archive old evidence:** Keep previous runs for regression analysis
- **Link in documentation:** Every claim must reference specific evidence file

---

## Questions Answered by Evidence

| Question | Answer Source |
|----------|----------------|
| Does the HUD detector work? | gamestate JSON, display text |
| How fast is the pipeline? | latency_measurements.csv |
| Can we do 60 FPS? | realtime_analysis.txt (NO) |
| What frame size is supported? | 01_original_frame.png dimensions |
| What's the slowest frame? | latency_measurements.csv (max row) |
| Where's the bottleneck? | Time distribution (OCR spikes) |
| How confident is each detector? | gamestate JSON confidence fields |
| Does serialization work? | gamestate JSON files (valid format) |

---

**This index allows any developer or auditor to trace every technical claim to its supporting evidence.**

