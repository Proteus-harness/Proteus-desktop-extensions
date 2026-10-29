#!/usr/bin/env python3
"""Export nine existing renderer measurements, without running measured programs.

Only report.json and MEASUREMENTS.md are published. Raw evidence and machine
paths remain local. Existing output is never overwritten. Run after bench.py:
    python3 -B experiments/native-renderers/summarize.py
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import statistics
import sys
import tomllib

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
KINDS = ("pele", "rust-mermaid", "rust-latex")
PACKAGES = dict(pele="mermaid-render", **{"rust-mermaid": "mermaid-rust-render", "rust-latex": "latex-render"})
PHASES = ("two-view-idle", "one-view-idle", "closed-view-idle")
MEMORY = ("pss_kib", "rss_kib", "pss_swap_kib", "swap_pss_kib")
THEME_KEYS = {"foreground", "background", "muted", "border", "accent", "danger",
              "surface", "surface_alt", "font_family", "font_family_mono", "radius", "radius_lg"}


def require(condition, reason):
    if not condition:
        raise ValueError(reason)


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def read(path, limit=16 * 1024 * 1024):
    """Reject partial/changing evidence; hash the exact bytes being parsed."""
    with Path(path).open("rb") as stream:
        before = os.fstat(stream.fileno())
        require(before.st_size <= limit, f"oversized input: {Path(path).name}")
        raw = stream.read(limit + 1)
        after = os.fstat(stream.fileno())
    require(len(raw) <= limit, "input grew beyond its budget")
    identity = lambda s: (s.st_dev, s.st_ino, s.st_size, s.st_mtime_ns, s.st_ctime_ns)
    require(identity(before) == identity(after) and len(raw) == before.st_size,
            f"input changed while reading: {Path(path).name}")
    return raw


def strict(raw):
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, "duplicate JSON key")
            result[key] = value
        return result
    def invalid(_):
        raise ValueError("nonfinite JSON number")
    return json.loads(raw, object_pairs_hook=pairs, parse_constant=invalid)


def number(value):
    require(isinstance(value, (int, float)) and not isinstance(value, bool)
            and math.isfinite(value) and value >= 0, "invalid measurement number")
    return value


def distribution(values):
    values = [number(value) for value in values]
    require(bool(values), "empty measurement distribution")
    return dict(median=statistics.median(values), minimum=min(values), maximum=max(values), n=len(values))


def jsonl(raw):
    require(raw.endswith(b"\n"), "incomplete JSONL evidence")
    return [strict(line) for line in raw.splitlines()]


def check_memory(sample):
    require(not sample["memory_incomplete_pids"], "incomplete process memory sample")
    rows = sample["processes"]
    require(sample["process_count"] == len(rows), "sample process count differs")
    for key, fields in (("rss_kib", ("Rss",)), ("pss_kib", ("Pss",)),
                        ("swap_pss_kib", ("SwapPss",)), ("pss_swap_kib", ("Pss", "SwapPss"))):
        total = sum(number(row[field]) for row in rows for field in fields)
        require(sample[key] == total, "sample memory total differs from its processes")


def protocol_parameters(records, expected_language):
    sent = [row["message"] for row in records if row["direction"] == "send"]
    hello = [message for message in sent if message["type"] == "hello"]
    require(len(hello) == 1 and hello[0]["protocol"] == 4, "unexpected protocol handshake")
    require(not any(message["type"] == "settings" for message in sent),
            "settings messages require an explicit exporter schema update")
    renders = [message for message in sent if message["type"] == "render"]
    require(len(renders) == 6, "expected first, three cached and two second-session requests")
    expected = [("left", "first", n) for n in range(1, 5)] + [("right", "second", 1), ("right", "second", 2)]
    themes = []
    for message, address in zip(renders, expected):
        content = message["content"]
        require((message["context"], message["view"], content["source_revision"]) == address,
                "unexpected render sequence")
        require(message["surface"] == "content" and content["language"] == expected_language,
                "unexpected render surface/language")
        require(set(content["context"]) == {"theme"}, "unreviewed content context parameters")
        theme = content["context"]["theme"]
        require(set(theme) == THEME_KEYS, "theme fields require an explicit exporter schema update")
        require(all(isinstance(v, str) if k not in {"radius", "radius_lg"}
                    else isinstance(v, (int, float)) and not isinstance(v, bool) and math.isfinite(v)
                    for k, v in theme.items()), "invalid theme value")
        themes.append({key: theme[key] for key in sorted(THEME_KEYS)})
    require(all(theme == themes[0] for theme in themes), "theme changes between render requests")
    require([m["view"] for m in sent if m["type"] == "close"] == ["first", "second"],
            "expected both view close requests")
    return dict(protocol=4, theme=themes[0], renderer_settings="package defaults; no settings messages sent")


def collect_case(root, name, expected_source, current_hashes):
    case = root / name
    paths = {key: case / filename for key, filename in
             (("report", "report.json"), ("samples", "samples.jsonl"),
              ("protocol", "protocol.jsonl"), ("source", "source.txt"))}
    raw = {key: read(path) for key, path in paths.items()}
    data = strict(raw["report"])
    kind = name.rsplit("-", 1)[0]
    require(data["ok"] is True and data["package"] == PACKAGES[kind], "failed/wrong package report")
    require(data["source_sha256"] == digest(raw["source"]) == digest(expected_source),
            "source bytes do not match the fixed synthetic fixture")
    for key in ("benchmark_sha256", "sampler_sha256"):
        require(data[key] == current_hashes[key], f"stale {key} in {name}")
    executable = Path(data["command"][0]).resolve(strict=True)
    require(data["binary_sha256"] == digest(read(executable, 128 * 1024 * 1024)),
            f"measured binary is not the current executable: {name}")
    manifest_raw = read(executable.parent.parent / "extension.json")
    manifest = strict(manifest_raw)
    require(data["manifest_sha256"] == digest(manifest_raw) and manifest["id"] == data["package"],
            f"package manifest changed: {name}")
    command = manifest["command"]
    require(data["command"] == [str((executable.parent.parent / command[0]).resolve()), *command[1:]],
            "command does not match the measured package manifest")
    language = "latex" if kind == "rust-latex" else "mermaid"
    require(data["language"] == language, "report language differs")
    params = protocol_parameters(jsonl(raw["protocol"]), language)
    events = jsonl(raw["samples"])
    samples = [event for event in events if event.get("type") == "sample"]
    require(bool(samples), "missing memory samples")
    for sample in samples:
        check_memory(sample)
    summaries = [event for event in events if event.get("type") == "summary"]
    require(len(summaries) == 1 and events[-1] is summaries[0], "missing final sampler summary")
    resources = data["resources"]
    require(all(summaries[0].get(key) == value for key, value in resources.items()),
            "raw sampler summary differs from report resources")
    for key in ("idle", "after_first_close", "after_last_close", "after_eof"):
        snapshot = data[key]
        require(any(snapshot == sample for sample in samples), "report snapshot absent from raw samples")
    require(data["after_last_close"]["process_count"] == 1
            and data["after_eof"]["process_count"] == 0, "last-close/EOF lifecycle differs")
    require(all(data["after_eof"][key] == 0 for key in MEMORY), "nonzero memory after EOF")
    memory = {}
    for phase in PHASES:
        selected = [sample for sample in samples if sample["phase"] == phase]
        require(len(selected) >= 2, "insufficient idle phase samples")
        memory[phase] = {key.replace("_kib", "_mib"): statistics.median(number(s[key]) for s in selected) / 1024
                         for key in MEMORY}
    peaks = {key.replace("_kib", "_mib"): max(number(sample[key]) for sample in samples) / 1024
             for key in ("pss_kib", "pss_swap_kib", "rss_kib")}
    require(all(resources["sampled_simultaneous_peaks_kib"][key] / 1024 == value
                for key, value in ((k.replace("_mib", "_kib"), v) for k, v in peaks.items())),
            "reported sampled peaks differ from raw samples")
    require(len(data["cached"]) == 3, "expected three cached requests per process")
    svg_sha = data["first"]["svg_sha256"]
    replies = [("first-1.svg", data["first"])]
    replies += [(f"first-{i + 2}.svg", reply) for i, reply in enumerate(data["cached"])]
    replies += [("second-1.svg", data["second_session"])]
    for filename, reply in replies:
        require(reply["svg_sha256"] == svg_sha == digest(read(case / filename, 2 * 1024 * 1024)),
                "SVG digest or cached equality differs")
    require(digest(read(case / "second-2.svg", 2 * 1024 * 1024)) == svg_sha,
            "remaining view changed after closing the first view")
    js = data["javascript_processes"]
    require(len(js) == (1 if kind == "pele" else 0), "unexpected JavaScript process count")
    environments = data["environment"]
    require(set(environments) == {"HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME"}, "missing isolation metadata")
    require(all(isinstance(value, str) and Path(value).is_absolute() for value in environments.values()),
            "invalid profile path metadata")
    require(len({str(Path(value).parent) for value in environments.values()}) == 1
            and all(Path(value).parent.name == name for value in environments.values()), "profile does not belong to its case")
    attribution = [{"executable": Path(row["exe"]).name, "pss_mib": number(row["Pss"]) / 1024,
                    "rss_mib": number(row["Rss"]) / 1024} for row in data["idle"]["processes"]]
    pid_names = {row["pid"]: Path(row["exe"]).name for sample in samples for row in sample["processes"]}
    hwm = [{"executable": pid_names[row["pid"]], "vm_hwm_kib": number(row["VmHWM"])}
           for row in resources["per_process_vm_hwm_kib"]]
    result = dict(case=name, package=data["package"], binary_sha256=data["binary_sha256"],
                  manifest_sha256=data["manifest_sha256"], source_sha256=data["source_sha256"],
                  svg_sha256=svg_sha, first_ms=number(data["first"]["render_ms"]),
                  first_since_launch_ms=number(data["first"]["since_launch_ms"]),
                  cached_ms=[number(reply["render_ms"]) for reply in data["cached"]],
                  memory_phase_medians_mib=memory, sampled_peaks_mib=peaks,
                  maximum_sampled_swap_pss_kib=max(sample["swap_pss_kib"] for sample in samples),
                  javascript_executables=sorted(Path(row["exe"]).name for row in js),
                  idle_process_attribution=attribution, per_process_vm_hwm=hwm,
                  after_eof_process_count=0, samples=len(samples),
                  scan_duration_ms=distribution(sample["scan_duration_seconds"] * 1000 for sample in samples),
                  local_evidence_sha256={key: digest(value) for key, value in raw.items()}, ok=True)
    result["cached_process_median_ms"] = statistics.median(result["cached_ms"])
    # Detect a concurrent rerun before publishing an export of mixed evidence.
    require(all(read(paths[key]) == value for key, value in raw.items()), f"case changed during export: {name}")
    return result, params, list(environments.values()), samples[0]["monotonic_ns"]


def portable(value):
    """Validation only, never path redaction: published fields are allowlisted."""
    if isinstance(value, dict):
        for item in value.values():
            portable(item)
    elif isinstance(value, list):
        for item in value:
            portable(item)
    elif isinstance(value, str):
        require(not value.startswith("/") and "/home/" not in value and "/tmp/" not in value,
                "absolute machine path in portable allowlist")


def collect(args):
    hashes = dict(benchmark_sha256=digest(read(HERE / "bench.py")), sampler_sha256=digest(read(args.sampler)))
    source_paths = dict(mermaid=HERE / "packages/mermaid-rust-render/tests/fixtures/flowchart.mmd",
                        latex=HERE / "fixtures/formula.tex")
    sources = {key: read(path, 256 * 1024) for key, path in source_paths.items()}
    packages = tomllib.loads(read(HERE / "Cargo.lock").decode())["package"]
    libraries = {p["name"]: p["version"] for p in packages if p["name"] in {"mermaid-rs-renderer", "kurbo", "latex-rust"}}
    libraries["pele"] = strict(read(ROOT / "packages/mermaid-render/vendor/pele/UPSTREAM.json"))["version"]
    upstream = strict(read(HERE / "packages/mermaid-rust-render/UPSTREAM.json"))
    patch_sha = digest(read(HERE / upstream["patch"]["path"]))
    require(patch_sha == upstream["patch"]["sha256"], "current Mermaid patch differs from its source manifest")
    result = dict(schema_version=1, status="passed", **hashes, exporter_sha256=digest(read(__file__)),
                  library_versions=libraries, mermaid_patch_sha256=patch_sha,
                  scope="Ready renderer process tree only; Python observer, Cargo, Core, GUI, model, SVG rasterization and GPU excluded.",
                  statistical_unit="Three fresh processes per renderer. Cached: median of three replies within each process, then median across processes. Memory: per-process idle phase median, then median across processes.",
                  profiles="Distinct caller-prepared HOME/XDG_CONFIG_HOME/XDG_CACHE_HOME paths recorded per process; historical emptiness is not independently verified by this exporter.",
                  os_cache="Warm/uncontrolled OS file cache; no cache dropping or CPU isolation.",
                  inputs={key: {"source": value.decode("utf-8"), "sha256": digest(value)} for key, value in sources.items()},
                  local_evidence="Only compact derived values and hashes are published; raw reports, protocol, samples and SVGs remain local.",
                  runs=[], renderers={})
    profiles, order, parameters = [], [], []
    for kind in KINDS:
        group = []
        for index in range(1, 4):
            name = f"{kind}-{index:02d}"
            run, params, paths, started = collect_case(args.input, name, sources["latex" if kind == "rust-latex" else "mermaid"], hashes)
            result["runs"].append(run)
            group.append(run)
            parameters.append(params)
            profiles.extend(paths)
            order.append((started, name))
        require(len({run["binary_sha256"] for run in group}) == 1, "mixed executable revisions within a renderer")
        aggregate = {key: distribution(run[key] for run in group)
                     for key in ("first_ms", "first_since_launch_ms", "cached_process_median_ms")}
        aggregate["memory_mib"] = {phase: {key: distribution(run["memory_phase_medians_mib"][phase][key] for run in group)
                                                    for key in ("pss_mib", "rss_mib", "pss_swap_mib")}
                                   for phase in PHASES}
        aggregate["sampled_peaks_mib"] = {key: distribution(run["sampled_peaks_mib"][key] for run in group)
                                           for key in ("pss_mib", "rss_mib", "pss_swap_mib")}
        aggregate.update(binary_sha256=group[0]["binary_sha256"], source_sha256=group[0]["source_sha256"],
                         maximum_sampled_swap_pss_kib=max(run["maximum_sampled_swap_pss_kib"] for run in group),
                         javascript_process_counts=[len(run["javascript_executables"]) for run in group],
                         after_eof_process_counts=[run["after_eof_process_count"] for run in group])
        result["renderers"][kind] = aggregate
    require(len(set(profiles)) == 27, "reused HOME/XDG directory between process cases")
    require(all(params == parameters[0] for params in parameters), "theme/settings differ between renderers")
    result["protocol_parameters"] = parameters[0]
    result["observed_order"] = [name for _, name in sorted(order)]
    order_file = args.input / "final-run-order.json"
    if order_file.exists():
        order_raw = read(order_file)
        require(strict(order_raw) == result["observed_order"], "declared process order differs from raw samples")
        result["local_order_sha256"] = digest(order_raw)
    result["limits"] = [
        "One three-node synthetic Mermaid flowchart. LaTeX uses a separate Gaussian-integral formula and is not a flowchart comparator.",
        "render_ms ends at receiver-thread arrival, not the full instrumented cycle or a presented frame. Sampler scan overhead is reported separately.",
        "Cached replies are within-process hits, not additional independent process replicates. Three processes do not establish tail percentiles or general speedups.",
        "Sampling begins after Popen; transient initialization peaks and short-lived children can be missed. No post-first-response settled memory measurement.",
        "Sampled peaks are lower bounds. VmHWM is per PID and is never summed as a simultaneous tree peak. VSZ/VmSize was not collected.",
        "Same-source SVG digests match within each renderer; matching source across engines does not establish visual equivalence. Native UI proof is separate.",
        "Resident memory after close does not prove a leak; static data, reusable caches and allocator retention are not separated.",
    ]
    if args.build_report and args.build_report.exists():
        raw = read(args.build_report)
        build = strict(raw)
        require(len(build["runs"]) >= 2 and all(run["exit_code"] == 0 for run in build["runs"][:2]), "failed build observation")
        result["separate_desktop_build"] = dict(local_report_sha256=digest(raw),
            desktop_commit=build["sources"]["desktop"]["head"], components=build["components"], jobs=build["jobs"],
            rebuild_wall_s=number(build["runs"][0]["time"]["wall_seconds"]),
            following_noop_assembly_wall_s=number(build["runs"][1]["time"]["wall_seconds"]),
            no_freshly_compiled_packages_second=not build["runs"][1]["compiled_packages"],
            scope="Separate release desktop-only build/assembly with existing caches; no archive. Not a clean build or before/after compiler speed comparison.")
    portable(result)
    return result


def markdown(report):
    labels = {"pele": "Pele + Node", "rust-mermaid": "Rust Mermaid + patch/kurbo", "rust-latex": "Rust LaTeX, отдельная формула"}
    lines = ["# Процессные рендереры: 29.10.2026", "", "Девять успешных прогонов: три свежих процесса каждого пакета. Источник — только актуальные `*-01..03`; архивные серии не используются.", "",
             "Первый ответ — медиана процессов. Cached — медиана трёх ответов внутри процесса, затем медиана процессов. Память — медиана выборок idle-фазы внутри процесса, затем медиана процессов. MiB = KiB / 1024.", "",
             "| Рендерер | Первый / cached, мс | Два view: PSS / RSS, MiB | После последнего close: PSS / RSS, MiB | Sampled peak PSS / RSS, MiB | Node / EOF процессы |",
             "|---|---:|---:|---:|---:|---:|"]
    for kind, value in report["renderers"].items():
        memory = value["memory_mib"]
        pair = lambda phase: f"{memory[phase]['pss_mib']['median']:.3f} / {memory[phase]['rss_mib']['median']:.3f}"
        peaks = value["sampled_peaks_mib"]
        lines.append(f"| {labels[kind]} | {value['first_ms']['median']:.3f} / {value['cached_process_median_ms']['median']:.3f} | {pair('two-view-idle')} | {pair('closed-view-idle')} | {peaks['pss_mib']['median']:.3f} / {peaks['rss_mib']['median']:.3f} | {value['javascript_process_counts'][0]} / 0 |")
    lines += ["", "Pele и Rust Mermaid получили один и тот же трёхузловой flowchart; LaTeX — отдельную формулу. Память Pele включает Node. После последнего close Node завершается; Rust-процессы остаются живы. После EOF процессов нет.", "",
              "Первый ответ от запуска процесса: " + "; ".join(f"{labels[k]} {v['first_since_launch_ms']['median']:.3f} мс" for k, v in report["renderers"].items()) + ".", "",
              "Библиотеки: " + ", ".join(f"{k} {v}" for k, v in report["library_versions"].items()) + ".", "",
              f"Mermaid patch SHA-256: `{report['mermaid_patch_sha256']}`.", "",
              "## Входы и параметры", "", "```mermaid", report["inputs"]["mermaid"]["source"].rstrip(), "```", "", "```latex", report["inputs"]["latex"]["source"].rstrip(), "```", "",
              "Тема и настройки, полный SHA каждого executable/source/manifest, per-run результаты, VmHWM по executable и SHA локальных evidence: [report.json](report.json). HOME, XDG paths, PID и машинные абсолютные пути не публикуются.", "",
              "## Границы измерения", "",
              "- Только готовое расширение и его наблюдаемые дочерние процессы; Python observer, Cargo, Core, GUI, GPUI/resvg и GPU не входят.",
              "- Записаны отдельные HOME/XDG_CONFIG_HOME/XDG_CACHE_HOME каждого процесса. Директории готовит вызывающий; exporter проверяет различие путей, а не их пустоту перед историческим запуском. Кэш ОС прогрет/неконтролируемый.",
              "- `render_ms` заканчивается timestamp получения ответа reader-потоком. Это не полный цикл и не готовый кадр; cached около 0,2 мс не означает такой throughput instrumented цикла.",
              "- Первая выборка идёт после Popen. Initialization peak и короткие всплески могут быть пропущены. Отдельная settled-память сразу после первого ответа не измерялась.",
              "- Sampled simultaneous peak — нижняя оценка. VmHWM не складывается между PID; VSZ/VmSize не собирался.",
              "- Три процесса не подтверждают p95/p99 и общий speedup сложных диаграмм. Девять cache hits не являются девятью независимыми процессами.",
              "- Сохранённая память после close не доказывает утечку. Равенство SVG внутри повторов проверено; визуальная эквивалентность движков и native proof — отдельная проверка."]
    swap = max(value["maximum_sampled_swap_pss_kib"] for value in report["renderers"].values())
    lines += ["", f"Максимальный наблюдённый SwapPss: {swap} KiB. Порядок запусков: " + ", ".join(report["observed_order"]) + "."]
    if "separate_desktop_build" in report:
        build = report["separate_desktop_build"]
        lines += ["", "## Отдельная сборка Desktop", "", f"Commit `{build['desktop_commit']}`, desktop-only release, jobs={build['jobs']}, без архива: rebuild + assembly {build['rebuild_wall_s']:.2f} с; следующий no-op + assembly {build['following_noop_assembly_wall_s']:.2f} с. Существующие Cargo/OS caches сохранены. Это не clean build и не before/after сравнение скорости компилятора."]
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, default=ROOT / "target/verification/native-renderers")
    parser.add_argument("--output", type=Path, default=HERE / "results/2026-10-29")
    parser.add_argument("--sampler", type=Path, default=ROOT.parent / "Proteus-core/examples/performance/common/proc_sampler.py")
    parser.add_argument("--build-report", type=Path, default=ROOT.parent / "Proteus-desktop/build/verification/native-renderer-unified-build/report.json")
    args = parser.parse_args()
    try:
        report = collect(args)
        text = markdown(report)
        portable(text)
        args.output.mkdir(parents=True, exist_ok=False)
        (args.output / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2, allow_nan=False) + "\n")
        (args.output / "MEASUREMENTS.md").write_text(text)
    except (OSError, ValueError, KeyError, TypeError) as error:
        parser.exit(1, f"renderer export failed: {error}\n")
    print(json.dumps(dict(status="passed", processes=len(report["runs"]), output=str(args.output))))


if __name__ == "__main__":
    main()
