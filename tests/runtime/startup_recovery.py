"""在独立本机测试库中验证真实宿主退出、崩溃和持久恢复；不访问现役服务。"""

import hashlib
import json
import os
import signal
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
from pathlib import Path


binary = Path(os.environ["AIO_RECOVERY_TEST_BINARY"]).resolve(strict=True)
database = os.environ["AIO_RECOVERY_TEST_DATABASE_URL"]
parsed = urllib.parse.urlparse(database)
assert parsed.hostname == "127.0.0.1" and parsed.path == "/identity_recovery_test"
root = Path(tempfile.mkdtemp(prefix="aio-startup-recovery-"))
source = root / "composition.json"
cache = root / "cache"
record_path = cache / "runtime-composition/record.json"
(root / "aio.toml").write_text("plugins = []\n", encoding="utf-8")
(root / "web").mkdir()
(root / "web/index.html").write_text("<!doctype html><title>恢复测试</title>", encoding="utf-8")
with socket.socket() as reservation:
    reservation.bind(("127.0.0.1", 0))
    port = reservation.getsockname()[1]
origin = "http://127.0.0.1:%d" % port
base_env = {key: value for key, value in os.environ.items() if not key.startswith("AIO_")}
recovery_env = dict(base_env, AIO_RUNTIME_COMPOSITION=str(source), AIO_PLUGIN_CACHE=str(cache))
env = dict(recovery_env, **{
    "AIO_DATABASE_URL": database,
    "AIO_CONFIG": str(root / "aio.toml"),
    "AIO_PROCESS_ROOT": str(root / "processes"),
    "AIO_PLUGIN_SUPERVISOR_SOCKET": str(root / "absent-supervisor.sock"),
    "AIO_PUBLIC_ORIGIN": origin,
    "AIO_WEB_HOST": "127.0.0.1", "AIO_WEB_PORT": str(port),
    "AIO_WEB_DIST": str(root / "web"),
    "AIO_BOOTSTRAP_ACCOUNT": "recovery-test",
    "AIO_BOOTSTRAP_PASSWORD": uuid.uuid4().hex,
})
process = None
cookie = None
log = open(str(root / "server.log"), "w", encoding="utf-8")
http = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def until(check, timeout=40):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = check()
        if value:
            return value
        time.sleep(0.1)
    raise AssertionError("等待条件超时；日志目录: " + str(root))


def record():
    return json.loads(record_path.read_text(encoding="utf-8"))


def configuration(value):
    pending = root / "composition.pending"
    pending.write_text(json.dumps(value), encoding="utf-8")
    pending.replace(source)


def request(path, value=None, method=None):
    headers = {"Cookie": cookie} if cookie else {}
    data = None
    if value is not None:
        data = json.dumps(value).encode("utf-8")
        headers.update({"Content-Type": "application/json", "Accept": "application/json, text/event-stream",
                        "MCP-Protocol-Version": "2025-06-18"})
    try:
        response = http.open(urllib.request.Request(origin + path, data=data, headers=headers, method=method), timeout=2)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        return response.status, response.headers, response.read()


def mcp_status():
    return request("/api/runtime/mcp", {"jsonrpc": "2.0", "id": 1, "method": "tools/list"})[0]


def launch(overrides=None):
    global process
    child_env = dict(env, **(overrides or {}))
    process = subprocess.Popen([str(binary)], cwd=str(root), env=child_env, stdout=log, stderr=subprocess.STDOUT)


def ready():
    assert process.poll() is None, "宿主提前退出；日志目录: " + str(root)
    try:
        return request("/health")[0] == 200
    except OSError:
        return False


def start(overrides=None):
    launch(overrides)
    until(ready)
    for path in ["/api/dictionaries", "/api/files", "/api/rbac", "/api/tenants", "/api/plugins/marketplace"]:
        assert request(path)[0] == 404, "disabled system route reappeared: " + path


def stop():
    process.send_signal(signal.SIGTERM)
    assert process.wait(timeout=20) == 0


def command(name, success=True):
    # 故意不提供数据库、产品配置文件和业务身份，证明恢复入口独立。
    result = subprocess.run([str(binary), "runtime", name], cwd=str(root), env=recovery_env,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10)
    assert (result.returncode == 0) == success, result.stderr.decode("utf-8")
    return result.stdout.decode("utf-8")


good = {"schema_version": 1, "mcp": {"enabled": False}, "evolution": {"enabled": False},
        "wasm": {"enabled": False},
        "access_control": {"enabled": False}, "tenants": {"enabled": False}}
native_only = dict(good, mcp={"enabled": True}, rhai={"enabled": False}, typescript={"enabled": False})


def no_scripts():
    assert mcp_status() == 200
    assert request("/api/runtime/enhancements")[0] == 404
    status, _, body = request("/api/runtime/mcp", {"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
    assert status == 200 and json.loads(body)["result"]["tools"] == []
    confirmed = record()["confirmed"]["value"]
    assert not confirmed["rhai"]["enabled"] and not confirmed["typescript"]["enabled"]


def script_restored(revision, saved_head):
    status, _, body = request("/api/runtime/enhancements")
    assert status == 200 and json.loads(body) == saved_head
    status, _, body = request("/api/runtime/mcp", {"jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": {"name": "v1.process.double", "arguments": {"input": 21}}})
    assert status == 200
    result = json.loads(body)["result"]
    assert result.get("isError") is not True
    assert result["structuredContent"] == {"revision": revision, "output": 42}


try:
    assert command("status").strip() == "null"
    assert not cache.exists()
    # 首次启动就不装配任何脚本语言，原生协议与身份服务仍可启动。
    configuration(native_only)
    start()
    status, headers, _ = request("/api/auth/login", {
        "account": env["AIO_BOOTSTRAP_ACCOUNT"], "password": env["AIO_BOOTSTRAP_PASSWORD"],
    })
    assert status == 200
    cookie = headers["Set-Cookie"].split(";")[0]
    no_scripts()
    stop()
    start()
    no_scripts()
    configuration(good)
    until(lambda: mcp_status() == 404 and request("/api/runtime/enhancements")[0] == 200 and record()["attempt"] is None)
    assert mcp_status() == 404
    initial = json.loads(command("status"))
    assert initial["confirmed"]["value"]["mcp"]["enabled"] is False
    assert initial["confirmed"]["value"]["evolution"]["enabled"] is False
    command("restore", False)
    command("retry", False)

    # 已发布版本在全部语言停用、进程强杀和单语言恢复后保持相同摘要和代数。
    status, _, body = request("/api/runtime/enhancements/revisions", {"schema_version": 1, "enhancements": [{
        "path": "/v1/process/double", "language": "rhai", "source": "fn invoke(input, context) { input * 2 }",
        "examples": [{"input": 21, "output": 42}],
    }]})
    assert status == 200
    revision = json.loads(body)["revision"]
    status, _, body = request("/api/runtime/enhancements/active", {"revision": revision, "expected_generation": 0}, "PUT")
    assert status == 200
    saved_head = json.loads(body)
    configuration(native_only)
    until(lambda: mcp_status() == 200 and request("/api/runtime/enhancements")[0] == 404 and record()["attempt"] is None)
    no_scripts()
    process.kill()
    assert process.wait(timeout=10) != 0
    start()
    no_scripts()
    configuration(dict(good, mcp={"enabled": True}, typescript={"enabled": False}))
    until(lambda: request("/api/runtime/enhancements")[0] == 200 and record()["attempt"] is None)
    script_restored(revision, saved_head)
    stop()
    start()
    script_restored(revision, saved_head)
    configuration(good)
    until(lambda: mcp_status() == 404 and record()["attempt"] is None)

    # 在线拒绝后重启，不能重新尝试或默认为全部能力启用。
    configuration({"schema_version": 9})
    until(lambda: record()["attempt"] and record()["attempt"]["rejected"])
    failed = record()["attempt"]
    assert mcp_status() == 404
    stop()
    start()
    assert record()["attempt"] == failed
    assert mcp_status() == 404
    stop()
    command("retry", False)
    command("restore")

    # 初次资源初始化真实失败；下一个全新进程才加载最后成功组合。
    blocked_storage = cache / "compiled"
    blocked_storage.write_text("not a directory", encoding="utf-8")
    storage_failure = {}
    configuration(dict(good, wasm={"enabled": True}))
    launch(storage_failure)
    assert process.wait(timeout=40) != 0
    failed = record()["attempt"]
    assert failed["rejected"]
    start(storage_failure)
    assert record()["attempt"] == failed
    assert mcp_status() == 404
    stop()
    command("retry")
    launch(storage_failure)
    assert process.wait(timeout=40) != 0
    assert record()["attempt"]["id"] != failed["id"]
    command("restore")

    # 缺失部署文件也只能恢复已确认组合，不能扩大权限。
    source.unlink()
    start()
    assert mcp_status() == 404
    stop()
    command("restore")

    # 真实崩溃：候选落盘后让业务初始化等待本任务持有的无响应 TCP 端口。
    candidate = dict(good, mcp={"enabled": True})
    configuration(candidate)
    digest = hashlib.sha256(source.read_bytes()).hexdigest()
    with socket.socket() as unavailable:
        unavailable.bind(("127.0.0.1", 0))
        unavailable.listen(8)
        stalled_database = "postgresql://postgres@127.0.0.1:%d/identity_recovery_test" % unavailable.getsockname()[1]
        launch({"AIO_DATABASE_URL": stalled_database})
        until(lambda: record()["attempt"] and record()["attempt"]["digest"] == digest)
        assert process.poll() is None
        interrupted = record()["attempt"]
        assert not interrupted["rejected"]
        process.kill()
        assert process.wait(timeout=10) != 0
    start()
    assert record()["attempt"] == interrupted
    assert mcp_status() == 404
    stop()
    command("retry")
    start()
    assert mcp_status() == 200
    assert record()["attempt"] is None

    # 新内容在线生效，并能在重启后持续保持停用状态。
    configuration(good)
    until(lambda: mcp_status() == 404 and record()["attempt"] is None)
    stop()
    start()
    assert mcp_status() == 404
    stop()

    # 端口绑定失败不留下新候选记录。
    before = record_path.read_bytes()
    configuration(candidate)
    with socket.socket() as occupied:
        occupied.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        occupied.bind(("127.0.0.1", port))
        occupied.listen(1)
        launch()
        assert process.wait(timeout=10) != 0
    assert record_path.read_bytes() == before

    # 记录无法落盘时停止服务，不能继续使用不可恢复的在线状态。
    command("restore")
    start()
    backup = record_path.with_suffix(".saved")
    record_path.replace(backup)
    record_path.mkdir()
    configuration(candidate)
    assert process.wait(timeout=15) != 0
    record_path.rmdir()
    backup.replace(record_path)
    command("restore")

    # 损坏的记录不得作为首次启动处理。
    record_path.write_text("{", encoding="utf-8")
    command("status", False)
    command("restore", False)
    launch()
    assert process.wait(timeout=10) != 0
    print("PASS: zero-language bootstrap, script suspension/restoration, independent commands, process locks, online/startup failure, crash recovery, retry/restore, port conflict, durable write failure, corrupt record")
    print("Artifacts: " + str(root))
finally:
    if process is not None and process.poll() is None:
        process.kill()
        process.wait(timeout=10)
    log.close()
