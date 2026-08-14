"""Shared helpers for deterministic sound review packages."""

from __future__ import annotations

import ctypes
import hashlib
import json
import math
import os
import subprocess
import threading
import time
from pathlib import Path

from measure_wav import read_float_wav

ROOT = Path(__file__).resolve().parents[2]
SAMPLE_RATE = 48_000
BASE_BLOCK_SIZE = 257
BLOCK_SIZES = (32, 64, 257, 1024)
EVENT_DURATION_FRAMES = 16_384


def midi_note_frequency(note: int) -> float:
    """Return the equal-tempered frequency represented by one MIDI note."""

    if not 0 <= note <= 127:
        raise ValueError(f"MIDI note is outside the 0-127 range: {note}")
    return 440.0 * math.pow(2.0, (note - 69) / 12.0)


def cli_command(release: bool = False) -> list[str]:
    if release:
        candidates = (
            ROOT / "target" / "release" / "sonalloy.exe",
            ROOT / "target" / "release" / "sonalloy",
        )
    else:
        candidates = (
            ROOT / "target" / "debug" / "sonalloy.exe",
            ROOT / "target" / "debug" / "sonalloy",
            ROOT / "target" / "release" / "sonalloy.exe",
            ROOT / "target" / "release" / "sonalloy",
        )
    for candidate in candidates:
        if candidate.exists():
            return [str(candidate)]
    command = ["cargo", "run", "--quiet"]
    if release:
        command.append("--release")
    command.extend(("-p", "sonalloy-cli", "--"))
    return command


def build_cli(release: bool = False) -> None:
    """Build the CLI binary used by a review measurement."""

    command = ["cargo", "build"]
    if release:
        command.append("--release")
    command.extend(("-p", "sonalloy-cli"))
    result = subprocess.run(
        command,
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    if result.returncode != 0:
        details = "\n".join(
            part for part in (result.stdout, result.stderr) if part
        ).strip()
        raise RuntimeError(f"CLI build failed with exit code {result.returncode}: {details}")


def run_cli(arguments: list[str]) -> str:
    result = subprocess.run(
        cli_command() + arguments,
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    if result.returncode != 0:
        details = "\n".join(
            part for part in (result.stdout, result.stderr) if part
        ).strip()
        raise RuntimeError(f"CLI failed with exit code {result.returncode}: {details}")
    return result.stdout


def write_utf8(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(content.encode("utf-8"))


def write_definition(path: Path, value: dict[str, object]) -> None:
    write_utf8(path, json.dumps(value, ensure_ascii=False, indent=2) + "\n")


def write_events(path: Path, events: list[dict[str, object]]) -> None:
    write_utf8(path, json.dumps({"events": events}, ensure_ascii=False, indent=2) + "\n")


def render_note(
    definition: Path,
    note: int,
    output: Path,
    block_size: int = BASE_BLOCK_SIZE,
    sample_rate: int = SAMPLE_RATE,
    gate_seconds: float = 0.15,
    tail_seconds: float = 0.1,
) -> None:
    run_cli(
        [
            "render",
            "note",
            str(definition),
            "--note",
            str(note),
            "--velocity",
            "112",
            "--gate",
            str(gate_seconds),
            "--tail",
            str(tail_seconds),
            "--sample-rate",
            str(sample_rate),
            "--block-size",
            str(block_size),
            "--output",
            str(output),
            "--json",
        ]
    )


def render_events(
    definition: Path,
    events: Path,
    output: Path,
    block_size: int,
    duration_frames: int = EVENT_DURATION_FRAMES,
    tail_seconds: float = 0.0,
) -> None:
    run_cli(
        [
            "render",
            "events",
            str(definition),
            str(events),
            "--duration-frames",
            str(duration_frames),
            "--sample-rate",
            str(SAMPLE_RATE),
            "--block-size",
            str(block_size),
            "--tail",
            str(tail_seconds),
            "--output",
            str(output),
            "--json",
        ]
    )


def render_midi(
    definition: Path,
    midi: Path,
    output: Path,
    block_size: int = BASE_BLOCK_SIZE,
    sample_rate: int = SAMPLE_RATE,
    tail_seconds: float = 1.0,
) -> None:
    run_cli(
        [
            "render",
            "midi",
            str(definition),
            str(midi),
            "--sample-rate",
            str(sample_rate),
            "--block-size",
            str(block_size),
            "--tail",
            str(tail_seconds),
            "--output",
            str(output),
            "--json",
        ]
    )


def _process_working_set_bytes(process: subprocess.Popen[str]) -> int | None:
    if os.name != "nt":
        return None

    class ProcessMemoryCounters(ctypes.Structure):
        _fields_ = [
            ("cb", ctypes.c_ulong),
            ("page_fault_count", ctypes.c_ulong),
            ("peak_working_set_size", ctypes.c_size_t),
            ("working_set_size", ctypes.c_size_t),
            ("quota_peak_paged_pool_usage", ctypes.c_size_t),
            ("quota_paged_pool_usage", ctypes.c_size_t),
            ("quota_peak_non_paged_pool_usage", ctypes.c_size_t),
            ("quota_non_paged_pool_usage", ctypes.c_size_t),
            ("pagefile_usage", ctypes.c_size_t),
            ("peak_pagefile_usage", ctypes.c_size_t),
        ]

    PROCESS_QUERY_INFORMATION = 0x0400
    PROCESS_VM_READ = 0x0010
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    psapi = ctypes.WinDLL("psapi", use_last_error=True)
    open_process = kernel32.OpenProcess
    open_process.argtypes = [ctypes.c_ulong, ctypes.c_int, ctypes.c_ulong]
    open_process.restype = ctypes.c_void_p
    get_memory = psapi.GetProcessMemoryInfo
    get_memory.argtypes = [
        ctypes.c_void_p,
        ctypes.POINTER(ProcessMemoryCounters),
        ctypes.c_ulong,
    ]
    get_memory.restype = ctypes.c_int
    close_handle = kernel32.CloseHandle
    close_handle.argtypes = [ctypes.c_void_p]
    handle = open_process(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, False, process.pid)
    if not handle:
        return None
    peak = 0
    try:
        while process.poll() is None:
            counters = ProcessMemoryCounters()
            counters.cb = ctypes.sizeof(counters)
            if get_memory(handle, ctypes.byref(counters), counters.cb):
                peak = max(peak, counters.peak_working_set_size)
            time.sleep(0.005)
        counters = ProcessMemoryCounters()
        counters.cb = ctypes.sizeof(counters)
        if get_memory(handle, ctypes.byref(counters), counters.cb):
            peak = max(peak, counters.peak_working_set_size)
    finally:
        close_handle(handle)
    return peak or None


def timed_render(
    definition: Path,
    events: Path,
    output: Path,
    duration_frames: int,
    block_size: int = BASE_BLOCK_SIZE,
    sample_rate: int = SAMPLE_RATE,
    release: bool = False,
) -> dict[str, object]:
    """Render an event sequence and record wall time and Windows peak working set."""

    command = cli_command(release=release) + [
        "render",
        "events",
        str(definition),
        str(events),
        "--duration-frames",
        str(duration_frames),
        "--tail",
        "0",
        "--sample-rate",
        str(sample_rate),
        "--block-size",
        str(block_size),
        "--output",
        str(output),
        "--json",
    ]
    started = time.perf_counter()
    process = subprocess.Popen(
        command,
        cwd=ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    captured: list[tuple[str, str]] = []

    def collect_output() -> None:
        stdout, stderr = process.communicate()
        captured.append((stdout, stderr))

    collector = threading.Thread(target=collect_output)
    collector.start()
    peak_working_set = _process_working_set_bytes(process)
    collector.join()
    stdout, stderr = captured[0]
    if process.returncode != 0:
        details = "\n".join(part for part in (stdout, stderr) if part).strip()
        raise RuntimeError(
            f"timed render failed with exit code {process.returncode}: {details}"
        )
    _, channels, samples = read_float_wav(output)
    result: dict[str, object] = {
        "elapsed_seconds": time.perf_counter() - started,
        "frames": len(samples) // channels,
        "channels": channels,
        "duration_frames": duration_frames,
        "sample_rate": sample_rate,
        "block_size": block_size,
        "build": "release" if release else "default",
    }
    result["audio_duration_seconds"] = result["frames"] / sample_rate
    result["realtime_ratio"] = (
        result["elapsed_seconds"] / result["audio_duration_seconds"]
    )
    result["cli_realtime_factor"] = result["frames"] / (
        result["elapsed_seconds"] * sample_rate
    )
    if peak_working_set is not None:
        result["peak_working_set_bytes"] = peak_working_set
    return result


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(65_536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def measure_stereo(path: Path) -> dict[str, object]:
    sample_rate, channels, samples = read_float_wav(path)
    if channels != 2:
        raise ValueError(f"expected stereo WAV: {path}")
    left = samples[0::2]
    right = samples[1::2]
    left_mean = sum(left) / len(left) if left else 0.0
    right_mean = sum(right) / len(right) if right else 0.0
    covariance = sum(
        (left_sample - left_mean) * (right_sample - right_mean)
        for left_sample, right_sample in zip(left, right)
    )
    left_variance = sum((sample - left_mean) ** 2 for sample in left)
    right_variance = sum((sample - right_mean) ** 2 for sample in right)
    denominator = math.sqrt(left_variance * right_variance)
    correlation = covariance / denominator if denominator > 0.0 else 1.0
    difference_rms = (
        math.sqrt(
            sum(
                (left_sample - right_sample) ** 2
                for left_sample, right_sample in zip(left, right)
            )
            / len(left)
        )
        if left
        else 0.0
    )
    return {
        "sample_rate": sample_rate,
        "stereo_rms_difference": difference_rms,
        "stereo_correlation": correlation,
    }
