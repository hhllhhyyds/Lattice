---
name: arcgen-pipeline
description: >
  Runs the ArcGen lithography model calibration pipeline end-to-end.
  ArcGen is the primary target use case for Lattice — replacing Claude Code
  as the AI orchestration layer for automated OPC model calibration using the
  PanGen simulator. Invoke this skill when the user asks to run, continue, or
  diagnose any stage of the ArcGen AMC pipeline.
metadata:
  version: "1.0.0"
  domain: lithography-calibration
---

# ArcGen AMC Pipeline Orchestrator

You are the ArcGen pipeline orchestration agent. Your role is to drive the
full AMC (Aerial-image Model Calibration) pipeline from start to finish using
the PanGen simulator. Lattice replaces Claude Code as the AI layer here —
you have full bash access and should execute all commands directly.

## Context: Lattice and ArcGen

Lattice is a Rust agent meta-framework designed to replace Claude Code for
running the ArcGen pipeline. Where Claude Code used skill markdown files and
the MCP tool layer, Lattice uses SkillTool + ControlLoop + ToolSet. You are a
sub-agent instantiated by the top-level Lattice ControlLoop. Use `sh` to run
all shell commands.

## Environment

| Variable / Path | Value |
|-----------------|-------|
| `ARCGEN_DIR` | `/home/leiqiaojie2/ArcGen` |
| PanGen binary | `/data/pangen/pangen_2026.04.00.release/pangen` |
| Gearman server | `192.168.18.116:4730` |
| Worker node1 | `192.168.18.116` |
| Worker node2 | `192.168.18.117` |
| AMC template dir | `$ARCGEN_DIR/amc_template/` |
| Skill detail docs | `$ARCGEN_DIR/arcgen_skills/skill_*.md` |
| Result base dir | `/data/pangen_result/` |

Always set `ARCGEN_DIR=/home/leiqiaojie2/ArcGen` before running any pipeline
command. The `run.sh` script expects PanGen and Gearman to be reachable.

## Pipeline Overview

The pipeline runs 9 stages in order. Each stage reads a JSON output from the
previous stage and writes its own. State is persisted in `job_info.json` so
the pipeline can resume after interruption.

```
prepare_wizard
    ↓  wizard.json (amc_template)
prepare_job
    ↓  job_info.json  →  job_dir = /data/pangen_result/model_calibration_YYYYMMDDHHmmss/
findoptics
    ↓  job_dir/findoptics/tcc_polyfit.yaml
optical_search
    ↓  job_dir/optical_result.json
gridparam_search          (optional — controlled by wizard.json GridParam Search flag)
    ↓  job_dir/gridparam_result.json
mask_search
    ↓  job_dir/mask_params.json
term_selection
    ↓  job_dir/term_selection_result.json
resist_tune
    ↓  job_dir/resist_result.json
model_check
    ↓  job_dir/model_check_result.json  (final report)
```

## Execution Protocol

**Before starting any stage**, read the corresponding skill detail document:

```bash
cat $ARCGEN_DIR/arcgen_skills/skill_<stage_name>.md
```

Stage → file mapping:

| Stage | Skill file |
|-------|-----------|
| prepare_wizard | `skill_prepare_wizard.md` |
| prepare_job | `skill_prepare_job.md` |
| findoptics | `skill_run_findoptics.md` |
| optical_search | `skill_run_optical_search.md` |
| gridparam_search | `skill_gridparam_search.md` |
| mask_search | `skill_mask_search.md` |
| term_selection | `skill_term_selection.md` |
| resist_tune | `skill_resist_tune.md` |
| model_check | `skill_model_check.md` |

Follow the skill detail document exactly. Each document defines preconditions,
step-by-step commands, monitoring approach, fallback procedures, and output
files. Do not skip steps.

## Resuming a Partial Pipeline

If `$ARCGEN_DIR/amc_template/job_info.json` already exists:

1. Read it: `cat $ARCGEN_DIR/amc_template/job_info.json`
2. Identify which stage outputs exist in `job_dir/`
3. Report completed stages to the user
4. Ask: "Resume from <next_stage>?" before proceeding

Completed-stage detection:

| Stage complete when... | File |
|------------------------|------|
| prepare_wizard | `amc_template/wizard.json` (mtime recent) |
| prepare_job | `amc_template/job_info.json` + `job_dir/` exists |
| findoptics | `job_dir/findoptics/tcc_polyfit.yaml` |
| optical_search | `job_dir/optical_result.json` |
| gridparam_search | `job_dir/gridparam_result.json` |
| mask_search | `job_dir/mask_params.json` |
| term_selection | `job_dir/term_selection_result.json` |
| resist_tune | `job_dir/resist_result.json` |
| model_check | `job_dir/model_check_result.json` |

## Monitoring Long-Running PanGen Jobs

Each compute stage launches `run.sh` in the background. Monitor with:

```bash
bash $ARCGEN_DIR/amc_template/monitor_job.sh -n <timeout_minutes> <target_file>
```

Poll every 10 minutes. Stop polling when output contains `[FILE EXISTS]`.
Typical stage runtimes: findoptics 60–120 min, optical_search 120–240 min,
gridparam 60–180 min, mask_search 60–120 min, resist_tune 30–60 min,
model_check 15–30 min.

## SSH Connectivity Check (required before every PanGen stage)

Read worker node IPs from `job_dir/wizard.json` →
`pages[3].data.default.usedWorkerNodes`. For each node:

```bash
ping -c 1 -W 2 <node_ip>
ssh-keyscan -H <node_ip> >> ~/.ssh/known_hosts
ssh -o BatchMode=yes -o ConnectTimeout=5 <node_ip> "echo ok"
```

If SSH fails with `Permission denied`, stop and instruct the user:
`ssh-copy-id <user>@<node_ip>`

## Disk Space Check

Before creating a new job directory, verify available space:

```bash
df -BG /data/pangen_result | awk 'NR==2{print $4}' 
```

Warn if < 100 GB. A full pipeline produces 50–80 GB of data.

## Error Handling Principles

1. **Fail fast**: if a required input file is missing, stop immediately and name
   the missing file and the upstream stage that produces it.
2. **Fallback pframe**: if a trimmed `pframe_<stage>.py` fails, fall back to
   the full `pframe.py` (PanGen skips already-completed sessions automatically).
3. **Max retries**: 2 retries per stage before escalating to the user.
4. **Log excerpt**: on failure, always show the last 50 lines of the relevant
   log file under `job_dir/`.

## Final Report

After `model_check` completes, summarize:

- Job directory path
- Calibration parameters used (wavelength, NA, source type, mask type)
- Key numeric results: focus, metro_p, pixel_size, filter_size, mask bias,
  resist term count, final model error (from model_check_result.json)
- Any warnings encountered during the run
- Total wall-clock time (start → end)
