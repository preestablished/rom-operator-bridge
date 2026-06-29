#!/usr/bin/env python3
"""Create private Tailscale HTTP validation cookie and evidence inputs."""

from __future__ import annotations

import argparse
import datetime as dt
import http.client
import http.cookies
import json
import os
import re
import socket
import subprocess
import sys
from pathlib import Path


ORIGIN = "http://tailrombridge.birb.homes"
HOST = "tailrombridge.birb.homes"
PORT = 80
PRIVATE_FILE_MODE = 0o600
PRIVATE_DIR_MODE = 0o700
ROOT_DIR = Path(__file__).resolve().parents[1]


class ResolvedHTTPConnection(http.client.HTTPConnection):
    def __init__(self, host: str, resolved_ip: str | None, *args: object, **kwargs: object) -> None:
        super().__init__(host, *args, **kwargs)
        self.resolved_ip = resolved_ip

    def connect(self) -> None:
        target = self.resolved_ip or self.host
        self.sock = socket.create_connection(
            (target, self.port), self.timeout, self.source_address
        )
        if self._tunnel_host:
            self._tunnel()


def require_value(name: str) -> str:
    value = os.environ.get(name, "").strip()
    if not value:
        raise RuntimeError(f"missing {name}")
    return value


def path_arg(value: str | None, env_name: str) -> Path:
    if value:
        return Path(value)
    env_value = os.environ.get(env_name, "").strip()
    if env_value:
        return Path(env_value)
    raise RuntimeError(f"missing --{env_name.lower().replace('_', '-')} or {env_name}")


def ensure_private_parent(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.parent.chmod(PRIVATE_DIR_MODE)


def ensure_outside_repo(path: Path, label: str) -> Path:
    resolved = path.expanduser().resolve(strict=False)
    repo = ROOT_DIR.resolve()
    if resolved == repo or repo in resolved.parents:
        raise RuntimeError(f"{label} must be outside the repository checkout")
    return resolved


def write_private_text(path: Path, contents: str) -> None:
    ensure_private_parent(path)
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, PRIVATE_FILE_MODE)
    with os.fdopen(fd, "w", encoding="utf-8") as handle:
        handle.write(contents)
        handle.flush()
        os.fsync(handle.fileno())
    path.chmod(PRIVATE_FILE_MODE)


def write_cookie_jar(path: Path, set_cookie_headers: list[str]) -> None:
    lines = ["# Netscape HTTP Cookie File"]
    for header in set_cookie_headers:
        cookie = http.cookies.SimpleCookie()
        cookie.load(header)
        for name, morsel in cookie.items():
            cookie_path = morsel["path"] or "/"
            secure = "TRUE" if morsel["secure"] else "FALSE"
            lines.append(
                "\t".join(
                    [
                        HOST,
                        "FALSE",
                        cookie_path,
                        secure,
                        "0",
                        name,
                        morsel.value,
                    ]
                )
            )
    write_private_text(path, "\n".join(lines) + "\n")


def sanitized_error(body_text: str) -> tuple[str, str]:
    try:
        data = json.loads(body_text)
    except json.JSONDecodeError:
        return "unparseable", "response body was not JSON"
    error = data.get("error") if isinstance(data, dict) else None
    if isinstance(error, dict):
        code = str(error.get("code") or "")
        message = str(error.get("message") or "")
    else:
        code = ""
        message = ""
    return code or "unknown", message or "no message"


def run_capture(args: list[str]) -> str:
    result = subprocess.run(args, check=False, text=True, capture_output=True)
    if result.returncode != 0:
        raise RuntimeError(f"command failed: {args[0]}")
    return result.stdout


def create_start_session_json(path: Path) -> None:
    payload = {
        "schema_version": 1,
        "operator_credential": require_value("ROM_OPERATOR_BRIDGE_OPERATOR_CREDENTIAL"),
        "backend_mode": require_value("ROM_OPERATOR_BRIDGE_BACKEND"),
        "requested_capabilities": ["input", "preview", "capture"],
    }
    write_private_text(path, json.dumps(payload, indent=2) + "\n")


def create_cookie_jar(
    start_session_json: Path,
    cookie_jar: Path,
    session_response: Path,
    bridge_ip: str | None,
) -> None:
    ensure_private_parent(cookie_jar)
    ensure_private_parent(session_response)
    body = start_session_json.read_text(encoding="utf-8")
    conn = ResolvedHTTPConnection(HOST, bridge_ip, port=PORT, timeout=15)
    try:
        conn.request(
            "POST",
            "/api/session/start",
            body=body,
            headers={
                "Host": HOST,
                "Origin": ORIGIN,
                "Content-Type": "application/json",
            },
        )
        response = conn.getresponse()
        response_body = response.read().decode("utf-8", errors="replace")
        write_private_text(session_response, response_body)
        set_cookie_headers = [
            value for key, value in response.getheaders() if key.lower() == "set-cookie"
        ]
        if not (200 <= response.status < 300):
            write_cookie_jar(cookie_jar, [])
            code, message = sanitized_error(response_body)
            print(
                f"tailscale-validation-inputs: FAIL session_cookie_http status={response.status} "
                f"code={code} message={message}",
                file=sys.stderr,
            )
            raise RuntimeError("session request failed")
        session_cookie_headers = []
        for header in set_cookie_headers:
            cookie = http.cookies.SimpleCookie()
            cookie.load(header)
            if "rom_operator_bridge_session" in cookie:
                session_cookie_headers.append(header)
        if not session_cookie_headers:
            write_cookie_jar(cookie_jar, [])
            raise RuntimeError("session response did not set operator session cookie")
        write_cookie_jar(cookie_jar, session_cookie_headers)
    finally:
        conn.close()


def create_network_evidence(path: Path, service_port: str) -> None:
    now = dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    listeners = run_capture(["ss", "-ltnp"])
    listener_lines = [
        line for line in listeners.splitlines() if re.search(rf"(:|\]){re.escape(service_port)}\b", line)
    ]
    route_lines = [
        line for line in listeners.splitlines() if re.search(r"(:|\])80\b", line)
    ]
    contents = "\n".join(
        [
            now,
            "",
            "bridge upstream listener evidence",
            *listener_lines,
            "",
            "tailscale http route listener evidence",
            *route_lines,
            "",
        ]
    )
    write_private_text(path, contents)


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(
        description="Prepare private Tailscale HTTP validation inputs."
    )
    parser.add_argument("--start-session-json")
    parser.add_argument("--cookie-jar")
    parser.add_argument("--session-response")
    parser.add_argument("--network-evidence")
    parser.add_argument("--bridge-ip")
    parser.add_argument("--service-port", default="7410")
    args = parser.parse_args(argv)

    try:
        start_session_json = ensure_outside_repo(
            path_arg(args.start_session_json, "ROM_BRIDGE_TAILSCALE_START_SESSION_JSON"),
            "start session JSON",
        )
        cookie_jar = ensure_outside_repo(
            path_arg(args.cookie_jar, "ROM_BRIDGE_TAILSCALE_SESSION_COOKIE_FILE"),
            "cookie jar",
        )
        session_response = ensure_outside_repo(
            path_arg(args.session_response, "ROM_BRIDGE_TAILSCALE_SESSION_RESPONSE"),
            "session response",
        )
        network_evidence = ensure_outside_repo(
            path_arg(args.network_evidence, "ROM_BRIDGE_TAILSCALE_NETWORK_EVIDENCE_FILE"),
            "network evidence",
        )
        bridge_ip = (
            args.bridge_ip
            or os.environ.get("ROM_BRIDGE_TAILSCALE_RESOLVE_IP", "").strip()
            or None
        )
        create_start_session_json(start_session_json)
        print("tailscale-validation-inputs: PASS start_session_json")
        create_cookie_jar(start_session_json, cookie_jar, session_response, bridge_ip)
        print("tailscale-validation-inputs: PASS session_cookie")
        create_network_evidence(network_evidence, args.service_port)
        print("tailscale-validation-inputs: PASS network_evidence")
    except Exception as error:
        print(f"tailscale-validation-inputs: FAIL {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
