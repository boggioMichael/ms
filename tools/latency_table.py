"""The voice turns' timing, as a table: the `[latency]` lines MapleSyrup
writes in a session's log.txt while its timing is on (`--latency`, or
MAPLESYRUP_LATENCY=1), each stage's milliseconds by mode (claude, model,
instant, live) and by where the game was (front: open in front; covered:
behind another window, its screen still read; behind: not read; closed;
unavailable): average, smallest and largest, and how many turns.

    python3 tools/latency_table.py "C:\\Users\\me\\Documents\\MapleSyrup sessions\\2026-10-31 20-00-00"
    python3 tools/latency_table.py log.txt other-session/log.txt --csv turns.csv

A folder is searched for its log.txt files. `--csv FILE` also writes every
turn's stages, one row each, for a spreadsheet. Standard library only."""
import argparse
import csv
import os
import re
import statistics
import sys

KIND = "[latency] "
# A turn's line: key=value pairs (see src/util/latency.rs).
PAIR = re.compile(r"(\w+)=(\S+)")
# Where the player first heard the answer: the PC's speakers, the phone
# playing its clip, the live call's first sound.
SOUND = ("pc_play", "played", "live_sound")
# The stages' stretches worth a row, by mode: a label, from, to (a stage,
# or SOUND for the first sound wherever it was heard).
PHONE = [
    ("phone: voice ended -> recogniser's last words", "voice", "spoke"),
    ("phone: last words -> recogniser closed the sentence", "spoke", "final"),
    ("phone: last words -> sent (the settle wait)", "spoke", "sent"),
    ("phone -> PC", "sent", "arrived"),
    ("PC: arrived -> main loop took it", "arrived", "heard"),
]
VOICE = [
    ("voice: asked -> first audio", "tts_sent", "tts_first"),
    ("voice: asked -> first line all made", "tts_sent", "tts_done"),
    ("PC: first audio -> heard on the speakers", "tts_first", "pc_play"),
    ("phone: line made -> clip handed", "tts_done", "clip"),
    ("phone: clip handed -> playing", "clip", "played"),
]
TOTALS = [
    ("TOTAL voice ended -> first sound", "voice", SOUND),
    ("TOTAL last words -> first sound", "spoke", SOUND),
    ("TOTAL PC took it -> first sound", "heard", SOUND),
]
ROWS = {
    "claude": PHONE + [
        ("to Claude: main loop -> bridge", "heard", "bridge"),
        ("Claude: bridge -> say (thinking, tools, search)", "bridge", "say"),
        ("say -> handed to the voice", "say", "queued"),
        ("waiting for the voice's lane", "queued", "lane"),
        ("lane -> voice asked", "lane", "tts_sent"),
    ] + VOICE + TOTALS,
    "model": PHONE + [
        ("main loop -> job", "heard", "queued"),
        ("waiting for the thinking lane", "queued", "lane"),
        ("lane -> request sent (the pictures)", "lane", "request"),
        ("model: request -> first token", "request", "first_token"),
        ("model: request -> web search began", "request", "search"),
        ("model: first token -> first sentence", "first_token", "first_sentence"),
        ("model: request -> last words", "request", "llm_done"),
        ("first sentence -> voice asked", "first_sentence", "tts_sent"),
    ] + VOICE + TOTALS,
    "instant": PHONE + [
        ("main loop -> job", "heard", "queued"),
        ("waiting for the lane", "queued", "lane"),
        ("lane -> voice asked", "lane", "tts_sent"),
    ] + VOICE + TOTALS,
    "live": [
        ("live: player talking (start -> stop)", "live_start", "live_stop"),
        ("live: voice ended -> OpenAI heard him stop", "voice", "live_stop"),
        ("live: stop -> response began", "live_stop", "live_response"),
        ("live: response -> first sound", "live_response", "live_sound"),
        ("TOTAL voice ended -> first sound", "voice", "live_sound"),
        ("TOTAL OpenAI heard him stop -> first sound", "live_stop", "live_sound"),
    ],
}
GAMES = ("front", "covered", "behind", "closed", "unavailable")


def turns_in(path):
    """The turns in one log.txt: a dict of each line's pairs."""
    with open(path, encoding="utf-8", errors="replace") as log:
        for line in log:
            at = line.find(KIND)
            if at < 0 or "turn=" not in line:
                continue
            yield dict(PAIR.findall(line[at + len(KIND):]))


def logs_in(paths):
    for path in paths:
        if os.path.isdir(path):
            for folder, _, files in os.walk(path):
                if "log.txt" in files:
                    yield os.path.join(folder, "log.txt")
        else:
            yield path


def ms(turn, stage):
    if stage == SOUND:
        heard = [int(turn[s]) for s in SOUND if s in turn]
        return min(heard) if heard else None
    return int(turn[stage]) if stage in turn else None


def spans_of(turn):
    """Each stretch on the line (`name@start+length`): its name and length."""
    for span in filter(None, turn.get("spans", "").split(",")):
        name, _, rest = span.partition("@")
        _, _, length = rest.partition("+")
        if length.isdigit():
            yield name, int(length)


def cell(values):
    if not values:
        return ""
    return f"{statistics.mean(values):.0f} ({min(values)}..{max(values)}) n={len(values)}"


def table(mode, turns, rows_of):
    games = [g for g in GAMES if any(t.get("game") == g for t in turns)]
    games += sorted({t.get("game", "?") for t in turns} - set(games))
    counts = ", ".join(f"{g} {sum(t.get('game') == g for t in turns)}" for g in games)
    out = [f"## {mode}: {len(turns)} turns ({counts})", ""]
    out.append("| stage, ms: avg (min..max) n | " + " | ".join(games) + " |")
    out.append("|---|" + "---|" * len(games))
    rows = []
    stretches = ROWS.get(rows_of, PHONE + VOICE + TOTALS)
    if rows_of != mode:
        # (Answered by MapleSyrup's own brain: its rows too.)
        stretches = stretches + [r for r in ROWS["model"] if r not in stretches]
    for label, start, end in stretches:
        cells = []
        for game in games:
            values = []
            for turn in (t for t in turns if t.get("game") == game):
                a, b = ms(turn, start), ms(turn, end)
                if a is not None and b is not None:
                    values.append(b - a)
            cells.append(cell(values))
        if any(cells):
            rows.append(f"| {label} | " + " | ".join(cells) + " |")
    names = sorted({name for t in turns for name, _ in spans_of(t)})
    for name in names:
        cells = [
            cell([length for t in turns if t.get("game") == game for n, length in spans_of(t) if n == name])
            for game in games
        ]
        rows.append(f"| span: {name} | " + " | ".join(cells) + " |")
    ends = {}
    for turn in turns:
        ends[turn.get("end", "?")] = ends.get(turn.get("end", "?"), 0) + 1
    out += rows
    out.append("")
    out.append("Ends: " + ", ".join(f"{e} {n}" for e, n in sorted(ends.items())))
    out.append("")
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("paths", nargs="+", help="log.txt files, or session folders")
    parser.add_argument("--csv", help="also write every turn's stages here")
    args = parser.parse_args()
    turns = [t for log in logs_in(args.paths) for t in turns_in(log)]
    if not turns:
        sys.exit("no [latency] lines found (was MapleSyrup started with --latency?)")
    # (A turn of Claude's that MapleSyrup answered itself, Claude taking too
    # long, is a mode of its own: its stages are its own brain's.)
    def mode_of(turn):
        by = turn.get("answered_by")
        return f"{turn.get('mode', '?')} answered by {by}" if by else turn.get("mode", "?")

    for mode in sorted({mode_of(t) for t in turns}):
        rows = mode if mode in ROWS else mode.split(" ")[0]
        print("\n".join(table(mode, [t for t in turns if mode_of(t) == mode], rows)))
    if args.csv:
        fields = []
        for turn in turns:
            fields += [k for k in turn if k not in fields]
        with open(args.csv, "w", newline="", encoding="utf-8") as out:
            writer = csv.DictWriter(out, fieldnames=fields)
            writer.writeheader()
            writer.writerows(turns)


if __name__ == "__main__":
    main()
