#!/usr/bin/env python3
"""
End-to-end check of the launcher against a real UNBEATEN backend.

Starts the backend from an UNBEATEN checkout on SQLite, ranks the song folders
in --charts, fakes an UNBEATABLE install in a temporary home folder (save file
+ CustomSongs), then drives the `ubl` CLI through sign-in, map installs and
updates, and score syncs, checking every step. Standard library only.

    cargo build -p ubl
    python3 scripts/e2e.py --unbeaten ../UNBEATEN --charts <song folders> \\
        --save <arcade-highscores.json> --ubl target/debug/ubl

The first chart folder is left out of CustomSongs so the launcher installs it;
the others are copied in as the player's existing copies.
"""

import argparse
import io
import json
import os
import re
import secrets
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import urllib.request
import zipfile
from pathlib import Path

STEP = 0


def step(msg):
    global STEP
    STEP += 1
    print(f"\n[{STEP}] {msg}", flush=True)


def check(cond, msg):
    if not cond:
        raise SystemExit(f"FAIL: {msg}")
    print(f"    ok: {msg}")


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class Env:
    def __init__(self, args):
        self.args = args
        self.tmp = Path(tempfile.mkdtemp(prefix="unbeaten-e2e-"))
        self.port = free_port()
        self.server = f"http://127.0.0.1:{self.port}"
        self.home = self.tmp / "home"
        self.data = self.home / "AppData" / "LocalLow" / "D-CELL GAMES" / "UNBEATABLE"
        self.songs = self.data / "CustomSongs"
        self.save = self.data / "arcade-highscores.json"
        self.state = self.tmp / "state"
        self.env = dict(os.environ)
        self.env.update({
            "DATABASE_URL": f"sqlite:///{self.tmp}/dev.db",
            "JWT_SECRET": "e2e-" + secrets.token_urlsafe(40),
            "DISCORD_CLIENT_ID": "0", "DISCORD_CLIENT_SECRET": "e2e",
            "DISCORD_REDIRECT_URI": f"{self.server}/api/auth/discord/callback",
            "FRONTEND_URL": self.server,
            "BEATMAPS_DIR": str(self.tmp / "beatmaps"),
            "SUBMISSIONS_DIR": str(self.tmp / "submissions"),
            "DOWNLOADS_CACHE_DIR": str(self.tmp / "downloads"),
            "COOKIE_SECURE": "false", "LOG_LEVEL": "WARNING",
        })
        self.server_proc = None

    # ── backend ──────────────────────────────────────────────────────────────
    def start_backend(self):
        backend = self.args.unbeaten / "backend"
        self.server_log = open(self.tmp / "server.log", "w")
        self.server_proc = subprocess.Popen(
            [sys.executable, "-m", "uvicorn", "main:app", "--host", "127.0.0.1", "--port", str(self.port)],
            cwd=backend, env=self.env, stdout=self.server_log, stderr=subprocess.STDOUT,
        )
        for _ in range(60):
            try:
                urllib.request.urlopen(f"{self.server}/api/stats", timeout=1)
                return
            except Exception:
                time.sleep(0.5)
        raise SystemExit("backend didn't start; see " + str(self.tmp / "server.log"))

    def seed(self):
        out = subprocess.run(
            [sys.executable, "scripts/dev_seed.py", "--server", self.server, "--charts", str(self.args.charts)],
            cwd=self.args.unbeaten / "backend", env=self.env, check=True, capture_output=True, text=True,
        )
        return json.loads(out.stdout)

    def http(self, method, path, cookie=None, body=None):
        req = urllib.request.Request(self.server + path, method=method)
        if cookie:
            req.add_header("Cookie", f"rr_session={cookie}")
        data = None
        if body is not None:
            data = json.dumps(body).encode()
            req.add_header("Content-Type", "application/json")
        with urllib.request.urlopen(req, data, timeout=60) as r:
            return json.loads(r.read() or b"null")

    def push_version(self, admin_cookie, beatmap_id, zip_bytes):
        path = self.tmp / "version.zip"
        path.write_bytes(zip_bytes)
        out = subprocess.run(
            ["curl", "-sf", "-b", f"rr_session={admin_cookie}", "-F", f"file=@{path};type=application/zip",
             f"{self.server}/api/admin/beatmaps/{beatmap_id}/new-version"],
            check=True, capture_output=True, text=True,
        )
        return json.loads(out.stdout)

    # ── launcher CLI ─────────────────────────────────────────────────────────
    def ubl_args(self, *args, paths=True):
        base = [str(self.args.ubl), "--server", self.server, "--state-dir", str(self.state),
                "--token-file", str(self.state / "token.json")]
        if paths:
            base += ["--save", str(self.save), "--custom-songs", str(self.songs)]
        return base + list(args)

    def ubl_env(self):
        env = dict(os.environ)
        env["HOME"] = str(self.home)
        env["USERPROFILE"] = str(self.home)
        return env

    def ubl(self, *args, paths=True, ok=True):
        r = subprocess.run(self.ubl_args(*args, paths=paths), env=self.ubl_env(), capture_output=True, text=True)
        if ok and r.returncode != 0:
            raise SystemExit(f"ubl {' '.join(args)} failed:\n{r.stderr}")
        return r

    def ubl_json(self, *args, paths=True):
        return json.loads(self.ubl(*args, paths=paths).stdout)

    def maps(self):
        out = {}
        for line in self.ubl("maps").stdout.splitlines():
            m = re.match(r"\s*(\d+)\s+v(\d+)\s+(\w+)\s+(.*)$", line)
            if m:
                out[int(m.group(1))] = (int(m.group(2)), m.group(3), m.group(4).strip())
        return out

    def sync_state(self):
        return json.loads((self.state / "sync_state.json").read_text())

    def stop(self):
        if self.server_proc:
            self.server_proc.terminate()
            self.server_proc.wait(10)


def load_save(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def write_save(path, data, atomic=True):
    raw = json.dumps(data)
    if atomic:
        tmp = path.with_suffix(".tmp")
        tmp.write_text(raw)
        os.replace(tmp, path)
    else:
        path.write_text(raw)


def improve(save_path, song, atomic=True):
    data = load_save(save_path)
    for e in data["highScores"]:
        if e["song"] == song:
            e["score"] += 1000
            e["updateCount"] = e.get("updateCount", 0) + 1
            write_save(save_path, data, atomic)
            return e
    raise SystemExit(f"no save entry {song}")


def first_tier_entry(save_path, folder):
    for e in load_save(save_path)["highScores"]:
        if e["song"].startswith(f"CUSTOM_{folder}/") and e.get("cleared", True):
            return e["song"]
    return None


def edit_one_note(chart: Path):
    text = chart.read_bytes()
    head, _, body = text.partition(b"[HitObjects]")
    lines = body.split(b"\n")
    for i, line in enumerate(lines):
        parts = line.strip().split(b",")
        if len(parts) >= 3 and parts[2].isdigit():
            parts[2] = str(int(parts[2]) + 7).encode()
            lines[i] = b",".join(parts) + (b"\r" if line.endswith(b"\r") else b"")
            break
    chart.write_bytes(head + b"[HitObjects]" + b"\n".join(lines))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--unbeaten", type=Path, required=True, help="UNBEATEN checkout")
    ap.add_argument("--charts", type=Path, required=True, help="folder of song folders")
    ap.add_argument("--save", type=Path, required=True, help="an arcade-highscores.json")
    ap.add_argument("--ubl", type=Path, required=True, help="the ubl binary")
    ap.add_argument("--keep", action="store_true", help="keep the temporary folder")
    args = ap.parse_args()
    env = Env(args)
    try:
        run(env)
        print(f"\nALL STEPS PASSED ({STEP})")
    finally:
        env.stop()
        if args.keep:
            print("kept", env.tmp)
        else:
            shutil.rmtree(env.tmp, ignore_errors=True)


def run(env):
    folders = sorted(p for p in env.args.charts.iterdir() if p.is_dir())
    to_install = folders[0]

    step("Start the backend and rank the sample maps")
    env.start_backend()
    seed = env.seed()
    ranked = {b["folder"]: b["id"] for b in seed["beatmaps"]}
    check(len(ranked) == len(folders), f"{len(ranked)} maps ranked")
    admin, player = seed["admin"]["cookie"], seed["player"]["cookie"]
    install_id = ranked[to_install.name]

    step("Fake an UNBEATABLE install; the launcher finds it")
    env.songs.mkdir(parents=True)
    shutil.copy(env.args.save, env.save)
    for f in folders[1:]:
        shutil.copytree(f, env.songs / f.name)
    det = env.ubl_json("detect", paths=False)
    check(Path(det["save_file"]) == env.save, "save file detected")
    check(Path(det["custom_songs"]) == env.songs, "CustomSongs detected")

    step("Sign in with a device code approved on the site")
    proc = subprocess.Popen(env.ubl_args("login", "--no-browser"), env=env.ubl_env(),
                            stderr=subprocess.PIPE, text=True)
    code = None
    for line in proc.stderr:
        m = re.search(r"\b([A-Z0-9]{4}-[A-Z0-9]{4})\b", line)
        if m:
            code = m.group(1)
            break
    check(code is not None, f"code shown: {code}")
    info = env.http("GET", f"/api/launcher/link/{code}", cookie=player)
    check(info["status"] == "pending", f"site shows the request from {info['device_name']}")
    env.http("POST", "/api/launcher/link/approve", cookie=player, body={"user_code": code, "approve": True})
    rest = proc.stderr.read()
    check(proc.wait(30) == 0 and "Signed in as dev-player" in rest, "launcher signed in")
    me = env.ubl_json("me")
    check(me["username"] == "dev-player", "profile loads with the launcher token")

    step("Map states, then install a map")
    maps = env.maps()
    check(maps[install_id][1] == "NotInstalled", f"{to_install.name} not installed")
    check(all(maps[ranked[f.name]][1] == "Installed" for f in folders[1:]), "existing copies recognised")
    out = env.ubl_json("install", str(install_id))
    check(Path(out["folder"]).name == to_install.name, "installed under the charter's folder name")
    check(env.maps()[install_id][1] == "Installed", "now installed")
    check(not (env.songs.parent / "UNBEATEN Launcher" / "staging" / "x").exists(), "staging cleaned")

    step("First sync: verified imports, failed runs and the install baseline")
    report = env.ubl_json("sync")
    states = env.sync_state()["entries"]
    by_folder = lambda f: {k: v for k, v in states.items() if k.startswith(f"CUSTOM_{f}/")}
    installed_entries = by_folder(to_install.name)
    if installed_entries:
        check(all(v["status"] in ("baseline", "failed_run") for v in installed_entries.values()),
              "the installed map's old save records were baselined, not sent")
    others = {k: v for f in folders[1:] for k, v in by_folder(f.name).items() if v["status"] != "failed_run"}
    check(others and all(v["status"] == "imported" and v["verified"] for v in others.values()),
          f"{len(others)} existing scores imported and verified")
    check(report["failed_runs"] == sum(1 for e in load_save(env.save)["highScores"] if not e.get("cleared", True)),
          f"{report['failed_runs']} failed runs kept local")
    check(report["totals"]["total_pp"] > 0, f"total PP now {report['totals']['total_pp']}")

    song = first_tier_entry(env.save, to_install.name)
    if song is None:
        song = f"CUSTOM_{to_install.name}/UNBEATABLE\\Classic"
        data = load_save(env.save)
        data["highScores"].append({"song": song, "score": 1000000, "accuracy": 1.0, "maxCombo": 1,
                                   "notes": [{"timing": "Perfect", "count": 1}], "modifierMask": 0,
                                   "cleared": True, "updateCount": 0})
        write_save(env.save, data)
        env.ubl("sync")
    tier = song.split("/", 1)[1].split("\\")[0]

    step("A better play on the installed map syncs (save rewritten in place and via rename)")
    for atomic in (False, True):
        improve(env.save, song, atomic=atomic)
        r = env.ubl_json("sync")
        st = env.sync_state()["entries"][song]
        check(r["sent"] == 1 and st["status"] in ("imported", "no_change") and st["verified"],
              f"sent once, verified ({st['status']})")

    step("A failed run stays local and never flags the player")
    data = load_save(env.save)
    data["highScores"].append({"song": f"CUSTOM_{to_install.name}/Easy\\Classic", "score": 5, "accuracy": 0.2,
                               "maxCombo": 2, "notes": [{"timing": "Miss", "count": 40}],
                               "modifierMask": 0, "cleared": False, "updateCount": 0})
    write_save(env.save, data)
    r = env.ubl_json("sync")
    check(r["sent"] == 0 and r["failed_runs"] == 1, "not sent")
    flagged = env.http("GET", "/api/admin/flagged", cookie=admin)
    check(not any(f.get("username") == "dev-player" for f in flagged), "no anticheat flag")

    chart = next(p for p in (env.songs / to_install.name).iterdir()
                 if p.suffix == ".txt" and f"[{tier.lower()}]" in p.name.lower())
    original = chart.read_bytes()

    step("An edited chart: old records are baselined, new plays come back modified_chart")
    edit_one_note(chart)
    r = env.ubl_json("sync")
    check(env.maps()[install_id][1] == "Modified", "map shows as modified")
    improve(env.save, song)
    env.ubl_json("sync")
    check(env.sync_state()["entries"][song]["status"] == "modified_chart", "play on the edited chart not imported")

    step("A new ranked version: local copy is outdated until updated")
    chart.write_bytes(original)
    env.ubl("sync")
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w") as zf:
        for p in sorted(to_install.iterdir()):
            if p.is_file() and not p.name.endswith(".auto"):
                data = p.read_bytes()
                if p.name == chart.name:
                    tmpc = env.tmp / "v2.txt"
                    tmpc.write_bytes(data)
                    edit_one_note(tmpc)
                    data = tmpc.read_bytes()
                zf.writestr(f"{to_install.name}/{p.name}", data)
    env.push_version(admin, install_id, buf.getvalue())
    maps = env.maps()
    check(maps[install_id][:2] == (2, "Outdated"), "v2 ranked; local copy outdated")
    improve(env.save, song)
    env.ubl_json("sync")
    check(env.sync_state()["entries"][song]["status"] == "outdated_chart", "play on the old version not imported")
    env.ubl("update-all")
    check(env.maps()[install_id][1] == "Installed", "updated to v2")
    backups = list((env.songs.parent / "UNBEATEN Launcher" / "backups").iterdir())
    check(len(backups) == 1, f"old copy kept in backups ({backups[0].name})")
    improve(env.save, song)
    env.ubl_json("sync")
    st = env.sync_state()["entries"][song]
    check(st["status"] in ("imported", "no_change") and st["verified"], f"play on v2 verified ({st['status']})")

    step("Watch mode: game start/exit and a sync after a play")
    fake_game = env.tmp / "UNBEATABLE.exe"
    shutil.copy(shutil.which("sleep"), fake_game)
    watch = subprocess.Popen(env.ubl_args("watch", "--interval", "1"), env=env.ubl_env(),
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    time.sleep(3)
    game = subprocess.Popen([str(fake_game), "6"])
    time.sleep(2.5)
    refused = env.ubl("install", str(install_id), ok=False)
    check(refused.returncode != 0 and "close UNBEATABLE" in refused.stderr, "installs refused while the game runs")
    improve(env.save, song)
    game.wait()
    time.sleep(5)
    watch.terminate()
    out, err = watch.communicate(10)
    check("UNBEATABLE started" in err and "UNBEATABLE closed" in err, "game start and exit seen")
    check(out.count('"sent": 1') >= 1, "the new play was synced while watching")

    step("Sign out revokes the token")
    env.ubl("logout")
    check(env.ubl("me", ok=False).returncode != 0, "launcher signed out")
    tokens = env.http("GET", "/api/launcher/tokens", cookie=player)
    check(tokens == [], "no launcher left in Settings")


if __name__ == "__main__":
    main()
