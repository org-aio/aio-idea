"""用真实 Wasm、字典 Sidecar、产品进程与独立测试库验收执行器暂停及恢复。"""

import hashlib
import json
import os
import shutil
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
from pathlib import Path


binary = Path(os.environ["AIO_EXECUTOR_TEST_BINARY"]).resolve(strict=True)
packager = Path(os.environ["AIO_EXECUTOR_TEST_PACKAGER"]).resolve(strict=True)
wasm = Path(os.environ["AIO_EXECUTOR_TEST_WASM_COMPONENT"]).resolve(strict=True)
sidecar = Path(os.environ["AIO_EXECUTOR_TEST_SIDECAR_PACKAGE"]).resolve(strict=True)
database = os.environ["AIO_EXECUTOR_TEST_DATABASE_URL"]
postgres = os.environ["AIO_EXECUTOR_TEST_POSTGRES_CONTAINER"]
parsed = urllib.parse.urlparse(database)
assert parsed.hostname == "127.0.0.1" and parsed.path == "/executor_recovery_test"
root = Path(tempfile.mkdtemp(prefix="aio-executor-recovery-"))
source = root / "composition.json"
record = root / "cache/runtime-composition/record.json"
(root / "processes").mkdir()
(root / "web").mkdir()
(root / "web/index.html").write_text("<!doctype html><title>Executor test</title>", encoding="utf-8")
(root / "aio.toml").write_text("plugins = []\n", encoding="utf-8")
with socket.socket() as reservation:
    reservation.bind(("127.0.0.1", 0))
    port = reservation.getsockname()[1]
base = "http://127.0.0.1:%d" % port
env = {key: value for key, value in os.environ.items() if not key.startswith("AIO_")}
env.update({
    "AIO_DATABASE_URL": database, "AIO_COMPONENT_DATABASE_URL": database,
    "AIO_COMPONENT_HOME": str(root / "components"), "AIO_PLUGIN_CACHE": str(root / "cache"),
    "AIO_CONFIG": str(root / "aio.toml"), "AIO_FILE_STORAGE_DIR": str(root / "files"),
    "AIO_PROCESS_ROOT": str(root / "processes"),
    "AIO_PROCESS_IMAGES": os.environ["AIO_EXECUTOR_TEST_PROCESS_IMAGES"],
    "AIO_PLUGIN_SUPERVISOR_SOCKET": str(root / "supervisor.sock"),
    "AIO_RUNTIME_COMPOSITION": str(source),
    "AIO_PUBLIC_ORIGIN": base, "AIO_WEB_HOST": "127.0.0.1", "AIO_WEB_PORT": str(port),
    "AIO_WEB_DIST": str(root / "web"), "AIO_BOOTSTRAP_ACCOUNT": "executor-test",
    "AIO_BOOTSTRAP_PASSWORD": uuid.uuid4().hex, "AIO_PLUGIN_PUBLISH_ACCOUNTS": "executor-test",
})
http = urllib.request.build_opener(urllib.request.ProxyHandler({}))
cookie = None
host = None
supervisor = None
host_log = open(str(root / "host.log"), "w", encoding="utf-8")
supervisor_log = open(str(root / "supervisor.log"), "w", encoding="utf-8")
host_digest = hashlib.sha256(binary.read_bytes()).hexdigest()
container_prefix = "aio-v2-" + hashlib.sha256(str(root / "processes").encode()).hexdigest()[:8] + "-"


def sql(statement):
    return subprocess.check_output([
        "docker", "exec", postgres, "psql", "-U", parsed.username, "-d", "executor_recovery_test",
        "-X", "-v", "ON_ERROR_STOP=1", "-Atc", statement,
    ], universal_newlines=True).strip()


def request(path, value=None, raw=None):
    headers = {"Cookie": cookie} if cookie else {}
    body = raw
    if raw is not None:
        headers["Content-Type"] = "application/vnd.aio.component+gzip"
    if value is not None:
        body = json.dumps(value).encode()
        headers.update({"Content-Type": "application/json", "Accept": "application/json, text/event-stream",
                        "MCP-Protocol-Version": "2025-06-18"})
    try:
        response = http.open(urllib.request.Request(base + path, data=body, headers=headers), timeout=90)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        return response.status, response.headers, response.read()


def success(path, value=None, raw=None):
    status, _, body = request(path, value, raw)
    assert 200 <= status < 300, (path, status, body[:500])
    return json.loads(body)["data"] if body else None


def until(predicate, timeout=60):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.1)
    raise AssertionError("等待超时；日志目录: " + str(root))


def configure(wasm_enabled, sidecar_enabled, restart=0):
    pending = root / "composition.pending"
    pending.write_text(json.dumps({"schema_version": 1,
        "wasm": {"enabled": wasm_enabled, "restart": restart},
        "sidecar": {"enabled": sidecar_enabled, "restart": restart}}), encoding="utf-8")
    pending.replace(source)
    if host is not None:
        desired = json.loads(source.read_text(encoding="utf-8"))
        until(lambda: all(json.loads(record.read_text(encoding="utf-8"))["confirmed"]["value"][key] == desired[key]
                          for key in ["wasm", "sidecar"]))


def ready():
    assert host.poll() is None, "宿主提前退出；日志目录: " + str(root)
    try:
        return request("/health")[0] == 200
    except OSError:
        return False


def launch():
    global host
    host = subprocess.Popen([str(binary)], cwd=str(root), env=env, stdout=host_log, stderr=subprocess.STDOUT)
    until(ready)


def stop():
    global host
    if host is not None and host.poll() is None:
        host.terminate()
        assert host.wait(timeout=40) == 0
    host = None


def protocols():
    status, _, body = request("/api/runtime/mcp", {"jsonrpc": "2.0", "id": 1, "method": "tools/list"})
    assert status == 200 and "tools" in json.loads(body)["result"]
    # 正式 gRPC 客户端由 Rust 组合测试覆盖，这里验证进程恢复后 HTTP 入口仍可发现能力。
    status, _, body = request("/api/runtime/capabilities")
    assert status == 200 and isinstance(json.loads(body), list)


def publish(package):
    published = success("/api/runtime/components/publish", raw=package.read_bytes())
    entry = next(item for item in success("/api/runtime/marketplace") if item["source_id"] == published["source_id"])
    success("/api/runtime/plugins/install", {"git": entry["git"], "rev": published["revision"]})
    return published["source_id"]


def package_wasm(version):
    candidate = root / ("wasm-" + version)
    (candidate / "web").mkdir(parents=True)
    (candidate / "web/index.html").write_text("<!doctype html><title>Fixture</title>", encoding="utf-8")
    shutil.copy2(str(wasm), str(candidate / "plugin.wasm"))
    (candidate / "aio-plugin.toml").write_text(
        "schema_version=2\n[plugin]\npermissions=['fixture.view']\n[plugin.marketplace]\n"
        "title='Executor recovery'\nsummary='Wasm recovery fixture'\nlicense='MIT'\n"
        "[plugin.runtime]\nartifact='plugin.wasm'\nhost_version='>=2026.10.16'\n"
        "[plugin.frontend]\npath='web'\n", encoding="utf-8")
    output = root / ("wasm-" + version + ".aio-plugin")
    subprocess.check_call([str(packager), str(candidate), "aio-plugin.toml",
        "https://github.com/example/executor-recovery.git", "a" * 40, version, str(output)])
    return output


def mount(source_id, page):
    return success("/api/runtime/frontend/mount", {"page_id": "component:%s:%s" % (source_id, page)})["token"]


def invoke(token, path, method="GET", body=None):
    payload = [] if body is None else list(json.dumps(body).encode())
    result = success("/api/runtime/components/%s/request" % token, {
        "method": method, "path": path, "body": payload,
        "headers": [{"name": "content-type", "value": "application/json"}],
    })
    assert 200 <= result["status"] < 300, result
    return bytes(result["body"])


def installed():
    return json.loads(sql("SELECT json_agg(i ORDER BY source_id) FROM component_installations i WHERE tenant_id='default'"))


def state(source_id, expected):
    catalog = success("/api/runtime/catalog")
    plugin = next(item for item in catalog["plugins"] if item["source_id"] == source_id)
    assert plugin["state"] == expected, plugin
    entry = next(item for item in success("/api/runtime/marketplace") if item["source_id"] == source_id)
    assert entry["state"] == expected, entry
    if expected != "active":
        prefix = "component:%s:" % source_id
        assert all(not item["id"].startswith(prefix) for item in catalog["pages"])
        assert all(not item["page_id"].startswith(prefix) for item in catalog["account_items"])
        assert all(item["source_id"] != source_id for item in catalog["plugin_settings"])
        assert all(not permission.startswith(prefix) for permission in success("/api/runtime/bootstrap")["permissions"])


def containers():
    names = subprocess.check_output(["docker", "ps", "--all", "--filter", "label=io.addzero.aio.v2=true",
                                     "--format", "{{.Names}}"], universal_newlines=True).splitlines()
    return [name for name in names if name.startswith(container_prefix)]


try:
    assert sql("SELECT count(*) FROM pg_tables WHERE schemaname='public'") == "0", "必须使用新的独立测试库"
    configure(False, False)
    supervisor = subprocess.Popen([str(binary), "supervisor"], cwd=str(root), env=env, stdout=supervisor_log, stderr=subprocess.STDOUT)
    until(lambda: (root / "supervisor.sock").exists())
    launch()
    status, headers, _ = request("/api/auth/login", {"account": env["AIO_BOOTSTRAP_ACCOUNT"], "password": env["AIO_BOOTSTRAP_PASSWORD"]})
    assert status == 200
    cookie = headers["Set-Cookie"].split(";")[0]
    protocols()
    configure(True, True)
    wasm_source = publish(package_wasm("1.0.0"))
    publish(package_wasm("2.0.0"))
    success("/api/runtime/plugins/%s/rollback" % wasm_source, {})
    sidecar_source = publish(sidecar)
    wasm_token = mount(wasm_source, "fixture")
    sidecar_token = mount(sidecar_source, "dictionary-management")
    assert invoke(wasm_token, "/echo", "POST", 42) == b"42"
    created = json.loads(invoke(sidecar_token, "/api/dictionaries/types", "POST", {
        "code": "executor-durable", "name": "Executor recovery", "description": "", "enabled": True,
    }))["data"]["id"]
    before = installed()
    assert next(item for item in before if item["source_id"] == wasm_source)["excluded_digest"]
    assert len(containers()) == 1
    configure(False, False)
    for source_id in [wasm_source, sidecar_source]:
        state(source_id, "suspended")
    assert not containers()
    assert installed() == before
    for token in [wasm_token, sidecar_token]:
        assert request("/api/runtime/components/%s/request" % token, {"method": "GET", "path": "/echo"})[0] >= 400
    protocols()
    host.kill()
    assert host.wait(timeout=20) != 0
    host = None
    launch()
    for source_id in [wasm_source, sidecar_source]:
        state(source_id, "suspended")
    assert installed() == before and not containers()
    configure(True, False)
    state(wasm_source, "active")
    state(sidecar_source, "suspended")
    assert invoke(mount(wasm_source, "fixture"), "/echo", "POST", 42) == b"42"
    assert installed() == before
    configure(False, True)
    state(wasm_source, "suspended")
    state(sidecar_source, "active")
    assert len(containers()) == 1
    data = json.loads(invoke(mount(sidecar_source, "dictionary-management"), "/api/dictionaries"))["data"]
    assert any(item["id"] == created for item in data["types"])
    assert installed() == before
    configure(True, True, 1)
    assert invoke(mount(wasm_source, "fixture"), "/echo", "POST", 42) == b"42"
    invoke(mount(sidecar_source, "dictionary-management"), "/api/dictionaries")
    assert installed() == before
    configure(False, False)
    success("/api/runtime/plugins/%s/disable" % wasm_source, {})
    success("/api/runtime/plugins/%s/uninstall" % sidecar_source, {})
    stop()
    launch()
    state(wasm_source, "disabled")
    assert len(installed()) == 1 and not containers()
    configure(True, True)
    state(wasm_source, "disabled")
    success("/api/runtime/plugins/%s/enable" % wasm_source, {})
    restored = installed()[0]
    original = next(item for item in before if item["source_id"] == wasm_source)
    assert (restored["digest"], restored["excluded_digest"]) == (original["digest"], original["excluded_digest"])
    assert invoke(mount(wasm_source, "fixture"), "/echo", "POST", 42) == b"42"
    success("/api/runtime/plugins/%s/uninstall" % wasm_source, {})
    protocols()
    assert hashlib.sha256(binary.read_bytes()).hexdigest() == host_digest
finally:
    try:
        stop()
    finally:
        if supervisor is not None and supervisor.poll() is None:
            supervisor.terminate()
            supervisor.wait(timeout=20)
        host_log.close()
        supervisor_log.close()

assert not containers()
print(json.dumps({"zeroExecutors": True, "partialRecovery": True, "crashRecovery": True,
    "versionAndGenerationRetained": True, "sidecarDataRetained": True,
    "pausedManagement": True, "containerCleanup": True, "logs": str(root)}), flush=True)
