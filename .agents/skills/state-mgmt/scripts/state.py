#!/usr/bin/env python3
"""Manage independent task state within a Git repository."""

import argparse
import datetime
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

STATUSES = ("pending", "progress", "complete")


def git_root():
    result = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True
    )
    if result.returncode:
        raise ValueError(f"cannot determine Git root: {result.stderr.strip()}")
    return Path(result.stdout.strip()).resolve()


def task_dir(root, value):
    path = Path(value)
    path = (path if path.is_absolute() else root / path).resolve()
    base = (root / "tasks").resolve()
    if base == root or base != root / "tasks" or path == base or not path.is_relative_to(base):
        raise ValueError("task directory must be inside the Git root's tasks directory")
    return path


def document_path(directory, value):
    path = Path(value)
    if not value or path.is_absolute() or not (directory / path).resolve().is_relative_to(directory):
        raise ValueError("document must be a relative path inside the task directory")
    if (directory / path).resolve() == directory:
        raise ValueError("document must identify a file")
    return path.as_posix()


def read_state(directory):
    with (directory / "state.json").open(encoding="utf-8") as file:
        state = json.load(file)
    if not isinstance(state, dict) or not isinstance(state.get("tasks"), dict):
        raise ValueError("invalid state: tasks must be an object")
    for key, task in state["tasks"].items():
        if not key or not isinstance(task, dict) or task.get("status") not in STATUSES:
            raise ValueError(f"invalid task: {key}")
        if "document" in task:
            if not isinstance(task["document"], str):
                raise ValueError(f"invalid document: {key}")
            document_path(directory, task["document"])
        if "note" in task and not isinstance(task["note"], str):
            raise ValueError(f"invalid note: {key}")
    return state


def write_state(directory, state):
    path = directory / "state.json"
    name = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=directory, delete=False) as file:
            name = file.name
            json.dump(state, file, ensure_ascii=False, indent=2)
            file.write("\n")
        os.replace(name, path)
    finally:
        if name and os.path.exists(name):
            os.unlink(name)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    init = commands.add_parser("init")
    choice = init.add_mutually_exclusive_group(required=True)
    choice.add_argument("--title")
    choice.add_argument("--directory")
    for command in ("add", "show", "set"):
        sub = commands.add_parser(command)
        sub.add_argument("directory")
        sub.add_argument("task_id", nargs="?" if command == "show" else None)
        if command == "set":
            sub.add_argument("status", choices=STATUSES)
            sub.add_argument("--document")
            sub.add_argument("--note")
    args = parser.parse_args()
    root = git_root()

    if args.command == "init":
        if args.title is not None:
            title = re.sub(r"[^\w-]+", "-", args.title, flags=re.UNICODE).strip("-_")
            if not title:
                raise ValueError("title is empty after normalization")
            base = root / "tasks"
            if base.is_symlink():
                raise ValueError("tasks directory must not be a symbolic link")
            base.mkdir(exist_ok=True)
            stem = f"{datetime.date.today():%Y%m%d}-{title}"
            suffix = 1
            while True:
                directory = base / (stem if suffix == 1 else f"{stem}-{suffix}")
                try:
                    directory.mkdir()
                    break
                except FileExistsError:
                    suffix += 1
        else:
            directory = task_dir(root, args.directory)
            if not directory.is_dir():
                raise ValueError("existing task directory not found")
        if (directory / "state.json").exists():
            read_state(directory)
        else:
            write_state(directory, {"tasks": {}})
        print(directory)
        return

    directory = task_dir(root, args.directory)
    state = read_state(directory)
    tasks = state["tasks"]
    if args.command == "add":
        if not args.task_id.strip():
            raise ValueError("task ID must not be empty")
        if args.task_id in tasks:
            raise ValueError("task ID already exists")
        tasks[args.task_id] = {"status": "pending"}
    elif args.command == "show":
        if args.task_id is not None and args.task_id not in tasks:
            raise ValueError("task ID not found")
        print(json.dumps(tasks[args.task_id] if args.task_id is not None else state, ensure_ascii=False))
        return
    else:
        if args.task_id not in tasks:
            raise ValueError("task ID not found")
        task = tasks[args.task_id]
        task["status"] = args.status
        if args.document is not None:
            task["document"] = document_path(directory, args.document)
        if args.note is not None:
            task["note"] = args.note
    write_state(directory, state)
    print("OK")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, json.JSONDecodeError) as error:
        print(f"error: {error}", file=sys.stderr)
        sys.exit(1)
