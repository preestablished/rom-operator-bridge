#!/usr/bin/env python3
"""Create private deployment validation cookie and network evidence inputs."""

from __future__ import annotations

import argparse
import datetime as dt
import http.client
import http.cookies
import json
import os
import re
import socket
import ssl
import subprocess
import sys
from pathlib import Path


ORIGIN = "https://rombridge.birb.homes"
HOST = "rombridge.birb.homes"
PORT = "443"
SERVICE_PORT = "7410"
PRIVATE_FILE_MODE = 0o600
PRIVATE_DIR_MODE = 0o700
ROOT_DIR = Path(__file__).resolve().parents[1]


class ResolvedHTTPSConnection(http.client.HTTPSConnection):
    def __init__(self, host: str, resolved_ip: str, *args: object, **kwargs: object) -> None:
        super().__init__(host, *args, **kwargs)
        self.resolved_ip = resolved_ip

    def connect(self) -> None:
        self.sock = socket.create_connection(
            (self.resolved_ip, self.port), self.timeout, self.source_address
        )
        if self._tunnel_host:
            self._tunnel()
        self.sock = self._context.wrap_socket(self.sock, server_hostname=self.host)


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
    elif isinstance(data, dict):
        code = str(data.get("code") or "")
        message = str(data.get("message") or "")
    else:
        code = ""
        message = ""
    return code or "unknown", message or "no message"


def run_capture(args: list[str], env: dict[str, str] | None = None) -> str:
    result = subprocess.run(args, check=False, text=True, capture_output=True, env=env)
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
    bridge_ip: str,
) -> None:
    ensure_private_parent(cookie_jar)
    ensure_private_parent(session_response)
    body = start_session_json.read_text(encoding="utf-8")
    context = ssl.create_default_context()
    conn = ResolvedHTTPSConnection(HOST, bridge_ip, port=int(PORT), context=context, timeout=15)
    try:
        conn.request(
            "POST",
            "/api/session/start",
            body=body,
            headers={
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
                f"validation-inputs: FAIL session_cookie_http status={response.status} "
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


def create_network_evidence(path: Path, kubeconfig: str) -> None:
    now = dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    listeners = run_capture(["ss", "-ltnp"])
    listener_lines = [
        line for line in listeners.splitlines() if re.search(r"(:|\])7410\b", line)
    ]
    env = os.environ.copy()
    env["KUBECONFIG"] = kubeconfig
    route_objects = run_capture(
        [
            "kubectl",
            "get",
            "ingress,svc,endpoints",
            "-n",
            "rom-operator-bridge",
            "-o",
            "wide",
        ],
        env=env,
    )
    contents = "\n".join(
        [
            now,
            "",
            "service listener evidence",
            *listener_lines,
            "",
            "k8s route objects",
            route_objects.rstrip("\n"),
            "",
        ]
    )
    write_private_text(path, contents)


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(
        description="Prepare private deployment-network validation inputs."
    )
    parser.add_argument("--start-session-json")
    parser.add_argument("--cookie-jar")
    parser.add_argument("--session-response")
    parser.add_argument("--network-evidence")
    parser.add_argument("--bridge-ip")
    parser.add_argument("--kubeconfig", default="/etc/rancher/k3s/k3s.yaml")
    args = parser.parse_args(argv)

    try:
        start_session_json = path_arg(args.start_session_json, "START_SESSION_JSON")
        cookie_jar = path_arg(args.cookie_jar, "COOKIE_JAR")
        session_response = path_arg(args.session_response, "SESSION_RESPONSE")
        network_evidence = path_arg(args.network_evidence, "NETWORK_EVIDENCE")
        bridge_ip = args.bridge_ip or require_value("BRIDGE_IP")
        start_session_json = ensure_outside_repo(start_session_json, "start session JSON")
        cookie_jar = ensure_outside_repo(cookie_jar, "cookie jar")
        session_response = ensure_outside_repo(session_response, "session response")
        network_evidence = ensure_outside_repo(network_evidence, "network evidence")
        create_start_session_json(start_session_json)
        print("validation-inputs: PASS start_session_json")
        create_cookie_jar(start_session_json, cookie_jar, session_response, bridge_ip)
        print("validation-inputs: PASS session_cookie")
        create_network_evidence(network_evidence, args.kubeconfig)
        print("validation-inputs: PASS network_evidence")
    except Exception as error:
        print(f"validation-inputs: FAIL {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
